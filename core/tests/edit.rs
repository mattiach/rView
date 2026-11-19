//! Markup core: shapes/undo, dirty/revisions, serde, crop and transforms, rendering/compositing.
//! Golden images are kept as hashes of the rendered pixels (no binary
//! test assets in the repository); `RVIEW_GOLDEN_DIR=<folder>` writes them as PNG to look at.

use rv_core::edit::render::{Source, render, same};
use rv_core::edit::*;
use std::sync::Arc;

fn rect(x: f32, y: f32, w: f32, h: f32) -> RectF {
    RectF { x, y, w, h }
}

fn pt(x: f32, y: f32) -> PointF {
    PointF { x, y }
}

fn shape(doc: &DocState, geom: Geom, style: Style) -> Shape {
    Shape {
        id: doc.peek_id(),
        geom,
        style,
    }
}

fn red_box(doc: &DocState) -> Shape {
    shape(
        doc,
        Geom::Rect {
            rect: rect(10.0, 10.0, 20.0, 10.0),
            rotation: 0.0,
            corner_radius: 0.0,
        },
        Style::default_for(ShapeKind::Rect),
    )
}

fn redaction(doc: &DocState) -> Redaction {
    Redaction {
        id: doc.peek_id(),
        area: Area::Rect {
            rect: rect(40.0, 40.0, 10.0, 10.0),
            rotation: 0.0,
        },
        kind: RedactKind::Solid { color: [0, 0, 0] },
    }
}

fn ids(doc: &DocState) -> (Vec<u64>, Vec<u64>) {
    (
        doc.shapes.iter().map(|s| s.id.0).collect(),
        doc.redactions.iter().map(|r| r.id.0).collect(),
    )
}

#[test]
fn ids_are_stable_and_shared_between_shapes_and_redactions() {
    let mut h = History::new(DocState::new(100, 80));
    let a = red_box(h.current());
    h.apply(&Edit::AddShape(a)).unwrap();
    let r = redaction(h.current());
    h.apply(&Edit::AddRedaction(r)).unwrap();
    let b = red_box(h.current());
    h.apply(&Edit::AddShape(b)).unwrap();
    assert_eq!(ids(h.current()), (vec![1, 3], vec![2]), "one counter for both lists");
    // an ID can't be reused, not even after removal
    let stale = Shape {
        id: ObjectId(1),
        ..red_box(h.current())
    };
    assert_eq!(
        h.current().apply(&Edit::AddShape(stale)),
        Err(EditError::IdReused(ObjectId(1)))
    );
    h.apply(&Edit::Reorder {
        id: ObjectId(3),
        to_index: 0,
    })
    .unwrap();
    assert_eq!(ids(h.current()).0, vec![3, 1], "reorder keeps IDs");
    h.apply(&Edit::Duplicate {
        ids: vec![ObjectId(1), ObjectId(2)],
        offset: VectorF { dx: 5.0, dy: 5.0 },
    })
    .unwrap();
    // redactions first in z, then shapes: copies get 4 (redaction) and 5 (shape)
    assert_eq!(ids(h.current()), (vec![3, 1, 5], vec![2, 4]));
    h.apply(&Edit::Remove(vec![ObjectId(3), ObjectId(4)])).unwrap();
    assert_eq!(ids(h.current()), (vec![1, 5], vec![2]));
    h.undo();
    h.undo();
    assert_eq!(ids(h.current()), (vec![3, 1], vec![2]));
    h.redo();
    assert_eq!(ids(h.current()), (vec![3, 1, 5], vec![2, 4]));
    assert!(h.current().ids_valid());
    // a new object after undo/redo still gets a fresh ID
    assert_eq!(h.current().peek_id(), ObjectId(6));
}

#[test]
fn errors_never_change_anything() {
    let mut h = History::new(DocState::new(100, 80));
    h.apply(&Edit::AddShape(red_box(h.current()))).unwrap();
    h.apply(&Edit::AddRedaction(redaction(h.current()))).unwrap();
    let before = h.current().clone();
    let style = Some(Style::default_for(ShapeKind::Rect));
    let e = |e: Edit| h.current().apply(&e);
    assert_eq!(
        e(Edit::UpdateShape {
            id: ObjectId(99),
            geom: None,
            style: style.clone()
        }),
        Err(EditError::UnknownId(ObjectId(99)))
    );
    assert_eq!(
        e(Edit::UpdateShape {
            id: ObjectId(2),
            geom: None,
            style: style.clone()
        }),
        Err(EditError::WrongKind(ObjectId(2)))
    );
    assert_eq!(
        e(Edit::UpdateRedaction {
            id: ObjectId(1),
            area: None,
            kind: None
        }),
        Err(EditError::WrongKind(ObjectId(1)))
    );
    // multi-ID edits are all-or-nothing
    let d = VectorF { dx: 1.0, dy: 1.0 };
    assert_eq!(
        e(Edit::Translate {
            ids: vec![ObjectId(1), ObjectId(42)],
            delta: d
        }),
        Err(EditError::UnknownId(ObjectId(42)))
    );
    assert!(e(Edit::Remove(vec![ObjectId(2), ObjectId(42)])).is_err());
    assert!(
        e(Edit::Duplicate {
            ids: vec![ObjectId(42)],
            offset: d
        })
        .is_err()
    );
    // too small or not finite
    let tiny = Shape {
        id: h.current().peek_id(),
        geom: Geom::Line {
            a: pt(1.0, 1.0),
            b: pt(1.2, 1.0),
        },
        style: Style::default_for(ShapeKind::Line),
    };
    assert_eq!(e(Edit::AddShape(tiny)), Err(EditError::TooSmall));
    let nan = Shape {
        id: h.current().peek_id(),
        geom: Geom::Line {
            a: pt(f32::NAN, 1.0),
            b: pt(20.0, 1.0),
        },
        style: Style::default_for(ShapeKind::Line),
    };
    assert_eq!(e(Edit::AddShape(nan)), Err(EditError::TooSmall));
    // a batch fails as a whole when one part fails
    assert_eq!(
        e(Edit::Batch(vec![Edit::FlipH, Edit::Remove(vec![ObjectId(42)])])),
        Err(EditError::UnknownId(ObjectId(42)))
    );
    assert!(h.apply(&Edit::Remove(vec![ObjectId(42)])).is_err());
    assert_eq!(**h.current(), *before, "nothing changed");
    assert_eq!(h.entries(), 2, "failed edits make no entry");
}

#[test]
fn mixed_selection_translate_remove_duplicate() {
    let mut h = History::new(DocState::new(100, 80));
    h.apply(&Edit::AddShape(red_box(h.current()))).unwrap();
    h.apply(&Edit::AddRedaction(redaction(h.current()))).unwrap();
    let both = vec![ObjectId(1), ObjectId(2)];
    h.apply(&Edit::Translate {
        ids: both.clone(),
        delta: VectorF { dx: 3.0, dy: -2.0 },
    })
    .unwrap();
    let d = h.current();
    assert!(matches!(&d.shapes[0].geom, Geom::Rect { rect, .. } if rect.x == 13.0 && rect.y == 8.0));
    assert!(matches!(&d.redactions[0].area, Area::Rect { rect, .. } if rect.x == 43.0 && rect.y == 38.0));
    h.apply(&Edit::Duplicate {
        ids: both.clone(),
        offset: VectorF::default(),
    })
    .unwrap();
    assert_eq!(ids(h.current()), (vec![1, 4], vec![2, 3]));
    h.apply(&Edit::Remove(vec![ObjectId(1), ObjectId(2), ObjectId(3), ObjectId(4)]))
        .unwrap();
    assert!(h.current().shapes.is_empty() && h.current().redactions.is_empty());
    // reorder stays inside its own list
    h.undo();
    h.apply(&Edit::Reorder {
        id: ObjectId(2),
        to_index: 99,
    })
    .unwrap();
    assert_eq!(
        ids(h.current()),
        (vec![1, 4], vec![3, 2]),
        "redaction moved among redactions"
    );
}

#[test]
fn gestures_dirty_and_revisions() {
    let mut h = History::new(DocState::new(100, 80));
    assert!(!h.is_dirty() && !h.can_undo());
    h.apply(&Edit::AddShape(red_box(h.current()))).unwrap();
    assert!(h.is_dirty());
    // a drag: many updates, one entry
    h.begin_gesture();
    for i in 1..=20 {
        h.update_gesture(&Edit::Translate {
            ids: vec![ObjectId(1)],
            delta: VectorF { dx: i as f32, dy: 0.0 },
        })
        .unwrap();
    }
    h.end_gesture();
    assert_eq!(h.entries(), 2, "add + one drag");
    assert!(
        matches!(&h.current().shapes[0].geom, Geom::Rect { rect, .. } if rect.x == 30.0),
        "total delta, not summed"
    );
    // Esc in a drag: back, no entry
    h.begin_gesture();
    h.update_gesture(&Edit::Translate {
        ids: vec![ObjectId(1)],
        delta: VectorF { dx: 50.0, dy: 0.0 },
    })
    .unwrap();
    h.cancel_gesture();
    assert_eq!(h.entries(), 2);
    assert!(matches!(&h.current().shapes[0].geom, Geom::Rect { rect, .. } if rect.x == 30.0));
    // undo during a gesture closes it first
    h.update_gesture(&Edit::Translate {
        ids: vec![ObjectId(1)],
        delta: VectorF { dx: 1.0, dy: 0.0 },
    })
    .unwrap();
    assert!(h.undo() && !h.in_gesture());
    assert!(matches!(&h.current().shapes[0].geom, Geom::Rect { rect, .. } if rect.x == 30.0));
    // saved / exported / undo back to the saved state
    h.mark_saved();
    assert!(!h.is_dirty());
    h.apply(&Edit::FlipH).unwrap();
    h.mark_exported();
    assert!(h.is_dirty(), "Export leaves the document dirty");
    assert!(h.exported_current());
    h.undo();
    assert!(!h.is_dirty(), "undo to the saved revision → clean");
    assert!(!h.exported_current());
    h.redo();
    assert!(h.is_dirty() && h.exported_current());
    // revisions are unique and never 0
    assert!(h.current().revision.0 >= 1);
}

#[test]
fn history_limit() {
    let mut h = History::new(DocState::new(100, 80));
    for _ in 0..history::MAX_ENTRIES + 20 {
        h.apply(&Edit::FlipH).unwrap();
    }
    assert_eq!(h.entries(), history::MAX_ENTRIES);
    // a big pen path shared between snapshots is counted once
    let pts: Arc<[PointF]> = (0..10_000)
        .map(|i| pt(i as f32 % 90.0, (i / 90) as f32 % 70.0))
        .collect();
    let pen = shape(
        h.current(),
        Geom::Pen { points: pts },
        Style::default_for(ShapeKind::Pen),
    );
    h.apply(&Edit::AddShape(pen)).unwrap();
    for _ in 0..50 {
        h.apply(&Edit::FlipV).unwrap();
    }
    assert_eq!(
        h.entries(),
        history::MAX_ENTRIES,
        "still bounded by entries, not by bytes"
    );
}

#[test]
fn serde_round_trip() {
    let mut h = History::new(DocState::new(64, 48));
    h.apply(&Edit::AddShape(red_box(h.current()))).unwrap();
    let pen = shape(
        h.current(),
        Geom::Pen {
            points: vec![pt(1.0, 2.0), pt(3.5, 4.25)].into(),
        },
        Style::default_for(ShapeKind::Pen),
    );
    h.apply(&Edit::AddShape(pen)).unwrap();
    let arrow = shape(
        h.current(),
        Geom::Arrow {
            a: pt(1.0, 1.0),
            b: pt(30.0, 20.0),
            heads: ArrowHeads::Both,
            bend: 0.0,
        },
        Style {
            fill: Some(Color::rgb(1, 2, 3)),
            ..Style::default_for(ShapeKind::Arrow)
        },
    );
    h.apply(&Edit::AddShape(arrow)).unwrap();
    h.apply(&Edit::AddRedaction(redaction(h.current()))).unwrap();
    h.apply(&Edit::SetCrop(Some(RectU {
        x: 2,
        y: 3,
        w: 40,
        h: 30,
    })))
    .unwrap();
    h.apply(&Edit::Rotate { quarter_turns: 1 }).unwrap();
    let json = serde_json::to_string(&**h.current()).unwrap();
    let back: DocState = serde_json::from_str(&json).unwrap();
    assert_eq!(back.revision, Revision(0), "revision is not saved");
    let mut expected = (**h.current()).clone();
    expected.revision = Revision(0);
    assert_eq!(back, expected);
    let loaded = History::from_loaded(back);
    assert!(loaded.current().revision.0 >= 1 && !loaded.is_dirty());
    assert_eq!(loaded.current().next_id, h.current().next_id);
}

#[test]
fn crop_and_document_rotation() {
    let mut h = History::new(DocState::new(100, 80));
    h.apply(&Edit::SetCrop(Some(RectU {
        x: 90,
        y: 70,
        w: 500,
        h: 500,
    })))
    .unwrap();
    assert_eq!(
        h.current().crop,
        Some(RectU {
            x: 90,
            y: 70,
            w: 10,
            h: 10
        }),
        "clamped to the image"
    );
    h.apply(&Edit::SetCrop(Some(RectU {
        x: 500,
        y: 500,
        w: 0,
        h: 0,
    })))
    .unwrap();
    assert_eq!(
        h.current().crop,
        Some(RectU {
            x: 99,
            y: 79,
            w: 1,
            h: 1
        }),
        "at least 1×1"
    );
    // an object outside the crop stays in the document
    h.apply(&Edit::SetCrop(Some(RectU {
        x: 50,
        y: 0,
        w: 50,
        h: 40,
    })))
    .unwrap();
    h.apply(&Edit::AddShape(red_box(h.current()))).unwrap();
    h.apply(&Edit::SetCrop(None)).unwrap();
    assert_eq!(h.current().shapes.len(), 1);
    // rotations and flips are a group
    let o = h.current().orient;
    for _ in 0..4 {
        h.apply(&Edit::Rotate { quarter_turns: 1 }).unwrap();
    }
    assert_eq!(h.current().orient, o);
    h.apply(&Edit::FlipH).unwrap();
    h.apply(&Edit::FlipH).unwrap();
    h.apply(&Edit::FlipV).unwrap();
    h.apply(&Edit::FlipV).unwrap();
    assert_eq!(h.current().orient, o);
    // canvas size after crop + rotate
    h.apply(&Edit::SetCrop(Some(RectU {
        x: 10,
        y: 5,
        w: 30,
        h: 20,
    })))
    .unwrap();
    h.apply(&Edit::Rotate { quarter_turns: -1 }).unwrap();
    assert_eq!(h.current().canvas_size(), (20, 30));
}

// ---------------------------------------------------------------------------------------------
// rendering

fn source(w: u32, h: u32) -> Vec<u8> {
    // grey background with a coloured top-left marker (orientation checks)
    let mut v = vec![0u8; (w * h * 4) as usize];
    for (i, p) in v.chunks_exact_mut(4).enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let c = if x < 4 && y < 4 { [0, 200, 0] } else { [128, 128, 128] };
        p.copy_from_slice(&[c[0], c[1], c[2], 255]);
    }
    v
}

fn px(img: &(u32, u32, Vec<u8>), x: u32, y: u32) -> [u8; 4] {
    let i = ((y * img.0 + x) * 4) as usize;
    img.2[i..i + 4].try_into().unwrap()
}

/// FNV-1a 64 of the pixels and size: the golden value.
fn hash(img: &(u32, u32, Vec<u8>)) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in img.0.to_le_bytes().iter().chain(&img.1.to_le_bytes()).chain(&img.2) {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn golden(name: &str, img: &(u32, u32, Vec<u8>), expected: u64) {
    if let Some(dir) = std::env::var_os("RVIEW_GOLDEN_DIR") {
        let p = std::path::Path::new(&dir).join(format!("{name}.png"));
        image::RgbaImage::from_raw(img.0, img.1, img.2.clone())
            .unwrap()
            .save(&p)
            .unwrap();
    }
    let got = hash(img);
    assert_eq!(
        got, expected,
        "golden {name}: hash {got:#018x} (RVIEW_GOLDEN_DIR=<folder> to see it)"
    );
}

fn doc_with(shapes: Vec<(Geom, Style)>) -> DocState {
    let mut d = DocState::new(64, 48);
    for (g, s) in shapes {
        let sh = Shape {
            id: d.peek_id(),
            geom: g,
            style: s,
        };
        d = d.apply(&Edit::AddShape(sh)).unwrap();
    }
    d
}

fn draw(d: &DocState) -> (u32, u32, Vec<u8>) {
    let src = source(64, 48);
    render(
        d,
        &Source {
            w: 64,
            h: 48,
            rgba: &src,
        },
        &same,
    )
}

const RED: Color = Color::rgb(255, 0, 0);
const BLUE: Color = Color::rgb(0, 0, 255);

fn solid(stroke: Option<Color>, fill: Option<Color>, width: f32, opacity: f32) -> Style {
    Style {
        stroke,
        fill,
        width,
        opacity,
    }
}

#[test]
fn render_each_shape_kind() {
    let cases: Vec<(&str, Geom, Style, u64)> = vec![
        (
            "rect_stroke",
            Geom::Rect {
                rect: rect(10.0, 10.0, 30.0, 20.0),
                rotation: 0.0,
                corner_radius: 0.0,
            },
            solid(Some(RED), None, 4.0, 1.0),
            0xa3e70f2c11f04cb5,
        ),
        (
            "rect_fill_rounded_rotated",
            Geom::Rect {
                rect: rect(12.0, 10.0, 30.0, 20.0),
                rotation: 0.4,
                corner_radius: 6.0,
            },
            solid(Some(RED), Some(BLUE), 3.0, 1.0),
            0xf64bc9823f9ca021,
        ),
        (
            "ellipse_fill",
            Geom::Ellipse {
                rect: rect(8.0, 6.0, 40.0, 30.0),
                rotation: 0.0,
            },
            solid(None, Some(BLUE), 1.0, 1.0),
            0x29856ca629228419,
        ),
        (
            "line",
            Geom::Line {
                a: pt(5.0, 40.0),
                b: pt(58.0, 8.0),
            },
            solid(Some(RED), None, 5.0, 1.0),
            0xc37e2a32172c2895,
        ),
        (
            "arrow_both",
            Geom::Arrow {
                a: pt(8.0, 24.0),
                b: pt(56.0, 24.0),
                heads: ArrowHeads::Both,
                bend: 0.0,
            },
            solid(Some(RED), None, 3.0, 1.0),
            0xb321cd26ac2ffe75,
        ),
        (
            "pen",
            Geom::Pen {
                points: vec![pt(5.0, 5.0), pt(20.0, 30.0), pt(40.0, 10.0), pt(60.0, 40.0)].into(),
            },
            solid(Some(BLUE), None, 3.0, 1.0),
            0x60a7253bffec8f75,
        ),
        (
            "highlight",
            Geom::Highlight {
                points: vec![pt(4.0, 24.0), pt(60.0, 24.0)].into(),
            },
            Style::default_for(ShapeKind::Highlight),
            0xee200e4166a51e59,
        ),
        (
            "rect_half_opacity",
            Geom::Rect {
                rect: rect(10.0, 10.0, 30.0, 20.0),
                rotation: 0.0,
                corner_radius: 0.0,
            },
            solid(Some(RED), Some(BLUE), 6.0, 0.5),
            0xd6a33d72a91b7555,
        ),
    ];
    let mut failures = vec![];
    for (name, g, s, expected) in cases {
        let img = draw(&doc_with(vec![(g, s)]));
        if let Err(e) = std::panic::catch_unwind(|| golden(name, &img, expected)) {
            failures.push(e.downcast_ref::<String>().cloned().unwrap_or_default());
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn render_semantics() {
    let bg = [128, 128, 128, 255];
    // stroke only: centre untouched, edge red
    let img = draw(&doc_with(vec![(
        Geom::Rect {
            rect: rect(10.0, 10.0, 30.0, 20.0),
            rotation: 0.0,
            corner_radius: 0.0,
        },
        solid(Some(RED), None, 4.0, 1.0),
    )]));
    assert_eq!((img.0, img.1), (64, 48));
    assert_eq!(px(&img, 25, 20), bg, "inside an unfilled rect");
    assert_eq!(px(&img, 10, 20), [255, 0, 0, 255], "on the outline");
    assert_eq!(px(&img, 5, 5), bg);
    // z-order: the later shape is on top
    let img = draw(&doc_with(vec![
        (
            Geom::Rect {
                rect: rect(10.0, 10.0, 20.0, 20.0),
                rotation: 0.0,
                corner_radius: 0.0,
            },
            solid(None, Some(RED), 1.0, 1.0),
        ),
        (
            Geom::Rect {
                rect: rect(20.0, 10.0, 20.0, 20.0),
                rotation: 0.0,
                corner_radius: 0.0,
            },
            solid(None, Some(BLUE), 1.0, 1.0),
        ),
    ]));
    assert_eq!(px(&img, 25, 20), [0, 0, 255, 255]);
    assert_eq!(px(&img, 15, 20), [255, 0, 0, 255]);
    // opacity 0.5 over grey: source-over on encoded values, fill and stroke as one layer
    let img = draw(&doc_with(vec![(
        Geom::Rect {
            rect: rect(10.0, 10.0, 30.0, 20.0),
            rotation: 0.0,
            corner_radius: 0.0,
        },
        solid(Some(RED), Some(BLUE), 6.0, 0.5),
    )]));
    let inside = px(&img, 25, 20);
    assert!(
        inside[0].abs_diff(64) <= 1 && inside[2].abs_diff(191) <= 1,
        "blue over grey at 50 %: {inside:?}"
    );
    let edge = px(&img, 10, 20);
    assert!(
        edge[0].abs_diff(191) <= 1 && edge[2].abs_diff(64) <= 1,
        "outline over grey, not over the fill: {edge:?}"
    );
}

#[test]
fn render_crop_and_orientation() {
    let base = doc_with(vec![(
        Geom::Rect {
            rect: rect(30.0, 20.0, 40.0, 40.0),
            rotation: 0.0,
            corner_radius: 0.0,
        },
        solid(None, Some(RED), 1.0, 1.0),
    )]);
    // crop: the shape is clipped, the output is the crop
    let cropped = base
        .apply(&Edit::SetCrop(Some(RectU {
            x: 0,
            y: 0,
            w: 40,
            h: 30,
        })))
        .unwrap();
    let img = draw(&cropped);
    assert_eq!((img.0, img.1), (40, 30));
    assert_eq!(px(&img, 35, 25), [255, 0, 0, 255], "shape inside the crop");
    assert_eq!(px(&img, 1, 1), [0, 200, 0, 255], "marker at the top-left");
    // rotate 90° clockwise: the top-left marker moves to the top-right
    let rotated = cropped.apply(&Edit::Rotate { quarter_turns: 1 }).unwrap();
    let img = draw(&rotated);
    assert_eq!((img.0, img.1), (30, 40));
    assert_eq!(px(&img, 28, 1), [0, 200, 0, 255]);
    golden("crop_rotate", &img, 0x8f9416cb3e70868b);
    // reset the crop: the whole shape is back
    let reset = rotated.apply(&Edit::SetCrop(None)).unwrap();
    let img = draw(&reset);
    assert_eq!((img.0, img.1), (48, 64));
    // deterministic: same document, same bytes
    assert_eq!(draw(&reset).2, img.2);
}

#[test]
fn curved_arrow_bows_sideways_and_is_hit_on_its_curve() {
    let arrow = |bend: f32| Shape {
        id: ObjectId(1),
        geom: Geom::Arrow {
            a: pt(10.0, 30.0),
            b: pt(54.0, 30.0),
            heads: ArrowHeads::End,
            bend,
        },
        style: Style::default_for(ShapeKind::Arrow),
    };
    let (straight, curved) = (arrow(0.0), arrow(0.4));
    // the middle of the curve is half the control point's offset away from the chord
    let mid = pt(32.0, 30.0 + 0.4 * 44.0 / 2.0);
    assert!(geom::hit(&curved, mid, 1.0) && !geom::hit(&straight, mid, 1.0));
    assert!(geom::hit(&straight, pt(32.0, 30.0), 1.0) && !geom::hit(&curved, pt(32.0, 30.0), 1.0));
    assert!(geom::bounds(&curved).unwrap().h > geom::bounds(&straight).unwrap().h);
    // the other side, and through a project (serde default for files without the field)
    assert!(geom::hit(&arrow(-0.4), pt(32.0, 30.0 - 0.4 * 44.0 / 2.0), 1.0));
    let old: Geom =
        serde_json::from_str(r#"{"Arrow":{"a":{"x":0.0,"y":0.0},"b":{"x":9.0,"y":0.0},"heads":"End"}}"#).unwrap();
    assert!(matches!(old, Geom::Arrow { bend, .. } if bend == 0.0));
    let doc = DocState::new(64, 48);
    assert!(
        doc.apply(&Edit::AddShape(arrow(1.5))).is_err(),
        "bend beyond ±1 refused"
    );
}
