//! Image shapes (watermarks): drawn over the image with their opacity and rotation, above the
//! redactions, unaffected by the adjustments, converted into the document's colour space, at
//! 8 and 16 bit; saved in `.rvm` projects (one entry per distinct image) and checked on load.

mod common;
#[path = "common/projects.rs"]
mod projects;

use common::*;
use projects::{cases, image_doc, tiny_png};
use rv_core::decode::{DecodeOptions, Pixels};
use rv_core::edit::render::{Source, render, same};
use rv_core::edit::*;
use rv_core::export::{DocSource, flatten};
use rv_core::project::{self, ProjectError};
use std::sync::Arc;

const W: u32 = 60;
const H: u32 = 40;

/// A 4×2 logo: left half opaque red, right half opaque blue.
fn logo() -> Arc<picture::ImageData> {
    let rgba: Vec<u8> = (0..8)
        .flat_map(|i| if i % 4 < 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] })
        .collect();
    let img = image::RgbaImage::from_raw(4, 2, rgba).unwrap();
    picture::ImageData::from_file(&encode(&img, Enc::Png, None, None), None, &DecodeOptions::default()).unwrap()
}

fn grey(w: u32, h: u32) -> Vec<u8> {
    [100u8, 100, 100, 255].repeat((w * h) as usize)
}

fn with_image(d: DocState, data: Arc<picture::ImageData>, rect: RectF, rotation: f32, opacity: f32) -> DocState {
    let mut style = Style::default_for(ShapeKind::Image);
    style.opacity = opacity;
    d.apply(&Edit::AddShape(Shape {
        id: d.peek_id(),
        geom: Geom::Image {
            rect,
            rotation,
            image: picture::ImageRef::new(data),
        },
        style,
    }))
    .unwrap()
}

const BOX: RectF = RectF {
    x: 10.0,
    y: 10.0,
    w: 40.0,
    h: 20.0,
};

fn at(v: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [v[i], v[i + 1], v[i + 2], v[i + 3]]
}

#[test]
fn drawn_stretched_with_opacity_and_rotation() {
    let src = grey(W, H);
    let d = with_image(DocState::new(W, H), logo(), BOX, 0.0, 1.0);
    let (_, _, v) = render(&d, &Source { w: W, h: H, rgba: &src }, &same);
    assert_eq!(at(&v, 15, 20), [255, 0, 0, 255], "left half red");
    assert_eq!(at(&v, 45, 20), [0, 0, 255, 255], "right half blue");
    assert_eq!(at(&v, 5, 5), [100, 100, 100, 255], "outside untouched");
    // half opacity: halfway between the image and the logo
    let d = with_image(DocState::new(W, H), logo(), BOX, 0.0, 0.5);
    let p = at(&render(&d, &Source { w: W, h: H, rgba: &src }, &same).2, 15, 20);
    assert!(p[0].abs_diff(178) <= 2 && p[1].abs_diff(50) <= 2, "{p:?}");
    // turned half a turn: red on the right
    let d = with_image(DocState::new(W, H), logo(), BOX, std::f32::consts::PI, 1.0);
    let (_, _, v) = render(&d, &Source { w: W, h: H, rgba: &src }, &same);
    assert_eq!((at(&v, 15, 20), at(&v, 45, 20)), ([0, 0, 255, 255], [255, 0, 0, 255]));
}

#[test]
fn above_redactions_and_not_adjusted() {
    let src = grey(W, H);
    let d = DocState::new(W, H);
    let d = d
        .apply(&Edit::AddRedaction(Redaction {
            id: d.peek_id(),
            area: Area::Rect {
                rect: BOX,
                rotation: 0.0,
            },
            kind: RedactKind::Solid { color: [0, 255, 0] },
        }))
        .unwrap();
    let d = with_image(d, logo(), BOX, 0.0, 1.0)
        .apply(&Edit::SetAdjust(Adjustments {
            exposure: 2.0,
            hue: 90.0,
            ..Default::default()
        }))
        .unwrap();
    let (_, _, v) = render(&d, &Source { w: W, h: H, rgba: &src }, &same);
    assert_eq!(at(&v, 15, 20), [255, 0, 0, 255], "on top, colours as inserted");
    assert_ne!(at(&v, 5, 5), [100, 100, 100, 255], "the image itself is adjusted");
}

#[test]
fn document_space_and_sixteen_bit() {
    // a P3 document: the logo's sRGB red becomes the P3 values of sRGB red, like shape colours
    let p3 = icc_p3();
    let file = encode(
        &image::RgbaImage::from_raw(W, H, grey(W, H)).unwrap(),
        Enc::Png,
        Some(p3.clone()),
        None,
    );
    let img = rv_core::decode::decode(&file, None, &DecodeOptions::default()).unwrap();
    let src = DocSource::new(img, Arc::from(file));
    let d = with_image(DocState::new(W, H), logo(), BOX, 0.0, 1.0);
    let v = rv_core::color::rgba8(flatten(&d, &src).pixels);
    let red = rv_core::color::srgb_to_doc(Some(&p3))(Color::rgb(255, 0, 0));
    assert_eq!(at(&v, 15, 20), [red.r, red.g, red.b, 255]);
    // 16 bit: stays 16 bit, the logo drawn in it
    let px: Vec<u16> = grey(W, H).iter().map(|v| *v as u16 * 257).collect();
    let src = DocSource {
        w: W,
        h: H,
        pixels: Arc::new(Pixels::Rgba16(px)),
        icc: None,
        exif: None,
        format: rv_core::formats::Format::Png,
        bytes: Arc::from(&[][..]),
        patch_cache: Arc::default(),
        base_cache: Arc::default(),
    };
    let f = flatten(&d, &src);
    let Pixels::Rgba16(v) = f.pixels else {
        panic!("not 16 bit")
    };
    let i = ((20 * W + 15) * 4) as usize;
    assert_eq!(&v[i..i + 4], &[65535, 0, 0, 65535]);
}

#[test]
fn reduced_when_drawn_small() {
    // a large striped logo drawn at 1/16 of its size: the stripes average out (no aliasing)
    let stripes: Vec<u8> = (0..512 * 512)
        .flat_map(|i| {
            if i % 2 == 0 {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 255]
            }
        })
        .collect();
    let img = image::RgbaImage::from_raw(512, 512, stripes).unwrap();
    let data =
        picture::ImageData::from_file(&encode(&img, Enc::Png, None, None), None, &DecodeOptions::default()).unwrap();
    let r = RectF {
        x: 10.0,
        y: 4.0,
        w: 32.0,
        h: 32.0,
    };
    let d = with_image(DocState::new(W, H), data, r, 0.0, 1.0);
    let (_, _, v) = render(
        &d,
        &Source {
            w: W,
            h: H,
            rgba: &grey(W, H),
        },
        &same,
    );
    for x in 14..38 {
        let p = at(&v, x, 20);
        assert!((100..=155).contains(&p[0]), "x {x}: {p:?} (aliased)");
    }
}

#[test]
fn edits_need_the_pixels() {
    let d = DocState::new(W, H);
    let bare = Shape {
        id: d.peek_id(),
        geom: Geom::Image {
            rect: BOX,
            rotation: 0.0,
            image: picture::ImageRef {
                id: "0123456789abcdef".into(),
                data: None,
            },
        },
        style: Style::default_for(ShapeKind::Image),
    };
    assert!(
        d.apply(&Edit::AddShape(bare)).is_err(),
        "an image shape without its pixels"
    );
    // duplicates share the pixels; moving keeps them
    let d = with_image(d, logo(), BOX, 0.0, 1.0);
    let id = d.shapes[0].id;
    let d = d
        .apply(&Edit::Duplicate {
            ids: vec![id],
            offset: VectorF { dx: 5.0, dy: 5.0 },
        })
        .unwrap()
        .apply(&Edit::Translate {
            ids: vec![id],
            delta: VectorF { dx: 1.0, dy: 0.0 },
        })
        .unwrap();
    let datas: Vec<_> = d
        .shapes
        .iter()
        .map(|s| match &s.geom {
            Geom::Image { image, .. } => image.data.clone().unwrap(),
            _ => unreachable!(),
        })
        .collect();
    assert!(Arc::ptr_eq(&datas[0], &datas[1]));
}

#[test]
fn projects_keep_the_pixels() {
    let png = tiny_png(projects::W, projects::H);
    let (doc, logo) = image_doc(projects::W, projects::H);
    let id = doc.shapes[0].id;
    let doc = doc
        .apply(&Edit::Duplicate {
            ids: vec![id],
            offset: VectorF { dx: 2.0, dy: 1.0 },
        })
        .unwrap();
    let bytes = project::write(&doc, &png, "photo.png", rv_core::formats::Format::Png).unwrap();
    let p = project::read(&bytes, &DecodeOptions::default()).unwrap();
    assert_eq!(p.manifest.version, 2, "image shapes need version 2");
    let names: Vec<String> = {
        let z = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        z.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).collect()
    };
    let entry = format!("image-{}.png", logo.id);
    assert_eq!(
        names.iter().filter(|n| **n == entry).count(),
        1,
        "one entry for both copies: {names:?}"
    );
    // the reopened project renders the same
    let src = |b: &[u8]| {
        DocSource::new(
            rv_core::decode::decode(b, None, &DecodeOptions::default()).unwrap(),
            Arc::from(b.to_vec()),
        )
    };
    assert_eq!(
        rv_core::color::rgba8(flatten(&doc, &src(&png)).pixels),
        rv_core::color::rgba8(flatten(&p.doc, &src(&p.original)).pixels)
    );
    // damaged image entries are refused
    let case = |n: &str| cases().into_iter().find(|(c, _)| *c == n).unwrap().1;
    for (name, what) in [
        ("image_missing", "missing"),
        ("image_unused", "no shape uses"),
        ("image_renamed", "differs from its name"),
        ("image_not_png", "PNG"),
    ] {
        match project::read(&case(name), &DecodeOptions::default()) {
            Err(ProjectError::Corrupt(m)) => assert!(m.contains(what), "{name}: {m}"),
            r => panic!("{name}: {:?}", r.err()),
        }
    }
}

#[test]
fn golden_watermark() {
    let src = grey(W, H);
    let d = with_image(DocState::new(W, H), logo(), BOX, 0.4, 0.7);
    let (w, h, v) = render(&d, &Source { w: W, h: H, rgba: &src }, &same);
    // anti-aliased edges are partly covered: never more opaque than the inside
    let inside = at(&v, 20, 20)[0];
    assert!(
        v.chunks_exact(4).all(|p| p[0] <= inside),
        "an edge pixel above the inside ({inside})"
    );
    if let Some(dir) = std::env::var_os("RVIEW_GOLDEN_DIR") {
        image::RgbaImage::from_raw(w, h, v.clone())
            .unwrap()
            .save(std::path::Path::new(&dir).join("watermark.png"))
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
        hash, 0x2b52_78e0_a011_67e4,
        "golden watermark: hash {hash:#018x} (RVIEW_GOLDEN_DIR=<folder> to see it)"
    );
}
