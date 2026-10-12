//! Router and request handlers.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{middleware, Json, Router};
use diskdash_store::Db;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::assets::{self, AssetSource};
use crate::error::ApiError;
use crate::events::{self, EventBus};
use crate::inventory::{DrivesResponse, InventoryService, RefreshJob};
use crate::jobs::builtin::NoopSleep;
use crate::jobs::runner::RunnerError;
use crate::jobs::{JobRunner, JobStatus};
use crate::logging;
use crate::security::{self, Security};
use crate::settings::{self, Settings};
use crate::system::{self, ElevationDetector, Health, SystemInfo};

#[derive(Clone)]
pub struct AppState {
    pub security: Arc<Security>,
    pub db: Db,
    pub jobs: JobRunner,
    pub events: Arc<EventBus>,
    pub inventory: Arc<InventoryService>,
    pub assets: Arc<dyn AssetSource>,
    pub elevation: Arc<dyn ElevationDetector>,
    pub data_dir: PathBuf,
    pub dev: bool,
}

pub fn build_router(state: AppState) -> Router {
    let sec = state.security.clone();
    // Test-only job kinds and the token endpoint exist only in dev mode.
    let jobs_route = if state.dev { get(list_jobs).post(submit_dev_job) } else { get(list_jobs) };
    let mut api = Router::new()
        .route("/health", get(health))
        .route("/system", get(system_info))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/jobs", jobs_route)
        .route("/jobs/{id}", get(get_job))
        .route("/jobs/{id}/cancel", post(cancel_job))
        .route("/events", get(event_stream))
        .route("/drives", get(list_drives))
        .route("/drives/refresh", post(refresh_drives))
        .route("/drives/{id}", get(get_drive));
    if state.dev {
        api = api.route("/session", get(dev_session));
    }
    let api = api.fallback(api_not_found);

    Router::new()
        .nest("/api/v1", api)
        .fallback(static_files)
        .with_state(state)
        .layer(middleware::from_fn_with_state(sec, security::guard))
        .layer(middleware::from_fn(logging::request_log))
}

async fn health() -> Json<Health> {
    Json(Health { status: "ok".into(), version: env!("CARGO_PKG_VERSION").into() })
}

async fn system_info(State(st): State<AppState>) -> Json<SystemInfo> {
    Json(system::system_info(st.elevation.as_ref(), &st.data_dir))
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, ApiError> + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f).await.map_err(ApiError::internal)?
}

async fn get_settings(State(st): State<AppState>) -> Result<Json<Settings>, ApiError> {
    let db = st.db.clone();
    blocking(move || {
        let conn = db.connect_read_only().map_err(ApiError::internal)?;
        settings::load(&conn).map(Json).map_err(ApiError::internal)
    })
    .await
}

async fn put_settings(
    State(st): State<AppState>,
    body: Result<Json<Settings>, JsonRejection>,
) -> Result<Json<Settings>, ApiError> {
    let Json(new) = body.map_err(|e| {
        let mut f = std::collections::BTreeMap::new();
        f.insert("body".to_string(), e.body_text());
        ApiError::Validation(f)
    })?;
    let errors = new.validate();
    if !errors.is_empty() {
        return Err(ApiError::Validation(errors));
    }
    let db = st.db.clone();
    blocking(move || {
        let mut conn = db.connect().map_err(ApiError::internal)?;
        settings::save(&mut conn, &new).map_err(ApiError::internal)?;
        Ok(Json(new))
    })
    .await
}

impl From<RunnerError> for ApiError {
    fn from(e: RunnerError) -> Self {
        match e {
            RunnerError::NotFound(id) => ApiError::NotFound(format!("job {id} not found")),
            RunnerError::ShuttingDown => ApiError::Conflict(e.to_string()),
            other => ApiError::internal(other),
        }
    }
}

#[derive(Deserialize)]
struct ListQuery {
    limit: Option<u32>,
}

async fn list_jobs(State(st): State<AppState>, Query(q): Query<ListQuery>) -> Result<Json<Vec<JobStatus>>, ApiError> {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let jobs = st.jobs.clone();
    blocking(move || Ok(Json(jobs.recent(limit)?))).await
}

async fn get_job(State(st): State<AppState>, Path(id): Path<u64>) -> Result<Json<JobStatus>, ApiError> {
    let jobs = st.jobs.clone();
    blocking(move || Ok(Json(jobs.get(id)?))).await
}

async fn cancel_job(State(st): State<AppState>, Path(id): Path<u64>) -> Result<Json<JobStatus>, ApiError> {
    let jobs = st.jobs.clone();
    blocking(move || Ok(Json(jobs.cancel(id)?))).await
}

#[derive(Debug, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[ts(export)]
pub enum DevJobRequest {
    NoopSleep {
        #[ts(type = "number")]
        duration_ms: u64,
    },
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct JobCreated {
    #[ts(type = "number")]
    pub id: u64,
}

async fn submit_dev_job(
    State(st): State<AppState>,
    Json(req): Json<DevJobRequest>,
) -> Result<Json<JobCreated>, ApiError> {
    let job = match req {
        DevJobRequest::NoopSleep { duration_ms } => NoopSleep { duration_ms: duration_ms.min(600_000), lock: None },
    };
    let jobs = st.jobs.clone();
    blocking(move || Ok(Json(JobCreated { id: jobs.submit(Box::new(job))? }))).await
}

async fn load_settings(db: &Db) -> Result<Settings, ApiError> {
    let db = db.clone();
    blocking(move || {
        let conn = db.connect_read_only().map_err(ApiError::internal)?;
        settings::load(&conn).map_err(ApiError::internal)
    })
    .await
}

/// Drive list with low-space flags from the current settings. The very first
/// call after install waits for the initial collection (about a second).
async fn list_drives(State(st): State<AppState>) -> Result<Json<DrivesResponse>, ApiError> {
    if !st.inventory.has_data() {
        let svc = st.inventory.clone();
        let _ = tokio::task::spawn_blocking(move || svc.refresh()).await;
    }
    let settings = load_settings(&st.db).await?;
    Ok(Json(st.inventory.snapshot(&settings)))
}

async fn get_drive(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<diskdash_inventory::model::Drive>, ApiError> {
    let settings = load_settings(&st.db).await?;
    let id = id.to_ascii_lowercase();
    st.inventory
        .snapshot(&settings)
        .drives
        .into_iter()
        .find(|d| d.id == id)
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("no drive with id {id}")))
}

async fn refresh_drives(State(st): State<AppState>) -> Result<Json<JobCreated>, ApiError> {
    let jobs = st.jobs.clone();
    let svc = st.inventory.clone();
    blocking(move || Ok(Json(JobCreated { id: jobs.submit(Box::new(RefreshJob(svc)))? }))).await
}

async fn event_stream(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let last_id = headers.get("last-event-id").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse().ok());
    events::sse_response(&st.events, last_id).into_response()
}

#[derive(Serialize)]
struct Session {
    token: String,
}

/// Dev mode only: lets the Vite dev server (which serves its own index.html)
/// obtain the token. Other origins cannot read this response (no CORS), and the
/// Host check blocks DNS rebinding.
async fn dev_session(State(st): State<AppState>) -> Json<Session> {
    Json(Session { token: st.security.token().to_string() })
}

async fn api_not_found(uri: Uri) -> ApiError {
    ApiError::NotFound(format!("no such endpoint: {}", uri.path()))
}

async fn static_files(State(st): State<AppState>, uri: Uri) -> Response {
    assets::serve(st.assets.as_ref(), uri.path(), st.security.token())
}
