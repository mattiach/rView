//! Inline text editing: the text is laid out and drawn by
//! `core::edit::text` (cosmic-text + tiny-skia) at every change; this module only keeps the caret
//! and the selection (byte indices) and maps them to and from that same layout, so the caret sits
//! on the glyphs the export draws. egui paints the caret and the selection.

use rv_core::edit::text::Layout;

#[derive(Clone, Debug, PartialEq)]
pub struct TextEdit {
    pub id: rv_core::edit::ObjectId,
    /// The shape was created by this editing session (removed again if left empty).
    pub new: bool,
    /// Byte index of the caret.
    pub cursor: usize,
    /// Other end of the selection, if any.
    pub anchor: Option<usize>,
}

/// Previous / next char boundary.
fn prev(s: &str, i: usize) -> usize {
    s[..i].char_indices().next_back().map_or(0, |(j, _)| j)
}
fn next(s: &str, i: usize) -> usize {
    s[i..].chars().next().map_or(i, |c| i + c.len_utf8())
}

/// Editing commands (from keys).
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    Insert(String),
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    SelectAll,
}

impl TextEdit {
    pub fn selection(&self) -> Option<(usize, usize)> {
        self.anchor
            .filter(|a| *a != self.cursor)
            .map(|a| (a.min(self.cursor), a.max(self.cursor)))
    }

    /// Apply a command to `text` (laid out as `l`); `extend` = Shift held. Returns the new text.
    pub fn apply(&mut self, text: &str, l: &Layout, cmd: Cmd, extend: bool) -> String {
        let mut s = text.to_string();
        self.cursor = self.cursor.min(s.len());
        let moving = matches!(cmd, Cmd::Left | Cmd::Right | Cmd::Up | Cmd::Down | Cmd::Home | Cmd::End);
        if moving {
            if extend {
                self.anchor.get_or_insert(self.cursor);
            } else if let Some((a, b)) = self.selection() {
                // a plain arrow collapses the selection to its side
                self.anchor = None;
                match cmd {
                    Cmd::Left | Cmd::Up | Cmd::Home => self.cursor = a,
                    _ => self.cursor = b,
                }
                if matches!(cmd, Cmd::Left | Cmd::Right) {
                    return s;
                }
            } else {
                self.anchor = None;
            }
        }
        match cmd {
            Cmd::Insert(t) => {
                let (a, b) = self.selection().unwrap_or((self.cursor, self.cursor));
                s.replace_range(a..b, &t);
                self.cursor = a + t.len();
                self.anchor = None;
            }
            Cmd::Backspace | Cmd::Delete => {
                let (a, b) = match self.selection() {
                    Some(r) => r,
                    None if cmd == Cmd::Backspace => (prev(&s, self.cursor), self.cursor),
                    None => (self.cursor, next(&s, self.cursor)),
                };
                s.replace_range(a..b, "");
                self.cursor = a;
                self.anchor = None;
            }
            Cmd::Left => self.cursor = prev(&s, self.cursor),
            Cmd::Right => self.cursor = next(&s, self.cursor),
            Cmd::Home | Cmd::End => {
                if let Some((_, _, stops)) = line_of(l, self.cursor) {
                    let first = stops.first().map_or(0, |x| x.0);
                    let last = stops.last().map_or(s.len(), |x| x.0);
                    self.cursor = if cmd == Cmd::Home { first } else { last };
                }
            }
            Cmd::Up | Cmd::Down => {
                let (x, top, h) = caret(l, self.cursor);
                let y = if cmd == Cmd::Up { top - h / 2.0 } else { top + h * 1.5 };
                if y >= 0.0 && y <= l.height {
                    self.cursor = hit(l, x, y, s.len());
                }
            }
            Cmd::SelectAll => {
                self.anchor = Some(0);
                self.cursor = s.len();
            }
        }
        self.cursor = self.cursor.min(s.len());
        s
    }
}

/// The line holding byte `i` (the last one whose first glyph starts at or before it).
fn line_of(l: &Layout, i: usize) -> Option<&rv_core::edit::text::Line> {
    l.lines
        .iter()
        .rev()
        .find(|(_, _, stops)| stops.first().is_some_and(|s| s.0 <= i))
        .or(l.lines.first())
}

/// Caret of byte `i`: (x, line top, line height) in the text box.
pub fn caret(l: &Layout, i: usize) -> (f32, f32, f32) {
    let Some((top, h, stops)) = line_of(l, i) else {
        return (0.0, 0.0, l.height);
    };
    let x = stops
        .iter()
        .rev()
        .find(|(b, _)| *b <= i)
        .or(stops.first())
        .map_or(0.0, |s| s.1);
    (x, *top, *h)
}

/// Byte index nearest to the point (x, y) of the text box.
pub fn hit(l: &Layout, x: f32, y: f32, len: usize) -> usize {
    let Some((_, _, stops)) = l.lines.iter().find(|(top, h, _)| y < top + h).or(l.lines.last()) else {
        return len;
    };
    stops
        .iter()
        .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
        .map_or(len, |s| s.0)
        .min(len)
}

/// Selection rectangles (x0, top, x1, height) in the text box, one per line it covers.
pub fn selection_rects(l: &Layout, a: usize, b: usize) -> Vec<(f32, f32, f32, f32)> {
    l.lines
        .iter()
        .filter_map(|(top, h, stops)| {
            let (first, last) = (stops.first()?.0, stops.last()?.0);
            if b <= first || a >= last {
                return None;
            }
            let x_at = |i: usize| stops.iter().rev().find(|(s, _)| *s <= i).map_or(0.0, |s| s.1);
            let x0 = x_at(a.max(first));
            let x1 = if b >= last { stops.last()?.1 } else { x_at(b) };
            Some((x0.min(x1), *top, x0.max(x1), *h))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rv_core::edit::{FontSpec, TextAlign, text};

    fn lay(s: &str) -> std::sync::Arc<Layout> {
        let f = FontSpec {
            family: text::DEFAULT_FAMILY.into(),
            size: 20.0,
            weight: 400,
            italic: false,
        };
        text::layout(s, &f, 500.0, TextAlign::Left)
    }

    fn ed() -> TextEdit {
        TextEdit {
            id: rv_core::edit::ObjectId(1),
            new: true,
            cursor: 0,
            anchor: None,
        }
    }

    #[test]
    fn typing_moving_deleting() {
        let mut e = ed();
        let mut s = String::new();
        for c in ["H", "é", "llo"] {
            s = e.apply(&s, &lay(&s), Cmd::Insert(c.into()), false);
        }
        assert_eq!((s.as_str(), e.cursor), ("Héllo", 6));
        s = e.apply(&s, &lay(&s), Cmd::Left, false);
        s = e.apply(&s, &lay(&s), Cmd::Left, false);
        s = e.apply(&s, &lay(&s), Cmd::Left, false);
        s = e.apply(&s, &lay(&s), Cmd::Backspace, false);
        assert_eq!((s.as_str(), e.cursor), ("Hllo", 1), "a two-byte char removed whole");
        s = e.apply(&s, &lay(&s), Cmd::End, true);
        assert_eq!(e.selection(), Some((1, 4)));
        s = e.apply(&s, &lay(&s), Cmd::Insert("i".into()), false);
        assert_eq!(s, "Hi");
        s = e.apply(&s, &lay(&s), Cmd::Insert("\nsecond".into()), false);
        let l = lay(&s);
        assert_eq!(l.lines.len(), 2);
        s = e.apply(&s, &l, Cmd::Up, false);
        assert!(e.cursor <= 2, "up to the first line: {}", e.cursor);
        s = e.apply(&s, &l, Cmd::SelectAll, false);
        s = e.apply(&s, &l, Cmd::Delete, false);
        assert_eq!((s.as_str(), e.cursor), ("", 0));
    }

    #[test]
    fn caret_and_hit_agree() {
        let s = "Hello world";
        let l = lay(s);
        for i in [0, 1, 5, 6, 11] {
            let (x, top, h) = caret(&l, i);
            assert_eq!(hit(&l, x, top + h / 2.0, s.len()), i, "byte {i}");
        }
        let (x0, _, _) = caret(&l, 0);
        let (x5, _, _) = caret(&l, 5);
        assert!(x5 > x0 + 20.0);
        assert_eq!(selection_rects(&l, 0, 5).len(), 1);
    }
}
