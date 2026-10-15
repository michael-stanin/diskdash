//! diskdash: a local, strictly read-only disk space dashboard.
//! Listens on 127.0.0.1 only and serves the embedded web UI.

use std::sync::Arc;

use anyhow::{bail, Context};
use diskdash::app::{build_router, AppState};
use diskdash::assets::EmbeddedAssets;
use diskdash::config::Config;
use diskdash::events::EventBus;
use diskdash::inventory::{spawn_auto_refresh, InventoryService, SystemSource};
use diskdash::jobs::JobRunner;
use diskdash::security::Security;
use diskdash::system::{ElevationDetector, WindowsElevation};
use diskdash_store::{open_catalog, DataDir, OwnFiles};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_args(std::env::args().skip(1)).context("invalid arguments")?;

    let data_dir = DataDir::resolve().context("cannot determine %LOCALAPPDATA%")?;
    let own = OwnFiles::new(&data_dir).context("cannot create the data directory")?;
    diskdash::logging::init(&own, &data_dir.logs_dir()).context("cannot initialise logging")?;

    let db = open_catalog(&data_dir, &own).context("cannot open the database")?;
    let events = Arc::new(EventBus::new());
    let workers = std::thread::available_parallelism().map(|n| n.get().min(4)).unwrap_or(2);
    let jobs = JobRunner::start(db.clone(), events.clone(), workers).context("cannot start the job runner")?;

    let listener = match diskdash::config::bind(&config).await {
        Ok(l) => l,
        Err(msg) => bail!(msg),
    };

    let elevated = WindowsElevation.is_elevated();
    let inventory =
        Arc::new(InventoryService::new(Box::new(SystemSource::detect(elevated)), db.clone(), events.clone()));
    spawn_auto_refresh(inventory.clone(), events.clone());
    if let Err(e) = diskdash::snapshots::reconcile(&db, &own, &data_dir.snapshots_dir()) {
        tracing::warn!(error = %e, "snapshot reconciliation failed");
    }
    if let Err(e) = diskdash::cleanup::CleanupService::reconcile(&db) {
        tracing::warn!(error = %e, "cleanup reconciliation failed");
    }
    let cleanup = Arc::new(diskdash::cleanup::CleanupService {
        db: db.clone(),
        own: own.clone(),
        jobs: jobs.clone(),
        rules_dir: data_dir.root().join("rules"),
        env: Arc::new(diskdash::cleanup_sys::SystemEnv),
        probe: Arc::new(diskdash::cleanup_sys::WindowsProbe),
    });
    let apps = Arc::new(diskdash::apps::AppsService {
        db: db.clone(),
        own: own.clone(),
        registry: Arc::new(diskdash_inventory::apps::registry::WindowsRegistry),
        prefetch: Arc::new(diskdash::apps::NoHelper),
    });
    if let Err(e) = diskdash::duplicates::DupService::reconcile(&db) {
        tracing::warn!(error = %e, "duplicate run reconciliation failed");
    }
    let dups = Arc::new(diskdash::duplicates::DupService {
        db: db.clone(),
        own: own.clone(),
        jobs: jobs.clone(),
        inventory: inventory.clone(),
        env: Arc::new(diskdash::cleanup_sys::SystemEnv),
        source: Arc::new(diskdash::dup_sys::FsSource),
    });
    let scans = Arc::new(diskdash::scans::ScanService {
        db: db.clone(),
        own: own.clone(),
        inventory: inventory.clone(),
        jobs: jobs.clone(),
        elevated,
        cleanup: Some(cleanup.clone()),
        last_access: diskdash_analysis::stale::query_setting,
    });

    let state = AppState {
        security: Arc::new(Security::new(config.port)),
        db,
        jobs: jobs.clone(),
        events,
        inventory,
        scans,
        cleanup,
        apps,
        dups,
        own,
        launcher: Arc::new(diskdash::shell::ExplorerLauncher),
        assets: Arc::new(EmbeddedAssets),
        elevation: Arc::new(WindowsElevation),
        data_dir: data_dir.root().to_path_buf(),
        dev: config.dev,
    };
    let url = format!("http://127.0.0.1:{}/", config.port);
    tracing::info!(%url, data_dir = %data_dir.root().display(), dev = config.dev, "diskdash listening");
    println!("diskdash is running at {url}  (Ctrl+C to stop)");

    if config.open_browser {
        open_browser(&url);
    }

    axum::serve(listener, build_router(state))
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutting down");
            jobs.shutdown();
        })
        .await
        .context("server error")?;
    Ok(())
}

/// Open the default browser (allowed side effect: it only shows our own page).
fn open_browser(url: &str) {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let url = HSTRING::from(url);
    // SAFETY: all strings are valid, NUL-terminated HSTRINGs that outlive the call.
    unsafe {
        ShellExecuteW(None, &HSTRING::from("open"), &url, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}
