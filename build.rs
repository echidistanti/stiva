// Puts the icon (and the name and version Explorer shows under Properties) into the Windows executable.
// Needs `windres` from mingw-w64 when cross-compiling, or the Windows SDK's `rc.exe` on Windows.
// Without them the program still builds, only without the icon.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    // Build scripts run on the machine that builds, so the target is read from the environment.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon.ico");
    res.set("ProductName", "Stiva")
        .set("FileDescription", "Stiva")
        .set("OriginalFilename", "stiva.exe")
        .set("LegalCopyright", "Copyright (c) 2026 echidistanti");
    if let Err(e) = res.compile() {
        println!("cargo:warning=the icon is not embedded in the executable: {e}");
    }
}
