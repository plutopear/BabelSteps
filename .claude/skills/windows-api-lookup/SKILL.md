---
name: windows-api-lookup
description: Look up Win32 and WinRT signatures and required Cargo features in the Rust `windows` crate without reading its enormous generated source files. Use whenever Rust code calls a Windows API, hits an unresolved import, "no method named", or a mismatched type from the windows crate, or needs a new `windows` feature flag.
---

# Windows API lookup

The crate's generated `mod.rs` files are very large (`UI/Accessibility` alone is about 21,000 lines), so reading one whole wastes a lot of context. Search them with Grep, and read only a few lines around a match.

## Finding a signature and its features

Search the registry copy with Grep, in `content` mode with `-B 1`. The line above each match lists the features it needs:

- **path:** `C:\Users\jason\.cargo\registry\src`
- **glob:** `**/windows-0.62*/src/Windows/Win32/UI/Accessibility/mod.rs` (use the namespace you need)
- **pattern:** `pub unsafe fn CreatePropertyCondition\b`

For example, a `#[cfg(all(feature = "Win32_System_Com", feature = "Win32_System_Ole", feature = "Win32_System_Variant"))]` line above a match means all three go in `spikes/win-capture/Cargo.toml` or the crate you are working in.

- A free function or struct: search `pub (unsafe )?fn Name\b` or `pub struct Name\b`.
- Unknown namespace: search with the glob `**/windows-0.62*/src/Windows/**/mod.rs` and `head_limit` 5.
- A struct's fields: Read with `offset` and `limit` 30 from the matched line.
- Sources appear only after a build has downloaded the crate. Confirm the version in `Cargo.lock` if the glob finds nothing.

## Patterns that already cost time here (windows 0.62)

| Need | Answer |
| --- | --- |
| `VARIANT` | `Win32::System::Variant::VARIANT`, not in `core`. Needs `Win32_System_Com`, `Win32_System_Ole`, and `Win32_System_Variant`. Build a boolean by setting `(*v.Anonymous.Anonymous).vt = VT_BOOL` and `.Anonymous.boolVal = VARIANT_TRUE` |
| `BOOL` | `windows::core::BOOL` |
| `HINSTANCE` from `GetModuleHandleW` | `HINSTANCE(module.0)` |
| Edit and button styles | `WINDOW_STYLE(ES_PASSWORD as u32)`, `WINDOW_STYLE(BS_PUSHBUTTON as u32)` |
| WinRT event sender | `TypedEventHandler::<Pool, IInspectable>::new(move \|sender, _\| ...)`, then `sender.as_ref()` |
| `GetMessageW` loop | Test `.0 > 0`, because it returns -1 on error |
| `SendInput` struct | `INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: ... } }` |

## Behavior, not signatures

Signatures do not say how an API behaves (threading, permissions, Windows version). For that, hand the question to the `doc-researcher` agent, which returns a short answer with sources. Treat its answers as leads until tested on this machine: M0 found several places where the real behavior surprised us, and they are listed in `research/m0-spike-results.md`.
