//! Markup document model. Non-destructive: the source pixels are never changed, the
//! document is data. Every object (shape or redaction) has a stable `ObjectId` from one counter;
//! z-order is the position in its list. All geometry is in image space (source pixels after the
//! EXIF orientation, before the document's crop and rotation).

use crate::orient::Orient;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ObjectId(pub u64);

/// Identifies a state in memory; not saved. Revision(0) = not assigned yet (only after loading),
/// `History` always gives a real one (>= 1).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Revision(pub u64);

#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct PointF {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct VectorF {
    pub dx: f32,
    pub dy: f32,
}

/// Rectangle with non-negative width and height (see `RectF::from_points`).
#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct RectU {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct SizeU {
    pub w: u32,
    pub h: u32,
}

/// sRGB, straight alpha.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ArrowHeads {
    End,
    Both,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Geom {
    /// `rotation` (radians) around the centre of `rect`.
    Rect {
        rect: RectF,
        rotation: f32,
        corner_radius: f32,
    },
    Ellipse {
        rect: RectF,
        rotation: f32,
    },
    Line {
        a: PointF,
        b: PointF,
    },
    /// `bend`: how far the shaft curves, as a fraction of its length (0 = straight; the sign is
    /// the side it bows to).
    Arrow {
        a: PointF,
        b: PointF,
        heads: ArrowHeads,
        #[serde(default)]
        bend: f32,
    },
    /// Shared between snapshots.
    Pen {
        points: Arc<[PointF]>,
    },
    /// Wide semi-transparent stroke (an ordinary source-over with the style's opacity).
    Highlight {
        points: Arc<[PointF]>,
    },
    /// `rect.w` is the wrapping width; `rect.h` follows the laid-out text (the app keeps it in
    /// step). Text colour = the style's stroke colour, optional background = its fill.
    Text {
        rect: RectF,
        rotation: f32,
        text: String,
        font: FontSpec,
        #[serde(default)]
        align: TextAlign,
    },
    /// A circle showing the image under it magnified (`zoom` 2–4); outline = the stroke.
    Loupe {
        center: PointF,
        radius: f32,
        zoom: f32,
    },
}

/// Font of a text shape (by family name; a family missing on another PC falls back to Lato).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct FontSpec {
    pub family: String,
    /// Image-space pixels.
    pub size: f32,
    #[serde(default = "FontSpec::normal_weight")]
    pub weight: u16,
    #[serde(default)]
    pub italic: bool,
}

impl FontSpec {
    fn normal_weight() -> u16 {
        400
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Kinds of shapes, for the remembered last style and the UI.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShapeKind {
    Rect,
    Ellipse,
    Line,
    Arrow,
    Pen,
    Highlight,
    Text,
    Loupe,
}

impl Geom {
    pub fn kind(&self) -> ShapeKind {
        match self {
            Geom::Rect { .. } => ShapeKind::Rect,
            Geom::Ellipse { .. } => ShapeKind::Ellipse,
            Geom::Line { .. } => ShapeKind::Line,
            Geom::Arrow { .. } => ShapeKind::Arrow,
            Geom::Pen { .. } => ShapeKind::Pen,
            Geom::Highlight { .. } => ShapeKind::Highlight,
            Geom::Text { .. } => ShapeKind::Text,
            Geom::Loupe { .. } => ShapeKind::Loupe,
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Style {
    pub stroke: Option<Color>,
    pub fill: Option<Color>,
    /// Image-space pixels (the export looks the same at every zoom).
    pub width: f32,
    /// 0..=1, applied to the whole shape (fill and stroke together).
    pub opacity: f32,
}

impl Style {
    /// Default style of a new shape of this kind (red outline; the highlighter yellow and wide).
    pub fn default_for(kind: ShapeKind) -> Style {
        let red = Some(Color::rgb(0xE5, 0x39, 0x35));
        match kind {
            ShapeKind::Highlight => Style {
                stroke: Some(Color::rgb(0xFF, 0xEB, 0x3B)),
                fill: None,
                width: 24.0,
                opacity: 0.4,
            },
            ShapeKind::Loupe => Style {
                stroke: Some(Color::rgb(255, 255, 255)),
                fill: None,
                width: 3.0,
                opacity: 1.0,
            },
            // text colour = stroke; the width is not used
            ShapeKind::Text => Style {
                stroke: red,
                fill: None,
                width: 1.0,
                opacity: 1.0,
            },
            _ => Style {
                stroke: red,
                fill: None,
                width: 4.0,
                opacity: 1.0,
            },
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Shape {
    pub id: ObjectId,
    pub geom: Geom,
    pub style: Style,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Area {
    Rect { rect: RectF, rotation: f32 },
    Ellipse { rect: RectF, rotation: f32 },
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum RedactKind {
    /// Visual effect, NOT a protection.
    Blur { radius: f32 },
    /// Lowers legibility, does not guarantee protection.
    Pixelate { block: u32 },
    /// Opaque cover: the pixels are replaced, alpha 255.
    Solid { color: [u8; 3] },
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Redaction {
    pub id: ObjectId,
    pub area: Area,
    pub kind: RedactKind,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct DocState {
    #[serde(skip)]
    pub revision: Revision,
    /// Image-space size (after the EXIF orientation).
    pub image_size: SizeU,
    #[serde(default)]
    pub orient: Orient,
    /// In image space; None = the whole image.
    #[serde(default)]
    pub crop: Option<RectU>,
    /// Applied to the pixels, in order, below every shape.
    #[serde(default)]
    pub redactions: Vec<Arc<Redaction>>,
    /// z-order = index (0 = bottom).
    #[serde(default)]
    pub shapes: Vec<Arc<Shape>>,
    /// Greater than every ID in `shapes` and `redactions`.
    pub next_id: u64,
}

/// Where an ID lives (result of `DocState::find`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjRef {
    Shape(usize),
    Redaction(usize),
}

impl DocState {
    pub fn new(w: u32, h: u32) -> DocState {
        DocState {
            revision: Revision(0),
            image_size: SizeU {
                w: w.max(1),
                h: h.max(1),
            },
            orient: Orient::IDENTITY,
            crop: None,
            redactions: Vec::new(),
            shapes: Vec::new(),
            next_id: 1,
        }
    }

    /// The ID the next new object gets (`AddShape`/`AddRedaction` use it; `apply` moves the
    /// counter past it).
    pub fn peek_id(&self) -> ObjectId {
        ObjectId(self.next_id)
    }

    pub fn find(&self, id: ObjectId) -> Option<ObjRef> {
        if let Some(i) = self.shapes.iter().position(|s| s.id == id) {
            return Some(ObjRef::Shape(i));
        }
        self.redactions.iter().position(|r| r.id == id).map(ObjRef::Redaction)
    }

    /// The visible image region: the crop, or the whole image.
    pub fn region(&self) -> RectU {
        self.crop.unwrap_or(RectU {
            x: 0,
            y: 0,
            w: self.image_size.w,
            h: self.image_size.h,
        })
    }

    /// Size of the rendered document (crop, then orientation).
    pub fn canvas_size(&self) -> (u32, u32) {
        let r = self.region();
        self.orient.apply_size(r.w, r.h)
    }

    /// IDs unique across both lists and below `next_id` (checked by `apply` and at load).
    pub fn ids_valid(&self) -> bool {
        let mut ids: Vec<u64> = self
            .shapes
            .iter()
            .map(|s| s.id.0)
            .chain(self.redactions.iter().map(|r| r.id.0))
            .collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        ids.len() == n && ids.last().is_none_or(|m| *m < self.next_id)
    }
}

impl RectF {
    /// Normalised rectangle through two corners.
    pub fn from_points(a: PointF, b: PointF) -> RectF {
        RectF {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            w: (a.x - b.x).abs(),
            h: (a.y - b.y).abs(),
        }
    }

    pub fn center(&self) -> PointF {
        PointF {
            x: self.x + self.w / 2.0,
            y: self.y + self.h / 2.0,
        }
    }

    pub fn translated(self, d: VectorF) -> RectF {
        RectF {
            x: self.x + d.dx,
            y: self.y + d.dy,
            ..self
        }
    }

    pub fn contains_rect(&self, o: &RectF) -> bool {
        o.x >= self.x && o.y >= self.y && o.x + o.w <= self.x + self.w && o.y + o.h <= self.y + self.h
    }
}

impl PointF {
    pub fn translated(self, d: VectorF) -> PointF {
        PointF {
            x: self.x + d.dx,
            y: self.y + d.dy,
        }
    }
}
