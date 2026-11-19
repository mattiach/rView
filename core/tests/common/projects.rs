//! `.rvm` archives built in memory: a valid project and one malformed archive per reader rule.
//! Shared by `tests/project.rs` and the fuzzer (`examples/fuzz_project.rs`, its seed corpus).
#![allow(dead_code)]

use rv_core::edit::*;
use rv_core::formats::Format;
use rv_core::project;
use std::io::Write;
use std::sync::Arc;

/// Small opaque gradient PNG.
pub fn tiny_png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        image::Rgba([(x * 255 / w) as u8, (y * 255 / h) as u8, 128, 255])
    });
    let mut out = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut out),
        img.as_raw(),
        w,
        h,
        image::ExtendedColorType::Rgba8,
    )
    .unwrap();
    out
}

fn pt(x: f32, y: f32) -> PointF {
    PointF { x, y }
}

/// A document with one shape of each kind and a redaction, z-order and styles all different.
pub fn sample_doc(w: u32, h: u32) -> DocState {
    let mut d = DocState::new(w, h);
    let (fw, fh) = (w as f32, h as f32);
    let style = |r, op| Style {
        stroke: Some(Color {
            r,
            g: 20,
            b: 200,
            a: 230,
        }),
        fill: (r > 100).then_some(Color::rgb(10, r, 30)),
        width: 2.0 + r as f32 / 100.0,
        opacity: op,
    };
    let geoms = [
        Geom::Rect {
            rect: RectF {
                x: fw * 0.1,
                y: fh * 0.1,
                w: fw * 0.4,
                h: fh * 0.3,
            },
            rotation: 0.3,
            corner_radius: 3.0,
        },
        Geom::Ellipse {
            rect: RectF {
                x: fw * 0.5,
                y: fh * 0.4,
                w: fw * 0.3,
                h: fh * 0.4,
            },
            rotation: -0.2,
        },
        Geom::Arrow {
            a: pt(1.0, fh - 2.0),
            b: pt(fw - 2.0, 1.0),
            heads: ArrowHeads::Both,
            bend: 0.0,
        },
        Geom::Pen {
            points: (0..20)
                .map(|i| pt(i as f32 * fw / 20.0, fh / 2.0 + (i % 3) as f32))
                .collect(),
        },
        Geom::Highlight {
            points: Arc::from(vec![pt(0.0, fh * 0.8), pt(fw, fh * 0.75)]),
        },
        Geom::Text {
            rect: RectF {
                x: 1.0,
                y: 2.0,
                w: fw - 2.0,
                h: 10.0,
            },
            rotation: 0.1,
            text: "Ab
c"
            .into(),
            font: FontSpec {
                family: "Lato".into(),
                size: 8.0,
                weight: 700,
                italic: true,
            },
            align: TextAlign::Center,
        },
    ];
    for (i, g) in geoms.into_iter().enumerate() {
        let s = Shape {
            id: d.peek_id(),
            geom: g,
            style: style((45 * i) as u8, if i == 2 { 0.5 } else { 1.0 }),
        };
        d = d.apply(&Edit::AddShape(s)).unwrap();
    }
    let red = Redaction {
        id: d.peek_id(),
        area: Area::Rect {
            rect: RectF {
                x: 1.0,
                y: 1.0,
                w: 4.0,
                h: 4.0,
            },
            rotation: 0.0,
        },
        kind: RedactKind::Solid { color: [0, 0, 0] },
    };
    d = d.apply(&Edit::AddRedaction(red)).unwrap();
    d = d
        .apply(&Edit::Reorder {
            id: ObjectId(1),
            to_index: 3,
        })
        .unwrap();
    d.apply(&Edit::SetCrop(Some(RectU {
        x: 2,
        y: 1,
        w: w - 4,
        h: h - 3,
    })))
    .unwrap()
    .apply(&Edit::Rotate { quarter_turns: 1 })
    .unwrap()
}

pub const W: u32 = 24;
pub const H: u32 = 16;

pub fn valid() -> Vec<u8> {
    let png = tiny_png(W, H);
    project::write(&sample_doc(W, H), &png, "photo.png", Format::Png).unwrap()
}

/// A document with one image shape (a 6×4 logo), and that logo.
pub fn image_doc(w: u32, h: u32) -> (DocState, Arc<picture::ImageData>) {
    let logo = picture::ImageData::from_file(&tiny_png(6, 4), None, &Default::default()).unwrap();
    let d = DocState::new(w, h);
    let d = d
        .apply(&Edit::AddShape(Shape {
            id: d.peek_id(),
            geom: Geom::Image {
                rect: RectF {
                    x: 2.0,
                    y: 3.0,
                    w: 12.0,
                    h: 8.0,
                },
                rotation: 0.2,
                image: picture::ImageRef::new(logo.clone()),
            },
            style: Style::default_for(ShapeKind::Image),
        }))
        .unwrap();
    (d, logo)
}

pub fn manifest(len: usize) -> Vec<u8> {
    format!(
        r#"{{"format":"rview-markup","version":1,"app":"test","created":"2026-11-13T00:00:00Z","original_name":"photo.png","original_len":{len}}}"#
    )
    .into_bytes()
}

pub fn doc_json(d: &DocState) -> Vec<u8> {
    serde_json::to_vec(d).unwrap()
}

/// An archive with these entries: (name, data, deflate).
pub fn build(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
    use zip::write::SimpleFileOptions;
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data, deflate) in entries {
        let m = if *deflate {
            zip::CompressionMethod::Deflated
        } else {
            zip::CompressionMethod::Stored
        };
        if name.ends_with('/') {
            z.add_directory(*name, SimpleFileOptions::default()).unwrap();
        } else {
            z.start_file(*name, SimpleFileOptions::default().compression_method(m))
                .unwrap();
            z.write_all(data).unwrap();
        }
    }
    z.finish().unwrap().into_inner()
}

/// The three normal entries with a changed document or manifest.
pub fn with(doc: &[u8], manifest_json: Option<Vec<u8>>) -> Vec<u8> {
    let png = tiny_png(W, H);
    let m = manifest_json.unwrap_or_else(|| manifest(png.len()));
    build(&[
        ("manifest.json", &m, true),
        ("document.json", doc, true),
        ("original.png", &png, false),
    ])
}

/// Replace every occurrence of `from` by `to` (same length) in the raw archive bytes.
pub fn patch(mut z: Vec<u8>, from: &[u8], to: &[u8]) -> Vec<u8> {
    assert_eq!(from.len(), to.len());
    let mut i = 0;
    while i + from.len() <= z.len() {
        if &z[i..i + from.len()] == from {
            z[i..i + from.len()].copy_from_slice(to);
            i += from.len();
        } else {
            i += 1;
        }
    }
    z
}

/// Positions of the local headers ("PK\3\4") and central headers ("PK\1\2").
pub fn headers(z: &[u8], sig: [u8; 4]) -> Vec<usize> {
    (0..z.len().saturating_sub(3)).filter(|&i| z[i..i + 4] == sig).collect()
}

pub const LOCAL: [u8; 4] = [b'P', b'K', 3, 4];
pub const CENTRAL: [u8; 4] = [b'P', b'K', 1, 2];

/// Set the compression method of every entry (local offset 8, central offset 10).
pub fn set_method(mut z: Vec<u8>, method: u16) -> Vec<u8> {
    for i in headers(&z, LOCAL) {
        z[i + 8..i + 10].copy_from_slice(&method.to_le_bytes());
    }
    for i in headers(&z, CENTRAL) {
        z[i + 10..i + 12].copy_from_slice(&method.to_le_bytes());
    }
    z
}

/// Set general purpose flag bit 0 (encrypted) on every entry.
pub fn set_encrypted(mut z: Vec<u8>) -> Vec<u8> {
    for i in headers(&z, LOCAL) {
        z[i + 6] |= 1;
    }
    for i in headers(&z, CENTRAL) {
        z[i + 8] |= 1;
    }
    z
}

/// Every malformed archive of the security tests, named (the fuzzer's seed corpus).
pub fn cases() -> Vec<(&'static str, Vec<u8>)> {
    let v = valid();
    let (idoc, logo) = image_doc(W, H);
    let (id, idj) = (logo.id.clone(), doc_json(&idoc));
    let mv2 = String::from_utf8(manifest(tiny_png(W, H).len()))
        .unwrap()
        .replace("\"version\":1", "\"version\":2")
        .into_bytes();
    let other = picture::ImageData::from_file(&tiny_png(5, 5), None, &Default::default()).unwrap();
    let png = tiny_png(W, H);
    let m = manifest(png.len());
    let d = doc_json(&sample_doc(W, H));
    let mut bad_ids = sample_doc(W, H);
    bad_ids.next_id = 2;
    let mut big_size = sample_doc(W, H);
    big_size.image_size.w += 1;
    let mut crop_out = sample_doc(W, H);
    crop_out.crop = Some(RectU {
        x: 20,
        y: 0,
        w: 10,
        h: 4,
    });
    let mut far = sample_doc(W, H);
    for s in &mut far.shapes {
        if let Geom::Rect { rect, .. } = &mut Arc::make_mut(s).geom {
            rect.x = 1e30;
        }
    }
    let mut wide = sample_doc(W, H);
    Arc::make_mut(&mut wide.shapes[1]).style.width = 1000.0;
    let mut bomb = b"{\"pad\":\"".to_vec();
    bomb.extend(std::iter::repeat_n(b' ', 2 << 20));
    bomb.extend_from_slice(b"\"}");
    let mut crc = v.clone();
    let at = crc.windows(4).position(|w| w == b"IDAT").unwrap();
    crc[at + 6] ^= 0xFF;
    let mut truncated = v.clone();
    truncated.truncate(v.len() - 10);
    vec![
        ("valid", v.clone()),
        ("not_a_zip", b"PK\x03\x04 nothing else".to_vec()),
        ("truncated", truncated),
        ("bzip2", set_method(v.clone(), 12)),
        ("encrypted", set_encrypted(v.clone())),
        (
            "four_entries",
            build(&[
                ("manifest.json", &m, true),
                ("document.json", &d, true),
                ("original.png", &png, false),
                ("extra.txt", b"x", false),
            ]),
        ),
        (
            "two_entries",
            build(&[("manifest.json", &m, true), ("original.png", &png, false)]),
        ),
        (
            "traversal",
            build(&[
                ("manifest.json", &m, true),
                ("document.json", &d, true),
                ("../original.png", &png, false),
            ]),
        ),
        (
            "backslash",
            build(&[
                ("manifest.json", &m, true),
                ("document.json", &d, true),
                ("x\\original.png", &png, false),
            ]),
        ),
        (
            "directory",
            build(&[
                ("manifest.json", &m, true),
                ("document.json", &d, true),
                ("original.png/", b"", false),
            ]),
        ),
        (
            "bad_ext",
            build(&[
                ("manifest.json", &m, true),
                ("document.json", &d, true),
                ("original.exe", &png, false),
            ]),
        ),
        (
            "nested_project",
            build(&[
                ("manifest.json", &m, true),
                ("document.json", &d, true),
                ("original.rvm", &png, false),
            ]),
        ),
        ("duplicate", patch(v.clone(), b"document.json", b"manifest.json")),
        (
            "manifest_too_big",
            with(&d, Some([m.clone(), vec![b' '; 70 * 1024]].concat())),
        ),
        ("bomb", with(&bomb, None)),
        ("crc", crc),
        ("len_mismatch", with(&d, Some(manifest(png.len() + 1)))),
        (
            "too_new",
            with(
                &d,
                Some(
                    String::from_utf8(m.clone())
                        .unwrap()
                        .replace("\"version\":1", "\"version\":3")
                        .into_bytes(),
                ),
            ),
        ),
        (
            "other_format",
            with(
                &d,
                Some(
                    String::from_utf8(m.clone())
                        .unwrap()
                        .replace("rview-markup", "other-markup")
                        .into_bytes(),
                ),
            ),
        ),
        ("bad_json", with(b"{\"image_size\":", None)),
        (
            "deep_json",
            with(&[vec![b'['; 100_000], vec![b']'; 100_000]].concat(), None),
        ),
        ("bad_ids", with(&doc_json(&bad_ids), None)),
        ("size_mismatch", with(&doc_json(&big_size), None)),
        ("crop_outside", with(&doc_json(&crop_out), None)),
        ("far_away", with(&doc_json(&far), None)),
        ("too_wide", with(&doc_json(&wide), None)),
        (
            "image_missing",
            build(&[
                ("manifest.json", &mv2, true),
                ("document.json", &idj, true),
                ("original.png", &png, false),
            ]),
        ),
        (
            "image_unused",
            build(&[
                ("manifest.json", &m, true),
                ("document.json", &d, true),
                ("original.png", &png, false),
                (&format!("image-{}.png", logo.id), &logo.png, false),
            ]),
        ),
        (
            "image_renamed",
            build(&[
                ("manifest.json", &mv2, true),
                ("document.json", &idj, true),
                ("original.png", &png, false),
                (&format!("image-{id}.png"), &other.png, false),
            ]),
        ),
        (
            "image_not_png",
            build(&[
                ("manifest.json", &mv2, true),
                ("document.json", &idj, true),
                ("original.png", &png, false),
                (&format!("image-{id}.png"), b"GIF89a....", false),
            ]),
        ),
        (
            "bad_adjust",
            with(
                format!(r#"{{"image_size":{{"w":{W},"h":{H}}},"next_id":1,"adjust":{{"exposure":9.0}}}}"#).as_bytes(),
                None,
            ),
        ),
        (
            "bad_original",
            build(&[
                ("manifest.json", &manifest(5), true),
                ("document.json", &d, true),
                ("original.png", b"12345", false),
            ]),
        ),
    ]
}
