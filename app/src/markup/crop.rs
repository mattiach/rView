//! Crop tool: a rectangle with 8 handles over the whole image, aspect presets (Free,
//! Original, 1:1, 4:3, 3:2, 16:9; X swaps them), Enter applies, Esc cancels. Non-destructive: the
//! draft starts from the document's crop, and Reset goes back to the whole image.

use super::select::{Handle, resize};
use rv_core::edit::{DocState, PointF, RectF, RectU};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Aspect {
    Free,
    Original,
    Ratio(u32, u32),
}

/// (message id, aspect) in menu order.
pub const PRESETS: &[(&str, Aspect)] = &[
    ("markup-crop-free", Aspect::Free),
    ("markup-crop-original", Aspect::Original),
    ("markup-crop-1-1", Aspect::Ratio(1, 1)),
    ("markup-crop-4-3", Aspect::Ratio(4, 3)),
    ("markup-crop-3-2", Aspect::Ratio(3, 2)),
    ("markup-crop-16-9", Aspect::Ratio(16, 9)),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Grab {
    Handle(Handle),
    Move,
    /// A new rectangle from this corner.
    New(PointF),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CropDraft {
    /// Image space.
    pub rect: RectF,
    pub aspect: Aspect,
    /// X: width and height of the ratio swapped.
    pub inverted: bool,
    pub drag: Option<(Grab, RectF, PointF)>,
}

impl CropDraft {
    pub fn new(doc: &DocState) -> CropDraft {
        let r = doc.region();
        CropDraft {
            rect: RectF {
                x: r.x as f32,
                y: r.y as f32,
                w: r.w as f32,
                h: r.h as f32,
            },
            aspect: Aspect::Free,
            inverted: false,
            drag: None,
        }
    }

    /// Width / height in image space, if the aspect is locked. Presets are as seen on screen, so
    /// with the document turned 90° they are swapped in image space.
    pub fn ratio(&self, doc: &DocState) -> Option<f32> {
        let shown = match self.aspect {
            Aspect::Free => return None,
            Aspect::Original => {
                let (w, h) = doc.orient.apply_size(doc.image_size.w, doc.image_size.h);
                w as f32 / h as f32
            }
            Aspect::Ratio(a, b) => a as f32 / b as f32,
        };
        let shown = if self.inverted { 1.0 / shown } else { shown };
        Some(if doc.orient.swaps_axes() { 1.0 / shown } else { shown })
    }

    pub fn set_aspect(&mut self, doc: &DocState, a: Aspect) {
        self.aspect = a;
        self.rect = self.constrained(doc, self.rect, Grab::Move);
    }

    pub fn invert(&mut self, doc: &DocState) {
        self.inverted = !self.inverted;
        self.rect = self.constrained(doc, self.rect, Grab::Move);
    }

    pub fn reset(&mut self, doc: &DocState) {
        self.rect = RectF {
            x: 0.0,
            y: 0.0,
            w: doc.image_size.w as f32,
            h: doc.image_size.h as f32,
        };
        self.rect = self.constrained(doc, self.rect, Grab::Move);
    }

    pub fn begin(&mut self, grab: Grab, p: PointF) {
        self.drag = Some((grab, self.rect, p));
    }

    /// Pointer moved to `p` (image space) during a drag.
    pub fn drag_to(&mut self, doc: &DocState, p: PointF) {
        let Some((grab, start, from)) = self.drag else { return };
        let r = match grab {
            Grab::Handle(h) => resize(&start, 0.0, h, p, false),
            Grab::Move => RectF {
                x: start.x + p.x - from.x,
                y: start.y + p.y - from.y,
                ..start
            },
            Grab::New(c) => RectF::from_points(c, p),
        };
        self.rect = self.constrained(doc, r, grab);
    }

    pub fn end(&mut self) {
        self.drag = None;
    }

    /// The aspect applied (around the fixed side or the centre), then kept inside the image.
    fn constrained(&self, doc: &DocState, mut r: RectF, grab: Grab) -> RectF {
        let (iw, ih) = (doc.image_size.w as f32, doc.image_size.h as f32);
        if grab == Grab::Move {
            r.w = r.w.min(iw);
            r.h = r.h.min(ih);
        }
        if let Some(q) = self.ratio(doc) {
            let c = r.center();
            let (w, h) = if r.w / q > r.h { (r.w, r.w / q) } else { (r.h * q, r.h) };
            // too big for the image: the largest rectangle of that aspect that fits
            let s = (iw / w).min(ih / h).min(1.0);
            let (w, h) = (w * s, h * s);
            r = match grab {
                Grab::Handle(Handle::Corner(0)) => RectF {
                    x: r.x + r.w - w,
                    y: r.y + r.h - h,
                    w,
                    h,
                },
                Grab::Handle(Handle::Corner(1)) => RectF {
                    x: r.x,
                    y: r.y + r.h - h,
                    w,
                    h,
                },
                Grab::Handle(Handle::Corner(3)) => RectF {
                    x: r.x + r.w - w,
                    y: r.y,
                    w,
                    h,
                },
                Grab::Handle(Handle::Corner(_)) | Grab::New(_) => RectF { x: r.x, y: r.y, w, h },
                _ => RectF {
                    x: c.x - w / 2.0,
                    y: c.y - h / 2.0,
                    w,
                    h,
                },
            };
        }
        if grab == Grab::Move {
            r.x = r.x.clamp(0.0, iw - r.w);
            r.y = r.y.clamp(0.0, ih - r.h);
            return r;
        }
        // resizing: cut what is outside the image (the aspect may then be off by the cut)
        let x0 = r.x.max(0.0);
        let y0 = r.y.max(0.0);
        let x1 = (r.x + r.w).min(iw);
        let y1 = (r.y + r.h).min(ih);
        RectF {
            x: x0,
            y: y0,
            w: (x1 - x0).max(1.0),
            h: (y1 - y0).max(1.0),
        }
    }

    /// The crop to store: whole pixels; None when it is the whole image.
    pub fn to_crop(&self, doc: &DocState) -> Option<RectU> {
        let (iw, ih) = (doc.image_size.w, doc.image_size.h);
        let x = self.rect.x.round().clamp(0.0, (iw - 1) as f32) as u32;
        let y = self.rect.y.round().clamp(0.0, (ih - 1) as f32) as u32;
        let w = (self.rect.w.round() as u32).clamp(1, iw - x);
        let h = (self.rect.h.round() as u32).clamp(1, ih - y);
        let c = RectU { x, y, w, h };
        (c != RectU {
            x: 0,
            y: 0,
            w: iw,
            h: ih,
        })
        .then_some(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rv_core::edit::Edit;

    #[test]
    fn presets_clamp_and_store() {
        let doc = DocState::new(400, 200);
        let mut d = CropDraft::new(&doc);
        assert_eq!(d.to_crop(&doc), None, "whole image = no crop");
        d.set_aspect(&doc, Aspect::Ratio(1, 1));
        assert_eq!((d.rect.w, d.rect.h), (200.0, 200.0), "largest square that fits");
        assert_eq!(d.rect.x, 100.0, "centred");
        // drag a new rectangle outside the image: cut at the borders
        d.set_aspect(&doc, Aspect::Free);
        d.begin(Grab::New(PointF { x: 350.0, y: 150.0 }), PointF { x: 350.0, y: 150.0 });
        d.drag_to(&doc, PointF { x: 500.0, y: 300.0 });
        d.end();
        assert_eq!(
            d.to_crop(&doc),
            Some(RectU {
                x: 350,
                y: 150,
                w: 50,
                h: 50
            })
        );
        // move stays inside
        d.begin(Grab::Move, PointF { x: 360.0, y: 160.0 });
        d.drag_to(&doc, PointF { x: 900.0, y: 900.0 });
        assert_eq!(
            d.to_crop(&doc),
            Some(RectU {
                x: 350,
                y: 150,
                w: 50,
                h: 50
            })
        );
        d.drag_to(&doc, PointF { x: 0.0, y: 0.0 });
        assert_eq!(
            d.to_crop(&doc),
            Some(RectU {
                x: 0,
                y: 0,
                w: 50,
                h: 50
            })
        );
        d.end();
        // 16:9 inverted on a document turned 90°: 9:16 on screen = 16:9 in image space
        let turned = doc.apply(&Edit::Rotate { quarter_turns: 1 }).unwrap();
        let mut d = CropDraft::new(&turned);
        d.aspect = Aspect::Ratio(16, 9);
        d.inverted = true;
        assert!((d.ratio(&turned).unwrap() - 16.0 / 9.0).abs() < 1e-6);
        d.reset(&turned);
        assert!((d.rect.w / d.rect.h - 16.0 / 9.0).abs() < 1e-3);
        // Original on a turned document = the shown image's aspect (200:400) → image space 2:1
        d.inverted = false;
        d.set_aspect(&turned, Aspect::Original);
        assert!((d.ratio(&turned).unwrap() - 2.0).abs() < 1e-6);
    }
}
