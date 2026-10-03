fn main() {
    // Link libc++ for Android targets — required by native dependencies
    // that compile C++ code (e.g., audio decoders in symphonia/rodio).
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "android" {
        println!("cargo:rustc-link-lib=c++_shared");
    }

    // Defining app permissions enables ACL checks for every application command.
    // controller.toml explicitly preserves the desktop-main domain allowlist.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().permissions_path_pattern("permissions/*.toml"),
    ))
    .expect("failed to build application permissions")
}
