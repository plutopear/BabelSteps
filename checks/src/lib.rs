//! Scanners behind the privacy checks in `tests/`. Each takes file text and
//! returns a list of problems, so the tests can prove every scanner fails on a
//! planted example before trusting a pass on the real repository.
//!
//! These are a second line of defence. The first is `no-network.ps1`, which
//! checks the resolved dependency tree; these catch network code written
//! directly against the standard library, the `windows` crate, or the browser.

use std::fs;
use std::path::{Path, PathBuf};

/// Rust names that mean the code can talk to a network. Matched as plain
/// substrings outside comments, so `windows::Networking::...`,
/// `Win32::Networking::WinSock`, and `use std::net::TcpStream` all hit.
/// `std::net` itself is not banned: its address types (`Ipv4Addr`) only parse
/// text, and `pii.rs` uses them to spot IP addresses. Opening a socket needs
/// one of the type names below, even after a glob import.
pub const RUST_NETWORK_APIS: &[&str] = &[
    "TcpStream",
    "TcpListener",
    "UdpSocket",
    "ToSocketAddrs",
    "Networking",
    "WinSock",
    "WSAStartup",
    "WSASocket",
    "WinHttp",
    "WinInet",
    "InternetOpen",
    "URLDownloadToFile",
    "Web::Http",
    "Web_Http",
    "HttpClient",
    "BackgroundTransfer",
];

/// `windows` crate features that bring in network APIs.
pub const WINDOWS_NETWORK_FEATURES: &[&str] = &["Networking", "Web_Http", "Web_Syndication", "Web_AtomPub"];

/// Ways a web page reaches a server. The frontend has no reason to contain any
/// of them: everything it needs comes from the app over Tauri's IPC.
pub const FRONTEND_NETWORK_APIS: &[&str] = &[
    "fetch(",
    "XMLHttpRequest",
    "WebSocket",
    "EventSource",
    "sendBeacon",
    "RTCPeerConnection",
    "http://",
    "https://",
    "//fonts.",
    "//cdn",
];

/// Every source a CSP directive may name. `http://ipc.localhost` is Tauri's
/// IPC channel on Windows, served inside the app; it never leaves the machine.
pub const CSP_ALLOWED_SOURCES: &[&str] = &[
    "'self'",
    "'none'",
    "'unsafe-inline'",
    "customprotocol:",
    "asset:",
    "ipc:",
    "http://ipc.localhost",
    "blob:",
    "data:",
];

/// The only Tauri permissions the app may grant: the folder and save pickers.
pub const ALLOWED_PERMISSIONS: &[&str] = &["dialog:allow-open", "dialog:allow-save"];

/// Removes `//` and `/* */` comments so a doc comment that explains why we do
/// not use `std::net` does not trip the scan. String contents are kept.
pub fn strip_rust_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let b = src.as_bytes();
    let mut i = 0;
    let mut in_str = false;
    let mut depth = 0usize;
    while i < b.len() {
        if depth > 0 {
            if b[i..].starts_with(b"*/") {
                depth -= 1;
                i += 2;
            } else if b[i..].starts_with(b"/*") {
                depth += 1;
                i += 2;
            } else {
                if b[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            continue;
        }
        if in_str {
            if b[i] == b'\\' && i + 1 < b.len() {
                out.push(b[i] as char);
                out.push(b[i + 1] as char);
                i += 2;
                continue;
            }
            if b[i] == b'"' {
                in_str = false;
            }
            push_byte(&mut out, src, &mut i);
            continue;
        }
        if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b[i..].starts_with(b"/*") {
            depth = 1;
            i += 2;
            continue;
        }
        if b[i] == b'"' {
            in_str = true;
        }
        push_byte(&mut out, src, &mut i);
    }
    out
}

fn push_byte(out: &mut String, src: &str, i: &mut usize) {
    // Copy one whole UTF-8 character.
    let ch = src[*i..].chars().next().unwrap();
    out.push(ch);
    *i += ch.len_utf8();
}

fn find_terms(label: &str, text: &str, terms: &[&str]) -> Vec<String> {
    let mut problems = Vec::new();
    for (n, line) in text.lines().enumerate() {
        for term in terms {
            if line.contains(term) {
                problems.push(format!("{label}:{}: uses `{term}`", n + 1));
            }
        }
    }
    problems
}

/// Rust source that names a network API outside a comment.
pub fn scan_rust(label: &str, src: &str) -> Vec<String> {
    find_terms(label, &strip_rust_comments(src), RUST_NETWORK_APIS)
}

/// A Cargo.toml that turns on a networking feature of the `windows` crate.
/// Only quoted feature names are checked, so comments and package names that
/// merely contain a word like "Web" do not count.
pub fn scan_cargo_toml(label: &str, src: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for (n, line) in src.lines().enumerate() {
        let code = line.split('#').next().unwrap_or("");
        for quoted in code.split('"').skip(1).step_by(2) {
            // Features are `_`-separated paths: "Win32_Networking_WinSock".
            let padded = format!("_{quoted}_");
            if WINDOWS_NETWORK_FEATURES.iter().any(|f| padded.contains(&format!("_{f}_"))) {
                problems.push(format!("{label}:{}: enables network feature \"{quoted}\"", n + 1));
            }
        }
    }
    problems
}

/// Frontend code or markup that could reach a server.
pub fn scan_frontend(label: &str, src: &str) -> Vec<String> {
    find_terms(label, src, FRONTEND_NETWORK_APIS)
}

/// A content security policy that lets the page reach anything outside the app.
pub fn scan_csp(csp: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let mut has_default = false;
    for directive in csp.split(';').map(str::trim).filter(|d| !d.is_empty()) {
        let mut parts = directive.split_whitespace();
        let name = parts.next().unwrap_or("");
        if name == "default-src" {
            has_default = true;
        }
        for source in parts {
            if !CSP_ALLOWED_SOURCES.contains(&source) {
                problems.push(format!("CSP `{name}` allows `{source}`"));
            }
        }
    }
    if !has_default {
        problems.push("CSP has no default-src, so anything it does not name is allowed".into());
    }
    problems
}

/// Every file under `dir` whose extension is in `exts`, skipping build output
/// and installed packages.
pub fn files_under(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if !matches!(name.as_ref(), "target" | "node_modules" | "gen" | "dist" | ".git") {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|e| exts.iter().any(|x| e == *x)) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_are_stripped_but_code_and_strings_stay() {
        let s = strip_rust_comments("a // std::net\n/* TcpStream /* nested */ */b \"// kept\"");
        assert!(!s.contains("std::net") && !s.contains("TcpStream"));
        assert!(s.contains('a') && s.contains('b') && s.contains("\"// kept\""));
    }
}
