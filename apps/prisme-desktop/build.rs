fn main() {
    // On Windows the manifest asking for the common controls' version 6 goes into every
    // executable Cargo links, the tests' included: without it, a test that links them stops
    // before it starts (STATUS_ENTRYPOINT_NOT_FOUND).
    let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run the Tauri build");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let manifest =
            std::env::current_dir().expect("the package's folder").join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        println!("cargo:rustc-link-arg=/WX");
    }
}
