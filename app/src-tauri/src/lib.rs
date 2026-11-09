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
            commands::export_single_html,
            recording::recording_status,
            recording::set_editor_unsaved,
            recording::start_recording,
            recording::list_capture_targets,
            recording::pause_recording,
            recording::resume_recording,
            recording::add_comment,
            recording::stop_recording,
            recording::clear_notice,
            recording::get_settings,
            recording::scan_recording,
            recording::hide_finding,
            recording::save_settings,
            recording::export_settings,
            recording::import_settings,
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
    recording::set_main_window_state(app, recording::MainWindow::Pending);
    // Built hidden: it is shown only once the hardening is confirmed.
    let main = WebviewWindowBuilder::new(app, "main", WebviewUrl::App(page.into()))
        .title("BabelSteps")
        .inner_size(1100.0, 760.0)
        .visible(cfg!(not(windows)))
        .build()?;
    // Fail closed: a window whose hardening (SmartScreen lookup off, no browser
    // menus or devtools) cannot be confirmed is never shown, and the user is told.
    #[cfg(windows)]
    {
        let w = main.clone();
        let queued = main.with_webview(move |pv| {
            let ok = unsafe { pv.controller().CoreWebView2() }.map(|core| pdf::windows::harden(&core)).unwrap_or(false);
            if ok {
                recording::set_main_window_state(w.app_handle(), recording::MainWindow::Ready);
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            } else {
                refuse_unhardened(&w);
            }
        });
        if queued.is_err() {
            refuse_unhardened(&main);
        } else {
            // If the hardening step never runs, say so instead of staying hidden silently.
            let w = main.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(15));
                if recording::main_window_pending(w.app_handle()) && w.app_handle().get_webview_window("main").is_some() {
                    refuse_unhardened(&w);
                }
            });
        }
    }
    #[cfg(not(windows))]
    recording::set_main_window_state(app, recording::MainWindow::Ready);
    Ok(main)
}

#[cfg(windows)]
fn refuse_unhardened(w: &tauri::WebviewWindow) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    // Marked refused first, so Show, double-click and the note shortcut leave
    // it hidden. It is destroyed only after the message is read, so that with
    // "Quit" chosen the app does not exit before the user sees why.
    recording::set_main_window_state(w.app_handle(), recording::MainWindow::Refused);
    let _ = w.hide();
    let w2 = w.clone();
    w.app_handle()
        .dialog()
        .message("BabelSteps closed its window because it could not apply its privacy settings to the built-in browser. Quit BabelSteps and start it again.")
        .title("Window closed")
        .kind(MessageDialogKind::Error)
        .show(move |_| {
            recording::note_window_refused(w2.app_handle());
            let _ = w2.destroy();
        });
}
