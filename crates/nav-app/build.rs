fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }
    let mut res = winres::WindowsResource::new();
    res.set_manifest_file("app.manifest");
    if let Err(e) = res.compile() {
        panic!("winres embed failed: {e}");
    }
}
