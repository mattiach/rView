//! Rename dialog: file name with everything before the last dot selected.

use std::path::{Path, PathBuf};

pub struct Rename {
    pub path: PathBuf,
    pub text: String,
    /// Preselected range in chars.
    pub selection: (usize, usize),
    first: bool,
}

impl Rename {
    pub fn new(path: &Path) -> Rename {
        let text = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Rename {
            selection: (0, stem_chars(&text)),
            path: path.to_path_buf(),
            text,
            first: true,
        }
    }

    pub(super) fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        ui.label(crate::i18n::t("rename-label"));
        super::text_field(ui, &mut self.text, &mut self.first, self.selection)
    }
}

/// Chars up to the last dot ("photo.2024.jpg" → "photo.2024"); the whole name without one or
/// when the dot is the first char (".hidden").
pub fn stem_chars(name: &str) -> usize {
    match name.rfind('.') {
        Some(b) if b > 0 => name[..b].chars().count(),
        _ => name.chars().count(),
    }
}

/// Checks a new name typed by the user. Ok(None) = same name, nothing to do.
pub fn validate(old: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    use crate::i18n::{t, tf};
    if name.is_empty() {
        return Err(t("rename-empty"));
    }
    if let Some(c) = name.chars().find(|c| r#"<>:"/\|?*"#.contains(*c) || c.is_control()) {
        return Err(tf("rename-bad-char", &[("char", &format!("{c:?}"))]));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(t("rename-bad-end"));
    }
    let new = old.with_file_name(name);
    Ok((new.file_name() != old.file_name()).then_some(new))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preselection_and_validation() {
        assert_eq!(stem_chars("photo.jpg"), 5);
        assert_eq!(stem_chars("photo.2024.jpg"), 10);
        assert_eq!(stem_chars("città è.png"), 7, "chars, not bytes");
        assert_eq!(stem_chars("noext"), 5);
        assert_eq!(stem_chars(".hidden"), 7);
        let r = Rename::new(Path::new(r"C:\d\città è.png"));
        assert_eq!((r.text.as_str(), r.selection), ("città è.png", (0, 7)));

        let old = Path::new(r"C:\d\a.png");
        assert_eq!(validate(old, "b.png"), Ok(Some(PathBuf::from(r"C:\d\b.png"))));
        assert_eq!(validate(old, "a.png"), Ok(None));
        assert_eq!(
            validate(old, "A.png"),
            Ok(Some(PathBuf::from(r"C:\d\A.png"))),
            "case-only rename"
        );
        assert!(validate(old, "").is_err());
        assert!(validate(old, "a/b.png").is_err());
        assert!(validate(old, "a?.png").is_err());
        assert!(validate(old, "a.png.").is_err());
    }
}
