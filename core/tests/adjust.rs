//! Image adjustments in the document: below the redactions (which read the adjusted pixels) and
//! the shapes (never adjusted); the export equals a render from scratch whatever the caches hold;
//! a Solid cover keeps its exact colour; 16-bit stays 16-bit; undo/redo reuse the caches.

use rv_core::decode::Pixels;
use rv_core::edit::render::{Source, render, render_pixels, same};
use rv_core::edit::*;
use rv_core::export::{DocSource, flatten};
use std::sync::Arc;

const W: u32 = 64;
const H: u32 = 40;

fn gradient() -> Vec<u8> {
    (0..W * H)
        .flat_map(|i| {
            let (x, y) = (i % W, i / W);
            [(x * 255 / W) as u8, (y * 255 / H) as u8, 90, 255]
        })
        .collect()
}

fn source(px: Pixels) -> DocSource {
    DocSource {
        w: W,
        h: H,
        pixels: Arc::new(px),
        icc: None,
        exif: None,
        format: rv_core::formats::Format::Png,
        bytes: Arc::from(&[][..]),
        patch_cache: Arc::default(),
        base_cache: Arc::default(),
    }
}

fn adj(exposure: f32, hue: f32) -> Adjustments {
    Adjustments {
        exposure,
        hue,
        contrast: 15.0,
        ..Default::default()
    }
}

/// A blur and a Solid cover, and an opaque red rectangle on top.
fn doc() -> DocState {
    let d = DocState::new(W, H);
    let mut d = d;
    for (x, kind) in [
        (4.0, RedactKind::Blur { radius: 6.0 }),
        (40.0, RedactKind::Solid { color: [10, 200, 30] }),
    ] {
        d = d
            .apply(&Edit::AddRedaction(Redaction {
                id: d.peek_id(),
                area: Area::Rect {
                    rect: RectF {
                        x,
                        y: 4.0,
                        w: 18.0,
                        h: 14.0,
                    },
                    rotation: 0.0,
                },
                kind,
            }))
            .unwrap();
    }
    let mut style = Style::default_for(ShapeKind::Rect);
    style.fill = Some(Color::rgb(255, 0, 0));
    style.opacity = 1.0;
    d.apply(&Edit::AddShape(Shape {
        id: d.peek_id(),
        geom: Geom::Rect {
            rect: RectF {
                x: 20.0,
                y: 26.0,
                w: 16.0,
                h: 10.0,
            },
            rotation: 0.0,
            corner_radius: 0.0,
        },
        style,
    }))
    .unwrap()
}

fn rgba(p: Pixels) -> Vec<u8> {
    rv_core::color::rgba8(p)
}

fn at(v: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [v[i], v[i + 1], v[i + 2], v[i + 3]]
}

#[test]
fn redactions_read_adjusted_pixels_and_shapes_stay_as_drawn() {
    let src = source(Pixels::Rgba8(gradient()));
    let plain = doc();
    let adjusted = plain.apply(&Edit::SetAdjust(adj(1.0, 40.0))).unwrap();
    let a = rgba(flatten(&adjusted, &src).pixels);
    let p = rgba(flatten(&plain, &src).pixels);
    // outside everything: adjusted
    assert_ne!(at(&a, 60, 30), at(&p, 60, 30));
    // the blurred area shows the adjusted image blurred: it equals blurring the adjusted source
    let pre = Pixels::Rgba8(rgba(adjust::apply(&src.pixels, W, &adjusted.adjust)));
    let mut no_adjust = adjusted.clone();
    no_adjust.adjust = Adjustments::default();
    let expect = rgba(render_pixels(&no_adjust, W, H, &pre, &same).2);
    assert_eq!(a, expect, "adjust, then redact, then draw");
    // the shape and the Solid cover keep their exact colours
    assert_eq!(at(&a, 28, 30), [255, 0, 0, 255], "shape not adjusted");
    assert_eq!(at(&a, 48, 10), [10, 200, 30, 255], "Solid colour exact");
}

#[test]
fn the_export_never_uses_stale_caches() {
    // one source, the preview asking for several adjustments in turn, the export after each:
    // always the same as a render from scratch
    let src = source(Pixels::Rgba8(gradient()));
    let base = doc();
    let sharp = Adjustments {
        sharpness: 80.0,
        ..adj(0.2, 0.0)
    };
    for a in [
        adj(1.0, 0.0),
        adj(-1.0, 90.0),
        sharp,
        Adjustments::default(),
        adj(1.0, 0.0),
        sharp,
    ] {
        let d = base.apply(&Edit::SetAdjust(a)).unwrap();
        let _ = src.patches(&d);
        let fresh = render(
            &d,
            &Source {
                w: W,
                h: H,
                rgba: &gradient(),
            },
            &same,
        )
        .2;
        assert_eq!(rgba(flatten(&d, &src).pixels), fresh, "{a:?}");
    }
}

#[test]
fn undo_redo_reuse_the_caches() {
    let src = source(Pixels::Rgba8(gradient()));
    let mut h = History::new(doc());
    h.apply(&Edit::SetAdjust(adj(0.5, 0.0))).unwrap();
    let first = src.base(&h.current().adjust);
    assert!(Arc::ptr_eq(&first, &src.base(&h.current().adjust)), "computed once");
    let _ = src.patches(h.current());
    h.apply(&Edit::SetAdjust(adj(0.5, 60.0))).unwrap();
    let _ = src.patches(h.current());
    let misses = src.patch_cache.lock().unwrap().misses;
    h.undo();
    assert!(
        Arc::ptr_eq(&first, &src.base(&h.current().adjust)),
        "the previous base kept"
    );
    let _ = src.patches(h.current());
    assert_eq!(
        src.patch_cache.lock().unwrap().misses,
        misses,
        "undo: patches from the cache"
    );
    h.undo();
    assert!(
        Arc::ptr_eq(&src.base(&h.current().adjust), &src.pixels),
        "neutral: the source itself"
    );
}

#[test]
fn sixteen_bit_documents_stay_sixteen_bit() {
    let px: Vec<u16> = gradient().iter().map(|v| (*v as u16 * 257).saturating_sub(3)).collect();
    let src = source(Pixels::Rgba16(px));
    let d = doc().apply(&Edit::SetAdjust(adj(0.3, 10.0))).unwrap();
    assert!(matches!(flatten(&d, &src).pixels, Pixels::Rgba16(_)));
    assert!(matches!(&*src.base(&d.adjust), Pixels::Rgba16(_)));
}

#[test]
fn golden_adjusted_document() {
    let d = doc()
        .apply(&Edit::SetAdjust(Adjustments {
            exposure: 0.7,
            brightness: -20.0,
            contrast: 25.0,
            saturation: 40.0,
            temperature: 30.0,
            hue: -25.0,
            sharpness: 0.0,
        }))
        .unwrap();
    let (w, h, v) = render(
        &d,
        &Source {
            w: W,
            h: H,
            rgba: &gradient(),
        },
        &same,
    );
    if let Some(dir) = std::env::var_os("RVIEW_GOLDEN_DIR") {
        image::RgbaImage::from_raw(w, h, v.clone())
            .unwrap()
            .save(std::path::Path::new(&dir).join("adjusted.png"))
            .unwrap();
    }
    let hash = v
        .iter()
        .chain(&w.to_le_bytes())
        .chain(&h.to_le_bytes())
        .fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ *b as u64).wrapping_mul(0x100000001b3)
        });
    assert_eq!(
        hash, 0xaeb3_60ef_d266_beff,
        "golden adjusted: hash {hash:#018x} (RVIEW_GOLDEN_DIR=<folder> to see it)"
    );
}

#[test]
fn golden_sharpened_document() {
    // sharpness on the adjusted image, below the redactions: the blur stays soft, the Solid
    // cover and the shape keep their exact colours
    let d = doc()
        .apply(&Edit::SetAdjust(Adjustments {
            sharpness: 70.0,
            contrast: 10.0,
            ..Default::default()
        }))
        .unwrap();
    let checker: Vec<u8> = (0..W * H)
        .flat_map(|i| {
            let (x, y) = (i % W, i / W);
            let v = if (x / 4 + y / 4) % 2 == 0 { 70 } else { 180 };
            [v, v / 2 + 40, 255 - v, 255]
        })
        .collect();
    let (w, h, v) = render(
        &d,
        &Source {
            w: W,
            h: H,
            rgba: &checker,
        },
        &same,
    );
    assert_eq!(at(&v, 48, 10), [10, 200, 30, 255], "Solid colour exact");
    assert_eq!(at(&v, 28, 30), [255, 0, 0, 255], "shape not sharpened");
    if let Some(dir) = std::env::var_os("RVIEW_GOLDEN_DIR") {
        image::RgbaImage::from_raw(w, h, v.clone())
            .unwrap()
            .save(std::path::Path::new(&dir).join("sharpened.png"))
            .unwrap();
    }
    let hash = v
        .iter()
        .chain(&w.to_le_bytes())
        .chain(&h.to_le_bytes())
        .fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ *b as u64).wrapping_mul(0x100000001b3)
        });
    assert_eq!(
        hash, 0x353a_80f9_2557_6c31,
        "golden sharpened: hash {hash:#018x} (RVIEW_GOLDEN_DIR=<folder> to see it)"
    );
}
