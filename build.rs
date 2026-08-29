fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    #[cfg(windows)]
    embed_icon();
}

#[cfg(windows)]
fn embed_icon() {
    let ico = "assets/icon.ico";
    if !std::path::Path::new(ico).is_file() {
        println!("cargo:warning={ico} missing; Windows exe will have no icon");
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(ico);
    res.set("FileDescription", "SSH local-forward tunnels");
    res.set("ProductName", "tunnel-ui");
    res.set("OriginalFilename", "tunnel-ui.exe");
    res.set("CompanyName", "TrasherDK");
    res.set("InternalName", "tunnel-ui");
    if let Err(e) = res.compile() {
        println!("cargo:warning=winresource: {e}");
    }
}
