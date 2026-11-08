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
            recording::clear_notice,
            recording::get_settings,
            recording::save_settings,
        ])
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            match event {
                // Closing the window while recording stops and saves first, in
                // both close modes. If that fails, keep the window open and say
                // so instead of closing silently. With "keep running in the tray"
                // the window is still destroyed, to free the WebView2 engine; the
                // exit that follows is refused in `run` below.
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    if let Err(e) = recording::stop_if_recording(window.app_handle()) {
                        api.prevent_close();
                        recording::report_stop_failure(window.app_handle(), &e);
                    }
                }
                tauri::WindowEvent::Destroyed => recording::main_window_destroyed(window.app_handle()),
                _ => {}
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

            build_main_window(app.handle(), false)?;
            recording::load_settings(app.handle());
            recording::setup_hotkeys(app.handle());
            recording::setup_tray(app.handle())?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the app");

    app.run(|handle, event| {
        // The last window closed (no exit code): with "keep running in the
        // tray", stay running. Quit from the tray menu passes a code and exits.
        if let tauri::RunEvent::ExitRequested { api, code: None, .. } = &event {
            if recording::stay_in_tray(handle) && handle.tray_by_id("babelsteps").is_some() {
                api.prevent_exit();
                return;
            }
        }
        // Quitting while recording stops and saves first.
        if let tauri::RunEvent::ExitRequested { api, .. } = &event {
            if let Err(e) = recording::stop_if_recording(handle) {
                api.prevent_exit();
                recording::report_stop_failure(handle, &e);
            }
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

/// The main window, with our own bundled page and the WebView2 hardening.
/// Built at start-up and again from the tray after a close-to-tray.
/// `for_note` opens it on the note box (the "Add a note" shortcut).
pub(crate) fn build_main_window(app: &tauri::AppHandle, for_note: bool) -> tauri::Result<tauri::WebviewWindow> {
    let page = if for_note { "index.html#note" } else { "index.html" };
    let main = WebviewWindowBuilder::new(app, "main", WebviewUrl::App(page.into()))
        .title("BabelSteps")
        .inner_size(1100.0, 760.0)
        .build()?;
    #[cfg(windows)]
    let _ = main.with_webview(|pv| unsafe {
        if let Ok(core) = pv.controller().CoreWebView2() {
            pdf::windows::harden(&core);
        }
    });
    Ok(main)
}
