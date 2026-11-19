//! Markup session of a viewer window: document history, tool, selection, the gesture
//! in progress, the crop draft. Input arrives here in image coordinates; every change of the
//! document goes through `History` as `Edit`s with object IDs. The egui toolbar and
//! Inspector (`ui`) and the E2E driver press the same named buttons (`button`).

pub mod crop;
pub mod overlay;
pub mod select;
pub mod textedit;
pub mod ui;

use crate::viewer::Affine;
use rv_core::edit::*;
use select::{Handle, SelKind, Selection};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Crop,
    Draw(ShapeKind),
    /// A rectangle or ellipse becomes a redaction.
    Redact,
    /// Click: a text box edited inline.
    Text,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Crop => "crop",
            Tool::Draw(ShapeKind::Rect) => "rect",
            Tool::Draw(ShapeKind::Ellipse) => "ellipse",
            Tool::Draw(ShapeKind::Line) => "line",
            Tool::Draw(ShapeKind::Arrow) => "arrow",
            Tool::Draw(ShapeKind::Pen) => "pen",
            Tool::Draw(ShapeKind::Highlight) => "highlight",
            Tool::Draw(ShapeKind::Text) | Tool::Text => "text",
            Tool::Draw(ShapeKind::Loupe) => "loupe",
            Tool::Redact => "redact",
        }
    }

    pub fn from_name(n: &str) -> Option<Tool> {
        Some(match n {
            "select" => Tool::Select,
            "crop" => Tool::Crop,
            "rect" => Tool::Draw(ShapeKind::Rect),
            "ellipse" => Tool::Draw(ShapeKind::Ellipse),
            "line" => Tool::Draw(ShapeKind::Line),
            "arrow" => Tool::Draw(ShapeKind::Arrow),
            "pen" => Tool::Draw(ShapeKind::Pen),
            "highlight" => Tool::Draw(ShapeKind::Highlight),
            "redact" => Tool::Redact,
            "loupe" => Tool::Draw(ShapeKind::Loupe),
            "text" => Tool::Text,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug)]
enum Drag {
    /// Moving the selection; the gesture starts at the first movement.
    Move {
        from: PointF,
        ids: Vec<ObjectId>,
    },
    Frame {
        id: ObjectId,
        handle: Handle,
        start: (RectF, f32),
        from: PointF,
    },
    End {
        id: ObjectId,
        which: u8,
    },
    Band {
        from: PointF,
        to: PointF,
        add: bool,
    },
    Create {
        from: PointF,
        points: Vec<PointF>,
    },
    Crop,
}

/// Modifier keys of a pointer event.
#[derive(Clone, Copy, Debug, Default)]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
}

/// What the app must do after a key or a button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Out {
    Handled,
    /// The key isn't Markup's: global shortcuts may use it.
    Pass,
    /// Leave Markup (the app asks first if the document is dirty).
    Exit,
    /// Run an app action (Save Project, Export…).
    Do(crate::actions::Action),
    /// ←/→ without a selection.
    Navigate(i8),
}

/// What the pointer is over (`Markup::hover`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hover {
    None,
    Handle(Handle),
    Move,
    Type,
    Create,
}

pub struct Markup {
    pub history: History,
    /// Source pixels at full precision and the original bytes, once decoded on a worker.
    pub source: Option<rv_core::export::DocSource>,
    /// The file the document came from (an image, or a `.rvm`).
    pub file: PathBuf,
    /// Where Save Project writes (set by the first Save Project, or the `.rvm` opened).
    pub project_path: Option<PathBuf>,
    /// The original's file name (proposed names, the project manifest).
    pub original_name: String,
    /// The view must show the file again when Markup ends (a project, or an overwritten file).
    pub reload_on_exit: bool,
    /// Redaction patches written into the image texture (preview).
    pub shown_patches: Vec<Arc<rv_core::edit::redact::Patch>>,
    pub tool: Tool,
    pub sel: Selection,
    /// The shape being drawn (not in the document until the button is released).
    pub pending: Option<Shape>,
    /// The redaction being drawn.
    pub pending_red: Option<Redaction>,
    /// Mode and shape of the next redaction (the last ones chosen; Solid black at first).
    pub redact_kind: RedactKind,
    pub redact_ellipse: bool,
    /// Font of the next text box (the last one chosen).
    pub text_font: (FontSpec, TextAlign),
    /// The text box being edited inline.
    pub editing: Option<textedit::TextEdit>,
    /// Font families of the loaded project that aren't installed (shown as Lato).
    pub missing_fonts: Vec<String>,
    pub crop: Option<crop::CropDraft>,
    /// Last style per kind.
    pub styles: BTreeMap<ShapeKind, Style>,
    pub overlay: Option<overlay::Overlay>,
    /// An Inspector control is being changed (one history entry while it is held).
    pub inspector_gesture: bool,
    drag: Option<Drag>,
}

/// Distance (screen px) within which a handle or an object is grabbed.
pub const GRAB_PX: f32 = 7.0;
/// Rotation handle distance above the top edge (screen px).
pub const ROTATE_PX: f32 = 26.0;

impl Markup {
    pub fn new(w: u32, h: u32, styles: BTreeMap<ShapeKind, Style>, tool: Tool, file: PathBuf) -> Markup {
        let original_name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut m = Markup {
            history: History::new(DocState::new(w, h)),
            source: None,
            reload_on_exit: false,
            shown_patches: Vec::new(),
            project_path: None,
            original_name,
            file,
            tool: Tool::Select,
            sel: Selection::default(),
            pending: None,
            pending_red: None,
            redact_kind: RedactKind::Solid { color: [0, 0, 0] },
            redact_ellipse: false,
            text_font: (
                FontSpec {
                    family: rv_core::edit::text::DEFAULT_FAMILY.into(),
                    size: 25.0,
                    weight: 400,
                    italic: false,
                },
                TextAlign::Left,
            ),
            editing: None,
            missing_fonts: Vec::new(),
            crop: None,
            styles,
            overlay: None,
            inspector_gesture: false,
            drag: None,
        };
        m.set_tool(tool);
        m
    }

    pub fn doc(&self) -> &Arc<DocState> {
        self.history.current()
    }

    /// The style for a new shape: the last one used for this kind, else the default with a width
    /// that reads at fit size (about 1/500 of the image's long side, 2–40 px).
    pub fn style_for(&self, kind: ShapeKind) -> Style {
        self.styles.get(&kind).cloned().unwrap_or_else(|| {
            let mut s = Style::default_for(kind);
            let long = self.doc().image_size.w.max(self.doc().image_size.h) as f32;
            let k = (long / 500.0 / 4.0).max(1.0);
            s.width = (s.width * k).round().clamp(2.0, 40.0);
            s
        })
    }

    pub fn set_tool(&mut self, t: Tool) {
        self.commit_text();
        self.history.end_gesture();
        self.drag = None;
        self.pending = None;
        self.pending_red = None;
        // another creation tool: the element being adjusted is confirmed
        if t != Tool::Select {
            self.sel.clear();
        }
        if t == Tool::Text {
            // installed fonts for the Font list, once, off the UI thread
            static ONCE: std::sync::Once = std::sync::Once::new();
            ONCE.call_once(|| {
                std::thread::spawn(rv_core::edit::text::load_system_fonts);
            });
        }
        self.crop = (t == Tool::Crop).then(|| crop::CropDraft::new(self.history.current()));
        self.tool = t;
    }

    /// A creation tool is active and an element is selected: it is being adjusted and must be
    /// confirmed before the tool creates another one.
    pub fn adjusting(&self) -> bool {
        matches!(self.tool, Tool::Draw(_) | Tool::Redact | Tool::Text) && !self.sel.is_empty() && self.editing.is_none()
    }

    /// What a press at `p` would do, for the pointer's shape: a handle of the selected element,
    /// moving it, typing in the edited text, drawing a new element, or nothing special.
    pub fn hover(&self, p: PointF, screen: (f32, f32), t: &Affine, scale: f32) -> Hover {
        let near = |q: PointF| {
            let (x, y) = t.apply(q.x, q.y);
            (x - screen.0).hypot(y - screen.1) <= GRAB_PX
        };
        let doc = self.doc();
        let tol = GRAB_PX / scale;
        let in_selection = || {
            self.sel.ids.iter().any(|id| {
                select::object_bounds(doc, *id).is_some_and(|b| {
                    p.x >= b.x - tol && p.y >= b.y - tol && p.x <= b.x + b.w + tol && p.y <= b.y + b.h + tol
                })
            })
        };
        if self.editing.is_some() {
            return if in_selection() { Hover::Type } else { Hover::None };
        }
        if self.tool == Tool::Crop {
            return Hover::None;
        }
        let creating = self.tool != Tool::Select;
        if (!creating || self.adjusting())
            && let Some((h, _)) = self.handles(scale).into_iter().find(|(_, q)| near(*q))
        {
            return Hover::Handle(h);
        }
        if self.adjusting() {
            return if in_selection() { Hover::Move } else { Hover::None };
        }
        if creating {
            return Hover::Create;
        }
        if select::hits(doc, p, tol).iter().any(|id| self.sel.contains(*id)) {
            Hover::Move
        } else {
            Hover::None
        }
    }

    /// Confirm the element being adjusted or typed: it stays in the document, the tool is ready
    /// for the next one.
    pub fn confirm(&mut self) {
        self.commit_text();
        self.history.end_gesture();
        self.drag = None;
        if self.tool != Tool::Select {
            self.sel.clear();
        }
    }

    fn after_change(&mut self) {
        let doc = self.history.current().clone();
        self.sel.retain(&doc);
    }

    /// The part of the image the view shows: the crop, except while cropping (the whole image).
    pub fn view_region(&self) -> Option<RectU> {
        if self.tool == Tool::Crop { None } else { self.doc().crop }
    }

    /// IDs being changed by the running gesture (the overlay's active layer).
    pub fn active_ids(&self) -> Vec<ObjectId> {
        match &self.drag {
            Some(Drag::Move { ids, .. }) if self.history.in_gesture() => ids.clone(),
            Some(Drag::Frame { id, .. } | Drag::End { id, .. }) => vec![*id],
            _ => vec![],
        }
    }

    pub fn band(&self) -> Option<RectF> {
        match &self.drag {
            Some(Drag::Band { from, to, .. }) => Some(RectF::from_points(*from, *to)),
            _ => None,
        }
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The single selected object's handles, in image space.
    pub fn handles(&self, scale: f32) -> Vec<(Handle, PointF)> {
        let doc = self.doc();
        let [id] = self.sel.ids[..] else { return vec![] };
        if let Some((rect, rotation)) = select::frame(doc, id) {
            return select::frame_handles(&rect, rotation, ROTATE_PX / scale);
        }
        if let Some((a, b)) = select::ends(doc, id) {
            return vec![(Handle::End(0), a), (Handle::End(1), b)];
        }
        vec![]
    }

    // ---------------------------------------------------------------- pointer

    /// Left button down at `p` (image space); `t` = screen_from_image, `screen` = pointer.
    pub fn press(&mut self, p: PointF, screen: (f32, f32), t: &Affine, scale: f32, mods: Mods) {
        if self.editing.is_some() {
            if self.caret_at(p, mods.shift) {
                return;
            }
            self.commit_text();
        }
        self.history.end_gesture();
        let near = |q: PointF| {
            let (x, y) = t.apply(q.x, q.y);
            (x - screen.0).hypot(y - screen.1) <= GRAB_PX
        };
        // An element was just inserted (or is selected) with a creation tool: until it is confirmed
        // (Enter, the Confirm button, Esc) the pointer adjusts it — handles resize/rotate, a drag
        // inside moves — and never creates another element.
        if self.adjusting() {
            let doc = self.doc().clone();
            if let Some((h, _)) = self.handles(scale).into_iter().find(|(_, q)| near(*q)) {
                let id = self.sel.ids[0];
                self.drag = Some(match h {
                    Handle::End(i) => Drag::End { id, which: i },
                    _ => Drag::Frame {
                        id,
                        handle: h,
                        start: select::frame(&doc, id).expect("frame"),
                        from: p,
                    },
                });
                return;
            }
            let tol = GRAB_PX / scale;
            let inside = self.sel.ids.iter().any(|id| {
                select::object_bounds(&doc, *id).is_some_and(|b| {
                    p.x >= b.x - tol && p.y >= b.y - tol && p.x <= b.x + b.w + tol && p.y <= b.y + b.h + tol
                })
            });
            if inside {
                self.drag = Some(Drag::Move {
                    from: p,
                    ids: self.sel.ids.clone(),
                });
            }
            return;
        }
        match self.tool {
            Tool::Draw(_) | Tool::Redact => {
                self.drag = Some(Drag::Create {
                    from: p,
                    points: vec![p],
                });
            }
            Tool::Text => {
                let doc = self.doc().clone();
                let on_text = select::hits(&doc, p, GRAB_PX / scale).into_iter().find(|id| {
                    matches!(doc.find(*id), Some(ObjRef::Shape(i)) if matches!(doc.shapes[i].geom, Geom::Text { .. }))
                });
                match on_text {
                    Some(id) => {
                        self.edit_text(id);
                        self.caret_at(p, false);
                    }
                    None => self.new_text(p),
                }
            }
            Tool::Crop => {
                let Some(c) = &mut self.crop else { return };
                let r = c.rect;
                let hs = select::frame_handles(&r, 0.0, 0.0);
                let grab = match hs.iter().find(|(h, q)| *h != Handle::Rotate && near(*q)) {
                    Some((h, _)) => crop::Grab::Handle(*h),
                    None if r.contains_rect(&RectF {
                        x: p.x,
                        y: p.y,
                        w: 0.0,
                        h: 0.0,
                    }) =>
                    {
                        crop::Grab::Move
                    }
                    None => crop::Grab::New(p),
                };
                c.begin(grab, p);
                self.drag = Some(Drag::Crop);
            }
            Tool::Select => {
                let doc = self.doc().clone();
                if !mods.shift
                    && let Some((h, _)) = self.handles(scale).into_iter().find(|(_, q)| near(*q))
                {
                    let id = self.sel.ids[0];
                    self.drag = Some(match h {
                        Handle::End(i) => Drag::End { id, which: i },
                        _ => Drag::Frame {
                            id,
                            handle: h,
                            start: select::frame(&doc, id).expect("frame"),
                            from: p,
                        },
                    });
                    return;
                }
                let hits = select::hits(&doc, p, GRAB_PX / scale);
                match select::pick(&hits, self.sel.primary, mods.alt) {
                    Some(id) if mods.shift => self.sel.toggle(id),
                    Some(id) => {
                        if !self.sel.contains(id) || mods.alt {
                            self.sel = Selection::only(id);
                        }
                        self.drag = Some(Drag::Move {
                            from: p,
                            ids: self.sel.ids.clone(),
                        });
                    }
                    None => {
                        if !mods.shift {
                            self.sel.clear();
                        }
                        self.drag = Some(Drag::Band {
                            from: p,
                            to: p,
                            add: mods.shift,
                        });
                    }
                }
            }
        }
    }

    /// Pointer moved to `p` (image space) with the button down.
    pub fn motion(&mut self, p: PointF, scale: f32, mods: Mods) {
        let Some(drag) = self.drag.clone() else { return };
        let doc_before = self.history.current().clone();
        match drag {
            Drag::Move { from, ids } => {
                let (mut dx, mut dy) = (p.x - from.x, p.y - from.y);
                if mods.shift {
                    // along the main axis only
                    if dx.abs() > dy.abs() { dy = 0.0 } else { dx = 0.0 }
                }
                if dx != 0.0 || dy != 0.0 || self.history.in_gesture() {
                    let _ = self.history.update_gesture(&Edit::Translate {
                        ids,
                        delta: VectorF { dx, dy },
                    });
                }
            }
            Drag::Frame {
                id,
                handle,
                start: (rect, rotation),
                from,
            } => {
                let (r, rot) = if handle == Handle::Rotate {
                    (rect, select::rotate(&rect, rotation, from, p, mods.shift))
                } else {
                    (select::resize(&rect, rotation, handle, p, mods.shift), rotation)
                };
                // relative to the gesture's start: `rect`/`rotation` are the frame when it began
                if let Some(e) = select::set_frame(&doc_before, id, r, rot) {
                    let _ = self.history.update_gesture(&e);
                }
            }
            Drag::End { id, which } => {
                if let Some((a, b)) = select::ends(&doc_before, id) {
                    let (a, b) = if which == 0 {
                        (if mods.shift { select::snap45(b, p) } else { p }, b)
                    } else {
                        (a, if mods.shift { select::snap45(a, p) } else { p })
                    };
                    if let Some(e) = select::set_ends(&doc_before, id, a, b) {
                        let _ = self.history.update_gesture(&e);
                    }
                }
            }
            Drag::Band { from, add, .. } => self.drag = Some(Drag::Band { from, to: p, add }),
            Drag::Create { from, points } if self.tool == Tool::Redact => {
                self.pending_red = Some(self.creating_red(from, p, mods.shift));
                self.drag = Some(Drag::Create { from, points });
            }
            Drag::Create { from, mut points } => {
                if let Tool::Draw(kind) = self.tool {
                    if matches!(kind, ShapeKind::Pen | ShapeKind::Highlight) {
                        let last = *points.last().unwrap();
                        if (p.x - last.x).hypot(p.y - last.y) * scale >= 1.0 {
                            points.push(p);
                        }
                    }
                    self.pending = self.creating(kind, from, p, &points, mods.shift);
                    self.drag = Some(Drag::Create { from, points });
                }
            }
            Drag::Crop => {
                let doc = self.doc().clone();
                if let Some(c) = &mut self.crop {
                    c.drag_to(&doc, p);
                }
            }
        }
    }

    /// The shape being drawn from `from` to `p`.
    fn creating(&self, kind: ShapeKind, from: PointF, p: PointF, points: &[PointF], shift: bool) -> Option<Shape> {
        let style = self.style_for(kind);
        let square = |p: PointF| {
            if !shift {
                return p;
            }
            let s = (p.x - from.x).abs().max((p.y - from.y).abs());
            PointF {
                x: from.x + s.copysign(p.x - from.x),
                y: from.y + s.copysign(p.y - from.y),
            }
        };
        let geom = match kind {
            ShapeKind::Rect => Geom::Rect {
                rect: RectF::from_points(from, square(p)),
                rotation: 0.0,
                corner_radius: 0.0,
            },
            ShapeKind::Ellipse => Geom::Ellipse {
                rect: RectF::from_points(from, square(p)),
                rotation: 0.0,
            },
            ShapeKind::Line => Geom::Line {
                a: from,
                b: if shift { select::snap45(from, p) } else { p },
            },
            ShapeKind::Arrow => Geom::Arrow {
                a: from,
                b: if shift { select::snap45(from, p) } else { p },
                heads: ArrowHeads::End,
                bend: 0.0,
            },
            ShapeKind::Pen => Geom::Pen { points: points.into() },
            ShapeKind::Highlight => Geom::Highlight { points: points.into() },
            // text boxes are made by a click (`new_text`)
            ShapeKind::Text => return None,
            // dragged from the centre outwards
            ShapeKind::Loupe => Geom::Loupe {
                center: from,
                radius: (p.x - from.x).hypot(p.y - from.y),
                zoom: 2.0,
            },
        };
        Some(Shape {
            id: self.doc().peek_id(),
            geom,
            style,
        })
    }

    pub fn release(&mut self, p: PointF) {
        let Some(drag) = self.drag.take() else { return };
        match drag {
            Drag::Move { .. } => {
                let moved = self.history.in_gesture();
                self.history.end_gesture();
                // Text tool: a click (no drag) on the text box being adjusted edits it there
                if !moved
                    && self.tool == Tool::Text
                    && let Some(id) = self.single_text()
                {
                    self.edit_text(id);
                    self.caret_at(p, false);
                }
            }
            Drag::Frame { .. } | Drag::End { .. } => self.history.end_gesture(),
            Drag::Band { from, to, add } => {
                let doc = self.doc().clone();
                let mut ids = if add { self.sel.ids.clone() } else { vec![] };
                ids.extend(select::inside(&doc, &RectF::from_points(from, to)));
                self.sel.set(ids);
            }
            Drag::Create { .. } if self.tool == Tool::Redact => {
                if let Some(r) = self.pending_red.take() {
                    let id = r.id;
                    if self.history.apply(&Edit::AddRedaction(r)).is_ok() {
                        self.sel = Selection::only(id);
                    }
                }
            }
            Drag::Create { .. } => {
                if let Some(s) = self.pending.take() {
                    let id = s.id;
                    // too small (a click) → nothing is added
                    if self.history.apply(&Edit::AddShape(s)).is_ok() {
                        self.sel = Selection::only(id);
                    }
                }
            }
            Drag::Crop => {
                if let Some(c) = &mut self.crop {
                    c.end();
                }
            }
        }
        self.after_change();
    }

    // ---------------------------------------------------------------- edits

    fn apply(&mut self, e: Edit) {
        if self.history.apply(&e).is_ok() {
            self.after_change();
        }
    }

    pub fn delete_selection(&mut self) {
        if !self.sel.is_empty() {
            self.apply(Edit::Remove(self.sel.ids.clone()));
        }
    }

    pub fn duplicate(&mut self) {
        if self.sel.is_empty() {
            return;
        }
        let first_new = self.doc().next_id;
        let n = self.sel.ids.len() as u64;
        self.apply(Edit::Duplicate {
            ids: self.sel.ids.clone(),
            offset: VectorF { dx: 10.0, dy: 10.0 },
        });
        // the selection moves to the copies
        self.sel.set((first_new..first_new + n).map(ObjectId).collect());
        self.after_change();
    }

    pub fn nudge(&mut self, dx: f32, dy: f32) {
        if !self.sel.is_empty() {
            self.apply(Edit::Translate {
                ids: self.sel.ids.clone(),
                delta: VectorF { dx, dy },
            });
        }
    }

    /// Ctrl+] / Ctrl+[ (one step), Ctrl+Shift+] / Ctrl+Shift+[ (to the top / bottom), inside the
    /// object's own list; only for a selection of one kind.
    pub fn reorder(&mut self, up: bool, all_the_way: bool) {
        let doc = self.doc().clone();
        if !matches!(self.sel.kind(&doc), SelKind::Shapes | SelKind::Redactions) {
            return;
        }
        let mut edits = vec![];
        for id in &self.sel.ids {
            let (i, len) = match doc.find(*id) {
                Some(ObjRef::Shape(i)) => (i, doc.shapes.len()),
                Some(ObjRef::Redaction(i)) => (i, doc.redactions.len()),
                None => continue,
            };
            let to = match (up, all_the_way) {
                (true, true) => len - 1,
                (true, false) => (i + 1).min(len - 1),
                (false, true) => 0,
                (false, false) => i.saturating_sub(1),
            };
            if to != i {
                edits.push(Edit::Reorder { id: *id, to_index: to });
            }
        }
        if !edits.is_empty() {
            self.apply(Edit::Batch(edits));
        }
    }

    pub fn select_all(&mut self) {
        self.sel.set(select::all_ids(self.doc()));
    }

    /// Tab / Shift+Tab: the next/previous object in z-order.
    pub fn cycle(&mut self, back: bool) {
        let all = select::all_ids(self.doc());
        if all.is_empty() {
            return;
        }
        let i = self.sel.primary.and_then(|p| all.iter().position(|x| *x == p));
        let n = all.len();
        let j = match (i, back) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(i), false) => (i + 1) % n,
            (Some(i), true) => (i + n - 1) % n,
        };
        self.sel = Selection::only(all[j]);
    }

    /// Change the style of the selected shapes (or of the next shape of the current tool) —
    /// coalesced while the Inspector control is held.
    pub fn set_style(&mut self, f: &dyn Fn(&mut Style)) {
        self.pause_text();
        let doc = self.doc().clone();
        let edits: Vec<Edit> = self
            .sel
            .ids
            .iter()
            .filter_map(|id| match doc.find(*id) {
                Some(ObjRef::Shape(i)) => {
                    let mut st = doc.shapes[i].style.clone();
                    f(&mut st);
                    self.styles.insert(doc.shapes[i].geom.kind(), st.clone());
                    Some(Edit::UpdateShape {
                        id: *id,
                        geom: None,
                        style: Some(st),
                    })
                }
                _ => None,
            })
            .collect();
        if edits.is_empty() {
            if let Tool::Draw(kind) = self.tool {
                let mut st = self.style_for(kind);
                f(&mut st);
                self.styles.insert(kind, st);
            }
            return;
        }
        if self.inspector_gesture {
            // one entry for the whole slider/picker interaction: the edit is relative to its start
            let _ = self.history.update_gesture(&Edit::Batch(edits));
        } else {
            self.apply(Edit::Batch(edits));
        }
    }

    /// Change the geometry of selected shapes of one kind (corner radius, arrow heads).
    pub fn set_geom(&mut self, f: &dyn Fn(&Geom) -> Option<Geom>) {
        self.pause_text();
        let doc = self.doc().clone();
        let edits: Vec<Edit> = self
            .sel
            .ids
            .iter()
            .filter_map(|id| match doc.find(*id) {
                Some(ObjRef::Shape(i)) => f(&doc.shapes[i].geom).map(|g| Edit::UpdateShape {
                    id: *id,
                    geom: Some(g),
                    style: None,
                }),
                _ => None,
            })
            .collect();
        if edits.is_empty() {
            return;
        }
        if self.inspector_gesture {
            let _ = self.history.update_gesture(&Edit::Batch(edits));
        } else {
            self.apply(Edit::Batch(edits));
        }
    }

    pub fn undo(&mut self) {
        self.commit_text();
        self.drag = None;
        self.pending = None;
        self.history.undo();
        self.after_change();
    }

    pub fn redo(&mut self) {
        self.commit_text();
        self.history.redo();
        self.after_change();
    }

    /// Toolbar / Inspector buttons by name (the driver presses the same ones).
    pub fn button(&mut self, name: &str) -> Out {
        if let Some(t) = name.strip_prefix("tool-").and_then(Tool::from_name) {
            self.set_tool(t);
            return Out::Handled;
        }
        match name {
            "done" => return Out::Exit,
            "confirm" => self.confirm(),
            "export" => return Out::Do(crate::actions::Action::Export),
            "undo" => self.undo(),
            "redo" => self.redo(),
            "rotate-left" => self.apply(Edit::Rotate { quarter_turns: -1 }),
            "rotate-right" => self.apply(Edit::Rotate { quarter_turns: 1 }),
            "flip-h" => self.apply(Edit::FlipH),
            "flip-v" => self.apply(Edit::FlipV),
            "delete" => self.delete_selection(),
            "duplicate" => self.duplicate(),
            "crop-apply" => {
                if let Some(c) = &self.crop {
                    let crop = c.to_crop(self.doc());
                    if crop != self.doc().crop {
                        self.apply(Edit::SetCrop(crop));
                    }
                }
                self.set_tool(Tool::Select);
            }
            "crop-reset" => {
                let doc = self.doc().clone();
                self.drag = None;
                if let Some(c) = &mut self.crop {
                    c.end();
                    c.reset(&doc);
                }
            }
            "crop-cancel" => self.set_tool(Tool::Select),
            "crop-invert" => {
                let doc = self.doc().clone();
                if let Some(c) = &mut self.crop {
                    c.invert(&doc);
                }
            }
            _ => return Out::Pass,
        }
        Out::Handled
    }

    // ---------------------------------------------------------------- redactions and text

    /// The redaction being drawn from `from` to `p` (Shift: a square / circle).
    fn creating_red(&self, from: PointF, p: PointF, shift: bool) -> Redaction {
        let to = if shift {
            let s = (p.x - from.x).abs().max((p.y - from.y).abs());
            PointF {
                x: from.x + s.copysign(p.x - from.x),
                y: from.y + s.copysign(p.y - from.y),
            }
        } else {
            p
        };
        let rect = RectF::from_points(from, to);
        Redaction {
            id: self.doc().peek_id(),
            area: if self.redact_ellipse {
                Area::Ellipse { rect, rotation: 0.0 }
            } else {
                Area::Rect { rect, rotation: 0.0 }
            },
            kind: self.redact_kind.clone(),
        }
    }

    /// Change the selected redactions (or the next one's mode/shape when none is selected).
    pub fn set_redaction(&mut self, f: &dyn Fn(&mut RedactKind, &mut bool)) {
        let doc = self.doc().clone();
        let edits: Vec<Edit> = self
            .sel
            .ids
            .iter()
            .filter_map(|id| match doc.find(*id) {
                Some(ObjRef::Redaction(i)) => {
                    let r = &doc.redactions[i];
                    let (mut kind, mut ell) = (r.kind.clone(), matches!(r.area, Area::Ellipse { .. }));
                    f(&mut kind, &mut ell);
                    let (Area::Rect { rect, rotation } | Area::Ellipse { rect, rotation }) = r.area.clone();
                    let area = if ell {
                        Area::Ellipse { rect, rotation }
                    } else {
                        Area::Rect { rect, rotation }
                    };
                    self.redact_kind = kind.clone();
                    self.redact_ellipse = ell;
                    Some(Edit::UpdateRedaction {
                        id: *id,
                        area: Some(area),
                        kind: Some(kind),
                    })
                }
                _ => None,
            })
            .collect();
        if edits.is_empty() {
            f(&mut self.redact_kind, &mut self.redact_ellipse);
        } else if self.inspector_gesture {
            let _ = self.history.update_gesture(&Edit::Batch(edits));
        } else {
            self.apply(Edit::Batch(edits));
        }
    }

    /// The only selected object, if it is a text box.
    pub fn single_text(&self) -> Option<ObjectId> {
        let [id] = self.sel.ids[..] else { return None };
        let doc = self.doc();
        matches!(doc.find(id), Some(ObjRef::Shape(i)) if matches!(doc.shapes[i].geom, Geom::Text { .. })).then_some(id)
    }

    /// A new text box at `p` (top-left), edited at once; upright on screen when the document is
    /// rotated.
    fn new_text(&mut self, p: PointF) {
        let doc = self.doc().clone();
        let iw = doc.image_size.w as f32;
        let (font, align) = self.text_font.clone();
        let w = (iw * 0.3).clamp(font.size * 4.0, iw.max(font.size * 4.0));
        let rotation = -(doc.orient.quarter_turns as f32) * std::f32::consts::FRAC_PI_2;
        let rect = RectF {
            x: p.x,
            y: p.y,
            w,
            h: 1.0,
        };
        let shape = Shape {
            id: doc.peek_id(),
            geom: select::text_geom(rect, rotation, "", &font, align),
            style: self.style_for(ShapeKind::Text),
        };
        let id = shape.id;
        self.history.end_gesture();
        self.history.begin_gesture();
        if self.history.update_gesture(&Edit::AddShape(shape)).is_ok() {
            self.sel = Selection::only(id);
            self.editing = Some(textedit::TextEdit {
                id,
                new: true,
                cursor: 0,
                anchor: None,
            });
        } else {
            self.history.cancel_gesture();
        }
    }

    /// Start editing an existing text box (caret at the end).
    pub fn edit_text(&mut self, id: ObjectId) {
        self.commit_text();
        let doc = self.doc().clone();
        let Some(ObjRef::Shape(i)) = doc.find(id) else { return };
        let Geom::Text { text, .. } = &doc.shapes[i].geom else {
            return;
        };
        self.history.end_gesture();
        self.sel = Selection::only(id);
        self.editing = Some(textedit::TextEdit {
            id,
            new: false,
            cursor: text.len(),
            anchor: None,
        });
    }

    /// The selected part of the text being edited.
    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.editing.as_ref()?.selection()?;
        let (s, _) = self.edited()?;
        match &s.geom {
            Geom::Text { text, .. } => text.get(a..b).map(str::to_string),
            _ => None,
        }
    }

    /// The caret of the text being edited: its top and bottom in image space (where the input
    /// method shows its candidates).
    pub fn caret_line(&self) -> Option<(PointF, PointF)> {
        let e = self.editing.as_ref()?;
        let (s, l) = self.edited()?;
        let Geom::Text { rect, rotation, .. } = &s.geom else {
            return None;
        };
        let (x, top, h) = textedit::caret(&l, e.cursor);
        let c = rect.center();
        let (sn, co) = rotation.sin_cos();
        let img = |x: f32, y: f32| {
            let (dx, dy) = (x - rect.w / 2.0, y - rect.h / 2.0);
            PointF {
                x: c.x + dx * co - dy * sn,
                y: c.y + dx * sn + dy * co,
            }
        };
        Some((img(x, top), img(x, top + h)))
    }

    /// The edited box and its layout.
    fn edited(&self) -> Option<(Shape, std::sync::Arc<rv_core::edit::text::Layout>)> {
        let e = self.editing.as_ref()?;
        let doc = self.doc();
        let Some(ObjRef::Shape(i)) = doc.find(e.id) else {
            return None;
        };
        let s = (*doc.shapes[i]).clone();
        let Geom::Text {
            rect,
            text,
            font,
            align,
            ..
        } = &s.geom
        else {
            return None;
        };
        let l = rv_core::edit::text::layout(text, font, rect.w, *align);
        Some((s, l))
    }

    /// A point (image space) in the box's own frame (unrotated, from its top-left).
    fn box_local(rect: &RectF, rotation: f32, p: PointF) -> (f32, f32) {
        let c = rect.center();
        let (s, co) = (-rotation).sin_cos();
        let (dx, dy) = (p.x - c.x, p.y - c.y);
        (dx * co - dy * s + rect.w / 2.0, dx * s + dy * co + rect.h / 2.0)
    }

    /// A press while editing: inside the box → move the caret there (Shift: select). False = outside.
    fn caret_at(&mut self, p: PointF, extend: bool) -> bool {
        let Some((s, l)) = self.edited() else { return false };
        let Geom::Text {
            rect, rotation, text, ..
        } = &s.geom
        else {
            return false;
        };
        let (x, y) = Self::box_local(rect, *rotation, p);
        let tol = 4.0;
        if x < -tol || y < -tol || x > rect.w + tol || y > l.height + tol {
            return false;
        }
        let i = textedit::hit(&l, x, y, text.len());
        if let Some(e) = &mut self.editing {
            if extend {
                e.anchor.get_or_insert(e.cursor);
            } else {
                e.anchor = None;
            }
            e.cursor = i;
        }
        true
    }

    /// An editing command on the edited box: one history entry for the whole session.
    pub fn text_cmd(&mut self, cmd: textedit::Cmd, extend: bool) {
        let Some((s, l)) = self.edited() else {
            self.editing = None;
            return;
        };
        let Some(mut e) = self.editing.clone() else { return };
        let Geom::Text {
            rect,
            rotation,
            text,
            font,
            align,
        } = &s.geom
        else {
            return;
        };
        let new = e.apply(text, &l, cmd, extend);
        if new != *text {
            let geom = select::text_geom(*rect, *rotation, &new, font, *align);
            if !self.history.in_gesture() {
                self.history.begin_gesture();
            }
            let edit = if e.new {
                Edit::AddShape(Shape { geom, ..s })
            } else {
                Edit::UpdateShape {
                    id: e.id,
                    geom: Some(geom),
                    style: None,
                }
            };
            let _ = self.history.update_gesture(&edit);
        }
        self.editing = Some(e);
    }

    /// An Inspector change while editing: the typing so far becomes its history entry, editing
    /// goes on.
    pub fn pause_text(&mut self) {
        if let Some(e) = &mut self.editing
            && self.history.in_gesture()
        {
            self.history.end_gesture();
            e.new = false;
        }
    }

    /// Finish editing (Esc, a click outside, another tool): an empty box is removed.
    pub fn commit_text(&mut self) {
        let Some(e) = self.editing.take() else { return };
        let empty = self.edited_text(e.id).is_none_or(|t| t.trim().is_empty());
        match (empty, e.new && self.history.in_gesture()) {
            (true, true) => self.history.cancel_gesture(),
            (true, false) => {
                self.history.end_gesture();
                if self.doc().find(e.id).is_some() {
                    self.apply(Edit::Remove(vec![e.id]));
                }
            }
            (false, _) => self.history.end_gesture(),
        }
        self.after_change();
    }

    fn edited_text(&self, id: ObjectId) -> Option<String> {
        let doc = self.doc();
        let Some(ObjRef::Shape(i)) = doc.find(id) else {
            return None;
        };
        match &doc.shapes[i].geom {
            Geom::Text { text, .. } => Some(text.clone()),
            _ => None,
        }
    }

    /// Change the font/alignment of the selected or edited text boxes (or of the next one).
    pub fn set_text_font(&mut self, f: &dyn Fn(&mut FontSpec, &mut TextAlign)) {
        let mut any = false;
        self.set_geom(&|g| match g {
            Geom::Text {
                rect,
                rotation,
                text,
                font,
                align,
            } => {
                let (mut font, mut align) = (font.clone(), *align);
                f(&mut font, &mut align);
                Some(select::text_geom(*rect, *rotation, text, &font, align))
            }
            _ => None,
        });
        let doc = self.doc().clone();
        for id in self.sel.ids.iter().chain(self.editing.as_ref().map(|e| &e.id)) {
            if let Some(ObjRef::Shape(i)) = doc.find(*id)
                && let Geom::Text { font, align, .. } = &doc.shapes[i].geom
            {
                self.text_font = (font.clone(), *align);
                any = true;
            }
        }
        if !any {
            let (mut font, mut align) = self.text_font.clone();
            f(&mut font, &mut align);
            self.text_font = (font, align);
        }
    }

    // ---------------------------------------------------------------- keys

    /// `undo_keys` / `redo_keys`: the sequences bound to Undo and Redo.
    pub fn key(&mut self, seq: &str, undo_keys: &[String], redo_keys: &[String]) -> Out {
        if self.editing.is_some() {
            // typing goes to the text box (the window's key events); nothing else fires meanwhile
            if seq == "Esc" {
                self.commit_text();
            }
            return Out::Handled;
        }
        if undo_keys.iter().any(|k| k == seq) {
            self.undo();
            return Out::Handled;
        }
        if redo_keys.iter().any(|k| k == seq) {
            self.redo();
            return Out::Handled;
        }
        let has_sel = !self.sel.is_empty();
        let step = if seq.starts_with("Shift+") { 10.0 } else { 1.0 };
        match seq {
            "Del" | "Backspace" | "Ctrl+Backspace" => self.delete_selection(),
            // never to the Recycle Bin in Markup; rename disabled
            "Shift+Del" | "F2" | "Ctrl+R" | "Ctrl+Enter" => {}
            "Left" | "Shift+Left" if has_sel => self.nudge(-step, 0.0),
            "Right" | "Shift+Right" if has_sel => self.nudge(step, 0.0),
            "Up" | "Shift+Up" if has_sel => self.nudge(0.0, -step),
            "Down" | "Shift+Down" if has_sel => self.nudge(0.0, step),
            "Left" => return Out::Navigate(-1),
            "Right" => return Out::Navigate(1),
            // view rotation is off in Markup
            "Up" | "Down" | "Shift+Left" | "Shift+Right" | "Shift+Up" | "Shift+Down" => {}
            "Ctrl+A" => self.select_all(),
            "Ctrl+D" => self.duplicate(),
            "Ctrl+]" => self.reorder(true, false),
            "Ctrl+[" => self.reorder(false, false),
            "Ctrl+Shift+]" | "Ctrl+}" | "Ctrl+Shift+}" => self.reorder(true, true),
            "Ctrl+Shift+[" | "Ctrl+{" | "Ctrl+Shift+{" => self.reorder(false, true),
            "Tab" => self.cycle(false),
            "Shift+Tab" => self.cycle(true),
            "V" => self.set_tool(Tool::Select),
            "C" => self.set_tool(Tool::Crop),
            "R" | "E" | "L" | "A" | "P" => {
                if let Some(k) = overlay::kind_for_key(seq) {
                    self.set_tool(Tool::Draw(k));
                }
            }
            "B" => self.set_tool(Tool::Redact),
            "T" => self.set_tool(Tool::Text),
            "Enter" if self.adjusting() => self.confirm(),
            "Enter" if self.tool != Tool::Crop && self.single_text().is_some() => {
                let id = self.single_text().unwrap();
                self.edit_text(id);
                self.text_cmd(textedit::Cmd::SelectAll, false);
            }
            "Enter" if self.tool == Tool::Crop => {
                self.button("crop-apply");
            }
            "Esc" => {
                // a gesture, then the crop, then the selection, then Markup itself
                if self.drag.is_some() || self.history.in_gesture() {
                    self.history.cancel_gesture();
                    self.drag = None;
                    self.pending = None;
                } else if self.tool == Tool::Crop {
                    self.set_tool(Tool::Select);
                } else if has_sel {
                    self.sel.clear();
                } else {
                    return Out::Exit;
                }
            }
            _ => return Out::Pass,
        }
        Out::Handled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A just-drawn element is adjusted, not duplicated, until it is confirmed (and the pointer
    /// says so).
    #[test]
    fn adjust_until_confirmed() {
        let t = Affine {
            m: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        };
        let mut m = Markup::new(
            200,
            200,
            BTreeMap::new(),
            Tool::Draw(ShapeKind::Rect),
            PathBuf::from("a.png"),
        );
        let pt = |x: f32, y: f32| PointF { x, y };
        let drag = |m: &mut Markup, a: PointF, b: PointF| {
            m.press(a, (a.x, a.y), &t, 1.0, Mods::default());
            m.motion(b, 1.0, Mods::default());
            m.release(b);
        };
        assert_eq!(m.hover(pt(50.0, 50.0), (50.0, 50.0), &t, 1.0), Hover::Create);
        drag(&mut m, pt(20.0, 20.0), pt(80.0, 60.0));
        assert!(m.adjusting() && m.doc().shapes.len() == 1);
        assert_eq!(
            m.hover(pt(80.0, 60.0), (80.0, 60.0), &t, 1.0),
            Hover::Handle(Handle::Corner(2))
        );
        assert_eq!(m.hover(pt(50.0, 40.0), (50.0, 40.0), &t, 1.0), Hover::Move);
        assert_eq!(m.hover(pt(150.0, 150.0), (150.0, 150.0), &t, 1.0), Hover::None);
        // outside: nothing is created
        drag(&mut m, pt(120.0, 120.0), pt(180.0, 180.0));
        assert_eq!(m.doc().shapes.len(), 1);
        // inside (not on the outline): moved
        drag(&mut m, pt(50.0, 40.0), pt(60.0, 45.0));
        let Geom::Rect { rect, .. } = m.doc().shapes[0].geom else {
            panic!()
        };
        assert_eq!((rect.x, rect.y), (30.0, 25.0));
        // Enter confirms, the tool draws again; another tool confirms too
        assert_eq!(m.key("Enter", &[], &[]), Out::Handled);
        assert!(!m.adjusting());
        drag(&mut m, pt(120.0, 120.0), pt(180.0, 180.0));
        assert_eq!(m.doc().shapes.len(), 2);
        m.set_tool(Tool::Draw(ShapeKind::Line));
        assert!(!m.adjusting() && m.sel.is_empty());
    }
}
