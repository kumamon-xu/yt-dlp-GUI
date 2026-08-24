fn main() {
    tauri_build::build();
    // tauri-build already links resource.lib into the app bin. Do not also emit
    // cargo:rustc-link-arg — that duplicates VERSION (CVT1100) on Windows builds.
    // cargo test --lib still needs the search path; the test-only #[link] in lib.rs
    // picks it up (the lib harness is not a Cargo [[bin]], so tauri-build skips it).
    #[cfg(windows)]
    {
        let out = std::env::var("OUT_DIR").unwrap();
        println!("cargo:rustc-link-search=native={out}");
    }
}
