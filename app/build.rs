// Embeds app.manifest (requireAdministrator => UAC prompt on double-click), the app icon and version info via winres.
fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=assets/icon_128.rgba");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }
    let mut res = winres::WindowsResource::new();
    // Cross-compiling from Linux: point winres at the mingw tools (override with WINDRES / AR env vars).
    if !cfg!(windows) {
        let windres = std::env::var("WINDRES").unwrap_or_else(|_| "x86_64-w64-mingw32-windres".into());
        let ar = std::env::var("AR_x86_64_pc_windows_gnullvm")
            .or_else(|_| std::env::var("AR"))
            .unwrap_or_else(|_| "x86_64-w64-mingw32-ar".into());
        res.set_windres_path(&windres);
        res.set_ar_path(&ar);
        res.set_toolkit_path("/");
    }
    res.set_manifest_file("app.manifest");
    res.set_icon("assets/app.ico");
    res.set("ProductName", "PrimeCrew Tweakinator 5000");
    res.set("FileDescription", "PrimeCrew Tweakinator 5000");
    res.set("OriginalFilename", "PrimeCrewTweakinator5000.exe");
    let ver = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let ver4 = format!("{ver}.0");
    res.set("FileVersion", &ver4);
    res.set("ProductVersion", &ver4);
    res.set("CompanyName", "PrimeCrew Software");
    res.set("LegalCopyright", "PrimeCrew Software");
    res.compile().expect("winres: failed to compile Windows resources (manifest + icon)");
    // GNU/LLVM linkers drop unreferenced archive members, which silently loses the manifest.
    // Link the compiled resource object directly so the requireAdministrator manifest is always embedded.
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if env == "gnu" {
        let out = std::env::var("OUT_DIR").unwrap();
        let obj = std::path::Path::new(&out).join("resource.o");
        if obj.exists() {
            println!("cargo:rustc-link-arg-bins={}", obj.display());
        }
    }
}
