//! Selection by ID: shapes and redactions, hit-testing, handles and the geometry of
//! resizing/rotating. Pure functions; every operation ends in an `Edit` with IDs, never indices.

use rv_core::edit::geom::{area_bounds, bounds, hit, hit_area};
use rv_core::edit::*;

/// UI state, outside the history.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    /// Selection order, no duplicates.
    pub ids: Vec<ObjectId>,
    pub primary: Option<ObjectId>,
}

/// What the selection holds (the operations allowed depend on it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelKind {
    Nothing,
    Shapes,
    Redactions,
    Mixed,
}

impl Selection {
    pub fn only(id: ObjectId) -> Selection {
        Selection {
            ids: vec![id],
            primary: Some(id),
        }
    }

    pub fn set(&mut self, ids: Vec<ObjectId>) {
        let mut v: Vec<ObjectId> = Vec::new();
        for id in ids {
            if !v.contains(&id) {
                v.push(id);
            }
        }
        self.primary = v.last().copied();
        self.ids = v;
    }

    pub fn toggle(&mut self, id: ObjectId) {
        if let Some(i) = self.ids.iter().position(|x| *x == id) {
            self.ids.remove(i);
        } else {
            self.ids.push(id);
        }
        self.primary = self.ids.last().copied();
    }

    pub fn clear(&mut self) {
        self.ids.clear();
        self.primary = None;
    }

    pub fn contains(&self, id: ObjectId) -> bool {
        self.ids.contains(&id)
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// After every apply/undo/redo: drop IDs the document no longer has.
    pub fn retain(&mut self, doc: &DocState) {
        self.ids.retain(|id| doc.find(*id).is_some());
        self.primary = self.ids.last().copied();
    }

    pub fn kind(&self, doc: &DocState) -> SelKind {
        let (mut shapes, mut reds) = (false, false);
        for id in &self.ids {
            match doc.find(*id) {
                Some(ObjRef::Shape(_)) => shapes = true,
                Some(ObjRef::Redaction(_)) => reds = true,
                None => {}
            }
        }
        match (shapes, reds) {
            (false, false) => SelKind::Nothing,
            (true, false) => SelKind::Shapes,
            (false, true) => SelKind::Redactions,
            (true, true) => SelKind::Mixed,
        }
    }
}

/// Objects under `p` (image space), the topmost first: shapes from the top of the z-order, then
/// redactions.
pub fn hits(doc: &DocState, p: PointF, tol: f32) -> Vec<ObjectId> {
    let shapes = doc.shapes.iter().rev().filter(|s| hit(s, p, tol)).map(|s| s.id);
    let reds = doc
        .redactions
        .iter()
        .rev()
        .filter(|r| hit_area(&r.area, p, tol))
        .map(|r| r.id);
    shapes.chain(reds).collect()
}

/// Click target: the topmost hit, or with Alt the one after `current` among the hits (cycling
/// through overlapping objects).
pub fn pick(hits: &[ObjectId], current: Option<ObjectId>, cycle: bool) -> Option<ObjectId> {
    if cycle && let Some(i) = current.and_then(|c| hits.iter().position(|h| *h == c)) {
        return hits.get((i + 1) % hits.len()).copied();
    }
    hits.first().copied()
}

/// Bounding box of an object (image space).
pub fn object_bounds(doc: &DocState, id: ObjectId) -> Option<RectF> {
    match doc.find(id)? {
        ObjRef::Shape(i) => bounds(&doc.shapes[i]),
        ObjRef::Redaction(i) => Some(area_bounds(&doc.redactions[i].area)),
    }
}

pub fn union(a: RectF, b: RectF) -> RectF {
    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
    let (x1, y1) = ((a.x + a.w).max(b.x + b.w), (a.y + a.h).max(b.y + b.h));
    RectF {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

pub fn selection_bounds(doc: &DocState, sel: &Selection) -> Option<RectF> {
    sel.ids.iter().filter_map(|id| object_bounds(doc, *id)).reduce(union)
}

/// Rubber band: objects whose bounds lie entirely inside `r`.
pub fn inside(doc: &DocState, r: &RectF) -> Vec<ObjectId> {
    let shapes = doc.shapes.iter().map(|s| s.id);
    let reds = doc.redactions.iter().map(|r| r.id);
    shapes
        .chain(reds)
        .filter(|id| object_bounds(doc, *id).is_some_and(|b| r.contains_rect(&b)))
        .collect()
}

/// Every object in z-order (Tab / Shift+Tab cycle, Ctrl+A).
pub fn all_ids(doc: &DocState) -> Vec<ObjectId> {
    doc.redactions
        .iter()
        .map(|r| r.id)
        .chain(doc.shapes.iter().map(|s| s.id))
        .collect()
}

/// The rectangle frame of a rect-like object (rectangle, ellipse, redaction area).
pub fn frame(doc: &DocState, id: ObjectId) -> Option<(RectF, f32)> {
    match doc.find(id)? {
        ObjRef::Shape(i) => match &doc.shapes[i].geom {
            Geom::Rect { rect, rotation, .. }
            | Geom::Ellipse { rect, rotation }
            | Geom::Text { rect, rotation, .. } => Some((*rect, *rotation)),
            // the square around the circle (no rotation)
            Geom::Loupe { center, radius, .. } => Some((
                RectF {
                    x: center.x - radius,
                    y: center.y - radius,
                    w: 2.0 * radius,
                    h: 2.0 * radius,
                },
                0.0,
            )),
            _ => None,
        },
        ObjRef::Redaction(i) => {
            let (Area::Rect { rect, rotation } | Area::Ellipse { rect, rotation }) = &doc.redactions[i].area;
            Some((*rect, *rotation))
        }
    }
}

/// A text box with its height from the layout (the height follows the content).
pub fn text_geom(rect: RectF, rotation: f32, text: &str, font: &FontSpec, align: TextAlign) -> Geom {
    let h = rv_core::edit::text::layout(text, font, rect.w.max(1.0), align).height;
    Geom::Text {
        rect: RectF { h, ..rect },
        rotation,
        text: text.to_string(),
        font: font.clone(),
        align,
    }
}

/// The edit that gives a rect-like object a new frame.
pub fn set_frame(doc: &DocState, id: ObjectId, rect: RectF, rotation: f32) -> Option<Edit> {
    match doc.find(id)? {
        ObjRef::Shape(i) => {
            let geom = match &doc.shapes[i].geom {
                Geom::Rect { corner_radius, .. } => Geom::Rect {
                    rect,
                    rotation,
                    corner_radius: *corner_radius,
                },
                Geom::Ellipse { .. } => Geom::Ellipse { rect, rotation },
                // the handles set the wrapping width; the height follows the text
                Geom::Text { text, font, align, .. } => text_geom(rect, rotation, text, font, *align),
                Geom::Loupe { zoom, .. } => Geom::Loupe {
                    center: rect.center(),
                    radius: (rect.w.max(rect.h) / 2.0).max(1.0),
                    zoom: *zoom,
                },
                _ => return None,
            };
            Some(Edit::UpdateShape {
                id,
                geom: Some(geom),
                style: None,
            })
        }
        ObjRef::Redaction(i) => {
            let area = match &doc.redactions[i].area {
                Area::Rect { .. } => Area::Rect { rect, rotation },
                Area::Ellipse { .. } => Area::Ellipse { rect, rotation },
            };
            Some(Edit::UpdateRedaction {
                id,
                area: Some(area),
                kind: None,
            })
        }
    }
}

/// Line/arrow end points.
pub fn ends(doc: &DocState, id: ObjectId) -> Option<(PointF, PointF)> {
    let ObjRef::Shape(i) = doc.find(id)? else { return None };
    match &doc.shapes[i].geom {
        Geom::Line { a, b } | Geom::Arrow { a, b, .. } => Some((*a, *b)),
        _ => None,
    }
}

pub fn set_ends(doc: &DocState, id: ObjectId, a: PointF, b: PointF) -> Option<Edit> {
    let ObjRef::Shape(i) = doc.find(id)? else { return None };
    let geom = match &doc.shapes[i].geom {
        Geom::Line { .. } => Geom::Line { a, b },
        Geom::Arrow { heads, bend, .. } => Geom::Arrow {
            a,
            b,
            heads: *heads,
            bend: *bend,
        },
        _ => return None,
    };
    Some(Edit::UpdateShape {
        id,
        geom: Some(geom),
        style: None,
    })
}

/// Handles of the selection's single object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// 0 = top-left, 1 = top-right, 2 = bottom-right, 3 = bottom-left (in the object's frame).
    Corner(u8),
    /// 0 = top, 1 = right, 2 = bottom, 3 = left.
    Edge(u8),
    Rotate,
    /// Line/arrow end 0 (a) or 1 (b).
    End(u8),
}

fn rot(p: (f32, f32), a: f32) -> (f32, f32) {
    let (s, c) = a.sin_cos();
    (p.0 * c - p.1 * s, p.0 * s + p.1 * c)
}

/// Handle positions (image space) of a rect-like frame; `rotate_off` = distance of the rotation
/// handle above the top edge, in image px.
pub fn frame_handles(rect: &RectF, rotation: f32, rotate_off: f32) -> Vec<(Handle, PointF)> {
    let c = rect.center();
    let (hw, hh) = (rect.w / 2.0, rect.h / 2.0);
    let at = |x: f32, y: f32| {
        let (rx, ry) = rot((x, y), rotation);
        PointF {
            x: c.x + rx,
            y: c.y + ry,
        }
    };
    vec![
        (Handle::Corner(0), at(-hw, -hh)),
        (Handle::Corner(1), at(hw, -hh)),
        (Handle::Corner(2), at(hw, hh)),
        (Handle::Corner(3), at(-hw, hh)),
        (Handle::Edge(0), at(0.0, -hh)),
        (Handle::Edge(1), at(hw, 0.0)),
        (Handle::Edge(2), at(0.0, hh)),
        (Handle::Edge(3), at(-hw, 0.0)),
        (Handle::Rotate, at(0.0, -hh - rotate_off)),
    ]
}

/// New frame when a resize handle is dragged to `p` (image space); the opposite side stays put.
/// `keep_aspect` (Shift) keeps the proportions with a corner handle.
pub fn resize(rect: &RectF, rotation: f32, h: Handle, p: PointF, keep_aspect: bool) -> RectF {
    let c = rect.center();
    let (lx, ly) = rot((p.x - c.x, p.y - c.y), -rotation);
    let (mut l, mut t, mut r, mut b) = (-rect.w / 2.0, -rect.h / 2.0, rect.w / 2.0, rect.h / 2.0);
    match h {
        Handle::Corner(0) => (l, t) = (lx, ly),
        Handle::Corner(1) => (r, t) = (lx, ly),
        Handle::Corner(2) => (r, b) = (lx, ly),
        Handle::Corner(_) => (l, b) = (lx, ly),
        Handle::Edge(0) => t = ly,
        Handle::Edge(1) => r = lx,
        Handle::Edge(2) => b = ly,
        Handle::Edge(_) => l = lx,
        _ => return *rect,
    }
    if keep_aspect && matches!(h, Handle::Corner(_)) && rect.h > 0.0 {
        let ratio = rect.w / rect.h;
        let (w, hgt) = ((r - l).abs(), (b - t).abs());
        // follow the larger change, derive the other side
        let (w, hgt) = if w / ratio > hgt {
            (w, w / ratio)
        } else {
            (hgt * ratio, hgt)
        };
        // the moved corner's side keeps its sign relative to the fixed corner
        match h {
            Handle::Corner(0) => (l, t) = (r - w.copysign(r - l), b - hgt.copysign(b - t)),
            Handle::Corner(1) => (r, t) = (l + w.copysign(r - l), b - hgt.copysign(b - t)),
            Handle::Corner(2) => (r, b) = (l + w.copysign(r - l), t + hgt.copysign(b - t)),
            _ => (l, b) = (r - w.copysign(r - l), t + hgt.copysign(b - t)),
        }
    }
    let (x0, x1) = (l.min(r), l.max(r));
    let (y0, y1) = (t.min(b), t.max(b));
    let (w, hgt) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
    let (cx, cy) = rot(((x0 + x1) / 2.0, (y0 + y1) / 2.0), rotation);
    RectF {
        x: c.x + cx - w / 2.0,
        y: c.y + cy - hgt / 2.0,
        w,
        h: hgt,
    }
}

/// New rotation when the rotation handle goes from `from` to `p` around the frame's centre;
/// `snap` (Shift) rounds to 15°.
pub fn rotate(rect: &RectF, start_rotation: f32, from: PointF, p: PointF, snap: bool) -> f32 {
    let c = rect.center();
    let a0 = (from.y - c.y).atan2(from.x - c.x);
    let a1 = (p.y - c.y).atan2(p.x - c.x);
    let r = start_rotation + (a1 - a0);
    if snap {
        let step = 15f32.to_radians();
        (r / step).round() * step
    } else {
        r
    }
}

/// `p` constrained to a multiple of 45° from `origin` (Shift on lines).
pub fn snap45(origin: PointF, p: PointF) -> PointF {
    let (dx, dy) = (p.x - origin.x, p.y - origin.y);
    let len = dx.hypot(dy);
    let step = 45f32.to_radians();
    let a = (dy.atan2(dx) / step).round() * step;
    PointF {
        x: origin.x + len * a.cos(),
        y: origin.y + len * a.sin(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> RectF {
        RectF { x, y, w, h }
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn doc() -> DocState {
        let mut d = DocState::new(200, 200);
        let style = Style::default_for(ShapeKind::Rect);
        for rect in [r(10.0, 10.0, 50.0, 50.0), r(30.0, 30.0, 50.0, 50.0)] {
            let s = Shape {
                id: d.peek_id(),
                geom: Geom::Rect {
                    rect,
                    rotation: 0.0,
                    corner_radius: 0.0,
                },
                style: Style {
                    fill: Some(Color::rgb(0, 0, 0)),
                    ..style.clone()
                },
            };
            d = d.apply(&Edit::AddShape(s)).unwrap();
        }
        let red = Redaction {
            id: d.peek_id(),
            area: Area::Rect {
                rect: r(40.0, 40.0, 10.0, 10.0),
                rotation: 0.0,
            },
            kind: RedactKind::Solid { color: [0; 3] },
        };
        d.apply(&Edit::AddRedaction(red)).unwrap()
    }

    #[test]
    fn hits_order_cycle_and_selection_kinds() {
        let d = doc();
        let p = PointF { x: 45.0, y: 45.0 };
        // shapes from the top, then the redaction underneath
        assert_eq!(hits(&d, p, 0.0), vec![ObjectId(2), ObjectId(1), ObjectId(3)]);
        let h = hits(&d, p, 0.0);
        assert_eq!(pick(&h, None, false), Some(ObjectId(2)));
        assert_eq!(
            pick(&h, Some(ObjectId(2)), true),
            Some(ObjectId(1)),
            "Alt+click: the one below"
        );
        assert_eq!(pick(&h, Some(ObjectId(3)), true), Some(ObjectId(2)), "wraps around");
        let mut s = Selection::only(ObjectId(1));
        assert_eq!(s.kind(&d), SelKind::Shapes);
        s.toggle(ObjectId(3));
        assert_eq!((s.kind(&d), s.primary), (SelKind::Mixed, Some(ObjectId(3))));
        s.toggle(ObjectId(1));
        assert_eq!(s.kind(&d), SelKind::Redactions);
        // removed objects leave the selection
        let d2 = d.apply(&Edit::Remove(vec![ObjectId(3)])).unwrap();
        s.retain(&d2);
        assert!(s.is_empty() && s.primary.is_none());
        // rubber band: only objects entirely inside
        let band = r(35.0, 35.0, 20.0, 20.0);
        assert_eq!(inside(&d, &band), vec![ObjectId(3)]);
        assert_eq!(all_ids(&d), vec![ObjectId(3), ObjectId(1), ObjectId(2)]);
    }

    #[test]
    fn resize_rotate_snap() {
        let rect = r(0.0, 0.0, 100.0, 50.0);
        // bottom-right corner to (150, 80): top-left stays
        let n = resize(&rect, 0.0, Handle::Corner(2), PointF { x: 150.0, y: 80.0 }, false);
        assert!(
            near(n.x, 0.0) && near(n.y, 0.0) && near(n.w, 150.0) && near(n.h, 80.0),
            "{n:?}"
        );
        // left edge only changes x/w
        let n = resize(&rect, 0.0, Handle::Edge(3), PointF { x: 20.0, y: 999.0 }, false);
        assert!(near(n.x, 20.0) && near(n.w, 80.0) && near(n.h, 50.0), "{n:?}");
        // crossing over flips and keeps at least 1 px
        let n = resize(&rect, 0.0, Handle::Edge(1), PointF { x: -30.0, y: 0.0 }, false);
        assert!(near(n.x, -30.0) && near(n.w, 30.0), "{n:?}");
        // keep aspect 2:1
        let n = resize(&rect, 0.0, Handle::Corner(2), PointF { x: 200.0, y: 60.0 }, true);
        assert!(near(n.w / n.h, 2.0) && near(n.x, 0.0) && near(n.y, 0.0), "{n:?}");
        // rotated 90°: dragging the frame's right edge moves along screen y; the left edge stays
        let q = std::f32::consts::FRAC_PI_2;
        let n = resize(&rect, q, Handle::Edge(1), PointF { x: 50.0, y: 100.0 }, false);
        assert!(near(n.w, 100.0 + 25.0), "{n:?}");
        let left_before = frame_handles(&rect, q, 0.0)[7].1;
        let left_after = frame_handles(&n, q, 0.0)[7].1;
        assert!(near(left_before.x, left_after.x) && near(left_before.y, left_after.y));
        // rotation: a quarter turn around the centre, snapped
        let c = rect.center();
        let from = PointF { x: c.x + 10.0, y: c.y };
        let to = PointF { x: c.x, y: c.y + 10.0 };
        assert!(near(rotate(&rect, 0.0, from, to, false), q));
        let almost = PointF {
            x: c.x + 10.0,
            y: c.y + 1.0,
        };
        assert!(near(rotate(&rect, 0.0, from, almost, true), 0.0), "snapped to 0°");
        let s = snap45(PointF { x: 0.0, y: 0.0 }, PointF { x: 10.0, y: 9.0 });
        assert!(near(s.x, s.y));
    }
}
