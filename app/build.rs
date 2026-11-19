use embed_manifest::manifest::{ActiveCodePage, DpiAwareness, Setting, SupportedOS};
use embed_manifest::new_manifest;

fn main() {
    // Per-monitor v2 DPI, Common Controls v6 (TaskDialog), UTF-8 code page, long paths.
    let manifest = new_manifest("rView")
        .dpi_awareness(DpiAwareness::PerMonitorV2)
        .active_code_page(ActiveCodePage::Utf8)
        .long_path_aware(Setting::Enabled)
        .supported_os(SupportedOS::Windows10..);
    // One resource script for everything: the icon (ordinal 1: Explorer, shortcuts, taskbar,
    // title bar), the manifest and the version info.
    println!("cargo:rerun-if-changed=../assets/icon/rview.ico");
    winresource::WindowsResource::new()
        .set_icon("../assets/icon/rview.ico")
        .set_manifest(&manifest.to_string())
        .set("ProductName", "rView")
        .set("FileDescription", "rView")
        .compile()
        .expect("embed resources");
    println!("cargo:rerun-if-changed=build.rs");
    languages();
}

/// UI languages: every `assets/i18n/<code>.ftl` is embedded, so adding a language is adding
/// a file (no code change). Generates `LANGUAGES: &[(code, ftl source)]`.
fn languages() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/i18n");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("assets/i18n")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "ftl"))
        .collect();
    files.sort();
    let mut out = String::from(
        "/// (language code, Fluent source), from assets/i18n/*.ftl.\npub static LANGUAGES: &[(&str, &str)] = &[\n",
    );
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
        let code = f.file_stem().unwrap().to_string_lossy();
        let path = f.canonicalize().unwrap();
        out.push_str(&format!(
            "    ({code:?}, include_str!({:?})),\n",
            path.display().to_string()
        ));
    }
    out.push_str("];\n");
    assert!(
        files.iter().any(|f| f.ends_with("en.ftl")),
        "assets/i18n/en.ftl is the source language"
    );
    let dest = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("languages.rs");
    std::fs::write(dest, out).unwrap();
}
