//! Folder scan/sort/index on a real directory.

use rv_core::folder::{self, Folder, ScanOptions};
use rv_core::settings::{Settings, SortMode};
use std::path::{Path, PathBuf};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> TempDir {
    let d = std::env::temp_dir().join(format!("rview-folder-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    TempDir(d)
}

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR";

fn names(files: &[PathBuf]) -> Vec<String> {
    files
        .iter()
        .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

fn set_hidden(p: &Path) {
    use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, SetFileAttributesW};
    // SAFETY: valid path string.
    unsafe { SetFileAttributesW(&windows::core::HSTRING::from(p.as_os_str()), FILE_ATTRIBUTE_HIDDEN).unwrap() };
}

#[test]
fn filters_hidden_appledouble_and_mime() {
    let t = temp("filters");
    let d = &t.0;
    for n in ["a10.png", "a2.JPG", "a1.png", "._a3.png", "notes.txt"] {
        std::fs::write(d.join(n), PNG_MAGIC).unwrap();
    }
    std::fs::write(d.join("noext"), PNG_MAGIC).unwrap();
    std::fs::write(d.join("hidden.png"), PNG_MAGIC).unwrap();
    set_hidden(&d.join("hidden.png"));
    std::fs::create_dir(d.join("sub.png")).unwrap(); // directories never count

    let mut s = Settings::default();
    assert_eq!(names(&Folder::load(d, &s, None).files), ["a1.png", "a2.JPG", "a10.png"]);
    s.skip_hidden = false;
    assert_eq!(
        names(&Folder::load(d, &s, None).files),
        ["a1.png", "a2.JPG", "a10.png", "hidden.png"]
    );
    s.skip_hidden = true;
    s.allow_mime_content_detection = true;
    // notes.txt has PNG magic bytes too: content detection accepts it, `._` stays excluded
    assert_eq!(
        names(&Folder::load(d, &s, None).files),
        ["a1.png", "a2.JPG", "a10.png", "noext", "notes.txt"]
    );
    assert_eq!(folder::list(d, true).len(), 3);
    let _ = ScanOptions::from(&s);
}

#[test]
fn index_fast_path_and_identity_fallback() {
    let t = temp("index");
    let d = &t.0;
    for n in ["x1.png", "x2.png", "x3.png"] {
        std::fs::write(d.join(n), PNG_MAGIC).unwrap();
    }
    let f = Folder::load(d, &Settings::default(), None);
    assert_eq!(f.index_of(&d.join("x2.png")), Some(1));
    assert_eq!(f.index_of(&d.join("X2.PNG")), Some(1)); // ordinal, case-insensitive
    let canon = std::fs::canonicalize(d.join("x3.png")).unwrap(); // \\?\C:\… form
    assert!(canon.to_string_lossy().starts_with(r"\\?\"));
    assert_eq!(f.index_of(&canon), Some(2)); // found through volume serial + file index
    assert_eq!(f.index_of(&d.join("missing.png")), None);
}

#[test]
fn random_seed_stable_until_tuple_changes() {
    let t = temp("random");
    let d = &t.0;
    for i in 0..12 {
        std::fs::write(d.join(format!("r{i}.png")), PNG_MAGIC).unwrap();
    }
    let s = Settings {
        sort_mode: SortMode::Random,
        ..Default::default()
    };
    let a = Folder::load(d, &s, None);
    let b = Folder::load(d, &s, Some(&a));
    assert_eq!(a.files, b.files, "same tuple → same order");
    std::fs::write(d.join("r12.png"), PNG_MAGIC).unwrap();
    let c = Folder::load(d, &s, Some(&b));
    assert_eq!(c.files.len(), 13);
    let mut back = Folder::load(d, &s, Some(&c));
    assert_eq!(back.files, c.files);
    // removing a file from the in-memory list (vanished during navigation)
    back.remove(&d.join("R5.PNG"));
    assert_eq!(back.files.len(), 12);
}
