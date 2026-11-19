//! Actions: one static table with stable keys, Windows names, default shortcuts and
//! enable rules. Menus and shortcuts are built from it.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    Open,
    OpenUrl,
    ReloadFile,
    OpenContainingFolder,
    ShowFileInfo,
    Undo,
    Copy,
    Paste,
    Rename,
    Delete,
    DeletePermanent,
    FirstFile,
    PreviousFile,
    NextFile,
    LastFile,
    RandomFile,
    ZoomIn,
    ZoomOut,
    OriginalSize,
    RotateRight,
    RotateLeft,
    Mirror,
    Flip,
    FullScreen,
    SaveFrameAs,
    Pause,
    NextFrame,
    DecreaseSpeed,
    ResetSpeed,
    IncreaseSpeed,
    Slideshow,
    Options,
    Quit,
    CloseWindow,
    Welcome,
    ClearRecents,
    OpenWithOther,
    Markup,
    Crop,
    SaveProject,
    Export,
    OverwriteOriginal,
    Redo,
}

/// When an action can run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Enable {
    Always,
    NeedsImage,
    NeedsAnimation,
    NeedsFolder,
    NeedsUndo,
    /// Static image decoded without error (or a project).
    NeedsEditable,
    InMarkup,
    /// In Markup, with an original in an exportable format, not a project.
    Overwritable,
}

/// Menu text comes from the i18n files: message `action-<key>` (see `name`).
pub struct ActionInfo {
    /// Stable key, used for saved shortcuts and as menu id.
    pub key: &'static str,
    /// QKeySequence-style strings ("Ctrl+Shift+O").
    pub default_keys: &'static [&'static str],
    pub enable: Enable,
}

const fn a(key: &'static str, default_keys: &'static [&'static str], enable: Enable) -> ActionInfo {
    ActionInfo {
        key,
        default_keys,
        enable,
    }
}

/// i18n message id of the action's menu text.
pub fn message_id(a: Action) -> String {
    format!("action-{}", info(a).key)
}

/// Menu text in the current language (Windows names, `&` = mnemonic).
pub fn name(a: Action) -> String {
    crate::i18n::t(&message_id(a))
}

use Enable::*;

#[rustfmt::skip]
pub const ACTIONS: &[(Action, ActionInfo)] = &[
    (Action::Open,                 a("open",                 &["Ctrl+O"], Always)),
    (Action::OpenUrl,              a("openurl",              &["Ctrl+Shift+O"], Always)),
    (Action::ReloadFile,           a("reloadfile",           &["F5"], NeedsImage)),
    (Action::OpenContainingFolder, a("opencontainingfolder", &["<"], NeedsImage)),
    (Action::ShowFileInfo,         a("showfileinfo",         &["I"], NeedsImage)),
    (Action::Undo,                 a("undo",                 &["Ctrl+Z", "Alt+Backspace"], NeedsUndo)),
    (Action::Copy,                 a("copy",                 &["Ctrl+C", "Ctrl+Ins"], NeedsImage)),
    (Action::Paste,                a("paste",                &["Ctrl+V", "Shift+Ins"], Always)),
    (Action::Rename,               a("rename",               &["Ctrl+Enter", "F2", "Ctrl+R"], NeedsImage)),
    (Action::Delete,               a("delete",               &["Ctrl+Backspace", "Del"], NeedsImage)),
    (Action::DeletePermanent,      a("deletepermanent",      &["Shift+Del"], NeedsImage)),
    (Action::FirstFile,            a("firstfile",            &["Home"], NeedsFolder)),
    (Action::PreviousFile,         a("previousfile",         &["Left"], NeedsFolder)),
    (Action::NextFile,             a("nextfile",             &["Right"], NeedsFolder)),
    (Action::LastFile,             a("lastfile",             &["End"], NeedsFolder)),
    (Action::RandomFile,           a("randomfile",           &["R"], NeedsFolder)),
    (Action::ZoomIn,               a("zoomin",               &["Ctrl++", "Ctrl+="], NeedsImage)),
    (Action::ZoomOut,              a("zoomout",              &["Ctrl+-"], NeedsImage)),
    (Action::OriginalSize,         a("originalsize",         &["O"], NeedsImage)),
    (Action::RotateRight,          a("rotateright",          &["Up"], NeedsImage)),
    (Action::RotateLeft,           a("rotateleft",           &["Down"], NeedsImage)),
    (Action::Mirror,               a("mirror",               &["F"], NeedsImage)),
    (Action::Flip,                 a("flip",                 &["Ctrl+F"], NeedsImage)),
    // F11 and Alt+Enter: both stay on fullscreen (Alt+Enter must not reach Flip)
    (Action::FullScreen,           a("fullscreen",           &["F11", "Alt+Enter"], Always)),
    (Action::SaveFrameAs,          a("saveframeas",          &["Ctrl+S"], NeedsAnimation)),
    (Action::Pause,                a("pause",                &["P"], NeedsAnimation)),
    (Action::NextFrame,            a("nextframe",            &["N"], NeedsAnimation)),
    (Action::DecreaseSpeed,        a("decreasespeed",        &["["], NeedsAnimation)),
    (Action::ResetSpeed,           a("resetspeed",           &["\\"], NeedsAnimation)),
    (Action::IncreaseSpeed,        a("increasespeed",        &["]"], NeedsAnimation)),
    (Action::Slideshow,            a("slideshow",            &[], NeedsImage)),
    (Action::Options,              a("options",              &[], Always)),
    (Action::Quit,                 a("quit",                 &["Ctrl+Q"], Always)),
    (Action::CloseWindow,          a("closewindow",          &["Ctrl+W"], Always)),
    (Action::Welcome,              a("welcome",              &[], Always)),
    (Action::ClearRecents,         a("clearrecents",         &[], Always)),
    (Action::OpenWithOther,        a("openwithother",        &[], NeedsImage)),
    (Action::Markup,               a("markup",               &["Ctrl+Shift+A"], NeedsEditable)),
    (Action::Crop,                 a("crop",                 &["Ctrl+K"], NeedsEditable)),
    (Action::SaveProject,          a("saveproject",          &["Ctrl+S"], InMarkup)),
    (Action::Export,               a("export",               &["Ctrl+Shift+S"], InMarkup)),
    (Action::OverwriteOriginal,    a("overwriteoriginal",    &[], Overwritable)),
    (Action::Redo,                 a("redo",                 &["Ctrl+Y", "Ctrl+Shift+Z"], InMarkup)),
];

pub fn info(a: Action) -> &'static ActionInfo {
    &ACTIONS
        .iter()
        .find(|(x, _)| *x == a)
        .expect("every action is in ACTIONS")
        .1
}

pub fn from_key(key: &str) -> Option<Action> {
    ACTIONS.iter().find(|(_, i)| i.key == key).map(|&(a, _)| a)
}

/// Facts the enable rules look at, per window.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct EnableState {
    pub has_image: bool,
    pub animated: bool,
    pub has_folder: bool,
    pub has_undo: bool,
    pub editable: bool,
    pub in_markup: bool,
    pub overwritable: bool,
}

pub fn is_enabled(a: Action, s: &EnableState) -> bool {
    match info(a).enable {
        Always => true,
        NeedsImage => s.has_image,
        NeedsAnimation => s.animated,
        NeedsFolder => s.has_folder,
        NeedsUndo => s.has_undo,
        NeedsEditable => s.editable,
        InMarkup => s.in_markup,
        Overwritable => s.in_markup && s.overwritable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_consistent() {
        let mut keys: Vec<&str> = ACTIONS.iter().map(|(_, i)| i.key).collect();
        keys.sort();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n, "duplicate action key");
        for (a, i) in ACTIONS {
            assert_eq!(from_key(i.key), Some(*a));
        }
    }

    #[test]
    fn enable_rules() {
        let none = EnableState::default();
        let image = EnableState {
            has_image: true,
            editable: true,
            ..none
        };
        let anim = EnableState {
            has_image: true,
            animated: true,
            has_folder: true,
            ..none
        };
        assert!(is_enabled(Action::Open, &none) && is_enabled(Action::FullScreen, &none));
        assert!(is_enabled(Action::Quit, &none));
        assert!(!is_enabled(Action::ZoomIn, &none) && is_enabled(Action::ZoomIn, &image));
        assert!(!is_enabled(Action::Slideshow, &none) && is_enabled(Action::Slideshow, &image));
        assert!(!is_enabled(Action::NextFile, &image) && is_enabled(Action::NextFile, &anim));
        assert!(!is_enabled(Action::Pause, &image) && is_enabled(Action::Pause, &anim));
        assert!(!is_enabled(Action::SaveFrameAs, &image) && is_enabled(Action::SaveFrameAs, &anim));
        // file operations
        assert!(!is_enabled(Action::Rename, &none) && is_enabled(Action::Rename, &image));
        assert!(is_enabled(Action::Delete, &image) && is_enabled(Action::Copy, &image));
        assert!(is_enabled(Action::Paste, &none) && is_enabled(Action::OpenUrl, &none));
        assert!(!is_enabled(Action::Undo, &image));
        assert!(is_enabled(Action::Undo, &EnableState { has_undo: true, ..none }));
        // always available
        for a in [Action::Options, Action::Welcome] {
            assert!(is_enabled(a, &none), "{a:?}");
        }
        // Markup needs an editable image, Redo needs Markup
        assert!(is_enabled(Action::Markup, &image) && is_enabled(Action::Crop, &image));
        assert!(!is_enabled(Action::Markup, &anim), "not for animations");
        let markup = EnableState {
            in_markup: true,
            ..image
        };
        assert!(!is_enabled(Action::Redo, &image) && is_enabled(Action::Redo, &markup));
        // Save Project / Export in Markup; Overwrite Original only for exportable originals
        assert!(!is_enabled(Action::SaveProject, &image) && !is_enabled(Action::Export, &image));
        assert!(is_enabled(Action::SaveProject, &markup) && is_enabled(Action::Export, &markup));
        assert!(!is_enabled(Action::OverwriteOriginal, &markup));
        let jpeg = EnableState {
            overwritable: true,
            ..markup
        };
        assert!(is_enabled(Action::OverwriteOriginal, &jpeg));
    }
}
