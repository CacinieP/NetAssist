fn main() {
    tauri_build::build();
    // AXIsProcessTrusted (used for the macOS Accessibility TCC check) lives in
    // the ApplicationServices framework. Only link it when compiling FOR macOS
    // (CARGO_CFG_TARGET_OS reflects the target, not the host).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=framework=ApplicationServices");
    }
}
