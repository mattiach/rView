//! Text shapes: cosmic-text shapes and wraps the text, skrifa gives the glyph
//! outlines, which become tiny-skia paths — so preview and export draw text exactly like the other
//! shapes (same paths, same rasteriser). Lato (official, OFL, `assets/fonts`) is embedded and is
//! the default; system fonts are loaded once, the first time they are needed.

use super::model::{FontSpec, RectF, TextAlign};
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, Weight, fontdb};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use tiny_skia::{Path, PathBuilder, Transform};

pub const DEFAULT_FAMILY: &str = "Lato";
/// Line height as a multiple of the font size.
const LINE_HEIGHT: f32 = 1.25;

const LATO: [&[u8]; 4] = [
    include_bytes!("../../../assets/fonts/Lato-Regular.ttf"),
    include_bytes!("../../../assets/fonts/Lato-Bold.ttf"),
    include_bytes!("../../../assets/fonts/Lato-Italic.ttf"),
    include_bytes!("../../../assets/fonts/Lato-BoldItalic.ttf"),
];

struct Fonts {
    sys: FontSystem,
    system_loaded: bool,
    /// (text, font, width, align) → layout cache.
    cache: HashMap<String, Arc<Layout>>,
}

fn fonts() -> MutexGuard<'static, Fonts> {
    static F: OnceLock<Mutex<Fonts>> = OnceLock::new();
    F.get_or_init(|| {
        let mut db = fontdb::Database::new();
        for data in LATO {
            db.load_font_data(data.to_vec());
        }
        db.set_sans_serif_family(DEFAULT_FAMILY);
        Mutex::new(Fonts {
            sys: FontSystem::new_with_locale_and_db("en-US".into(), db),
            system_loaded: false,
            cache: HashMap::new(),
        })
    })
    .lock()
    .unwrap_or_else(|e| e.into_inner())
}

/// Load the installed fonts (first use of the Text tool, or a project naming another family).
/// A few hundred ms once; Lato stays the default.
pub fn load_system_fonts() {
    let mut f = fonts();
    if !f.system_loaded {
        let t = std::time::Instant::now();
        f.sys.db_mut().load_system_fonts();
        f.sys.db_mut().set_sans_serif_family(DEFAULT_FAMILY);
        f.system_loaded = true;
        f.cache.clear();
        log::debug!("system fonts loaded in {:?}", t.elapsed());
    }
}

/// Families that can be chosen (Lato first, then the installed ones once loaded), sorted.
pub fn families() -> Vec<String> {
    let f = fonts();
    let mut v: Vec<String> = f
        .sys
        .db()
        .faces()
        .filter_map(|face| face.families.first().map(|(n, _)| n.clone()))
        .filter(|n| n != DEFAULT_FAMILY)
        .collect();
    v.sort_by_key(|n| n.to_lowercase());
    v.dedup();
    v.insert(0, DEFAULT_FAMILY.into());
    v
}

/// The family is available (else the text falls back to Lato).
pub fn has_family(name: &str) -> bool {
    if name == DEFAULT_FAMILY {
        return true;
    }
    load_system_fonts();
    let f = fonts();
    f.sys
        .db()
        .faces()
        .any(|face| face.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))
}

/// One positioned glyph: font face, glyph, baseline origin in the text box (px), size.
#[derive(Clone, Debug)]
struct Glyph {
    face: fontdb::ID,
    id: u16,
    x: f32,
    y: f32,
    size: f32,
}

/// A line: (top, height, [(byte index, x)] of its glyph starts and its end).
pub type Line = (f32, f32, Vec<(usize, f32)>);

/// A laid-out text: glyphs relative to the box's top-left, and the lines' extents (caret, hit).
#[derive(Debug)]
pub struct Layout {
    glyphs: Vec<Glyph>,
    /// Height of all the lines (at least one line, also for an empty text).
    pub height: f32,
    /// Per line: (top, height, [(byte index, x)] of every glyph start and the line end).
    pub lines: Vec<Line>,
}

fn attrs<'a>(font: &'a FontSpec, family_ok: bool) -> Attrs<'a> {
    let family = if family_ok {
        Family::Name(&font.family)
    } else {
        Family::Name(DEFAULT_FAMILY)
    };
    Attrs::new()
        .family(family)
        .weight(Weight(font.weight))
        .style(if font.italic { Style::Italic } else { Style::Normal })
}

/// Lay out `text` in a box `width` px wide (cached).
pub fn layout(text: &str, font: &FontSpec, width: f32, align: TextAlign) -> Arc<Layout> {
    let family_ok = font.family == DEFAULT_FAMILY || has_family(&font.family);
    if !family_ok {
        log::warn!("font {:?} is not installed: Lato is used", font.family);
    }
    let key = format!(
        "{}\u{0}{}\u{0}{}\u{0}{}\u{0}{}\u{0}{}\u{0}{:?}",
        text,
        font.family,
        font.size.to_bits(),
        font.weight,
        font.italic,
        width.to_bits(),
        align
    );
    let mut f = fonts();
    if let Some(l) = f.cache.get(&key) {
        return l.clone();
    }
    let size = font.size.clamp(1.0, 2000.0);
    let line_h = size * LINE_HEIGHT;
    let sys = &mut f.sys;
    let mut buf = Buffer::new(sys, Metrics::new(size, line_h));
    buf.set_size(Some(width.max(1.0)), None);
    let al = match align {
        TextAlign::Left => cosmic_text::Align::Left,
        TextAlign::Center => cosmic_text::Align::Center,
        TextAlign::Right => cosmic_text::Align::Right,
    };
    buf.set_text(text, &attrs(font, family_ok), Shaping::Advanced, Some(al));
    buf.shape_until_scroll(sys, false);
    let mut glyphs = Vec::new();
    let mut lines = Vec::new();
    let mut bottom: f32 = 0.0;
    // byte offset of each buffer line in `text` (layout runs index within their own line)
    let starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    for run in buf.layout_runs() {
        let base = starts.get(run.line_i).copied().unwrap_or(0);
        let mut stops = Vec::new();
        for g in run.glyphs {
            glyphs.push(Glyph {
                face: g.font_id,
                id: g.glyph_id,
                x: g.x + g.x_offset,
                y: run.line_y + g.y_offset,
                size: g.font_size,
            });
            stops.push((base + g.start, g.x));
        }
        let end = base + run.glyphs.iter().map(|g| g.end).max().unwrap_or(0);
        let end_x = run.glyphs.iter().map(|g| g.x + g.w).fold(0.0f32, f32::max);
        // an empty line: its caret goes where the alignment puts it
        let empty_x = match align {
            TextAlign::Left => 0.0,
            TextAlign::Center => width / 2.0,
            TextAlign::Right => width,
        };
        stops.push((end, if run.glyphs.is_empty() { empty_x } else { end_x }));
        lines.push((run.line_top, run.line_height, stops));
        bottom = bottom.max(run.line_top + run.line_height);
    }
    let l = Arc::new(Layout {
        glyphs,
        height: bottom.max(line_h),
        lines,
    });
    // The cache is cleared when it grows past 2000 layouts, an LRU if that ever shows
    if f.cache.len() > 2000 {
        f.cache.clear();
    }
    f.cache.insert(key, l.clone());
    l
}

/// tiny-skia path builder fed by skrifa (font units → px, y down, offset to the glyph origin).
struct Pen<'a> {
    pb: &'a mut PathBuilder,
    ox: f32,
    oy: f32,
}

impl skrifa::outline::OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(self.ox + x, self.oy - y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(self.ox + x, self.oy - y);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.pb.quad_to(self.ox + cx, self.oy - cy, self.ox + x, self.oy - y);
    }
    fn curve_to(&mut self, c0x: f32, c0y: f32, c1x: f32, c1y: f32, x: f32, y: f32) {
        self.pb.cubic_to(
            self.ox + c0x,
            self.oy - c0y,
            self.ox + c1x,
            self.oy - c1y,
            self.ox + x,
            self.oy - y,
        );
    }
    fn close(&mut self) {
        self.pb.close();
    }
}

/// The glyphs of a text box as one path in image space (rotated about the box centre).
pub fn glyph_path(rect: &RectF, rotation: f32, layout: &Layout) -> Option<Path> {
    use skrifa::MetadataProvider;
    let mut pb = PathBuilder::new();
    let mut f = fonts();
    for g in &layout.glyphs {
        let Some(face) = f.sys.db().face(g.face).map(|fc| fc.index) else {
            continue;
        };
        let Some(font) = f.sys.get_font(g.face, Weight::NORMAL) else {
            continue;
        };
        let Ok(fr) = skrifa::FontRef::from_index(font.data(), face) else {
            continue;
        };
        let Some(outline) = fr.outline_glyphs().get(skrifa::GlyphId::new(g.id as u32)) else {
            continue;
        };
        let mut pen = Pen {
            pb: &mut pb,
            ox: rect.x + g.x,
            oy: rect.y + g.y,
        };
        let settings = skrifa::outline::DrawSettings::unhinted(
            skrifa::instance::Size::new(g.size),
            skrifa::instance::LocationRef::default(),
        );
        let _ = outline.draw(settings, &mut pen);
    }
    drop(f);
    let c = rect.center();
    pb.finish()?
        .transform(Transform::from_rotate_at(rotation.to_degrees(), c.x, c.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lato(size: f32) -> FontSpec {
        FontSpec {
            family: DEFAULT_FAMILY.into(),
            size,
            weight: 400,
            italic: false,
        }
    }

    #[test]
    fn wraps_and_grows() {
        let one = layout("Hello", &lato(20.0), 400.0, TextAlign::Left);
        assert_eq!(one.lines.len(), 1);
        assert!((one.height - 25.0).abs() < 0.01, "{}", one.height);
        let many = layout("Hello world, this wraps", &lato(20.0), 60.0, TextAlign::Left);
        assert!(many.lines.len() >= 3, "{} lines", many.lines.len());
        assert!(many.height > one.height * 2.5);
        let empty = layout("", &lato(20.0), 100.0, TextAlign::Left);
        assert!((empty.height - 25.0).abs() < 0.01, "an empty text keeps one line");
        let lines = layout("a\n\nb", &lato(10.0), 100.0, TextAlign::Left);
        assert_eq!(lines.lines.len(), 3);
        // byte positions across lines
        assert_eq!(lines.lines[2].2.first().map(|s| s.0), Some(3));
    }

    #[test]
    fn glyphs_become_paths() {
        let l = layout("Hi", &lato(40.0), 400.0, TextAlign::Left);
        let r = RectF {
            x: 10.0,
            y: 20.0,
            w: 400.0,
            h: l.height,
        };
        let p = glyph_path(&r, 0.0, &l).expect("outlines");
        let b = p.bounds();
        assert!(
            b.left() >= 10.0 && b.top() >= 20.0 && b.bottom() <= 20.0 + l.height,
            "{b:?}"
        );
        assert!(b.width() > 20.0 && b.height() > 20.0, "{b:?}");
    }

    #[test]
    fn missing_family_falls_back_to_lato() {
        let mut f = lato(20.0);
        f.family = "No Such Font 123".into();
        let a = layout("Abc", &f, 300.0, TextAlign::Left);
        let b = layout("Abc", &lato(20.0), 300.0, TextAlign::Left);
        assert_eq!(a.lines[0].2, b.lines[0].2);
    }
}
