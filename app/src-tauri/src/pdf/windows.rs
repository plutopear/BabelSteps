//! WebView2 `PrintToPdf` backend. Loads the prepared `document.html` in a
//! hidden, unfocused window, waits for navigation and image decoding, then
//! prints US Letter with 0.75in margins. Nothing here is shown to the user.

use super::{PdfError, PdfJob, PdfPrinter, MARGIN_IN, PAGE_HEIGHT_IN, PAGE_WIDTH_IN};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Url, WebviewUrl, WebviewWindowBuilder};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    ExecuteScriptCompletedHandler, NavigationCompletedEventHandler, PrintToPdfCompletedHandler,
};
use windows::core::{Interface, BOOL, HSTRING, PWSTR};

const LOAD_TIMEOUT: Duration = Duration::from_secs(60);
const DECODE_TIMEOUT: Duration = Duration::from_secs(180);
const PRINT_TIMEOUT: Duration = Duration::from_secs(300);

pub struct WebView2Printer {
    pub app: AppHandle,
}

/// Turn off things a print window never needs, and switch off the SmartScreen
/// reputation lookup (`IsReputationCheckingRequired`). This is a setting we
/// apply, not a proven guarantee about all WebView2 traffic; that is measured
/// separately.
pub fn harden(core: &ICoreWebView2) {
    unsafe {
        if let Ok(s) = core.Settings() {
            if let Ok(s8) = s.cast::<ICoreWebView2Settings8>() {
                let _ = s8.SetIsReputationCheckingRequired(false);
            }
            let _ = s.SetAreDevToolsEnabled(false);
            let _ = s.SetAreDefaultContextMenusEnabled(false);
            let _ = s.SetIsStatusBarEnabled(false);
        }
    }
}

enum Nav {
    Done(bool),
}

fn pwstr_to_string(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: WebView2 returns a NUL-terminated CoTaskMem string we own.
    let s = unsafe { p.to_string().unwrap_or_default() };
    unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _)) };
    s
}

/// Closes the print window on every exit path.
struct WindowGuard(tauri::WebviewWindow);
impl Drop for WindowGuard {
    fn drop(&mut self) {
        let _ = self.0.destroy();
    }
}

fn run_script(win: &tauri::WebviewWindow, script: &str) -> Result<String, PdfError> {
    let (tx, rx) = mpsc::channel::<Result<String, String>>();
    let script = HSTRING::from(script);
    win.with_webview(move |pv| unsafe {
        let started = (|| -> windows::core::Result<()> {
            let core = pv.controller().CoreWebView2()?;
            let tx2 = tx.clone();
            core.ExecuteScript(
                &script,
                &ExecuteScriptCompletedHandler::create(Box::new(move |res, json| {
                    let _ = tx2.send(res.map(|_| json).map_err(|e| e.to_string()));
                    Ok(())
                })),
            )
        })();
        if let Err(e) = started {
            let _ = tx.send(Err(e.to_string()));
        }
    })
    .map_err(|e| PdfError(format!("could not reach the print window: {e}")))?;
    rx.recv_timeout(Duration::from_secs(30))
        .map_err(|_| PdfError("the print window did not answer".into()))?
        .map_err(|e| PdfError(format!("script failed: {e}")))
}

impl PdfPrinter for WebView2Printer {
    fn print_to_pdf(&self, job: &PdfJob) -> Result<(), PdfError> {
        let url = Url::from_file_path(&job.html_file).map_err(|_| PdfError("bad document path".into()))?;
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let label = format!("pdf-print-{}", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed));

        // Created hidden and unfocused. It stays that way.
        let win = WebviewWindowBuilder::new(&self.app, label, WebviewUrl::External("about:blank".parse().unwrap()))
            .visible(false)
            .focused(false)
            .skip_taskbar(true)
            .inner_size(816.0, 1056.0)
            .build()
            .map_err(|e| {
                PdfError(format!(
                    "could not create a hidden print window ({e}). Printing needs a window; no window was shown."
                ))
            })?;
        let win = WindowGuard(win);
        let win = &win.0;

        // 1. Navigate and wait for load.
        let (nav_tx, nav_rx) = mpsc::channel::<Nav>();
        let target = HSTRING::from(url.as_str());
        win.with_webview(move |pv| unsafe {
            let result = (|| -> windows::core::Result<()> {
                let core = pv.controller().CoreWebView2()?;
                harden(&core);
                let tx = nav_tx.clone();
                let mut token = 0i64;
                core.add_NavigationCompleted(
                    &NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
                        // Ignore the initial about:blank navigation.
                        let mut src = PWSTR::null();
                        let is_ours = sender
                            .as_ref()
                            .map(|s| s.Source(&mut src).is_ok() && pwstr_to_string(src).starts_with("file:"))
                            .unwrap_or(false);
                        if is_ours {
                            let mut ok = BOOL(0);
                            if let Some(a) = args {
                                let _ = a.IsSuccess(&mut ok);
                            }
                            let _ = tx.send(Nav::Done(ok.as_bool()));
                        }
                        Ok(())
                    })),
                    &mut token,
                )?;
                core.Navigate(&target)
            })();
            if result.is_err() {
                let _ = nav_tx.send(Nav::Done(false));
            }
        })
        .map_err(|e| PdfError(format!("could not reach the print window: {e}")))?;
        match nav_rx.recv_timeout(LOAD_TIMEOUT) {
            Ok(Nav::Done(true)) => {}
            Ok(Nav::Done(false)) => return Err(PdfError("the document page failed to load".into())),
            Err(_) => return Err(PdfError("timed out loading the document page".into())),
        }

        // 2. Wait until every image is decoded.
        run_script(
            win,
            "window.__bs_ready=false;window.__bs_ok=false;\
             Promise.all(Array.from(document.images).map(function(i){return i.decode().then(function(){return true},function(){return false})}))\
             .then(function(r){window.__bs_ok=r.every(Boolean);window.__bs_ready=true});'started'",
        )?;
        let t0 = Instant::now();
        loop {
            let r = run_script(win, "window.__bs_ready?(window.__bs_ok?'ok':'bad'):'wait'")?;
            match r.trim_matches('"') {
                "ok" => break,
                "bad" => return Err(PdfError("an image in the document could not be decoded".into())),
                _ => {}
            }
            if t0.elapsed() > DECODE_TIMEOUT {
                return Err(PdfError("timed out waiting for images to decode".into()));
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        // 3. Print.
        let (pr_tx, pr_rx) = mpsc::channel::<Result<(), String>>();
        let out = HSTRING::from(job.out_pdf.as_os_str());
        let title = HSTRING::from(job.title.as_str());
        let footer_uri = job.footer_uri.as_deref().map(HSTRING::from);
        let header_footer = job.header_footer;
        win.with_webview(move |pv| unsafe {
            let result = (|| -> windows::core::Result<()> {
                let core = pv.controller().CoreWebView2()?;
                let env = core.cast::<ICoreWebView2_2>()?.Environment()?;
                let settings = env.cast::<ICoreWebView2Environment6>()?.CreatePrintSettings()?;
                settings.SetOrientation(COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT)?;
                settings.SetPageWidth(PAGE_WIDTH_IN)?;
                settings.SetPageHeight(PAGE_HEIGHT_IN)?;
                settings.SetMarginTop(MARGIN_IN)?;
                settings.SetMarginBottom(MARGIN_IN)?;
                settings.SetMarginLeft(MARGIN_IN)?;
                settings.SetMarginRight(MARGIN_IN)?;
                settings.SetShouldPrintBackgrounds(true)?;
                settings.SetShouldPrintHeaderAndFooter(header_footer)?;
                if header_footer {
                    settings.SetHeaderTitle(&title)?;
                    if let Some(f) = &footer_uri {
                        settings.SetFooterUri(f)?;
                    }
                }
                let tx = pr_tx.clone();
                core.cast::<ICoreWebView2_7>()?.PrintToPdf(
                    &out,
                    &settings,
                    &PrintToPdfCompletedHandler::create(Box::new(move |res, ok| {
                        let _ = tx.send(match res {
                            Ok(()) if ok => Ok(()),
                            Ok(()) => Err("the engine reported that printing did not succeed".to_string()),
                            Err(e) => Err(e.to_string()),
                        });
                        Ok(())
                    })),
                )
            })();
            if let Err(e) = result {
                let _ = pr_tx.send(Err(e.to_string()));
            }
        })
        .map_err(|e| PdfError(format!("could not reach the print window: {e}")))?;
        pr_rx
            .recv_timeout(PRINT_TIMEOUT)
            .map_err(|_| PdfError("timed out while printing".into()))?
            .map_err(PdfError)
    }
}
