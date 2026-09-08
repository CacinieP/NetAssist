fn main() {
    tauri_build::build();
    // AXIsProcessTrusted (used for the macOS Accessibility TCC check) lives in
    // the ApplicationServices framework. Only link it when compiling for macOS.
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-lib=framework=ApplicationServices");
}
