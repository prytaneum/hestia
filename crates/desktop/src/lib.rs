mod bridge;
mod commands;

use bridge::Bridge;
use tauri::Manager;

/// Start the daemon unless one is running, and return without opening a window.
pub fn start_daemon() -> std::process::ExitCode {
    let started = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())
        .and_then(|runtime| {
            runtime
                .block_on(client::Client::start())
                .map_err(|e| e.to_string())
        });
    match started {
        Ok(_) => {
            tracing::info!("daemon started without the shell");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            tracing::error!(%error, "cannot start the daemon");
            std::process::ExitCode::FAILURE
        }
    }
}

fn is_quit_signal(argv: &[String]) -> bool {
    argv.iter().any(|arg| arg == common::app::DESKTOP_QUIT_ARG)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Single-instance must be registered first: a second launch (e.g. the
        // tray's left-click) hands its args to the running instance and exits,
        // and we surface the existing window instead of opening another.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if is_quit_signal(&argv) {
                tracing::info!("quit requested by a second instance");
                app.exit(0);
                return;
            }
            tracing::info!(?argv, "second instance; focusing the open window");
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            // Opening a `.hestia` file while a window is up is the common
            // case: the shell launches us again, and the path comes here.
            if let Some(path) = commands::open::archive_from(&argv) {
                commands::open::deliver(app, path);
            }
        }))
        // Links open through `commands::opener`; the plugin's click handler
        // would route them past it.
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .manage(Bridge::default())
        .manage(commands::open::PendingArchive::default())
        .setup(|app| {
            if is_quit_signal(&std::env::args().collect::<Vec<_>>()) {
                app.handle().exit(0);
                return Ok(());
            }
            commands::splash::arm(app.handle().clone());
            // A file the app was *launched* with arrives before the webview
            // can listen, so it waits until the webview asks for it.
            if let Some(path) = commands::open::archive_from(&std::env::args().collect::<Vec<_>>())
            {
                app.state::<commands::open::PendingArchive>().set(path);
            }
            tracing::info!(version = common::app::VERSION_LABEL, "desktop shell ready");
            bridge::start(app.handle().clone());
            bridge::watch(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bridge::ipc_call,
            bridge::start_daemon,
            commands::auth::account_login_sisu,
            commands::prefs::prefs_list,
            commands::prefs::prefs_set,
            commands::prefs::prefs_remove,
            commands::icons::icons_list,
            commands::icons::icon_set,
            commands::icons::icon_remove,
            commands::icons::icon_set_bytes,
            commands::icons::icon_fetch,
            commands::icons::icon_config,
            commands::screenshots::screenshots_allow,
            commands::open::pending_archive,
            commands::opener::open_path,
            commands::opener::open_url,
            commands::splash::ready,
            commands::diagnostics::log_write,
            commands::diagnostics::crash_report
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
