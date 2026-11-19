//! Build script for `posthaze-ui` to configure the Windows stack size and embed
//! the application icon and PE metadata.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Windows MSVC defaults to a 1 MiB main-thread stack, which debug GPUI
        // element trees can exceed; raise it to 8 MiB to match Unix defaults.
        println!("cargo:rustc-link-arg=/STACK:8388608");
    }

    #[cfg(target_os = "windows")]
    {
        let icon_path = "../../assets/posthaze.ico";
        println!("cargo:rerun-if-changed={icon_path}");

        let mut res = tauri_winres::WindowsResource::new();
        res.set_icon(icon_path)
            .set("ProductName", "Posthaze")
            .set("FileDescription", "Posthaze Download Manager")
            .set("LegalCopyright", "MIT License");

        if let Err(err) = res.compile() {
            panic!("failed to compile Windows icon/resource: {err}");
        }
    }
}
