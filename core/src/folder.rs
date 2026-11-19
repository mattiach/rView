//! Folder listing and navigation: filters, six sort modes, random seed, current index
//! (ordinal fast path + file identity), navigation with loop, rescan rule. Pure apart from `scan`.

use crate::formats::{Format, is_supported_path};
use crate::settings::{AfterDelete, Settings, SortMode};
use std::cmp::Ordering;
use std::hash::{BuildHasher, Hasher};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
/// Rescan only when the previous load is at least this old (fast navigation keeps the list).
pub const RESCAN_AFTER: Duration = Duration::from_secs(3);

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain([0]).collect()
}

/// Explorer's natural order (`StrCmpLogicalW`: `img2 < img10`, case-insensitive).
pub fn natural_cmp(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> Ordering {
    let (a, b) = (wide(a), wide(b));
    // SAFETY: both buffers are NUL-terminated UTF-16 and live for the call.
    let r = unsafe {
        windows::Win32::UI::Shell::StrCmpLogicalW(windows::core::PCWSTR(a.as_ptr()), windows::core::PCWSTR(b.as_ptr()))
    };
    r.cmp(&0)
}

/// Case-insensitive ordinal comparison (`CompareStringOrdinal(…, TRUE)`).
pub fn same_path_ordinal(a: &Path, b: &Path) -> bool {
    let a: Vec<u16> = a.as_os_str().encode_wide().collect();
    let b: Vec<u16> = b.as_os_str().encode_wide().collect();
    // SAFETY: slices are valid for the call.
    unsafe {
        windows::Win32::Globalization::CompareStringOrdinal(&a, &b, true) == windows::Win32::Globalization::CSTR_EQUAL
    }
}

/// The long form of an existing path (`GetLongPathNameW`): 8.3 components such as `RUNNER~1`
/// expanded, as the shell records paths (the Recycle Bin's original locations). The path itself
/// when it doesn't exist or can't be expanded. Paths above MAX_PATH go through their `\\?\` form
/// (the API refuses them otherwise) and come back without it.
pub fn long_path(p: &Path) -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Storage::FileSystem::GetLongPathNameW;
    let w: Vec<u16> = p.as_os_str().encode_wide().collect();
    let wide = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    let (verbatim, unc) = (wide(r"\\?\"), wide(r"\\?\UNC\"));
    // the prefix added here (removed from the result), if any
    let (src, added): (Vec<u16>, Vec<u16>) =
        if w.len() < 248 || w.starts_with(&wide(r"\\")) && w.get(2) == Some(&(b'?' as u16)) {
            (w, vec![])
        } else if w.starts_with(&wide(r"\\")) {
            ([unc.clone(), w[2..].to_vec()].concat(), unc.clone())
        } else {
            ([verbatim.clone(), w].concat(), verbatim)
        };
    let src: Vec<u16> = src
        .into_iter()
        .map(|c| if c == b'/' as u16 { b'\\' as u16 } else { c })
        .chain([0])
        .collect();
    let mut buf = vec![0u16; 512];
    loop {
        // SAFETY: `src` is NUL-terminated; `buf` is writable for its length.
        let n = unsafe { GetLongPathNameW(windows::core::PCWSTR(src.as_ptr()), Some(&mut buf)) } as usize;
        match n {
            0 => return p.to_path_buf(),
            n if n < buf.len() => {
                let out = &buf[..n];
                let out = match out.strip_prefix(&added[..]) {
                    Some(rest) if added == unc => [wide(r"\\"), rest.to_vec()].concat(),
                    Some(rest) if !added.is_empty() => rest.to_vec(),
                    _ => out.to_vec(),
                };
                return PathBuf::from(std::ffi::OsString::from_wide(&out));
            }
            // too small: `n` is the size needed (with the NUL)
            n => buf.resize(n, 0),
        }
    }
}

/// (volume serial, file index) from `GetFileInformationByHandle`.
pub fn file_identity(path: &Path) -> Option<(u32, u64)> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    // Metadata-only handle (no read access, works for directories too), closed on return.
    let f = std::fs::OpenOptions::new()
        .access_mode(0)
        .share_mode(7)
        .custom_flags(0x0200_0000) // FILE_FLAG_BACKUP_SEMANTICS
        .open(path)
        .ok()?;
    let mut info = windows::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: valid handle owned by `f` for the duration of the call.
    unsafe {
        windows::Win32::Storage::FileSystem::GetFileInformationByHandle(
            windows::Win32::Foundation::HANDLE(f.as_raw_handle()),
            &mut info,
        )
        .ok()?;
    }
    Some((
        info.dwVolumeSerialNumber,
        (info.nFileIndexHigh as u64) << 32 | info.nFileIndexLow as u64,
    ))
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub modified: SystemTime,
    pub created: SystemTime,
    pub size: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct ScanOptions {
    pub skip_hidden: bool,
    /// Also accept files with an unknown extension whose magic bytes are a supported format.
    pub mime_detection: bool,
}

impl From<&Settings> for ScanOptions {
    fn from(s: &Settings) -> Self {
        ScanOptions {
            skip_hidden: s.skip_hidden,
            mime_detection: s.allow_mime_content_detection,
        }
    }
}

fn sniffs_as_image(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 64];
    let n = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .unwrap_or(0);
    Format::sniff(&head[..n]).is_some()
}

/// Compatible files of `dir` (non-recursive). Metadata comes from the directory enumeration.
pub fn scan(dir: &Path, opts: ScanOptions) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.flatten()
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            if !md.is_file() || (opts.skip_hidden && md.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0) {
                return None;
            }
            let path = e.path();
            let apple_double = e.file_name().to_string_lossy().starts_with("._");
            let ok = is_supported_path(&path) || (opts.mime_detection && !apple_double && sniffs_as_image(&path));
            ok.then(|| Entry {
                modified: md.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                created: md.created().unwrap_or(SystemTime::UNIX_EPOCH),
                size: md.len(),
                path,
            })
        })
        .collect()
}

fn name(e: &Entry) -> &std::ffi::OsStr {
    e.path.file_name().unwrap_or_default()
}

fn ext(e: &Entry) -> &std::ffi::OsStr {
    e.path.extension().unwrap_or_default()
}

/// Sort per `SortMode`. "Ascending" dates/sizes put the newest/largest
/// first; `descending` reverses. `seed` is used only by `Random`.
pub fn sort(entries: &mut [Entry], mode: SortMode, descending: bool, seed: u64) {
    let by_name = |a: &Entry, b: &Entry| natural_cmp(name(a), name(b));
    match mode {
        SortMode::Name => entries.sort_by(by_name),
        SortMode::Modified => entries.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| by_name(a, b))),
        SortMode::Created => entries.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| by_name(a, b))),
        SortMode::Size => entries.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| by_name(a, b))),
        SortMode::Type => entries.sort_by(|a, b| natural_cmp(ext(a), ext(b)).then_with(|| by_name(a, b))),
        SortMode::Random => {
            entries.sort_by(by_name); // deterministic start, then a seeded Fisher–Yates
            let mut rng = Rng(seed | 1);
            for i in (1..entries.len()).rev() {
                entries.swap(i, (rng.next_u64() % (i as u64 + 1)) as usize);
            }
        }
    }
    if descending {
        entries.reverse();
    }
}

/// xorshift64*: enough for shuffles and "random file".
#[derive(Clone, Debug)]
pub struct Rng(pub u64);

impl Rng {
    pub fn from_entropy() -> Rng {
        Rng(std::hash::RandomState::new().build_hasher().finish() | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// Compatible files of `dir` in natural name order (folder given on the command line).
pub fn list(dir: &Path, skip_hidden: bool) -> Vec<PathBuf> {
    let mut e = scan(
        dir,
        ScanOptions {
            skip_hidden,
            mime_detection: false,
        },
    );
    sort(&mut e, SortMode::Name, false, 0);
    e.into_iter().map(|e| e.path).collect()
}

type SeedKey = (PathBuf, usize, SortMode, bool);

/// The sorted list of one folder plus the state that has to survive rescans.
#[derive(Clone, Debug)]
pub struct Folder {
    pub dir: PathBuf,
    pub files: Vec<PathBuf>,
    seed_key: SeedKey,
    seed: u64,
}

impl Folder {
    /// Scan + sort. The random seed is kept from `prev` while (folder, count, mode, direction) is
    /// unchanged, and renewed otherwise.
    pub fn load(dir: &Path, s: &Settings, prev: Option<&Folder>) -> Folder {
        let mut entries = scan(dir, s.into());
        let key = (dir.to_path_buf(), entries.len(), s.sort_mode, s.sort_descending);
        let seed = match prev {
            Some(p) if p.seed_key == key => p.seed,
            _ => Rng::from_entropy().next_u64(),
        };
        sort(&mut entries, s.sort_mode, s.sort_descending, seed);
        Folder {
            dir: dir.to_path_buf(),
            files: entries.into_iter().map(|e| e.path).collect(),
            seed_key: key,
            seed,
        }
    }

    /// Index of `path`: case-insensitive ordinal fast path confirmed by file identity; full identity
    /// scan only if the path is written differently (8.3 names, `\\?\`, relative, …).
    pub fn index_of(&self, path: &Path) -> Option<usize> {
        let id = file_identity(path);
        if let Some(i) = self.files.iter().position(|f| same_path_ordinal(f, path))
            && (id.is_none() || file_identity(&self.files[i]) == id)
        {
            return Some(i);
        }
        let id = id?;
        self.files.iter().position(|f| file_identity(f) == Some(id))
    }

    /// Ordinal fast path only: no I/O, usable on the UI thread.
    pub fn index_of_fast(&self, path: &Path) -> Option<usize> {
        self.files.iter().position(|f| same_path_ordinal(f, path))
    }

    pub fn remove(&mut self, path: &Path) {
        self.files.retain(|f| !same_path_ordinal(f, path));
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nav {
    First,
    Previous,
    Next,
    Last,
    Random,
}

impl Nav {
    /// Direction of travel: +1 forward, -1 backward (Random/First count as forward, Last as backward).
    pub fn direction(self) -> i8 {
        match self {
            Nav::Previous | Nav::Last => -1,
            _ => 1,
        }
    }
}

/// Target index, or `None` when nothing changes (empty list, edge without loop).
pub fn navigate(len: usize, cur: Option<usize>, nav: Nav, looping: bool, rng: &mut Rng) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let last = len - 1;
    let target = match (nav, cur) {
        (Nav::First, _) => 0,
        (Nav::Last, _) => last,
        (Nav::Next, None) => 0,
        (Nav::Previous, None) => last,
        (Nav::Next, Some(i)) if i >= last => {
            if looping {
                0
            } else {
                return None;
            }
        }
        (Nav::Next, Some(i)) => i + 1,
        (Nav::Previous, Some(0)) => {
            if looping {
                last
            } else {
                return None;
            }
        }
        (Nav::Previous, Some(i)) => i - 1,
        (Nav::Random, cur) => {
            if len == 1 {
                0
            } else {
                // any index but the current one
                let r = (rng.next_u64() % (len as u64 - cur.is_some() as u64)) as usize;
                match cur {
                    Some(c) if r >= c => r + 1,
                    _ => r,
                }
            }
        }
    };
    (Some(target) != cur).then_some(target)
}

/// File to show after the one at `deleted` was removed from a list that now has `len` files
/// (`afterdelete`). `None` = nothing (Do Nothing, or no file left): the window is emptied.
pub fn after_delete(len: usize, deleted: usize, mode: AfterDelete, looping: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match mode {
        AfterDelete::DoNothing => None,
        // the next file has moved into the deleted one's place
        AfterDelete::MoveForward if deleted < len => Some(deleted),
        AfterDelete::MoveForward => Some(if looping { 0 } else { len - 1 }),
        AfterDelete::MoveBack if deleted > 0 => Some(deleted - 1),
        AfterDelete::MoveBack => Some(if looping { len - 1 } else { 0 }),
    }
}

/// "Rescan only if ≥ 3 s since the last load".
pub fn should_rescan(last_load: Option<Instant>) -> bool {
    last_load.is_none_or(|t| t.elapsed() >= RESCAN_AFTER)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 8.3 form of an existing path (`GetShortPathNameW`), if the volume has 8.3 names.
    pub(crate) fn short_path(p: &Path) -> Option<PathBuf> {
        use std::os::windows::ffi::OsStringExt;
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
    fn long_path_expands_8dot3_names() {
        let d = std::env::temp_dir().join(format!("rview-long-path-check-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let long = long_path(&d);
        match short_path(&d) {
            Some(s) => {
                assert!(s.to_string_lossy().contains('~'), "{}", s.display());
                assert!(
                    same_path_ordinal(&long_path(&s), &long),
                    "{} → {}",
                    s.display(),
                    long_path(&s).display()
                );
                // above MAX_PATH, with the 8.3 component at the start
                let mut tail = PathBuf::new();
                while long.join(&tail).as_os_str().len() < 300 {
                    tail.push("a-rather-long-folder-name");
                }
                std::fs::create_dir_all(long.join(&tail)).unwrap();
                let deep = s.join(&tail);
                let back = long_path(&deep);
                assert!(
                    same_path_ordinal(&back, &long.join(&tail)),
                    "{} → {}",
                    deep.display(),
                    back.display()
                );
            }
            None => println!("SKIPPED: no 8.3 names on the temp folder's volume"),
        }
        let gone = d.join("missing").join("x.png");
        assert_eq!(long_path(&gone), gone, "a missing path is returned unchanged");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn after_delete_modes() {
        use AfterDelete::*;
        // 5 files, the one at 2 deleted → 4 left
        assert_eq!(after_delete(4, 2, MoveForward, true), Some(2));
        assert_eq!(after_delete(4, 2, MoveBack, true), Some(1));
        assert_eq!(after_delete(4, 2, DoNothing, true), None);
        // last one deleted
        assert_eq!(after_delete(4, 4, MoveForward, true), Some(0));
        assert_eq!(after_delete(4, 4, MoveForward, false), Some(3));
        // first one deleted
        assert_eq!(after_delete(4, 0, MoveBack, true), Some(3));
        assert_eq!(after_delete(4, 0, MoveBack, false), Some(0));
        // nothing left
        assert_eq!(after_delete(0, 0, MoveForward, true), None);
    }

    fn entry(name: &str, age_s: u64, size: u64) -> Entry {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 - age_s);
        Entry {
            path: PathBuf::from(format!("C:/d/{name}")),
            modified: t,
            created: t,
            size,
        }
    }

    fn names(e: &[Entry]) -> Vec<String> {
        e.iter()
            .map(|e| e.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["img10.png", "IMG2.png", "img1.png"];
        v.sort_by(|a, b| natural_cmp(a.as_ref(), b.as_ref()));
        assert_eq!(v, ["img1.png", "IMG2.png", "img10.png"]);
    }

    #[test]
    fn sort_modes_like_qt() {
        let base = vec![
            entry("b10.png", 30, 5),
            entry("b2.jpg", 10, 50),
            entry("a.png", 20, 500),
        ];
        let run = |m, d| {
            let mut e = base.clone();
            sort(&mut e, m, d, 7);
            names(&e)
        };
        assert_eq!(run(SortMode::Name, false), ["a.png", "b2.jpg", "b10.png"]);
        assert_eq!(run(SortMode::Name, true), ["b10.png", "b2.jpg", "a.png"]);
        // "ascending" = newest / largest first
        assert_eq!(run(SortMode::Modified, false), ["b2.jpg", "a.png", "b10.png"]);
        assert_eq!(run(SortMode::Created, true), ["b10.png", "a.png", "b2.jpg"]);
        assert_eq!(run(SortMode::Size, false), ["a.png", "b2.jpg", "b10.png"]);
        assert_eq!(run(SortMode::Type, false), ["b2.jpg", "a.png", "b10.png"]);
        // random: same seed → same order, a permutation of the input
        let r = run(SortMode::Random, false);
        assert_eq!(r, run(SortMode::Random, false));
        let mut sorted = r.clone();
        sorted.sort();
        assert_eq!(sorted, ["a.png", "b10.png", "b2.jpg"]);
    }

    #[test]
    fn navigation_with_and_without_loop() {
        let mut rng = Rng(42);
        assert_eq!(navigate(3, Some(2), Nav::Next, true, &mut rng), Some(0));
        assert_eq!(navigate(3, Some(2), Nav::Next, false, &mut rng), None);
        assert_eq!(navigate(3, Some(0), Nav::Previous, true, &mut rng), Some(2));
        assert_eq!(navigate(3, Some(0), Nav::Previous, false, &mut rng), None);
        assert_eq!(navigate(3, Some(1), Nav::First, false, &mut rng), Some(0));
        assert_eq!(navigate(3, Some(1), Nav::Last, false, &mut rng), Some(2));
        assert_eq!(navigate(3, Some(0), Nav::First, false, &mut rng), None);
        assert_eq!(navigate(0, None, Nav::Next, true, &mut rng), None);
        for _ in 0..100 {
            let r = navigate(5, Some(3), Nav::Random, true, &mut rng).unwrap();
            assert!(r < 5 && r != 3);
        }
        assert_eq!(navigate(1, Some(0), Nav::Random, true, &mut rng), None);
    }

    #[test]
    fn rescan_rule() {
        assert!(should_rescan(None));
        assert!(!should_rescan(Some(Instant::now())));
        assert!(should_rescan(Instant::now().checked_sub(Duration::from_secs(4))));
    }
}
