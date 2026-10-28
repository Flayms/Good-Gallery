//! Gives the Windows executable its icon and version information (shown in Explorer and the taskbar).

fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/app-icon.ico")
        .set("ProductName", "Good Gallery")
        .set("FileDescription", "Good Gallery")
        .set("OriginalFilename", "good-gallery.exe")
        .set("LegalCopyright", "GPL-3.0-only");
    if let Err(error) = resource.compile() {
        panic!("can't embed the Windows resources (rc.exe from the Windows SDK is needed): {error}");
    }
}
