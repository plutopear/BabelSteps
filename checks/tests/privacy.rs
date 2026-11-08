//! Privacy promise checks that run with `cargo test --workspace`, locally and in
//! CI (`docs/04-privacy-and-security.md`, section 1).
//!
//! Each repository check is paired with a test that plants a violation and
//! shows the scanner catches it, so a pass means "looked and found nothing",
//! not "looked at nothing".

use babelsteps_privacy_checks::*;
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn label(path: &Path) -> String {
    path.strip_prefix(root()).unwrap_or(path).display().to_string().replace('\\', "/")
}

fn fail_on(problems: Vec<String>, what: &str) {
    assert!(problems.is_empty(), "{what}:\n  {}", problems.join("\n  "));
}

/// Every Rust crate in the workspace except this one, which has to name the
/// patterns it looks for.
fn rust_dirs() -> Vec<PathBuf> {
    ["core", "platform", "app/src-tauri", "spikes"].iter().map(|d| root().join(d)).collect()
}

// ---- Rust source ---------------------------------------------------------

#[test]
fn rust_source_uses_no_network_api() {
    let files: Vec<_> = rust_dirs().iter().flat_map(|d| files_under(d, &["rs"])).collect();
    // A walk that found nothing would pass. Make sure it saw the real code.
    for must in ["core/src/lib.rs", "platform/windows/src/pipeline.rs", "app/src-tauri/src/lib.rs"] {
        assert!(files.iter().any(|f| label(f) == must), "scan did not reach {must}");
    }
    let problems = files.iter().flat_map(|f| scan_rust(&label(f), &read(f))).collect();
    fail_on(problems, "network API in Rust source");
}

#[test]
fn rust_scan_catches_planted_network_code() {
    for bad in [
        "use std::net::TcpStream;",
        "let s = std::net::UdpSocket::bind(addr)?;",
        "use windows::Win32::Networking::WinSock::WSAStartup;",
        "let c = windows::Web::Http::HttpClient::new()?;",
    ] {
        assert!(!scan_rust("planted.rs", bad).is_empty(), "missed: {bad}");
    }
    assert!(scan_rust("ok.rs", "// We never use std::net here.\nfn main() {}").is_empty());
}

// ---- windows crate features -----------------------------------------------

#[test]
fn no_crate_turns_on_windows_network_features() {
    let mut files: Vec<_> = rust_dirs().iter().flat_map(|d| files_under(d, &["toml"])).collect();
    files.retain(|f| f.file_name().is_some_and(|n| n == "Cargo.toml"));
    files.push(root().join("Cargo.toml"));
    assert!(files.iter().any(|f| label(f) == "platform/windows/Cargo.toml"), "scan did not reach the Windows crate");
    let problems = files.iter().flat_map(|f| scan_cargo_toml(&label(f), &read(f))).collect();
    fail_on(problems, "network feature of the windows crate");
}

#[test]
fn cargo_scan_catches_planted_feature() {
    assert!(!scan_cargo_toml("p", "features = [\"Win32_Foundation\", \"Win32_Networking_WinSock\"]").is_empty());
    assert!(!scan_cargo_toml("p", "  \"Web_Http\",").is_empty());
    assert!(!scan_cargo_toml("p", "  \"Networking_Sockets\",").is_empty());
    assert!(scan_cargo_toml("p", "  \"Win32_Foundation\", # not Networking").is_empty());
}

// ---- Frontend --------------------------------------------------------------

#[test]
fn frontend_loads_nothing_remote() {
    let mut files = files_under(&root().join("app/src"), &["ts", "js", "svelte", "css", "html"]);
    files.push(root().join("app/index.html"));
    assert!(files.len() > 1, "scan found no frontend files");
    let problems = files.iter().flat_map(|f| scan_frontend(&label(f), &read(f))).collect();
    fail_on(problems, "frontend code that could reach a server");
}

#[test]
fn frontend_scan_catches_planted_requests() {
    for bad in [
        "await fetch('/api')",
        "new WebSocket(url)",
        "<link href=\"https://fonts.example/css\" rel=\"stylesheet\">",
        "navigator.sendBeacon(u, d)",
    ] {
        assert!(!scan_frontend("planted", bad).is_empty(), "missed: {bad}");
    }
    assert!(scan_frontend("ok", "import { invoke } from '@tauri-apps/api/core';").is_empty());
}

// ---- Tauri configuration ---------------------------------------------------

fn tauri_conf() -> serde_json::Value {
    serde_json::from_str(&read(&root().join("app/src-tauri/tauri.conf.json"))).expect("tauri.conf.json is not JSON")
}

#[test]
fn csp_allows_nothing_outside_the_app() {
    let conf = tauri_conf();
    let csp = conf["app"]["security"]["csp"].as_str().expect("tauri.conf.json must set a CSP string");
    fail_on(scan_csp(csp), "content security policy");
    // These switches would disable or loosen the CSP Tauri injects.
    for key in ["dangerousDisableAssetCspModification", "dangerousRemoteDomainIpcAccess"] {
        assert!(conf["app"]["security"].get(key).is_none(), "tauri.conf.json sets {key}");
    }
}

#[test]
fn csp_scan_catches_planted_sources() {
    let ok = "default-src 'self'; connect-src ipc: http://ipc.localhost";
    assert!(scan_csp(ok).is_empty());
    assert!(!scan_csp("default-src 'self'; connect-src https://telemetry.example").is_empty());
    assert!(!scan_csp("default-src *").is_empty());
    assert!(!scan_csp("default-src 'self'; img-src https:").is_empty());
    assert!(!scan_csp("img-src 'self'").is_empty(), "missing default-src must fail");
}

#[test]
fn windows_open_only_app_pages() {
    let conf = tauri_conf();
    let windows = conf["app"]["windows"].as_array().cloned().unwrap_or_default();
    for w in windows {
        if let Some(url) = w["url"].as_str() {
            assert!(!url.contains("://"), "a window opens a remote URL: {url}");
        }
    }
    // The app creates its window in code. Check that code too.
    let src = read(&root().join("app/src-tauri/src/lib.rs"));
    assert!(!src.contains("WebviewUrl::External"), "app/src-tauri/src/lib.rs opens an external URL");
}

#[test]
fn capabilities_grant_only_the_pickers() {
    let dir = root().join("app/src-tauri/capabilities");
    let files = files_under(&dir, &["json", "toml"]);
    assert!(!files.is_empty(), "no capability files found in {}", dir.display());
    let mut problems = Vec::new();
    for f in &files {
        let v: serde_json::Value = serde_json::from_str(&read(f)).unwrap_or_else(|e| panic!("{}: {e}", label(f)));
        if v.get("remote").is_some() {
            problems.push(format!("{}: grants IPC to remote URLs", label(f)));
        }
        for p in v["permissions"].as_array().cloned().unwrap_or_default() {
            // A permission is a string or an object with an "identifier".
            let id = p.as_str().map(String::from).or_else(|| p["identifier"].as_str().map(String::from)).unwrap_or_default();
            if !ALLOWED_PERMISSIONS.contains(&id.as_str()) {
                problems.push(format!("{}: grants `{id}`", label(f)));
            }
        }
    }
    fail_on(problems, "Tauri capability beyond the folder and save pickers");
}
