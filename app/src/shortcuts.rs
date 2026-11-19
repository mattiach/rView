//! Shortcuts: defaults from the action table plus per-action custom lists (saved
//! by action key). Key strings look like "Ctrl+Shift+O".

use crate::actions::{self, ACTIONS, Action, EnableState};
use std::collections::HashMap;

/// Canonical form of a key sequence: modifiers in Ctrl, Alt, Shift order, canonical key names.
pub fn normalize(s: &str) -> Option<String> {
    let mut rest = s.trim();
    let (mut ctrl, mut alt, mut shift) = (false, false, false);
    loop {
        let lower = rest.to_ascii_lowercase();
        let take = |p: &str| lower.starts_with(p) && rest.len() > p.len();
        if take("ctrl+") {
            ctrl = true;
            rest = &rest[5..];
        } else if take("alt+") {
            alt = true;
            rest = &rest[4..];
        } else if take("shift+") {
            shift = true;
            rest = &rest[6..];
        } else {
            break;
        }
    }
    let key = canonical_key(rest)?;
    let mut out = String::new();
    for (on, m) in [(ctrl, "Ctrl+"), (alt, "Alt+"), (shift, "Shift+")] {
        if on {
            out.push_str(m);
        }
    }
    out.push_str(&key);
    Some(out)
}

fn canonical_key(k: &str) -> Option<String> {
    const NAMED: &[(&[&str], &str)] = &[
        (&["up"], "Up"),
        (&["down"], "Down"),
        (&["left"], "Left"),
        (&["right"], "Right"),
        (&["enter", "return"], "Enter"),
        (&["esc", "escape"], "Esc"),
        (&["del", "delete"], "Del"),
        (&["ins", "insert"], "Ins"),
        (&["backspace"], "Backspace"),
        (&["home"], "Home"),
        (&["end"], "End"),
        (&["pgup", "pageup"], "PgUp"),
        (&["pgdown", "pagedown"], "PgDown"),
        (&["tab"], "Tab"),
        (&["space"], "Space"),
    ];
    let lower = k.to_ascii_lowercase();
    if let Some((_, name)) = NAMED.iter().find(|(aliases, _)| aliases.contains(&lower.as_str())) {
        return Some(name.to_string());
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok())
        && (1..=24).contains(&n)
    {
        return Some(format!("F{n}"));
    }
    let mut chars = k.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if !c.is_control() && !c.is_whitespace() => Some(c.to_uppercase().collect()),
        _ => None,
    }
}

pub struct Shortcuts {
    by_seq: HashMap<String, Vec<Action>>,
    by_action: HashMap<Action, Vec<String>>,
}

impl Shortcuts {
    /// `custom`: action key → sequences, replacing that action's defaults (empty list = no shortcut).
    pub fn new(custom: &HashMap<String, Vec<String>>) -> Shortcuts {
        let mut by_seq: HashMap<String, Vec<Action>> = HashMap::new();
        let mut by_action = HashMap::new();
        for (a, info) in ACTIONS {
            let keys: Vec<String> = match custom.get(info.key) {
                Some(list) => list.iter().filter_map(|s| normalize(s)).collect(),
                None => info.default_keys.iter().filter_map(|s| normalize(s)).collect(),
            };
            for k in &keys {
                by_seq.entry(k.clone()).or_default().push(*a);
            }
            by_action.insert(*a, keys);
        }
        Shortcuts { by_seq, by_action }
    }

    /// Action for a pressed sequence. A sequence shared by several actions (Ctrl+S: Save Frame As
    /// outside Markup, Save Project inside) goes to the one that is enabled now.
    pub fn lookup(&self, seq: &str, state: &EnableState) -> Option<Action> {
        let list = self.by_seq.get(&normalize(seq)?)?;
        list.iter().copied().find(|a| actions::is_enabled(*a, state))
    }

    /// Bound at all (enabled or not): Esc leaves fullscreen only when no action uses it.
    pub fn is_bound(&self, seq: &str) -> bool {
        normalize(seq).is_some_and(|s| self.by_seq.contains_key(&s))
    }

    pub fn keys(&self, a: Action) -> &[String] {
        self.by_action.get(&a).map_or(&[], |v| v.as_slice())
    }
}

/// winit key event → QKeySequence-style string, or None for bare modifiers.
pub fn key_sequence(event: &winit::event::KeyEvent, mods: winit::keyboard::ModifiersState) -> Option<String> {
    use winit::keyboard::{Key, NamedKey};
    use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
    let mut shift = mods.shift_key();
    let name = match &event.logical_key {
        Key::Named(n) => match n {
            NamedKey::ArrowUp => "Up".into(),
            NamedKey::ArrowDown => "Down".into(),
            NamedKey::ArrowLeft => "Left".into(),
            NamedKey::ArrowRight => "Right".into(),
            NamedKey::Enter => "Enter".into(),
            NamedKey::Escape => "Esc".into(),
            NamedKey::Delete => "Del".into(),
            NamedKey::Insert => "Ins".into(),
            NamedKey::Backspace => "Backspace".into(),
            NamedKey::Home => "Home".into(),
            NamedKey::End => "End".into(),
            NamedKey::PageUp => "PgUp".into(),
            NamedKey::PageDown => "PgDown".into(),
            NamedKey::Tab => "Tab".into(),
            NamedKey::Space => "Space".into(),
            NamedKey::Control | NamedKey::Shift | NamedKey::Alt | NamedKey::Super => return None,
            other => format!("{other:?}"), // F1…F24 → "F11"
        },
        Key::Character(c) => {
            // With Ctrl held Windows may report control characters: use the unmodified key.
            let c = if c.chars().all(|ch| ch.is_control()) {
                match event.key_without_modifiers() {
                    Key::Character(c) => c.to_string(),
                    _ => return None,
                }
            } else {
                c.to_string()
            };
            // Shift that produced a symbol ("+" from Shift+=) is not part of the sequence.
            if !c.chars().any(|ch| ch.is_alphabetic()) {
                shift = false;
            }
            c.to_uppercase()
        }
        _ => return None,
    };
    let mut s = String::new();
    if mods.control_key() {
        s.push_str("Ctrl+");
    }
    if mods.alt_key() {
        s.push_str("Alt+");
    }
    if shift {
        s.push_str("Shift+");
    }
    s.push_str(&name);
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization() {
        assert_eq!(normalize("ctrl+shift+o").as_deref(), Some("Ctrl+Shift+O"));
        assert_eq!(normalize("Shift+Ctrl+O").as_deref(), Some("Ctrl+Shift+O"));
        assert_eq!(normalize("Ctrl++").as_deref(), Some("Ctrl++"));
        assert_eq!(normalize("Alt+Return").as_deref(), Some("Alt+Enter"));
        assert_eq!(normalize("Delete").as_deref(), Some("Del"));
        assert_eq!(normalize("f11").as_deref(), Some("F11"));
        assert_eq!(normalize("\\").as_deref(), Some("\\"));
        assert_eq!(normalize("Ctrl+"), None);
        assert_eq!(normalize("F99"), None);
        assert_eq!(normalize("Bogus"), None);
    }

    #[test]
    fn defaults_conflicts_and_custom() {
        let anim = EnableState {
            has_image: true,
            animated: true,
            has_folder: true,
            ..Default::default()
        };
        let s = Shortcuts::new(&HashMap::new());
        assert_eq!(s.lookup("Ctrl+=", &anim), Some(Action::ZoomIn));
        assert_eq!(s.lookup("ctrl++", &anim), Some(Action::ZoomIn));
        // both fullscreen shortcuts (Alt+Enter must not reach Flip)
        assert_eq!(s.lookup("F11", &anim), Some(Action::FullScreen));
        assert_eq!(s.lookup("Alt+Enter", &anim), Some(Action::FullScreen));
        assert_eq!(s.lookup("Ctrl+F", &anim), Some(Action::Flip));
        // Ctrl+S outside Markup = Save Frame As (Save Project needs Markup)
        assert_eq!(s.lookup("Ctrl+S", &anim), Some(Action::SaveFrameAs));
        assert_eq!(s.lookup("Ctrl+S", &EnableState::default()), None);
        assert!(!s.is_bound("Esc"));
        assert_eq!(s.keys(Action::Rename), ["Ctrl+Enter", "F2", "Ctrl+R"]);

        let custom = HashMap::from([
            (
                "nextfile".to_string(),
                vec!["Space".to_string(), "pagedown".to_string()],
            ),
            ("zoomin".to_string(), vec![]),
        ]);
        let s = Shortcuts::new(&custom);
        assert_eq!(s.lookup("Space", &anim), Some(Action::NextFile));
        assert_eq!(s.lookup("PgDown", &anim), Some(Action::NextFile));
        assert_eq!(s.lookup("Right", &anim), None, "custom list replaces the defaults");
        assert_eq!(s.lookup("Ctrl+=", &anim), None);
    }
}
