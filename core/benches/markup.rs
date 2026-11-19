//! Markup benches with their budgets: redaction patch 1000×1000 (< 30 ms), screen
//! overlay with 200 shapes at 2560×1440 (raster < 8 ms), PNG export of 24 MP with 50 shapes (< 2 s).
//! `cargo bench -p rv-core --bench markup`

use rv_core::decode::Pixels;
use rv_core::edit::redact::{PatchCache, SrcPx, patches};
use rv_core::edit::render::{draw_shapes_banded, same};
use rv_core::edit::tiny_skia::Transform;
use rv_core::edit::*;
use rv_core::export::{DocSource, ExportFormat, ExportOptions, encode, flatten};
use rv_core::formats::Format;
use rv_core::settings::MetadataPreset;
use std::sync::Arc;

fn noise(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let n = i.wrapping_mul(2654435761) >> 24;
            [n as u8, (i / w) as u8, (n ^ i) as u8, 255]
        })
        .collect()
}

/// `n` shapes of every kind spread over a `w`×`h` image.
fn shapes(n: usize, w: u32, h: u32) -> DocState {
    let mut doc = DocState::new(w, h);
    let (fw, fh) = (w as f32, h as f32);
    for i in 0..n {
        let f = i as f32 / n as f32;
        let (x, y) = (fw * (0.05 + 0.85 * f), fh * (0.05 + 0.85 * ((f * 7.0) % 1.0)));
        let (sw, sh) = (fw * 0.08, fh * 0.06);
        let pts: Arc<[PointF]> = (0..64)
            .map(|k| PointF {
                x: x + sw * k as f32 / 64.0,
                y: y + sh * 0.5 * (k as f32 * 0.3).sin(),
            })
            .collect();
        let (kind, geom) = match i % 6 {
            0 => (
                ShapeKind::Rect,
                Geom::Rect {
                    rect: RectF { x, y, w: sw, h: sh },
                    rotation: 0.3,
                    corner_radius: 8.0,
                },
            ),
            1 => (
                ShapeKind::Ellipse,
                Geom::Ellipse {
                    rect: RectF { x, y, w: sw, h: sh },
                    rotation: 0.0,
                },
            ),
            2 => (
                ShapeKind::Arrow,
                Geom::Arrow {
                    a: PointF { x, y },
                    b: PointF { x: x + sw, y: y + sh },
                    heads: ArrowHeads::Both,
                    bend: 0.2,
                },
            ),
            3 => (
                ShapeKind::Line,
                Geom::Line {
                    a: PointF { x, y: y + sh },
                    b: PointF { x: x + sw, y },
                },
            ),
            4 => (ShapeKind::Pen, Geom::Pen { points: pts }),
            _ => (ShapeKind::Highlight, Geom::Highlight { points: pts }),
        };
        let shape = Shape {
            id: doc.peek_id(),
            geom,
            style: Style::default_for(kind),
        };
        doc = doc.apply(&Edit::AddShape(shape)).unwrap();
    }
    doc
}

fn main() {
    // redaction patch 1000×1000 on a 12 MP image
    let (w, h) = (4000u32, 3000u32);
    let src = noise(w, h);
    for kind in [
        RedactKind::Blur { radius: 30.0 },
        RedactKind::Pixelate { block: 20 },
        RedactKind::Solid { color: [0, 0, 0] },
    ] {
        let mut doc = DocState::new(w, h);
        doc = doc
            .apply(&Edit::AddRedaction(Redaction {
                id: doc.peek_id(),
                area: Area::Rect {
                    rect: RectF {
                        x: 1000.0,
                        y: 1000.0,
                        w: 1000.0,
                        h: 1000.0,
                    },
                    rotation: 0.0,
                },
                kind: kind.clone(),
            }))
            .unwrap();
        bench(&format!("redaction patch 1000x1000 on 12 MP: {kind:?}"), 10, || {
            patches(&doc, w, h, SrcPx::R8(&src), &same, &mut PatchCache::default())
        });
    }

    // the screen overlay: 200 shapes, the whole image fitted to 2560×1440
    let doc = shapes(200, w, h);
    let fit = 1440.0 / h as f32;
    bench("overlay raster 200 shapes at 2560x1440", 20, || {
        draw_shapes_banded(
            2560,
            1440,
            &doc.shapes,
            Transform::from_scale(fit, fit),
            fit,
            &same,
            &|_| None,
        )
    });

    // export: 24 MP with 50 shapes, flatten + PNG encode
    let (w, h) = (6000u32, 4000u32);
    let source = DocSource {
        w,
        h,
        pixels: Arc::new(Pixels::Rgba8(noise(w, h))),
        icc: None,
        exif: None,
        format: Format::Png,
        bytes: Arc::from(&[][..]),
        patch_cache: Arc::default(),
        base_cache: Arc::default(),
    };
    let doc = shapes(50, w, h);
    let opts = ExportOptions {
        format: ExportFormat::Png,
        jpeg_quality: 100,
        background: [255; 3],
        flatten: false,
        dpi: None,
        size: None,
        preset: MetadataPreset::StripAll,
    };
    bench("export PNG 24 MP with 50 shapes (render + encode)", 5, || {
        encode(&flatten(&doc, &source), None, &opts).unwrap()
    });
    bench("resize 24 MP to 50 % (Lanczos-3, premultiplied)", 5, || {
        rv_core::resample::resize(&source.pixels, (w, h), (w / 2, h / 2))
    });
    let every = rv_core::edit::Adjustments {
        exposure: 0.5,
        brightness: 10.0,
        contrast: 20.0,
        saturation: 30.0,
        temperature: -20.0,
        hue: 15.0,
        sharpness: 50.0,
    };
    let tone = rv_core::edit::Adjustments {
        saturation: 0.0,
        hue: 0.0,
        sharpness: 0.0,
        ..every
    };
    bench("adjust 24 MP, tone only (per-channel table)", 5, || {
        rv_core::edit::adjust::apply(&source.pixels, w, &tone)
    });
    let sharp = rv_core::edit::Adjustments {
        sharpness: 50.0,
        ..Default::default()
    };
    watermark(&noise(4000, 3000));
    bench("adjust 24 MP, sharpness only (luma unsharp mask)", 5, || {
        rv_core::edit::adjust::apply(&source.pixels, w, &sharp)
    });
    bench(
        "adjust 24 MP, every parameter (table + colour matrix + sharpness)",
        5,
        || rv_core::edit::adjust::apply(&source.pixels, w, &every),
    );
}

/// Median and best of `n` runs of `f` (after one warm-up), printed as `BENCH <name>: ...`.
fn bench<T>(name: &str, n: usize, mut f: impl FnMut() -> T) {
    std::hint::black_box(f());
    let mut t: Vec<f64> = (0..n)
        .map(|_| {
            let s = std::time::Instant::now();
            std::hint::black_box(f());
            s.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    t.sort_by(f64::total_cmp);
    println!(
        "BENCH {name}: median {:.1} ms, best {:.1} ms ({n} runs)",
        t[n / 2],
        t[0]
    );
}

/// A 12 MP photo inserted as a watermark, then the screen layer redrawn the way a drag redraws
/// it: moved (same size) and resized (a new size every frame), at half opacity.
fn watermark(rgba12: &[u8]) {
    let mut file = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new_with_quality(
            &mut file,
            image::codecs::png::CompressionType::Fast,
            image::codecs::png::FilterType::NoFilter,
        ),
        rgba12,
        4000,
        3000,
        image::ExtendedColorType::Rgba8,
    )
    .unwrap();
    let opts = rv_core::decode::DecodeOptions::default();
    bench(
        "watermark insert: 12 MP PNG decoded, reduced to 2048, stored",
        3,
        || picture::ImageData::from_file(&file, None, &opts).unwrap(),
    );
    let data = picture::ImageData::from_file(&file, None, &opts).unwrap();
    let shape = |w: f32| {
        let mut style = Style::default_for(ShapeKind::Image);
        style.opacity = 0.5;
        Arc::new(Shape {
            id: ObjectId(1),
            geom: Geom::Image {
                rect: RectF {
                    x: 300.0,
                    y: 200.0,
                    w,
                    h: w * 0.75,
                },
                rotation: 0.0,
                image: picture::ImageRef::new(data.clone()),
            },
            style,
        })
    };
    let frame = |w: f32| draw_shapes_banded(2560, 1440, &[shape(w)], Transform::identity(), 1.0, &same, &|_| None);
    // the first frame after inserting: nothing converted or reduced yet
    let mut fresh: Vec<_> = (0..6)
        .map(|_| picture::ImageData::from_png(&data.png).unwrap())
        .collect();
    bench("watermark first frame after insert (cold cache)", 5, || {
        let d = fresh.pop().unwrap();
        let mut style = Style::default_for(ShapeKind::Image);
        style.opacity = 0.5;
        let s = Arc::new(Shape {
            id: ObjectId(1),
            geom: Geom::Image {
                rect: RectF {
                    x: 300.0,
                    y: 200.0,
                    w: 700.0,
                    h: 525.0,
                },
                rotation: 0.0,
                image: picture::ImageRef::new(d),
            },
            style,
        });
        draw_shapes_banded(2560, 1440, &[s], Transform::identity(), 1.0, &same, &|_| None)
    });
    bench("watermark drag frame, moved (2560x1440 layer)", 20, || frame(900.0));
    bench("watermark drag frame, moved, drawn large (full-size level)", 20, || {
        frame(1800.0)
    });
    let mut w = 600.0;
    bench("watermark drag frame, resized (new size each frame)", 20, || {
        w += 7.0;
        frame(w)
    });
}
