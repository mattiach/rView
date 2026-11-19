//! Persistent state, separate from settings: `%APPDATA%\rView\state.toml`.
//! Window geometry, recent files, firstlaunch, configversion, lastupdatecheck, shortcuts.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Window placement as saved by `GetWindowPlacement` (normal rectangle + maximized flag).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Geometry {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub maximized: bool,
}

/// One entry of Open Recent: menu text + path.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Recent {
    pub name: String,
    pub path: PathBuf,
}

/// Entries kept in Open Recent.
pub const MAX_RECENTS: usize = 10;

/// Version of the settings/state format written by this build.
pub const CONFIG_VERSION: u32 = 1;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub geometry: Option<Geometry>,
    /// Most recent first (`recents.recentFiles`).
    pub recents: Vec<Recent>,
    /// The Welcome window has not been shown yet.
    #[serde(rename = "firstlaunch")]
    pub first_launch: bool,
    #[serde(rename = "configversion")]
    pub config_version: u32,
    /// Last update check, seconds since the Unix epoch (at most every 4 h).
    #[serde(rename = "lastupdatecheck")]
    pub last_update_check: i64,
    /// Folder the Open dialog starts in (`recents.lastFileDialogDir`).
    #[serde(rename = "lastfiledialogdir")]
    pub last_file_dialog_dir: Option<PathBuf>,
    /// Custom shortcuts: action key → key sequences, replacing that action's defaults.
    pub shortcuts: std::collections::BTreeMap<String, Vec<String>>,
}

impl Default for State {
    fn default() -> Self {
        State {
            geometry: None,
            recents: Vec::new(),
            first_launch: true,
            config_version: CONFIG_VERSION,
            last_update_check: 0,
            last_file_dialog_dir: None,
            shortcuts: Default::default(),
        }
    }
}

impl State {
    /// `path` becomes the first entry (moved up if already present; same file = ordinal
    /// case-insensitive path), at most MAX_RECENTS. Returns true if the list changed.
    pub fn add_recent(&mut self, path: &Path) -> bool {
        let same = |r: &Recent| crate::folder::same_path_ordinal(&r.path, path);
        if self.recents.first().is_some_and(same) {
            return false;
        }
        self.recents.retain(|r| !same(r));
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        self.recents.insert(
            0,
            Recent {
                name,
                path: path.to_path_buf(),
            },
        );
        self.recents.truncate(MAX_RECENTS);
        true
    }

    /// Entries whose file no longer exists are dropped. Returns true if the list changed.
    /// One `exists()` per entry: call it off the UI thread (a network share can take seconds).
    pub fn prune_recents(&mut self) -> bool {
        let missing: Vec<PathBuf> = self
            .recents
            .iter()
            .filter(|r| !r.path.exists())
            .map(|r| r.path.clone())
            .collect();
        self.forget_recents(&missing)
    }

    /// Drop these paths from the recents. Returns true if the list changed.
    pub fn forget_recents(&mut self, paths: &[PathBuf]) -> bool {
        let n = self.recents.len();
        self.recents
            .retain(|r| !paths.iter().any(|p| crate::folder::same_path_ordinal(p, &r.path)));
        self.recents.len() != n
    }
}

/// `%APPDATA%\rView\state.toml` (Roaming AppData via `SHGetKnownFolderPath`).
pub fn default_path() -> Option<PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    // SAFETY: the returned PWSTR is freed with CoTaskMemFree after being copied.
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, None).ok()?;
        let dir = PathBuf::from(p.to_string().ok()?);
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        Some(dir.join("rView").join("state.toml"))
    }
}

/// Missing or unreadable file → defaults (never fails: state is a convenience).
pub fn load(path: &Path) -> State {
    match std::fs::read_to_string(path) {
        Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
            log::warn!("{}: {e}; using defaults", path.display());
            State::default()
        }),
        Err(_) => State::default(),
    }
}

/// Atomic write (no decoder verification), creating the folder if needed.
pub fn save(path: &Path, state: &State) -> Result<(), crate::atomic::SaveError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = toml::to_string(state).map_err(std::io::Error::other)?;
    crate::atomic::write_atomic(path, text.as_bytes(), |b| {
        toml::from_str::<State>(&String::from_utf8_lossy(b)).is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_tolerance() {
        let dir = std::env::temp_dir().join(format!("rview-state-{}", std::process::id()));
        let p = dir.join("sub").join("state.toml");
        assert_eq!(load(&p), State::default(), "missing file → defaults");
        let s = State {
            geometry: Some(Geometry {
                left: -10,
                top: 20,
                right: 1000,
                bottom: 800,
                maximized: true,
            }),
            recents: vec![Recent {
                name: "foto è.jpg".into(),
                path: PathBuf::from(r"C:\x\foto è.jpg"),
            }],
            first_launch: false,
            config_version: CONFIG_VERSION,
            last_update_check: 1_763_000_000,
            last_file_dialog_dir: Some(PathBuf::from(r"C:\Foto")),
            shortcuts: [("nextfile".to_string(), vec!["Space".to_string()])].into(),
        };
        save(&p, &s).unwrap();
        assert_eq!(load(&p), s);
        // unknown keys (newer versions) and missing keys are tolerated
        std::fs::write(&p, "future_key = 3\n").unwrap();
        assert_eq!(load(&p), State::default());
        std::fs::write(&p, "not toml [[[").unwrap();
        assert_eq!(load(&p), State::default());
        let _ = std::fs::remove_dir_all(&dir);
        assert!(default_path().is_some_and(|p| p.ends_with("rView\\state.toml")));
    }

    #[test]
    fn recents_order_dedup_limit_prune() {
        let dir = std::env::temp_dir().join(format!("rview-recents-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = State::default();
        let files: Vec<PathBuf> = (0..12).map(|i| dir.join(format!("f{i}.png"))).collect();
        for f in &files {
            std::fs::write(f, b"x").unwrap();
            assert!(s.add_recent(f));
        }
        assert_eq!(s.recents.len(), MAX_RECENTS);
        assert_eq!(s.recents[0].name, "f11.png");
        assert_eq!(s.recents[9].name, "f2.png", "oldest dropped");
        // the same file in another case: moved to the top, not duplicated
        let upper = PathBuf::from(files[5].to_string_lossy().to_uppercase());
        assert!(s.add_recent(&upper));
        assert_eq!(s.recents.len(), MAX_RECENTS);
        assert!(crate::folder::same_path_ordinal(&s.recents[0].path, &files[5]));
        assert!(!s.add_recent(&files[5]), "already first: no change");
        std::fs::remove_file(&files[11]).unwrap();
        assert!(s.prune_recents());
        assert!(s.recents.iter().all(|r| r.name != "f11.png"));
        assert!(!s.prune_recents());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
