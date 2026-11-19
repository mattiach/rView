//! Native Win32 menus via muda. One set per window, because the enabled state
//! depends on the window's image. Menu ids are action keys; shortcuts are only shown as text
//! ("\t" + key), key handling lives in `shortcuts.rs`.

use crate::actions::{self, Action, EnableState};
use crate::i18n;
use crate::shortcuts::Shortcuts;
use muda::{ContextMenu, IconMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

/// Submenus whose first entries change at run time.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dynamic {
    /// Open Recent: ids "recent:<i>".
    Recent,
    /// Open With: ids "openwith:<i>".
    OpenWith,
}

enum Entry {
    A(Action),
    Sep,
    Sub(&'static str, &'static [Entry]),
    Dyn(Dynamic, &'static str, &'static [Entry]),
}
use Entry::{A, Dyn, Sep, Sub};

const RECENT: Entry = Dyn(Dynamic::Recent, "menu-recent", &[A(Action::ClearRecents)]);
const OPEN_WITH: Entry = Dyn(Dynamic::OpenWith, "menu-openwith", &[A(Action::OpenWithOther)]);

/// Menu id of the i-th recent file / Open With app, and back.
pub fn recent_id(i: usize) -> String {
    format!("recent:{i}")
}
pub fn open_with_id(i: usize) -> String {
    format!("openwith:{i}")
}
pub fn parse_dynamic(id: &str) -> Option<(bool, usize)> {
    if let Some(i) = id.strip_prefix("recent:") {
        return Some((true, i.parse().ok()?));
    }
    Some((false, id.strip_prefix("openwith:")?.parse().ok()?))
}

/// Items added at the top of a dynamic submenu (removed on the next update).
#[derive(Default)]
struct DynItems {
    items: Vec<IconMenuItem>,
    sep: Option<PredefinedMenuItem>,
    texts: Vec<String>,
}

#[rustfmt::skip]
const VIEW: &[Entry] = &[
    A(Action::ZoomIn), A(Action::ZoomOut), A(Action::OriginalSize), Sep,
    A(Action::RotateRight), A(Action::RotateLeft), Sep, A(Action::Mirror), A(Action::Flip), Sep,
    A(Action::FullScreen),
];
/// Commands for animated images (GIF, APNG, animated WebP/JXL), grouped so their purpose shows.
#[rustfmt::skip]
const ANIMATION: &[Entry] = &[
    A(Action::Pause), A(Action::PreviousFrame), A(Action::NextFrame), A(Action::SaveFrameAs), Sep,
    A(Action::DecreaseSpeed), A(Action::ResetSpeed), A(Action::IncreaseSpeed),
];
const TOOLS: &[Entry] = &[
    Sub("menu-animation", ANIMATION),
    A(Action::Slideshow),
    Sep,
    A(Action::ColorPicker),
];
const HELP: &[Entry] = &[A(Action::Welcome)];

#[rustfmt::skip]
/// The menu bar: submenus, and Settings as a command of its own.
const BAR: &[Entry] = &[
    Sub("menu-file", &[
        A(Action::Open), A(Action::OpenUrl), RECENT, A(Action::ReloadFile), Sep,
        OPEN_WITH, A(Action::OpenContainingFolder), A(Action::ShowFileInfo), Sep, A(Action::CloseWindow), A(Action::Quit),
    ]),
    Sub("menu-edit", &[
        A(Action::Undo), A(Action::Redo), Sep, A(Action::Copy), A(Action::Paste), A(Action::Rename), Sep,
        A(Action::Delete), A(Action::DeletePermanent),
    ]),
    Sub("menu-view", VIEW),
    Sub("menu-go", &[
        A(Action::FirstFile), A(Action::PreviousFile), A(Action::NextFile), A(Action::LastFile), A(Action::RandomFile),
    ]),
    Sub("menu-markup", &[
        A(Action::Markup), A(Action::Crop), Sep,
        A(Action::SaveProject), A(Action::Export), A(Action::OverwriteOriginal),
    ]),
    Sub("menu-tools", TOOLS),
    A(Action::Options),
    Sub("menu-help", HELP),
];

#[rustfmt::skip]
const CONTEXT: &[Entry] = &[
    A(Action::Open), A(Action::OpenUrl), RECENT, OPEN_WITH, A(Action::OpenContainingFolder),
    A(Action::ShowFileInfo), Sep, A(Action::Markup), A(Action::Rename), A(Action::Delete), Sep,
    A(Action::NextFile), A(Action::PreviousFile), Sep,
    Sub("menu-view", VIEW), Sub("menu-tools", TOOLS), A(Action::Options), Sub("menu-help", HELP),
];

/// Texts that depend on state.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Toggles {
    pub fullscreen: bool,
    pub slideshow: bool,
    pub paused: bool,
    /// The last Undo Delete entry is the backup of an Overwrite Original.
    pub restore: bool,
}

pub struct Menus {
    pub bar: Menu,
    pub context: Submenu,
    items: Vec<(Action, MenuItem)>,
    /// (kind, submenu, current dynamic items): each dynamic submenu of the bar and the context menu.
    dynamic: Vec<(Dynamic, Submenu, DynItems)>,
    applied: Option<(EnableState, Toggles)>,
    shortcuts: Vec<Option<String>>,
    /// Full texts last written (muda's `text()` getter drops the "\tshortcut" part).
    texts: Vec<String>,
}

fn label(a: Action, t: &Toggles) -> String {
    match a {
        Action::FullScreen if t.fullscreen => i18n::t("action-fullscreen-exit"),
        Action::Slideshow if t.slideshow => i18n::t("action-slideshow-stop"),
        Action::Pause if t.paused => i18n::t("action-pause-resume"),
        Action::Undo if t.restore => i18n::t("action-undo-restore"),
        _ => actions::name(a),
    }
}

impl Menus {
    pub fn build(shortcuts: &Shortcuts) -> Menus {
        let (mut items, mut dynamic) = (Vec::new(), Vec::new());
        let bar = Menu::new();
        for e in BAR {
            match e {
                Sub(name, entries) => {
                    let _ = bar.append(&submenu(name, entries, &mut items, &mut dynamic));
                }
                A(a) => {
                    let _ = bar.append(&item(*a, &mut items));
                }
                _ => {}
            }
        }
        let context = submenu("", CONTEXT, &mut items, &mut dynamic);
        let keys = items.iter().map(|(a, _)| shortcuts.keys(*a).first().cloned()).collect();
        let texts = vec![String::new(); items.len()];
        let mut m = Menus {
            bar,
            context,
            items,
            dynamic,
            applied: None,
            shortcuts: keys,
            texts,
        };
        m.update(&EnableState::default(), &Toggles::default());
        m
    }

    /// Apply enabled state and dynamic texts (only when they changed).
    pub fn update(&mut self, state: &EnableState, t: &Toggles) {
        if self.applied == Some((*state, *t)) {
            return;
        }
        for (((a, item), key), written) in self.items.iter().zip(&self.shortcuts).zip(&mut self.texts) {
            item.set_enabled(actions::is_enabled(*a, state));
            let text = label(*a, t);
            *written = match key {
                Some(k) => format!("{text}\t{k}"),
                None => text.to_string(),
            };
            item.set_text(written.as_str());
        }
        self.applied = Some((*state, *t));
    }

    /// Attach the menu bar to a window, shown only if `visible` (MenuBarEnabled, not fullscreen).
    pub fn attach(&self, hwnd: isize, visible: bool) {
        // SAFETY: hwnd is a live top-level window owned by this thread.
        unsafe {
            if let Err(e) = self.bar.init_for_hwnd_with_theme(hwnd, muda::MenuTheme::Auto) {
                log::warn!("menu bar: {e}");
            }
            if !visible {
                let _ = self.bar.hide_for_hwnd(hwnd);
            }
        }
    }

    /// Open Recent entries (file names), most recent first.
    pub fn set_recents(&mut self, names: &[String]) {
        let entries: Vec<(String, Option<muda::Icon>)> = names.iter().map(|n| (n.clone(), None)).collect();
        self.set_dynamic(Dynamic::Recent, &entries, recent_id);
    }

    /// Open With entries (app names, icons), default app first.
    pub fn set_open_with(&mut self, apps: &[(String, Option<muda::Icon>)]) {
        self.set_dynamic(Dynamic::OpenWith, apps, open_with_id);
    }

    fn set_dynamic(&mut self, kind: Dynamic, entries: &[(String, Option<muda::Icon>)], id: fn(usize) -> String) {
        for (k, sub, d) in &mut self.dynamic {
            if *k != kind {
                continue;
            }
            let texts: Vec<String> = entries.iter().map(|(t, _)| t.clone()).collect();
            if d.texts == texts && kind == Dynamic::Recent {
                continue;
            }
            for item in d.items.drain(..) {
                let _ = sub.remove(&item);
            }
            if let Some(sep) = d.sep.take() {
                let _ = sub.remove(&sep);
            }
            for (i, (text, icon)) in entries.iter().enumerate() {
                // '&' would become a mnemonic: file and app names are shown as they are
                let item = IconMenuItem::with_id(id(i), text.replace('&', "&&"), true, icon.clone(), None);
                let _ = sub.insert(&item, i);
                d.items.push(item);
            }
            if !entries.is_empty() {
                let sep = PredefinedMenuItem::separator();
                let _ = sub.insert(&sep, entries.len());
                d.sep = Some(sep);
            }
            d.texts = texts;
        }
    }

    /// Texts of the dynamic entries of the menu bar: (Open Recent, Open With), for tests.
    pub fn dynamic_texts(&self) -> (Vec<String>, Vec<String>) {
        let find = |kind| {
            self.dynamic
                .iter()
                .find(|(k, _, _)| *k == kind)
                .map(|(_, _, d)| d.texts.clone())
                .unwrap_or_default()
        };
        (find(Dynamic::Recent), find(Dynamic::OpenWith))
    }

    /// MenuBarEnabled changed (Settings).
    pub fn set_bar_visible(&self, hwnd: isize, visible: bool) {
        // SAFETY: hwnd is a live top-level window owned by this thread, the bar is attached to it.
        unsafe {
            let _ = if visible {
                self.bar.show_for_hwnd(hwnd)
            } else {
                self.bar.hide_for_hwnd(hwnd)
            };
        }
    }

    /// Before replacing this window's menus (language or shortcuts changed).
    pub fn detach(&self, hwnd: isize) {
        // SAFETY: as above.
        unsafe {
            let _ = self.bar.remove_for_hwnd(hwnd);
        }
    }

    pub fn show_context(&self, hwnd: isize, x: f64, y: f64) {
        // SAFETY: as above; blocks in the native menu loop until dismissed.
        unsafe {
            self.context
                .show_context_menu_for_hwnd(hwnd, Some(muda::dpi::PhysicalPosition::new(x, y).into()));
        }
    }

    /// (action, enabled as read back from the native menu, full text) of every item.
    pub fn snapshot(&self) -> Vec<(Action, bool, String)> {
        self.items
            .iter()
            .zip(&self.texts)
            .map(|((a, i), t)| (*a, i.is_enabled(), t.clone()))
            .collect()
    }
}

fn item(a: Action, items: &mut Vec<(Action, MenuItem)>) -> MenuItem {
    let item = MenuItem::with_id(actions::info(a).key, actions::name(a), true, None);
    items.push((a, item.clone()));
    item
}

fn submenu(
    name: &str,
    entries: &[Entry],
    items: &mut Vec<(Action, MenuItem)>,
    dynamic: &mut Vec<(Dynamic, Submenu, DynItems)>,
) -> Submenu {
    // the context menu itself has no title
    let title = if name.is_empty() { String::new() } else { i18n::t(name) };
    let s = Submenu::new(title, true);
    for e in entries {
        match e {
            A(a) => {
                let _ = s.append(&item(*a, items));
            }
            Sep => {
                let _ = s.append(&PredefinedMenuItem::separator());
            }
            Sub(n, inner) => {
                let _ = s.append(&submenu(n, inner, items, dynamic));
            }
            Dyn(kind, n, inner) => {
                let sub = submenu(n, inner, items, dynamic);
                let _ = s.append(&sub);
                dynamic.push((*kind, sub, DynItems::default()));
            }
        }
    }
    s
}
