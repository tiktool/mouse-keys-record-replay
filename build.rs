#[path = "src/appicon.rs"]
mod appicon;

fn main() {
    println!("cargo:rerun-if-changed=src/appicon.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app.ico");
    std::fs::write(&out, appicon::ico(&[16, 20, 24, 32, 40, 48, 64, 128])).unwrap();
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon(out.to_str().unwrap());
        res.set("ProductName", "Mouse Keys Record Replay");
        res.set("FileDescription", "Mouse Keys Record Replay");
        res.set("CompanyName", "TikTools");
        res.set("LegalCopyright", "Copyright TikTools. MIT License.");
        res.compile().unwrap();
    }
}
