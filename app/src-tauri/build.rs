fn main() {
    // A release build without `custom-protocol` loads the dev server at
    // localhost:1420 instead of the bundled frontend: a window that cannot
    // connect, and a client/server shape this product must not have.
    // The headless self-test (feature `selftest`) opens no window, so it is exempt.
    let release = std::env::var("PROFILE").is_ok_and(|p| p == "release");
    let embedded = std::env::var_os("CARGO_FEATURE_CUSTOM_PROTOCOL").is_some();
    let headless = std::env::var_os("CARGO_FEATURE_SELFTEST").is_some();
    if release && !embedded && !headless {
        panic!(
            "release build without the bundled frontend: it would load the dev server (localhost:1420).              Build with: cargo build --release -p babelsteps-app --features custom-protocol"
        );
    }
    tauri_build::build()
}
