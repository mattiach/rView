//! Edits: `DocState::apply` is pure — it returns a new state and shares every object
//! it doesn't touch (`Arc`). An edit on several IDs is all-or-nothing; errors never panic.

use super::model::*;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// The ID comes from `DocState::peek_id` (never reused).
    AddShape(Shape),
    AddRedaction(Redaction),
    UpdateShape {
        id: ObjectId,
        geom: Option<Geom>,
        style: Option<Style>,
    },
    UpdateRedaction {
        id: ObjectId,
        area: Option<Area>,
        kind: Option<RedactKind>,
    },
    /// Shapes and/or redactions.
    Translate {
        ids: Vec<ObjectId>,
        delta: VectorF,
    },
    Remove(Vec<ObjectId>),
    /// Index inside the object's own list (shapes among shapes, redactions among redactions).
    Reorder {
        id: ObjectId,
        to_index: usize,
    },
    /// New IDs; each copy goes on top of its own list.
    Duplicate {
        ids: Vec<ObjectId>,
        offset: VectorF,
    },
    SetCrop(Option<RectU>),
    Rotate {
        quarter_turns: i8,
    },
    FlipH,
    FlipV,
    /// The image adjustments, replaced as a whole.
    SetAdjust(super::adjust::Adjustments),
    /// Several edits as one (the Inspector changing every selected shape): applied in order,
    /// all-or-nothing.
    Batch(Vec<Edit>),
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum EditError {
    #[error("no object with id {0:?}")]
    UnknownId(ObjectId),
    #[error("object {0:?} is not of the kind this edit changes")]
    WrongKind(ObjectId),
    #[error("id {0:?} is already used or below the next free id")]
    IdReused(ObjectId),
    #[error("the object would be smaller than 1 px")]
    TooSmall,
    #[error("nothing to do")]
    Empty,
    #[error("a value is out of its range")]
    OutOfRange,
}

/// Smallest object size in image pixels.
const MIN: f32 = 1.0;

fn translate_geom(g: &Geom, d: VectorF) -> Geom {
    let pts = |p: &Arc<[PointF]>| -> Arc<[PointF]> { p.iter().map(|q| q.translated(d)).collect() };
    match g {
        Geom::Rect {
            rect,
            rotation,
            corner_radius,
        } => Geom::Rect {
            rect: rect.translated(d),
            rotation: *rotation,
            corner_radius: *corner_radius,
        },
        Geom::Ellipse { rect, rotation } => Geom::Ellipse {
            rect: rect.translated(d),
            rotation: *rotation,
        },
        Geom::Line { a, b } => Geom::Line {
            a: a.translated(d),
            b: b.translated(d),
        },
        Geom::Arrow { a, b, heads, bend } => Geom::Arrow {
            a: a.translated(d),
            b: b.translated(d),
            heads: *heads,
            bend: *bend,
        },
        Geom::Pen { points } => Geom::Pen { points: pts(points) },
        Geom::Highlight { points } => Geom::Highlight { points: pts(points) },
        Geom::Loupe { center, radius, zoom } => Geom::Loupe {
            center: center.translated(d),
            radius: *radius,
            zoom: *zoom,
        },
        Geom::Text {
            rect,
            rotation,
            text,
            font,
            align,
        } => Geom::Text {
            rect: rect.translated(d),
            rotation: *rotation,
            text: text.clone(),
            font: font.clone(),
            align: *align,
        },
        Geom::Image { rect, rotation, image } => Geom::Image {
            rect: rect.translated(d),
            rotation: *rotation,
            image: image.clone(),
        },
    }
}

fn translate_area(a: &Area, d: VectorF) -> Area {
    match a {
        Area::Rect { rect, rotation } => Area::Rect {
            rect: rect.translated(d),
            rotation: *rotation,
        },
        Area::Ellipse { rect, rotation } => Area::Ellipse {
            rect: rect.translated(d),
            rotation: *rotation,
        },
    }
}

fn finite(v: &[f32]) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// Geometry check: finite numbers, at least 1 px (a rect side, a line length, a path extent).
pub(crate) fn check_geom(g: &Geom) -> Result<(), EditError> {
    let ok = match g {
        Geom::Rect {
            rect,
            rotation,
            corner_radius,
        } => finite(&[rect.x, rect.y, rect.w, rect.h, *rotation, *corner_radius]) && rect.w >= MIN && rect.h >= MIN,
        Geom::Ellipse { rect, rotation } => {
            finite(&[rect.x, rect.y, rect.w, rect.h, *rotation]) && rect.w >= MIN && rect.h >= MIN
        }
        Geom::Line { a, b } => finite(&[a.x, a.y, b.x, b.y]) && ((a.x - b.x).hypot(a.y - b.y)) >= MIN,
        Geom::Arrow { a, b, bend, .. } => {
            finite(&[a.x, a.y, b.x, b.y, *bend]) && bend.abs() <= 1.0 && ((a.x - b.x).hypot(a.y - b.y)) >= MIN
        }
        Geom::Pen { points } | Geom::Highlight { points } => {
            !points.is_empty() && points.iter().all(|p| finite(&[p.x, p.y]))
        }
        Geom::Loupe { center, radius, zoom } => {
            finite(&[center.x, center.y, *radius, *zoom]) && *radius >= MIN && (1.0..=8.0).contains(zoom)
        }
        Geom::Text {
            rect, rotation, font, ..
        } => finite(&[rect.x, rect.y, rect.w, rect.h, *rotation, font.size]) && rect.w >= MIN && font.size > 0.0,
        // its pixels must come with it
        Geom::Image { rect, rotation, image } => {
            finite(&[rect.x, rect.y, rect.w, rect.h, *rotation])
                && rect.w >= MIN
                && rect.h >= MIN
                && image.data.as_ref().is_some_and(|d| d.id == image.id)
        }
    };
    if ok { Ok(()) } else { Err(EditError::TooSmall) }
}

pub(crate) fn check_style(s: &Style) -> Result<(), EditError> {
    if finite(&[s.width, s.opacity]) && s.width > 0.0 && (0.0..=1.0).contains(&s.opacity) {
        Ok(())
    } else {
        Err(EditError::TooSmall)
    }
}

pub(crate) fn check_area(a: &Area) -> Result<(), EditError> {
    let (Area::Rect { rect, rotation } | Area::Ellipse { rect, rotation }) = a;
    if finite(&[rect.x, rect.y, rect.w, rect.h, *rotation]) && rect.w >= MIN && rect.h >= MIN {
        Ok(())
    } else {
        Err(EditError::TooSmall)
    }
}

impl DocState {
    fn fresh_id(&self, id: ObjectId) -> Result<(), EditError> {
        if id.0 < self.next_id || self.find(id).is_some() {
            Err(EditError::IdReused(id))
        } else {
            Ok(())
        }
    }

    /// All IDs must exist (all-or-nothing edits).
    fn all_known(&self, ids: &[ObjectId]) -> Result<(), EditError> {
        match ids.iter().find(|id| self.find(**id).is_none()) {
            Some(id) => Err(EditError::UnknownId(*id)),
            None if ids.is_empty() => Err(EditError::Empty),
            None => Ok(()),
        }
    }

    /// Crop clamped to the image, at least 1×1.
    pub fn clamp_crop(&self, c: RectU) -> RectU {
        let (iw, ih) = (self.image_size.w, self.image_size.h);
        let x = c.x.min(iw - 1);
        let y = c.y.min(ih - 1);
        RectU {
            x,
            y,
            w: c.w.clamp(1, iw - x),
            h: c.h.clamp(1, ih - y),
        }
    }

    pub fn apply(&self, e: &Edit) -> Result<DocState, EditError> {
        let mut s = self.clone();
        match e {
            Edit::AddShape(shape) => {
                self.fresh_id(shape.id)?;
                check_geom(&shape.geom)?;
                check_style(&shape.style)?;
                s.next_id = shape.id.0 + 1;
                s.shapes.push(Arc::new(shape.clone()));
            }
            Edit::AddRedaction(r) => {
                self.fresh_id(r.id)?;
                check_area(&r.area)?;
                s.next_id = r.id.0 + 1;
                s.redactions.push(Arc::new(r.clone()));
            }
            Edit::UpdateShape { id, geom, style } => {
                let Some(ObjRef::Shape(i)) = self.find(*id) else {
                    return Err(match self.find(*id) {
                        Some(_) => EditError::WrongKind(*id),
                        None => EditError::UnknownId(*id),
                    });
                };
                let mut shape = (*s.shapes[i]).clone();
                if let Some(g) = geom {
                    check_geom(g)?;
                    shape.geom = g.clone();
                }
                if let Some(st) = style {
                    check_style(st)?;
                    shape.style = st.clone();
                }
                s.shapes[i] = Arc::new(shape);
            }
            Edit::UpdateRedaction { id, area, kind } => {
                let Some(ObjRef::Redaction(i)) = self.find(*id) else {
                    return Err(match self.find(*id) {
                        Some(_) => EditError::WrongKind(*id),
                        None => EditError::UnknownId(*id),
                    });
                };
                let mut r = (*s.redactions[i]).clone();
                if let Some(a) = area {
                    check_area(a)?;
                    r.area = a.clone();
                }
                if let Some(k) = kind {
                    r.kind = k.clone();
                }
                s.redactions[i] = Arc::new(r);
            }
            Edit::Translate { ids, delta } => {
                self.all_known(ids)?;
                if !finite(&[delta.dx, delta.dy]) {
                    return Err(EditError::TooSmall);
                }
                for id in ids {
                    match self.find(*id) {
                        Some(ObjRef::Shape(i)) => {
                            let mut sh = (*s.shapes[i]).clone();
                            sh.geom = translate_geom(&sh.geom, *delta);
                            s.shapes[i] = Arc::new(sh);
                        }
                        Some(ObjRef::Redaction(i)) => {
                            let mut r = (*s.redactions[i]).clone();
                            r.area = translate_area(&r.area, *delta);
                            s.redactions[i] = Arc::new(r);
                        }
                        None => unreachable!("checked by all_known"),
                    }
                }
            }
            Edit::Remove(ids) => {
                self.all_known(ids)?;
                s.shapes.retain(|x| !ids.contains(&x.id));
                s.redactions.retain(|x| !ids.contains(&x.id));
            }
            Edit::Reorder { id, to_index } => match self.find(*id) {
                Some(ObjRef::Shape(i)) => {
                    let x = s.shapes.remove(i);
                    let to = (*to_index).min(s.shapes.len());
                    s.shapes.insert(to, x);
                }
                Some(ObjRef::Redaction(i)) => {
                    let x = s.redactions.remove(i);
                    let to = (*to_index).min(s.redactions.len());
                    s.redactions.insert(to, x);
                }
                None => return Err(EditError::UnknownId(*id)),
            },
            Edit::Duplicate { ids, offset } => {
                self.all_known(ids)?;
                // copies in the originals' z-order, each on top of its own list
                let mut order: Vec<ObjRef> = ids.iter().filter_map(|id| self.find(*id)).collect();
                order.sort_by_key(|r| match r {
                    ObjRef::Redaction(i) => (0, *i),
                    ObjRef::Shape(i) => (1, *i),
                });
                order.dedup();
                for r in order {
                    let id = ObjectId(s.next_id);
                    s.next_id += 1;
                    match r {
                        ObjRef::Shape(i) => {
                            let mut c = (*self.shapes[i]).clone();
                            c.id = id;
                            c.geom = translate_geom(&c.geom, *offset);
                            s.shapes.push(Arc::new(c));
                        }
                        ObjRef::Redaction(i) => {
                            let mut c = (*self.redactions[i]).clone();
                            c.id = id;
                            c.area = translate_area(&c.area, *offset);
                            s.redactions.push(Arc::new(c));
                        }
                    }
                }
            }
            Edit::SetCrop(c) => s.crop = c.map(|c| self.clamp_crop(c)),
            Edit::Rotate { quarter_turns } => s.orient = s.orient.rotated(*quarter_turns),
            Edit::FlipH => s.orient = s.orient.mirrored(),
            Edit::FlipV => s.orient = s.orient.flipped(),
            Edit::SetAdjust(a) => {
                if !a.valid() {
                    return Err(EditError::OutOfRange);
                }
                s.adjust = *a;
            }
            Edit::Batch(list) => {
                if list.is_empty() {
                    return Err(EditError::Empty);
                }
                for e in list {
                    s = s.apply(e)?;
                }
            }
        }
        debug_assert!(s.ids_valid(), "id invariant after {e:?}");
        Ok(s)
    }
}
