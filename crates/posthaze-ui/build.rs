//! Build script for `posthaze-ui` to embed the Windows application icon and PE metadata.

fn main() {
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
