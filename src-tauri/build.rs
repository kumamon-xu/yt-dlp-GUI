fn main() {
    tauri_build::build();
    // tauri-build only links the Windows resource (comctl32 v6 manifest) into bins.
    // cargo test --lib produces a deps/*.exe without it, which then fails at load
    // with STATUS_ENTRYPOINT_NOT_FOUND (TaskDialogIndirect on comctl32 v5).
    #[cfg(windows)]
    {
        let resource =
            std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("resource.lib");
        if resource.exists() {
            // Applies to the lib test harness (rustc-link-arg-tests is only for tests/*.rs).
            println!("cargo:rustc-link-arg={}", resource.display());
        }
    }
}
