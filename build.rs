// Embed Windows resources (app icon, version info) at build time.
fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/dblens.ico");
        res.set("ProductName", "DbLens");
        res.set("FileDescription", "DbLens Universal Data Viewer");
        res.set("LegalCopyright", "DbLens");
        res.set("FileVersion", "0.1.0.0");
        res.set("ProductVersion", "0.1.0.0");
        if let Err(e) = res.compile() {
            // Don't fail the build if windres is missing on non-MSVC toolchains.
            eprintln!("cargo:warning=winres failed (icon may be missing): {e}");
        }
    }
    println!("cargo:rerun-if-changed=assets/dblens.ico");
}
