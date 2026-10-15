//! Router and request handlers.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{middleware, Json, Router};
use diskdash_store::{Db, OwnFiles};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::assets::{self, AssetSource};
use crate::cleanup::{
    CandidatePage, CandidateQuery, CleanupOverview, CleanupService, EvaluateRequest, RulesView, SnapshotCleanup,
};
use crate::error::ApiError;
use crate::events::{self, EventBus};
use crate::inventory::{DrivesResponse, InventoryService, RefreshJob};
use crate::jobs::builtin::NoopSleep;
use crate::jobs::runner::RunnerError;
use crate::jobs::{JobRunner, JobStatus};
use crate::logging;
use crate::scans::{ScanRequest, ScanService, ScanStarted};
use crate::security::{self, Security};
use crate::settings::{self, Settings};
use crate::shell::Launcher;
use crate::snapshots::{
    self, CategoryTotal, EntryView, ErrorsResponse, ExtBreakdown, ListFilter, Measure, Page, Snapshot, SnapshotData,
    TopQuery, TreeResponse,
};
use crate::system::{self, ElevationDetector, Health, SystemInfo};

#[derive(Clone)]
pub struct AppState {
    pub security: Arc<Security>,
    pub db: Db,
    pub jobs: JobRunner,
    pub events: Arc<EventBus>,
    pub inventory: Arc<InventoryService>,
    pub scans: Arc<ScanService>,
    pub cleanup: Arc<CleanupService>,
    pub apps: Arc<crate::apps::AppsService>,
    pub dups: Arc<crate::duplicates::DupService>,
    pub plans: Arc<crate::plans::PlanService>,
    pub tiering: Arc<crate::tiering::TieringService>,
    pub own: OwnFiles,
    pub launcher: Arc<dyn Launcher>,
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
        .route("/drives/{id}", get(get_drive))
        .route("/scans", post(start_scan))
        .route("/snapshots", get(list_snapshots))
        .route("/snapshots/latest", get(latest_snapshots))
        .route("/snapshots/{id}", get(get_snapshot).delete(delete_snapshot))
        .route("/snapshots/{id}/resume", post(resume_snapshot))
        .route("/snapshots/{id}/cleanup", get(snapshot_cleanup))
        .route("/snapshots/{id}/stale", get(snapshot_stale))
        .route("/snapshots/{id}/old-large", get(snapshot_old_large))
        .route("/cleanup/overview", get(cleanup_overview))
        .route("/cleanup/candidates", get(cleanup_candidates))
        .route("/cleanup/evaluate", post(cleanup_evaluate))
        .route("/rules", get(list_rules))
        .route("/apps", get(list_apps))
        .route("/tiering", get(tiering_suggestions))
        .route("/tiering/planned", post(tiering_planned))
        .route("/plans", get(list_plans).post(create_plan))
        .route("/plans/{id}", get(get_plan).patch(rename_plan).delete(delete_plan))
        .route("/plans/{id}/items", post(add_plan_item))
        .route("/plans/{id}/items/{item}", axum::routing::patch(patch_plan_item).delete(remove_plan_item))
        .route("/plans/{id}/export", get(export_plan))
        .route("/plans/{id}/verify", post(verify_plan))
        .route("/duplicates/runs", post(start_duplicates))
        .route("/duplicates/status", get(duplicates_status))
        .route("/duplicates/groups", get(duplicates_groups))
        .route("/duplicates/suggestions", post(duplicates_suggest).delete(duplicates_clear))
        .route("/duplicates/near", get(duplicates_near))
        .route("/duplicates/skips", get(duplicates_skips))
        .route("/snapshots/{id}/children", get(snapshot_children))
        .route("/snapshots/{id}/tree", get(snapshot_tree))
        .route("/shell/reveal", post(reveal))
        .route("/snapshots/{id}/top-files", get(snapshot_top_files))
        .route("/snapshots/{id}/top-dirs", get(snapshot_top_dirs))
        .route("/snapshots/{id}/categories", get(snapshot_categories))
        .route("/snapshots/{id}/extensions", get(snapshot_extensions))
        .route("/snapshots/{id}/errors", get(snapshot_errors));
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

async fn start_scan(State(st): State<AppState>, Json(req): Json<ScanRequest>) -> Result<Json<ScanStarted>, ApiError> {
    let settings = load_settings(&st.db).await?;
    let scans = st.scans.clone();
    blocking(move || scans.start(&req, &settings).map(Json)).await
}

async fn resume_snapshot(State(st): State<AppState>, Path(id): Path<i64>) -> Result<Json<JobCreated>, ApiError> {
    let settings = load_settings(&st.db).await?;
    let scans = st.scans.clone();
    blocking(move || Ok(Json(JobCreated { id: scans.resume(id, &settings)? }))).await
}

/// Built-in plus user path protection for the current settings.
fn protection_for(st: &AppState) -> Result<diskdash_analysis::eval::protected::Protected, ApiError> {
    let conn = st.db.connect_read_only().map_err(ApiError::internal)?;
    let settings = settings::load(&conn).map_err(ApiError::internal)?;
    Ok(crate::protection::build(st.cleanup.env.as_ref(), &settings))
}

async fn snapshot_stale(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Query(p): Query<crate::stale::StaleParams>,
) -> Result<Json<SnapshotData<crate::stale::StalePageView>>, ApiError> {
    blocking(move || {
        let protected = protection_for(&st)?;
        with_snapshot(&st, id, |c, snap| crate::stale::stale_items(c, snap, &protected, &p)).map(Json)
    })
    .await
}

async fn snapshot_old_large(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Query(p): Query<crate::stale::StaleParams>,
) -> Result<Json<SnapshotData<crate::stale::StalePageView>>, ApiError> {
    blocking(move || {
        let protected = protection_for(&st)?;
        with_snapshot(&st, id, |c, snap| crate::stale::old_large_files(c, snap, &protected, &p)).map(Json)
    })
    .await
}

async fn snapshot_cleanup(State(st): State<AppState>, Path(id): Path<i64>) -> Result<Json<SnapshotCleanup>, ApiError> {
    let svc = st.cleanup.clone();
    blocking(move || svc.snapshot_cleanup(id).map(Json)).await
}

async fn cleanup_overview(State(st): State<AppState>) -> Result<Json<CleanupOverview>, ApiError> {
    let svc = st.cleanup.clone();
    blocking(move || svc.overview().map(Json)).await
}

async fn cleanup_candidates(
    State(st): State<AppState>,
    Query(q): Query<CandidateQuery>,
) -> Result<Json<CandidatePage>, ApiError> {
    let svc = st.cleanup.clone();
    blocking(move || svc.candidates(&q).map(Json)).await
}

/// Run the rules again (after editing rule files, or when an earlier run failed).
async fn cleanup_evaluate(
    State(st): State<AppState>,
    body: Option<Json<EvaluateRequest>>,
) -> Result<Json<JobCreated>, ApiError> {
    let ids = body.map(|Json(b)| b.snapshot_ids).unwrap_or_default();
    let svc = st.cleanup.clone();
    blocking(move || Ok(Json(JobCreated { id: svc.rerun(&ids)? }))).await
}

async fn list_rules(State(st): State<AppState>) -> Result<Json<RulesView>, ApiError> {
    let svc = st.cleanup.clone();
    blocking(move || Ok(Json(svc.rules_view()))).await
}

async fn list_snapshots(
    State(st): State<AppState>,
    Query(f): Query<ListFilter>,
) -> Result<Json<Vec<Snapshot>>, ApiError> {
    let db = st.db.clone();
    blocking(move || {
        let conn = db.connect_read_only().map_err(ApiError::internal)?;
        snapshots::list(&conn, &f).map(Json).map_err(ApiError::internal)
    })
    .await
}

async fn latest_snapshots(State(st): State<AppState>) -> Result<Json<Vec<Snapshot>>, ApiError> {
    let db = st.db.clone();
    blocking(move || {
        let conn = db.connect_read_only().map_err(ApiError::internal)?;
        snapshots::latest(&conn).map(Json).map_err(ApiError::internal)
    })
    .await
}

fn load_snapshot(db: &Db, id: i64) -> Result<Snapshot, ApiError> {
    let conn = db.connect_read_only().map_err(ApiError::internal)?;
    snapshots::get(&conn, id)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::NotFound(format!("snapshot {id} not found")))
}

async fn get_snapshot(State(st): State<AppState>, Path(id): Path<i64>) -> Result<Json<Snapshot>, ApiError> {
    let db = st.db.clone();
    blocking(move || load_snapshot(&db, id).map(Json)).await
}

async fn delete_snapshot(State(st): State<AppState>, Path(id): Path<i64>) -> Result<Json<serde_json::Value>, ApiError> {
    let (db, own) = (st.db.clone(), st.own.clone());
    blocking(move || {
        let snap = load_snapshot(&db, id)?;
        let conn = db.connect().map_err(ApiError::internal)?;
        snapshots::delete(&conn, &own, &snap)?;
        Ok(Json(serde_json::json!({ "deleted": id })))
    })
    .await
}

#[derive(Deserialize)]
struct EntryQuery {
    path: Option<String>,
    node: Option<i64>,
    depth: Option<u32>,
    offset: Option<u32>,
    category: Option<String>,
    under: Option<String>,
    #[serde(default)]
    measure: Measure,
    limit: Option<u32>,
    cursor: Option<u32>,
    max_depth: Option<u32>,
    code: Option<u32>,
}

/// Open the snapshot's own database; only complete snapshots can be browsed.
fn with_snapshot<T>(
    st: &AppState,
    id: i64,
    f: impl FnOnce(&rusqlite::Connection, &Snapshot) -> Result<T, ApiError>,
) -> Result<SnapshotData<T>, ApiError>
where
    T: ts_rs::TS,
{
    let snap = load_snapshot(&st.db, id)?;
    if snap.status != snapshots::SnapshotStatus::Complete {
        return Err(ApiError::Conflict(format!(
            "snapshot {id} is {}; only complete scans can be browsed",
            snap.status.as_str()
        )));
    }
    let conn = snapshots::open_snapshot(&st.own, &snap)?;
    Ok(SnapshotData::new(&snap, f(&conn, &snap)?))
}

async fn snapshot_children(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<EntryQuery>,
) -> Result<Json<SnapshotData<Page<EntryView>>>, ApiError> {
    blocking(move || {
        with_snapshot(&st, id, |c, _| {
            let dir = match q.node {
                Some(node) => snapshots::find_dir_by_id(c, node)?,
                None => snapshots::find_dir(c, q.path.as_deref())?,
            };
            snapshots::children(c, &dir, q.measure, q.limit.unwrap_or(100), q.cursor.unwrap_or(0))
        })
        .map(Json)
    })
    .await
}

async fn snapshot_top_files(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<EntryQuery>,
) -> Result<Json<SnapshotData<Vec<EntryView>>>, ApiError> {
    blocking(move || {
        with_snapshot(&st, id, |c, _| {
            let dir = match q.node {
                Some(node) => snapshots::find_dir_by_id(c, node)?,
                None => snapshots::find_dir(c, q.under.as_deref())?,
            };
            let tq =
                TopQuery { limit: q.limit.unwrap_or(20), offset: q.offset.unwrap_or(0), category: q.category.clone() };
            snapshots::top_files(c, &dir, q.measure, &tq)
        })
        .map(Json)
    })
    .await
}

async fn snapshot_top_dirs(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<EntryQuery>,
) -> Result<Json<SnapshotData<Vec<EntryView>>>, ApiError> {
    blocking(move || {
        with_snapshot(&st, id, |c, _| {
            let dir = match q.node {
                Some(node) => snapshots::find_dir_by_id(c, node)?,
                None => snapshots::find_dir(c, q.under.as_deref())?,
            };
            let tq = TopQuery { limit: q.limit.unwrap_or(20), offset: q.offset.unwrap_or(0), category: None };
            snapshots::top_dirs(c, &dir, q.max_depth, q.measure, &tq)
        })
        .map(Json)
    })
    .await
}

async fn snapshot_categories(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<EntryQuery>,
) -> Result<Json<SnapshotData<Vec<CategoryTotal>>>, ApiError> {
    blocking(move || {
        with_snapshot(&st, id, |c, _| {
            let dir = match q.node {
                Some(node) => snapshots::find_dir_by_id(c, node)?,
                None => snapshots::find_dir(c, q.under.as_deref())?,
            };
            snapshots::categories(c, &dir)
        })
        .map(Json)
    })
    .await
}

async fn snapshot_extensions(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<EntryQuery>,
) -> Result<Json<SnapshotData<ExtBreakdown>>, ApiError> {
    blocking(move || {
        with_snapshot(&st, id, |c, _| {
            let dir = match q.node {
                Some(node) => snapshots::find_dir_by_id(c, node)?,
                None => snapshots::find_dir(c, q.under.as_deref())?,
            };
            snapshots::extensions(c, &dir, q.limit.unwrap_or(20))
        })
        .map(Json)
    })
    .await
}

/// Children (and grandchildren) of a node, capped per level, for the treemap.
async fn snapshot_tree(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<EntryQuery>,
) -> Result<Json<SnapshotData<TreeResponse>>, ApiError> {
    let settings = load_settings(&st.db).await?;
    blocking(move || {
        with_snapshot(&st, id, |c, _| {
            let limit = q.limit.unwrap_or(settings.chart_children_per_level);
            snapshots::tree(c, q.node.unwrap_or(0), q.depth.unwrap_or(2).clamp(1, 2), q.measure, limit)
        })
        .map(Json)
    })
    .await
}

#[derive(Deserialize, TS)]
#[ts(export)]
pub struct RevealRequest {
    #[ts(type = "number")]
    pub snapshot_id: i64,
    #[ts(type = "number")]
    pub entry_id: i64,
}

/// Show an item from a snapshot in Explorer. The path comes from the snapshot,
/// never from the client.
async fn reveal(
    State(st): State<AppState>,
    Json(req): Json<RevealRequest>,
) -> Result<axum::http::StatusCode, ApiError> {
    blocking(move || {
        let snap = load_snapshot(&st.db, req.snapshot_id)?;
        let conn = snapshots::open_snapshot(&st.own, &snap)?;
        let (path, _) = snapshots::entry_path(&conn, req.entry_id)?;
        if !crate::shell::exists(&path) {
            return Err(ApiError::Gone(format!("path no longer exists: {path}")));
        }
        st.launcher.reveal(&path).map_err(ApiError::internal)?;
        Ok(axum::http::StatusCode::NO_CONTENT)
    })
    .await
}

async fn snapshot_errors(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<EntryQuery>,
) -> Result<Json<SnapshotData<ErrorsResponse>>, ApiError> {
    blocking(move || {
        // Errors are useful for partial snapshots too.
        let snap = load_snapshot(&st.db, id)?;
        let conn = snapshots::open_snapshot(&st.own, &snap)?;
        let data = snapshots::errors(&conn, q.code, q.limit.unwrap_or(100), q.cursor.unwrap_or(0))?;
        Ok(Json(SnapshotData::new(&snap, data)))
    })
    .await
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

/// Installed applications (read-only registry inventory).
async fn list_apps(State(st): State<AppState>) -> Result<Json<crate::apps::AppsResponse>, ApiError> {
    let svc = st.apps.clone();
    blocking(move || svc.list().map(Json)).await
}

async fn start_duplicates(
    State(st): State<AppState>,
    Json(req): Json<crate::duplicates::DupRequest>,
) -> Result<Json<JobCreated>, ApiError> {
    let svc = st.dups.clone();
    blocking(move || Ok(Json(JobCreated { id: svc.start(&req)? }))).await
}

async fn duplicates_status(State(st): State<AppState>) -> Result<Json<Option<crate::duplicates::DupRun>>, ApiError> {
    let svc = st.dups.clone();
    blocking(move || svc.status().map(Json)).await
}

async fn duplicates_groups(
    State(st): State<AppState>,
    Query(f): Query<crate::duplicates::GroupFilter>,
) -> Result<Json<crate::duplicates::DupGroupsPage>, ApiError> {
    let svc = st.dups.clone();
    blocking(move || svc.groups(&f).map(Json)).await
}

async fn duplicates_suggest(
    State(st): State<AppState>,
    Json(req): Json<crate::duplicates::SuggestRequest>,
) -> Result<Json<crate::duplicates::SuggestResult>, ApiError> {
    let svc = st.dups.clone();
    blocking(move || svc.suggest(&req).map(Json)).await
}

async fn duplicates_clear(State(st): State<AppState>) -> Result<axum::http::StatusCode, ApiError> {
    let svc = st.dups.clone();
    blocking(move || svc.clear_suggestions().map(|()| axum::http::StatusCode::NO_CONTENT)).await
}

async fn duplicates_near(State(st): State<AppState>) -> Result<Json<Vec<crate::duplicates::NearView>>, ApiError> {
    let svc = st.dups.clone();
    blocking(move || svc.near().map(Json)).await
}

#[derive(Deserialize)]
struct SkipQuery {
    reason: Option<String>,
}

async fn duplicates_skips(
    State(st): State<AppState>,
    Query(q): Query<SkipQuery>,
) -> Result<Json<Vec<crate::duplicates::SkipRow>>, ApiError> {
    let svc = st.dups.clone();
    blocking(move || svc.skips(q.reason.as_deref()).map(Json)).await
}

#[derive(Deserialize)]
struct NewPlan {
    name: String,
}

async fn list_plans(State(st): State<AppState>) -> Result<Json<Vec<crate::plans::PlanSummary>>, ApiError> {
    let svc = st.plans.clone();
    blocking(move || svc.list().map(Json)).await
}

async fn create_plan(
    State(st): State<AppState>,
    Json(b): Json<NewPlan>,
) -> Result<Json<crate::plans::PlanSummary>, ApiError> {
    let svc = st.plans.clone();
    blocking(move || svc.create(&b.name).map(Json)).await
}

async fn get_plan(State(st): State<AppState>, Path(id): Path<i64>) -> Result<Json<crate::plans::PlanView>, ApiError> {
    let svc = st.plans.clone();
    blocking(move || svc.get(id).map(Json)).await
}

async fn rename_plan(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Json(b): Json<crate::plans::PlanPatch>,
) -> Result<Json<crate::plans::PlanSummary>, ApiError> {
    let svc = st.plans.clone();
    blocking(move || svc.rename(id, &b.name).map(Json)).await
}

async fn delete_plan(State(st): State<AppState>, Path(id): Path<i64>) -> Result<axum::http::StatusCode, ApiError> {
    let svc = st.plans.clone();
    blocking(move || svc.delete(id).map(|()| axum::http::StatusCode::NO_CONTENT)).await
}

async fn add_plan_item(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<crate::plans::AddItemRequest>,
) -> Result<Json<crate::plans::AddItemResult>, ApiError> {
    let svc = st.plans.clone();
    blocking(move || svc.add_item(id, &req).map(Json)).await
}

async fn patch_plan_item(
    State(st): State<AppState>,
    Path((id, item)): Path<(i64, i64)>,
    Json(p): Json<crate::plans::ItemPatch>,
) -> Result<Json<crate::plans::PlanItemView>, ApiError> {
    let svc = st.plans.clone();
    blocking(move || svc.patch_item(id, item, &p).map(Json)).await
}

async fn remove_plan_item(
    State(st): State<AppState>,
    Path((id, item)): Path<(i64, i64)>,
) -> Result<axum::http::StatusCode, ApiError> {
    let svc = st.plans.clone();
    blocking(move || svc.remove_item(id, item).map(|()| axum::http::StatusCode::NO_CONTENT)).await
}

#[derive(Deserialize)]
struct ExportQuery {
    format: Option<String>,
}

/// Download a plan as CSV or TXT: paths and instructions, never a script.
async fn export_plan(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let svc = st.plans.clone();
    let format = q.format.unwrap_or_else(|| "csv".into());
    let (name, content_type, body) = blocking(move || svc.export(id, &format)).await?;
    let mut resp = body.into_response();
    let h = resp.headers_mut();
    h.insert(axum::http::header::CONTENT_TYPE, axum::http::HeaderValue::from_static(content_type));
    if let Ok(v) = axum::http::HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
        h.insert(axum::http::header::CONTENT_DISPOSITION, v);
    }
    Ok(resp)
}

async fn verify_plan(State(st): State<AppState>, Path(id): Path<i64>) -> Result<Json<JobCreated>, ApiError> {
    let svc = st.plans.clone();
    blocking(move || Ok(Json(JobCreated { id: svc.submit_verify(&[], Some(id))? }))).await
}

async fn tiering_suggestions(State(st): State<AppState>) -> Result<Json<crate::tiering::TieringResponse>, ApiError> {
    let svc = st.tiering.clone();
    blocking(move || svc.suggestions().map(Json)).await
}

async fn tiering_planned(
    State(st): State<AppState>,
    Json(req): Json<crate::tiering::PlannedRequest>,
) -> Result<Json<crate::tiering::TieringResponse>, ApiError> {
    let svc = st.tiering.clone();
    blocking(move || svc.set_planned(&req).map(Json)).await
}
