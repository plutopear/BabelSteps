//! BabelSteps desktop shell. No network code lives here: no HTTP plugin, no
//! updater, no shell or opener. The webview is only given our own bundled
//! frontend (see `tauri.conf.json` for the content security policy).

mod commands;
mod error;
mod pdf;
#[cfg(feature = "selftest")]
mod selftest;

use tauri::{WebviewUrl, WebviewWindowBuilder};

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::AppState::default())
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
        ])
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
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the app");

    app.run(|_handle, event| {
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
