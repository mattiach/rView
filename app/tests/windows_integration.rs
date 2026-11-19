//! Windows integrations: Recycle Bin, rename, clipboard, Open With, Show in
//! Explorer, locale formatting. Files live in a temporary folder; unicode names and paths longer
//! than 260 characters are part of the checks (the Recycle Bin refuses those: Windows would delete
//! them for good).
//! Side effects on this machine: the clipboard content is replaced; the deleted test files pass
//! through the Recycle Bin and are restored from it; Show in Explorer opens a window, so it runs
//! only with RVIEW_TEST_SHELL_UI=1 or on CI (prints SKIPPED otherwise).

use rview::platform;
use std::path::{Path, PathBuf};

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rview-win-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A folder whose files have paths longer than MAX_PATH.
fn long_dir(base: &Path) -> PathBuf {
    let mut d = base.to_path_buf();
    while d.as_os_str().len() < 280 {
        d = d.join("cartella-molto-lunga-è-ü-漢字");
    }
    std::fs::create_dir_all(&d).unwrap();
    assert!(d.as_os_str().len() > 260);
    d
}

/// The 8.3 form of an existing path, if the volume has 8.3 names.
fn short_path(p: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let src: Vec<u16> = p.as_os_str().encode_wide().chain([0]).collect();
    let mut buf = vec![0u16; 1024];
    // SAFETY: `src` is NUL-terminated; `buf` is writable for its length.
    let n = unsafe {
        windows::Win32::Storage::FileSystem::GetShortPathNameW(windows::core::PCWSTR(src.as_ptr()), Some(&mut buf))
    } as usize;
    let s = PathBuf::from(std::ffi::OsString::from_wide(buf.get(..n)?));
    (n > 0 && n < buf.len() && s != p).then_some(s)
}

#[test]
fn recycle_and_restore_round_trip() {
    let dir = temp_dir("bin");
    assert!(
        platform::has_recycle_bin(&dir),
        "the temp folder's drive has a Recycle Bin"
    );
    let long = long_dir(&dir);
    // the same folder given with 8.3 names (as GitHub runners give the temp folder): the bin records
    // the long form, the item must still be found
    let named = dir.join("cartella-con-nome-lungo");
    std::fs::create_dir_all(&named).unwrap();
    let short = short_path(&named).unwrap_or_else(|| {
        println!("SKIPPED: no 8.3 names on this volume (recycle via an 8.3 path)");
        named.clone()
    });
    // a full path of 260 characters or more can't be recorded by the bin (Windows would delete the
    // file for good): refused, the file stays
    let f = long.join("foto città 漢字.png");
    std::fs::write(&f, b"content").unwrap();
    assert!(std::os::windows::ffi::OsStrExt::encode_wide(f.as_os_str()).count() >= 260);
    assert!(platform::recycle(&f).is_err(), "long path refused");
    assert_eq!(
        std::fs::read(&f).unwrap(),
        b"content",
        "long path: the file is still there"
    );
    for (i, folder) in [&dir, &short].into_iter().enumerate() {
        let f = folder.join(format!("foto città {i} 漢字.png"));
        std::fs::write(&f, b"content").unwrap();
        let id = platform::recycle(&f).expect("moved to the Recycle Bin");
        assert!(!f.exists());
        let id = id.unwrap_or_else(|| panic!("{} not found in the Recycle Bin", f.display()));
        // a new file with the same name blocks the restore and nothing changes
        std::fs::write(&f, b"other").unwrap();
        assert!(platform::restore(&id).is_err());
        assert_eq!(std::fs::read(&f).unwrap(), b"other");
        std::fs::remove_file(&f).unwrap();
        let back = platform::restore(&id).expect("restored");
        // the bin gives back the long form of an 8.3 path: the same file
        let same =
            rv_core::folder::same_path_ordinal(&rv_core::folder::long_path(&back), &rv_core::folder::long_path(&f));
        assert!(same, "{} restored as {}", f.display(), back.display());
        assert_eq!(std::fs::read(&f).unwrap(), b"content", "{}", f.display());
        assert!(platform::restore(&id).is_err(), "no longer in the bin");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rename_never_replaces() {
    let dir = temp_dir("ren");
    let long = long_dir(&dir);
    for folder in [&dir, &long] {
        let (a, b) = (folder.join("a.png"), folder.join("b è.png"));
        std::fs::write(&a, b"A").unwrap();
        std::fs::write(&b, b"B").unwrap();
        let e = platform::rename_no_replace(&a, &b).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists, "{e}");
        assert_eq!(std::fs::read(&b).unwrap(), b"B", "the existing file is untouched");
        let c = folder.join("città 漢字.png");
        platform::rename_no_replace(&a, &c).unwrap();
        assert!(!a.exists() && std::fs::read(&c).unwrap() == b"A");
        // case-only rename
        let upper = folder.join("CITTÀ 漢字.png");
        platform::rename_no_replace(&c, &upper).unwrap();
        let names: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"CITTÀ 漢字.png".to_string()), "{names:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn clipboard_png_dibv5_and_files_together() {
    let (w, h) = (3u32, 2u32);
    let rgba: Vec<u8> = (0..w * h)
        .flat_map(|i| [i as u8 * 40, 200, 10, 128 + i as u8])
        .collect();
    let png = b"\x89PNG fake payload".to_vec();
    let dib = platform::dibv5_bytes(w, h, &rgba);
    let files = vec![PathBuf::from(r"C:\Temp\città 漢字.png"), PathBuf::from(r"D:\x\y.jpg")];
    platform::clipboard_set(None, &png, &dib, &files).unwrap();

    let got_png = platform::clipboard_get(platform::png_format()).expect("PNG format");
    assert_eq!(&got_png[..png.len()], &png[..], "GlobalSize may round up");
    let got_dib = platform::clipboard_get(platform::CF_DIBV5).expect("CF_DIBV5");
    assert_eq!(&got_dib[..dib.len()], &dib[..]);
    assert_eq!(platform::clipboard_files(), files);
    // DIBV5 layout: header 124 bytes, bottom-up BGRA: the first stored row is the last image row
    assert_eq!(u32::from_le_bytes(dib[0..4].try_into().unwrap()), 124);
    let first = &dib[124..128];
    let last_row_first_px = &rgba[((h - 1) * w * 4) as usize..][..4];
    assert_eq!(
        first,
        &[
            last_row_first_px[2],
            last_row_first_px[1],
            last_row_first_px[0],
            last_row_first_px[3]
        ]
    );
}

#[test]
fn open_with_lists_the_default_app_first() {
    let _com = platform::ComThread::init();
    let png = std::env::temp_dir().join("rview-openwith-probe.png");
    let apps = platform::open_with_handlers(&png);
    println!(
        "Open With .png: {:?} (default: {:?})",
        apps.iter().map(|a| (&a.ui_name, a.icon.is_some())).collect::<Vec<_>>(),
        platform::default_app_name(".png")
    );
    if apps.is_empty() {
        println!("SKIPPED: no recommended app for .png on this machine");
        return;
    }
    if let Some(d) = platform::default_app_name(".png")
        && apps.iter().any(|a| a.ui_name == d)
    {
        assert_eq!(apps[0].ui_name, d, "default app first");
    }
    let rest: Vec<&str> = apps[1..].iter().map(|a| a.ui_name.as_str()).collect();
    let mut sorted = rest.clone();
    sorted.sort_by(|a, b| rv_core::folder::natural_cmp(a.as_ref(), b.as_ref()));
    assert_eq!(rest, sorted, "the others in natural order");
}

#[test]
fn show_in_explorer_call() {
    if std::env::var_os("RVIEW_TEST_SHELL_UI").is_none() && std::env::var_os("GITHUB_ACTIONS").is_none() {
        println!("SKIPPED: show_in_explorer opens an Explorer window (set RVIEW_TEST_SHELL_UI=1)");
        return;
    }
    let _com = platform::ComThread::init();
    let dir = temp_dir("explorer");
    let f = dir.join("select è.png");
    std::fs::write(&f, b"x").unwrap();
    platform::show_in_explorer(&f).unwrap();
    assert!(platform::show_in_explorer(&dir.join("missing.png")).is_err());
}

#[test]
fn locale_formatting() {
    let (dec, _, _) = platform::number_format();
    assert!(!dec.is_empty(), "decimal separator");
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_763_000_000);
    let s = platform::local_date_time(t);
    assert!(s.chars().any(|c| c.is_ascii_digit()) && s.contains("20"), "{s}");
}
