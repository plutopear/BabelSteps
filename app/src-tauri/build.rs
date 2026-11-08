fn main() {
    // A release build that is still in Tauri's "dev" mode loads the dev server
    // at localhost:1420 instead of the bundled frontend: a window that cannot
    // connect, and a client/server shape this product must not have.
    // Ask Tauri itself (`tauri_build::is_dev`, the same switch that decides the
    // window URL) rather than our own feature flag: the Tauri CLI turns the
    // mode off with `--features tauri/custom-protocol`, which never sets
    // `CARGO_FEATURE_CUSTOM_PROTOCOL` on this crate, so checking only that
    // flag wrongly refused a correct `npm run tauri build`.
    // The headless self-test (feature `selftest`) opens no window, so it is exempt.
    let release = std::env::var("PROFILE").is_ok_and(|p| p == "release");
    let headless = std::env::var_os("CARGO_FEATURE_SELFTEST").is_some();
    if release && tauri_build::is_dev() && !headless {
        panic!(
            "release build without the bundled frontend: it would load the dev server (localhost:1420). \
             Build with: cargo build --release -p babelsteps-app --features custom-protocol \
             (or: npm run tauri build -- --no-bundle, from app/)"
        );
    }
    tauri_build::build()
}
