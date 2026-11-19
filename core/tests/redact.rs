//! Redactions and text: pixelate blocks, Solid conservative
//! cover (also in the exported PNG), Solid over Blur, blur edges, the patch cache on undo/redo and
//! mode changes, 16-bit sources, golden renders (text with Lato), patch timing.

use rv_core::decode::Pixels;
use rv_core::edit::redact::{PatchCache, SrcPx, patches};
use rv_core::edit::render::{Source, render, same};
use rv_core::edit::*;
use std::sync::Arc;

fn rect(x: f32, y: f32, w: f32, h: f32) -> RectF {
    RectF { x, y, w, h }
}

/// A busy opaque image: every pixel different from its neighbours.
fn noise(w: u32, h: u32) -> Vec<u8> {
    let mut s = 0x1234_5678u32;
    (0..w * h)
        .flat_map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            [(s >> 8) as u8, (s >> 16) as u8, (s >> 24) as u8, 255]
        })
        .collect()
}

fn with(doc: DocState, area: Area, kind: RedactKind) -> DocState {
    let r = Redaction {
        id: doc.peek_id(),
        area,
        kind,
    };
    doc.apply(&Edit::AddRedaction(r)).unwrap()
}

fn px(img: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [img[i], img[i + 1], img[i + 2], img[i + 3]]
}

/// Inside the area, sampled (a subset of the pixels the area touches).
fn sampled_inside(a: &Area, x: u32, y: u32) -> bool {
    (0..8).any(|i| {
        (0..8).any(|j| {
            geom::hit_area(
                a,
                PointF {
                    x: x as f32 + (i as f32 + 0.5) / 8.0,
                    y: y as f32 + (j as f32 + 0.5) / 8.0,
                },
                0.0,
            )
        })
    })
}

/// The pixel square, grown by `m`, is entirely outside the area (checked on a fine grid).
fn clearly_outside(a: &Area, x: u32, y: u32, m: f32) -> bool {
    (0..=10).all(|i| {
        (0..=10).all(|j| {
            let p = PointF {
                x: x as f32 - m + (1.0 + 2.0 * m) * i as f32 / 10.0,
                y: y as f32 - m + (1.0 + 2.0 * m) * j as f32 / 10.0,
            };
            !geom::hit_area(a, p, 0.0)
        })
    })
}

#[test]
fn pixelate_blocks_are_flat_and_outside_is_untouched() {
    let (w, h) = (64u32, 48u32);
    let src = noise(w, h);
    let area = Area::Rect {
        rect: rect(10.0, 6.0, 40.0, 30.0),
        rotation: 0.0,
    };
    let doc = with(DocState::new(w, h), area.clone(), RedactKind::Pixelate { block: 8 });
    let (_, _, out) = render(&doc, &Source { w, h, rgba: &src }, &same);
    for y in 0..h {
        for x in 0..w {
            let inside = (10..50).contains(&x) && (6..36).contains(&y);
            if !inside {
                assert_eq!(px(&out, w, x, y), px(&src, w, x, y), "outside changed at {x},{y}");
                continue;
            }
            // blocks anchored at the area's origin (10, 6): same value in each block
            let (bx, by) = (10 + (x - 10) / 8 * 8, 6 + (y - 6) / 8 * 8);
            assert_eq!(px(&out, w, x, y), px(&out, w, bx, by), "block not flat at {x},{y}");
        }
    }
    assert_ne!(px(&out, w, 10, 6), px(&src, w, 10, 6));
}

#[test]
fn solid_covers_every_touched_pixel_exactly() {
    let (w, h) = (80u32, 60u32);
    let src = noise(w, h);
    for area in [
        Area::Rect {
            rect: rect(12.3, 9.7, 31.4, 17.2),
            rotation: 0.47,
        },
        Area::Ellipse {
            rect: rect(40.6, 20.2, 25.9, 33.1),
            rotation: -0.8,
        },
    ] {
        let doc = with(
            DocState::new(w, h),
            area.clone(),
            RedactKind::Solid { color: [10, 200, 30] },
        );
        let (_, _, out) = render(&doc, &Source { w, h, rgba: &src }, &same);
        let (mut covered, mut kept) = (0, 0);
        for y in 0..h {
            for x in 0..w {
                let p = px(&out, w, x, y);
                if sampled_inside(&area, x, y) {
                    assert_eq!(p, [10, 200, 30, 255], "touched pixel {x},{y} not covered ({area:?})");
                    covered += 1;
                } else if clearly_outside(&area, x, y, 0.01) {
                    assert_eq!(p, px(&src, w, x, y), "pixel {x},{y} outside changed");
                    kept += 1;
                }
            }
        }
        assert!(covered > 200 && kept > 1000, "{covered} covered, {kept} kept");
    }
}

#[test]
fn solid_in_exported_png_has_no_original_value() {
    use rv_core::export::*;
    let (w, h) = (80u32, 60u32);
    let src = noise(w, h);
    let mut png = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut png),
        &src,
        w,
        h,
        image::ExtendedColorType::Rgba8,
    )
    .unwrap();
    let img = rv_core::decode::decode(&png, None, &Default::default()).unwrap();
    let ds = DocSource::new(img, Arc::from(png));
    let area = Area::Ellipse {
        rect: rect(20.5, 10.5, 30.0, 25.0),
        rotation: 0.3,
    };
    let doc = with(
        DocState::new(w, h),
        area.clone(),
        RedactKind::Solid { color: [0, 0, 0] },
    );
    let flat = flatten(&doc, &ds);
    let o = ExportOptions {
        format: ExportFormat::Png,
        jpeg_quality: 90,
        background: [255; 3],
        preset: rv_core::settings::MetadataPreset::StripAll,
    };
    let out = encode(&flat, None, &o).unwrap();
    let back = rv_core::color::rgba8(rv_core::decode::decode(&out, None, &Default::default()).unwrap().pixels);
    for y in 0..h {
        for x in 0..w {
            if sampled_inside(&area, x, y) {
                assert_eq!(px(&back, w, x, y), [0, 0, 0, 255], "{x},{y}");
            }
        }
    }
}

#[test]
fn solid_over_blur_wins_and_blur_stays_in_the_image() {
    let (w, h) = (60u32, 40u32);
    let src = noise(w, h);
    let blur = Area::Rect {
        rect: rect(0.0, 0.0, 30.0, 20.0),
        rotation: 0.0,
    };
    let solid = Area::Rect {
        rect: rect(20.0, 10.0, 20.0, 20.0),
        rotation: 0.0,
    };
    let doc = with(DocState::new(w, h), blur, RedactKind::Blur { radius: 12.0 });
    let doc = with(doc, solid, RedactKind::Solid { color: [255, 0, 255] });
    let (_, _, out) = render(&doc, &Source { w, h, rgba: &src }, &same);
    // the overlap (20..30, 10..20) is the solid colour
    for y in 10..20 {
        for x in 20..30 {
            assert_eq!(px(&out, w, x, y), [255, 0, 255, 255]);
        }
    }
    // blurred: neighbours close to each other; deterministic
    let d = |a: [u8; 4], b: [u8; 4]| (0..3).map(|c| (a[c] as i32 - b[c] as i32).abs()).max().unwrap();
    assert!(d(px(&out, w, 5, 5), px(&out, w, 6, 5)) < 12);
    assert_eq!(render(&doc, &Source { w, h, rgba: &src }, &same).2, out);
    // a uniform image stays uniform under a blur at its corner: nothing is read outside it
    let flat = [200u8, 100, 50, 255].repeat((w * h) as usize);
    let edge = with(
        DocState::new(w, h),
        Area::Rect {
            rect: rect(0.0, 0.0, 15.0, 15.0),
            rotation: 0.0,
        },
        RedactKind::Blur { radius: 20.0 },
    );
    assert_eq!(render(&edge, &Source { w, h, rgba: &flat }, &same).2, flat);
}

#[test]
fn sixteen_bit_solid_is_exact() {
    let (w, h) = (20u32, 10u32);
    let src: Vec<u16> = (0..w * h * 4).map(|i| (i * 977 % 65536) as u16 | 0xFF).collect();
    let mut src = src;
    for a in src.chunks_exact_mut(4) {
        a[3] = 65535;
    }
    let doc = with(
        DocState::new(w, h),
        Area::Rect {
            rect: rect(2.0, 2.0, 5.0, 5.0),
            rotation: 0.0,
        },
        RedactKind::Solid { color: [1, 2, 3] },
    );
    let (_, _, out) = render::render_pixels(&doc, w, h, &Pixels::Rgba16(src.clone()), &same);
    let Pixels::Rgba16(v) = out else { panic!("16-bit") };
    let at = |x: u32, y: u32| &v[((y * w + x) * 4) as usize..][..4];
    assert_eq!(at(3, 3), &[257, 514, 771, 65535]);
    assert_eq!(at(10, 5), &src[((5 * w + 10) * 4) as usize..][..4]);
}

#[test]
fn cache_hits_on_undo_redo_and_only_dependents_recompute() {
    let (w, h) = (120u32, 60u32);
    let src = noise(w, h);
    let mut hist = History::new(DocState::new(w, h));
    let area = |x: f32| Area::Rect {
        rect: rect(x, 10.0, 25.0, 25.0),
        rotation: 0.0,
    };
    // three separate redactions, then one overlapping the third
    for (x, k) in [
        (5.0, RedactKind::Blur { radius: 4.0 }),
        (45.0, RedactKind::Pixelate { block: 5 }),
        (85.0, RedactKind::Solid { color: [0, 0, 0] }),
    ] {
        let d = hist.current().clone();
        hist.apply(&Edit::AddRedaction(Redaction {
            id: d.peek_id(),
            area: area(x),
            kind: k,
        }))
        .unwrap();
    }
    let mut c = PatchCache::default();
    let run = |doc: &DocState, c: &mut PatchCache| {
        let before = (c.hits, c.misses);
        let p = patches(doc, w, h, SrcPx::R8(&src), &same, c);
        (p.len(), c.hits - before.0, c.misses - before.1)
    };
    assert_eq!(run(hist.current(), &mut c), (3, 0, 3));
    assert_eq!(
        run(hist.current(), &mut c),
        (3, 3, 0),
        "same document: all from the cache"
    );
    // mode change of the middle one: only it is computed again
    let mid = hist.current().redactions[1].id;
    hist.apply(&Edit::UpdateRedaction {
        id: mid,
        area: None,
        kind: Some(RedactKind::Solid { color: [9, 9, 9] }),
    })
    .unwrap();
    assert_eq!(run(hist.current(), &mut c), (3, 2, 1));
    // undo / redo: nothing computed
    hist.undo();
    assert_eq!(run(hist.current(), &mut c), (3, 3, 0));
    hist.redo();
    assert_eq!(run(hist.current(), &mut c), (3, 3, 0));
    // the first one moved onto the second: the second depends on it now, the third doesn't
    let first = hist.current().redactions[0].id;
    hist.apply(&Edit::Translate {
        ids: vec![first],
        delta: VectorF { dx: 30.0, dy: 0.0 },
    })
    .unwrap();
    assert_eq!(run(hist.current(), &mut c), (3, 1, 2));
}

// ---------------------------------------------------------------- golden renders

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

fn gradient(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            [(x * 255 / w) as u8, (y * 255 / h) as u8, ((x + y) * 3 % 256) as u8, 255]
        })
        .collect()
}

#[test]
fn golden_redactions() {
    let (w, h) = (96u32, 64u32);
    let src = gradient(w, h);
    let mut doc = DocState::new(w, h);
    doc = with(
        doc,
        Area::Ellipse {
            rect: rect(4.0, 4.0, 40.0, 28.0),
            rotation: 0.4,
        },
        RedactKind::Blur { radius: 10.0 },
    );
    doc = with(
        doc,
        Area::Rect {
            rect: rect(50.0, 6.0, 40.0, 30.0),
            rotation: -0.3,
        },
        RedactKind::Pixelate { block: 7 },
    );
    doc = with(
        doc,
        Area::Rect {
            rect: rect(20.5, 36.5, 50.0, 20.0),
            rotation: 0.2,
        },
        RedactKind::Solid { color: [20, 20, 20] },
    );
    golden(
        "redactions",
        &render(&doc, &Source { w, h, rgba: &src }, &same),
        0x6b22f7f29cf3fb4b,
    );
}

#[test]
fn golden_text_with_lato() {
    let (w, h) = (200u32, 90u32);
    let src = vec![255u8; (w * h * 4) as usize];
    let mut doc = DocState::new(w, h);
    for (i, (txt, weight, italic, align)) in [
        ("Hello, rView!", 400, false, TextAlign::Left),
        ("Bold & wrapped text here", 700, false, TextAlign::Center),
        ("Italic", 400, true, TextAlign::Right),
    ]
    .into_iter()
    .enumerate()
    {
        let font = FontSpec {
            family: text::DEFAULT_FAMILY.into(),
            size: 18.0,
            weight,
            italic,
        };
        let s = Shape {
            id: doc.peek_id(),
            geom: Geom::Text {
                rect: rect(
                    4.0 + i as f32 * 2.0,
                    4.0 + i as f32 * 28.0,
                    if i == 1 { 120.0 } else { 190.0 },
                    23.0,
                ),
                rotation: if i == 2 { 0.05 } else { 0.0 },
                text: txt.into(),
                font,
                align,
            },
            style: Style {
                stroke: Some(Color::rgb(20, 40, 160)),
                fill: (i == 1).then_some(Color {
                    r: 255,
                    g: 230,
                    b: 120,
                    a: 200,
                }),
                width: 1.0,
                opacity: 1.0,
            },
        };
        doc = doc.apply(&Edit::AddShape(s)).unwrap();
    }
    golden(
        "text_lato",
        &render(&doc, &Source { w, h, rgba: &src }, &same),
        0x98c7404c5d8ebe9c,
    );
}

/// A 1000×1000 patch in < 30 ms (12 MP image). Checked in release builds only.
#[test]
fn patch_timing() {
    let (w, h) = (4000u32, 3000u32);
    let src = noise(w, h);
    let area = Area::Rect {
        rect: rect(1000.0, 1000.0, 1000.0, 1000.0),
        rotation: 0.0,
    };
    for kind in [
        RedactKind::Blur { radius: 30.0 },
        RedactKind::Pixelate { block: 20 },
        RedactKind::Solid { color: [0, 0, 0] },
    ] {
        let doc = with(DocState::new(w, h), area.clone(), kind.clone());
        let mut best = f64::MAX;
        for _ in 0..3 {
            let mut c = PatchCache::default();
            let t = std::time::Instant::now();
            let p = patches(&doc, w, h, SrcPx::R8(&src), &same, &mut c);
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(p.len(), 1);
        }
        println!("PERF patch 1000x1000 on 12 MP, {kind:?}: {best:.1} ms");
        if !cfg!(debug_assertions) {
            assert!(best < 30.0, "{kind:?}: {best:.1} ms");
        }
    }
}

// ---------------------------------------------------------------- loupe

#[test]
fn loupe_magnifies_the_image_and_its_redactions() {
    let (w, h) = (120u32, 80u32);
    // 10×10 flat blocks, each a different colour
    let src: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let (bx, by) = ((i % w) / 10, (i / w) / 10);
            [(bx * 20) as u8, (by * 30) as u8, ((bx + by) * 7) as u8, 255]
        })
        .collect();
    let c = PointF { x: 60.0, y: 40.0 };
    let mut doc = with(
        DocState::new(w, h),
        Area::Rect {
            rect: rect(62.0, 42.0, 6.0, 6.0),
            rotation: 0.0,
        },
        RedactKind::Solid { color: [255, 0, 255] },
    );
    let mut style = Style::default_for(ShapeKind::Loupe);
    style.width = 2.0;
    doc = doc
        .apply(&Edit::AddShape(Shape {
            id: doc.peek_id(),
            geom: Geom::Loupe {
                center: c,
                radius: 30.0,
                zoom: 2.0,
            },
            style,
        }))
        .unwrap();
    let out = render(&doc, &Source { w, h, rgba: &src }, &same);
    let at = |img: &[u8], x: u32, y: u32| px(img, w, x, y);
    // a point d from the centre shows the image at d / 2 (inside a flat block)
    assert_eq!(at(&out.2, 60 - 16, 40 - 16), at(&src, 60 - 8, 40 - 8));
    assert_eq!(at(&out.2, 60 + 14, 40 - 18), at(&src, 60 + 7, 40 - 9));
    // the Solid redaction at (62..68, 42..48) appears magnified at (64..76, 44..56)
    assert_eq!(at(&out.2, 70, 50), [255, 0, 255, 255]);
    // outside the circle: the image itself
    assert_eq!(at(&out.2, 5, 5), at(&src, 5, 5));
    golden("loupe", &out, 0x150b941b733688c7);
    // the same with a crop (the target's transform is not identity): same pixels, shifted
    let cropped = doc
        .apply(&Edit::SetCrop(Some(RectU {
            x: 20,
            y: 10,
            w: 90,
            h: 60,
        })))
        .unwrap();
    let (_, _, c) = render(&cropped, &Source { w, h, rgba: &src }, &same);
    for (x, y) in [(44u32, 24u32), (74, 22), (70, 50), (60, 40)] {
        assert_eq!(px(&c, 90, x - 20, y - 10), at(&out.2, x, y), "cropped loupe at {x},{y}");
    }
}
