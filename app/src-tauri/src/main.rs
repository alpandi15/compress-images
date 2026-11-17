// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod integrations;
mod job;
#[cfg(target_os = "macos")]
mod mac_single_instance;
mod quick;
mod settings;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use compress_core::Options;
use settings::Settings;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder};

use crate::job::JobManager;
use crate::quick::QuickQueue;

const MAIN: &str = "main";
const QUICK: &str = "quick";
/// Sent to the quick window when another batch arrived while it was open.
const QUICK_BATCH_EVENT: &str = "quick-batch";

struct AppState {
    settings_path: PathBuf,
    settings: Mutex<Settings>,
    quick: QuickQueue,
}

fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .try_init();

    let cwd = std::env::current_dir().unwrap_or_default();
    let launch_paths = quick::parse_args(std::env::args_os(), &cwd);
    let context = tauri::generate_context!();

    // macOS: decide primary/secondary before Tauri starts (see mac_single_instance.rs).
    #[cfg(target_os = "macos")]
    let mut mac_primary = {
        use mac_single_instance::{acquire, paths_for, Role};
        let (lock, socket) = paths_for(&context.config().identifier);
        let argv: Vec<String> = std::env::args_os()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        match acquire(&lock, &socket, &cwd.to_string_lossy(), &argv) {
            Ok(Role::Forwarded) => return,
            Ok(Role::Primary(p)) => Some(p),
            Err(e) => {
                tracing::warn!("single instance unavailable, running standalone: {e}");
                None
            }
        }
    };

    let builder = tauri::Builder::default();
    // Windows/Linux: the plugin is atomic there (named mutex / D-Bus name). It must be the
    // first plugin: later launches forward their arguments and exit.
    #[cfg(not(target_os = "macos"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
        route(app, quick::parse_args(argv, Path::new(&cwd)));
    }));

    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(JobManager::default())
        .setup(move |app| {
            let path = settings::path_in(app.path().app_config_dir()?);
            let (loaded, warnings) = settings::load(&path);
            for w in warnings {
                tracing::warn!("{w}");
            }
            app.manage(AppState {
                settings_path: path,
                settings: Mutex::new(loaded),
                quick: QuickQueue::new(quick::DEBOUNCE),
            });
            #[cfg(target_os = "macos")]
            if let Some(primary) = mac_primary.take() {
                let handle = app.handle().clone();
                primary.listen(move |argv, cwd| {
                    route(&handle, quick::parse_args(argv, Path::new(&cwd)));
                });
            }
            route(app.handle(), launch_paths.clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            tracing::debug!(window = window.label(), ?event, "window event");
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.state::<JobManager>().defer_close(window.label()) {
                    tracing::debug!(window = window.label(), "close deferred until job stops");
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            start_job,
            cancel_job,
            is_job_running,
            take_quick_batch,
            show_main_window,
            log_from_ui,
            integration_status,
            integration_install,
            integration_uninstall,
        ])
        .build(context)
        .expect("error while building Compress Images")
        .run(|_app, event| match event {
            RunEvent::ExitRequested { code, .. } => tracing::debug!(?code, "exit requested"),
            RunEvent::Exit => tracing::debug!("exiting"),
            _ => {}
        });
}

/// Launch arguments decide the mode: `--compress <paths>` → quick mode, otherwise the GUI.
fn route(app: &AppHandle, quick_paths: Option<Vec<PathBuf>>) {
    tracing::debug!(?quick_paths, "route launch");
    match quick_paths {
        Some(paths) if !paths.is_empty() => enqueue_quick(app, paths),
        _ => {
            if let Err(e) = show_main(app) {
                tracing::error!("cannot open main window: {e}");
            }
        }
    }
}

fn show_main(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        w.show()?;
        return w.set_focus();
    }
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN)
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    WebviewWindowBuilder::from_config(app, &config)?.build()?;
    Ok(())
}

fn enqueue_quick(app: &AppHandle, paths: Vec<PathBuf>) {
    let handle = app.clone();
    app.state::<AppState>().quick.push(paths, move || {
        tracing::debug!("quick batch settled");
        if let Some(w) = handle.get_webview_window(QUICK) {
            let _ = handle.emit_to(QUICK, QUICK_BATCH_EVENT, ());
            let _ = w.set_focus();
            return;
        }
        let built = WebviewWindowBuilder::new(&handle, QUICK, WebviewUrl::App("index.html".into()))
            .title("Compress Images")
            .inner_size(460.0, 300.0)
            .resizable(false)
            .maximizable(false)
            .always_on_top(true)
            .center()
            .build();
        if let Err(e) = built {
            tracing::error!("cannot open quick window: {e}");
        }
    });
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    state
        .settings
        .lock()
        .map(|s| s.clone())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
    settings::save(&state.settings_path, &settings)?;
    *state.settings.lock().map_err(|e| e.to_string())? = settings;
    Ok(())
}

#[tauri::command]
fn start_job(
    app: AppHandle,
    window: tauri::Window,
    jobs: State<'_, JobManager>,
    paths: Vec<PathBuf>,
    options: Options,
) -> Result<(), String> {
    jobs.start(app.clone(), window.label().to_owned(), paths, options)
}

#[tauri::command]
fn cancel_job(jobs: State<'_, JobManager>) {
    jobs.cancel();
}

#[tauri::command]
fn is_job_running(jobs: State<'_, JobManager>) -> bool {
    jobs.is_running()
}

/// Paths waiting for the quick window (lossy for non-UTF-8 names).
#[tauri::command]
fn take_quick_batch(state: State<'_, AppState>) -> Vec<String> {
    state
        .quick
        .take_ready()
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

#[tauri::command]
fn show_main_window(app: AppHandle) -> Result<(), String> {
    show_main(&app).map_err(|e| e.to_string())
}

/// Lets the frontend write to the same log as the backend (errors, lifecycle in debug).
#[tauri::command]
fn log_from_ui(window: tauri::Window, level: String, message: String) {
    let w = window.label();
    match level.as_str() {
        "error" => tracing::error!(window = w, "ui: {message}"),
        "warn" => tracing::warn!(window = w, "ui: {message}"),
        _ => tracing::debug!(window = w, "ui: {message}"),
    }
}

fn integration_env(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let launcher = integrations::launcher_path().map_err(|e| e.to_string())?;
    Ok((home, launcher))
}

#[tauri::command]
fn integration_status(app: AppHandle) -> Result<integrations::IntegrationStatus, String> {
    let (home, launcher) = integration_env(&app)?;
    Ok(integrations::status(&home, &launcher))
}

#[tauri::command]
fn integration_install(app: AppHandle) -> Result<integrations::IntegrationStatus, String> {
    let (home, launcher) = integration_env(&app)?;
    tracing::debug!(launcher = %launcher.display(), "installing context-menu integration");
    integrations::install(&home, &launcher)
}

#[tauri::command]
fn integration_uninstall(app: AppHandle) -> Result<integrations::IntegrationStatus, String> {
    let (home, launcher) = integration_env(&app)?;
    tracing::debug!("removing context-menu integration");
    integrations::uninstall(&home, &launcher)
}
