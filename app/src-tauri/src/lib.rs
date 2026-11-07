//! BabelSteps desktop shell. No network code lives here: no HTTP plugin, no
//! updater, no shell or opener. The webview is only given our own bundled
//! frontend (see `tauri.conf.json` for the content security policy).

mod commands;
mod error;
mod pdf;
mod recording;
#[cfg(feature = "selftest")]
mod selftest;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::AppState::default())
        .manage(recording::RecState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_default_library_folder,
            commands::list_library,
            commands::create_recording,
            commands::open_recording,
            commands::close_recording,
            commands::save_recording,
            commands::rename_recording,
            commands::delete_recording,
            commands::read_screenshot,
            commands::export_markdown,
            commands::export_pdf,
            recording::recording_status,
            recording::set_editor_unsaved,
            recording::start_recording,
            recording::pause_recording,
            recording::resume_recording,
            recording::add_comment,
            recording::stop_recording,
        ])
        .on_window_event(|window, event| {
            // Closing the window while recording stops and saves first.
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                if window.label() == "main" {
                    recording::stop_if_recording(window.app_handle());
                }
            }
        })
        .setup(|app| {
            // Redacted copies left by an export that was killed or timed out.
            pdf::sweep_stale_temp();

            #[cfg(feature = "selftest")]
            if let Some(dir) = selftest::requested_dir() {
                // Headless: no main window is ever created.
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    let code = selftest::run(&handle, &dir);
                    handle.exit(code);
                });
                return Ok(());
            }

            let main = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("BabelSteps")
                .inner_size(1100.0, 760.0)
                .build()?;
            #[cfg(windows)]
            let _ = main.with_webview(|pv| unsafe {
                if let Ok(core) = pv.controller().CoreWebView2() {
                    pdf::windows::harden(&core);
                }
            });
            #[cfg(not(windows))]
            let _ = main;
            recording::setup_hotkeys(app.handle());
            recording::setup_tray(app.handle())?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the app");

    app.run(|handle, event| {
        // Quitting while recording stops and saves first.
        if let tauri::RunEvent::ExitRequested { .. } = &event {
            recording::stop_if_recording(handle);
        }
        // The headless self-test has no windows; do not exit when the print
        // window closes.
        #[cfg(feature = "selftest")]
        if let tauri::RunEvent::ExitRequested { api, code, .. } = &event {
            if code.is_none() && selftest::requested_dir().is_some() {
                api.prevent_exit();
            }
        }
        let _ = event;
    });
}
