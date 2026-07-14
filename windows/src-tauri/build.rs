fn main() {
    // Direct Cargo builds are the repository's native verification path. Repack the WebView assets
    // whenever the already-validated frontend output changes instead of reusing stale build output.
    println!("cargo:rerun-if-changed=../dist");
    tauri_build::build()
}
