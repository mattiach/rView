//! Atomic writes: encode in memory, write a hidden temp file in the destination folder,
//! verify it, commit with `MoveFileExW` (or `ReplaceFileW` for Overwrite Original). On any error the
//! destination is untouched and no temp file is left.

use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use windows::Win32::Storage::FileSystem::*;
use windows::core::PCWSTR;

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("Cannot write the file: {0}. Try saving with another name or in another folder.")]
    Io(#[from] std::io::Error),
    #[error("The written file could not be read back correctly; nothing was changed.")]
    Verify,
    #[error("The destination is in use by another program; nothing was changed. Close it or save with another name.")]
    Locked,
}

const TMP_MARK: &str = ".rvtmp-";
const BAK_MARK: &str = ".rvbak-";
const ORPHAN_AGE: Duration = Duration::from_secs(24 * 3600);

fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain([0]).collect()
}

fn temp_path(dst: &Path, mark: &str) -> PathBuf {
    let name = dst
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let n = crate::folder::Rng::from_entropy().next_u64() as u32;
    dst.with_file_name(format!(".{name}{mark}{n:08x}"))
}

fn set_attributes(p: &Path, a: FILE_FLAGS_AND_ATTRIBUTES) {
    // SAFETY: NUL-terminated path alive for the call; a failure only leaves the attributes as they were.
    unsafe {
        let _ = SetFileAttributesW(PCWSTR(wide(p).as_ptr()), a);
    }
}

/// Temp files left by a crash more than 24 h ago in `dir` are removed.
pub fn clean_orphans(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() > ORPHAN_AGE);
        if name.starts_with('.') && name.contains(TMP_MARK) && old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

fn win_err(e: windows::core::Error) -> SaveError {
    // ERROR_ACCESS_DENIED (5), ERROR_SHARING_VIOLATION (32), ERROR_LOCK_VIOLATION (33),
    // ERROR_UNABLE_TO_REMOVE_REPLACED (1175): the destination is open in another program
    if matches!(e.code().0 as u32 & 0xFFFF, 5 | 32 | 33 | 1175) {
        SaveError::Locked
    } else {
        SaveError::Io(std::io::Error::other(e.message()))
    }
}

/// Write the hidden temp file next to `dst` and verify it (points 2–3).
fn write_temp(dst: &Path, bytes: &[u8], verify: impl Fn(&[u8]) -> bool) -> Result<PathBuf, SaveError> {
    if let Some(dir) = dst.parent() {
        clean_orphans(dir);
    }
    let tmp = temp_path(dst, TMP_MARK);
    let result = (|| {
        let mut f = std::fs::File::create_new(&tmp)?;
        set_attributes(&tmp, FILE_ATTRIBUTE_HIDDEN);
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        if !verify(&std::fs::read(&tmp)?) {
            return Err(SaveError::Verify);
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(tmp),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Write `bytes` to `dst` atomically. `verify` receives the bytes read back from the temp file
/// (e.g. decodes the header) and must return true.
pub fn write_atomic(dst: &Path, bytes: &[u8], verify: impl Fn(&[u8]) -> bool) -> Result<(), SaveError> {
    let tmp = write_temp(dst, bytes, verify)?;
    // SAFETY: both paths NUL-terminated and alive for the call.
    let moved = unsafe {
        MoveFileExW(
            PCWSTR(wide(&tmp).as_ptr()),
            PCWSTR(wide(dst).as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if let Err(e) = moved {
        let _ = std::fs::remove_file(&tmp);
        return Err(win_err(e));
    }
    // the destination now has the temp's attributes: make it visible
    set_attributes(dst, FILE_ATTRIBUTE_NORMAL);
    Ok(())
}

/// Overwrite Original (point 4): `ReplaceFileW` keeps the original's attributes, ACL and creation
/// time. The previous content stays in a hidden backup next to it, whose path is returned (the app
/// moves it to the Recycle Bin, so the overwrite can be undone).
pub fn replace_original(dst: &Path, bytes: &[u8], verify: impl Fn(&[u8]) -> bool) -> Result<PathBuf, SaveError> {
    let tmp = write_temp(dst, bytes, verify)?;
    let backup = temp_path(dst, BAK_MARK);
    // SAFETY: NUL-terminated paths alive for the call.
    let r = unsafe {
        ReplaceFileW(
            PCWSTR(wide(dst).as_ptr()),
            PCWSTR(wide(&tmp).as_ptr()),
            PCWSTR(wide(&backup).as_ptr()),
            REPLACEFILE_IGNORE_MERGE_ERRORS,
            None,
            None,
        )
    };
    if let Err(e) = r {
        // ERROR_UNABLE_TO_MOVE_REPLACEMENT_2 (1177): the original already became the backup and the
        // new file could not take its place: put the original back
        if e.code().0 as u32 & 0xFFFF == 1177 {
            let _ = std::fs::rename(&backup, dst);
        }
        let _ = std::fs::remove_file(&tmp);
        return Err(win_err(e));
    }
    // the backup is the original file renamed (its attributes): hide it until it is recycled
    set_attributes(
        &backup,
        FILE_FLAGS_AND_ATTRIBUTES(attributes(&backup) | FILE_ATTRIBUTE_HIDDEN.0),
    );
    Ok(backup)
}

fn attributes(p: &Path) -> u32 {
    // SAFETY: NUL-terminated path alive for the call.
    match unsafe { GetFileAttributesW(PCWSTR(wide(p).as_ptr())) } {
        INVALID_FILE_ATTRIBUTES => 0,
        a => a,
    }
}

/// Undo of Overwrite Original ("Restore original"): the backup (back from the Recycle Bin) takes
/// the original's place again, visible.
pub fn restore_backup(backup: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::rename(backup, dst)?;
    set_attributes(
        dst,
        FILE_FLAGS_AND_ATTRIBUTES(attributes(dst) & !FILE_ATTRIBUTE_HIDDEN.0),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rview-atomic-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn temps(d: &Path) -> usize {
        std::fs::read_dir(d)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(TMP_MARK))
            .count()
    }

    #[test]
    fn writes_replaces_and_leaves_no_temp() {
        let d = dir("ok");
        let p = d.join("a.png");
        write_atomic(&p, b"one", |_| true).unwrap();
        write_atomic(&p, b"two", |b| b == b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        let attrs = std::os::windows::fs::MetadataExt::file_attributes(&std::fs::metadata(&p).unwrap());
        assert_eq!(attrs & 0x2, 0, "destination must not stay hidden");
        assert_eq!(temps(&d), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn failed_verify_keeps_original() {
        let d = dir("verify");
        let p = d.join("a.png");
        std::fs::write(&p, b"original").unwrap();
        assert!(matches!(write_atomic(&p, b"new", |_| false), Err(SaveError::Verify)));
        assert!(matches!(
            replace_original(&p, b"new", |_| false),
            Err(SaveError::Verify)
        ));
        assert_eq!(std::fs::read(&p).unwrap(), b"original");
        assert_eq!(temps(&d), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn locked_destination_keeps_original() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = dir("locked");
        let p = d.join("a.png");
        std::fs::write(&p, b"original").unwrap();
        let lock = std::fs::OpenOptions::new().read(true).share_mode(0).open(&p).unwrap();
        let r = write_atomic(&p, b"new", |_| true);
        assert!(matches!(r, Err(SaveError::Locked)), "{r:?}");
        let r = replace_original(&p, b"new", |_| true);
        assert!(matches!(r, Err(SaveError::Locked)), "{r:?}");
        drop(lock);
        assert_eq!(std::fs::read(&p).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1, "only the original is left");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn replace_keeps_attributes_and_makes_a_backup() {
        let d = dir("replace");
        let p = d.join("a.png");
        std::fs::write(&p, b"original").unwrap();
        set_attributes(&p, FILE_ATTRIBUTE_NOT_CONTENT_INDEXED);
        let backup = replace_original(&p, b"new", |b| b == b"new").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
        assert_eq!(std::fs::read(&backup).unwrap(), b"original");
        let attrs = |p: &Path| std::os::windows::fs::MetadataExt::file_attributes(&std::fs::metadata(p).unwrap());
        assert_ne!(attrs(&p) & FILE_ATTRIBUTE_NOT_CONTENT_INDEXED.0, 0, "attributes kept");
        assert_eq!(attrs(&p) & FILE_ATTRIBUTE_HIDDEN.0, 0);
        assert_ne!(attrs(&backup) & FILE_ATTRIBUTE_HIDDEN.0, 0, "backup hidden");
        assert_eq!(temps(&d), 0);
        restore_backup(&backup, &p).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"original");
        assert_eq!(attrs(&p) & FILE_ATTRIBUTE_HIDDEN.0, 0, "restored visible");
        assert_ne!(
            attrs(&p) & FILE_ATTRIBUTE_NOT_CONTENT_INDEXED.0,
            0,
            "with its attributes"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn old_orphans_are_cleaned() {
        let d = dir("orphan");
        let orphan = d.join(".x.png.rvtmp-deadbeef");
        let fresh = d.join(".y.png.rvtmp-cafebabe");
        std::fs::write(&orphan, b"").unwrap();
        std::fs::write(&fresh, b"").unwrap();
        let old = SystemTime::now() - Duration::from_secs(25 * 3600);
        std::fs::File::options()
            .write(true)
            .open(&orphan)
            .unwrap()
            .set_modified(old)
            .unwrap();
        clean_orphans(&d);
        assert!(!orphan.exists() && fresh.exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
