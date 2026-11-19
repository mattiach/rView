//! `.rvm` projects: round trip, and one test per safe-reading rule.

mod common;
#[path = "common/projects.rs"]
mod projects;

use projects::*;
use rv_core::decode::DecodeOptions;
use rv_core::edit::*;
use rv_core::export::{DocSource, flatten};
use rv_core::project::{self, ProjectError};
use std::sync::Arc;

fn read(b: &[u8]) -> Result<project::Project, ProjectError> {
    project::read(b, &DecodeOptions::default())
}

fn case(name: &str) -> Vec<u8> {
    cases().into_iter().find(|(n, _)| *n == name).unwrap().1
}

#[track_caller]
fn corrupt(name: &str, what: &str) {
    match read(&case(name)) {
        Err(ProjectError::Corrupt(m)) => assert!(m.contains(what), "{name}: {m:?} lacks {what:?}"),
        Err(e) => panic!("{name}: {e:?}, expected Corrupt"),
        Ok(_) => panic!("{name}: accepted"),
    }
}

#[test]
fn round_trip_keeps_ids_order_styles_and_render() {
    let png = tiny_png(W, H);
    let doc = sample_doc(W, H);
    let bytes = project::write(&doc, &png, "photo.png", rv_core::formats::Format::Png).unwrap();
    let p = read(&bytes).unwrap();
    assert_eq!(p.original, png, "original bytes identical");
    assert_eq!(p.manifest.original_name, "photo.png");
    assert_eq!(p.manifest.format, "rview-markup");
    assert!(p.manifest.app.starts_with("rView "));
    let mut loaded = p.doc.clone();
    loaded.revision = doc.revision;
    assert_eq!(loaded, doc, "IDs, z-order, styles, crop, orientation");
    // the history gives the loaded state a real revision and it starts clean
    let h = History::from_loaded(p.doc.clone());
    assert!(!h.is_dirty() && h.current().revision.0 >= 1);

    // export before and after reopening: bit-identical
    let src = |png: &[u8]| {
        let img = rv_core::decode::decode(png, None, &DecodeOptions::default()).unwrap();
        DocSource::new(img, Arc::from(png.to_vec()))
    };
    let (a, b) = (flatten(&doc, &src(&png)), flatten(&p.doc, &src(&p.original)));
    assert_eq!((a.w, a.h), (b.w, b.h));
    assert_eq!(rv_core::color::rgba8(a.pixels), rv_core::color::rgba8(b.pixels));

    // entries as rule 9 says: JSON deflated, original stored
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let m = |z: &mut zip::ZipArchive<_>, n: &str| z.by_name(n).unwrap().compression();
    assert_eq!(m(&mut z, "manifest.json"), zip::CompressionMethod::Deflated);
    assert_eq!(m(&mut z, "document.json"), zip::CompressionMethod::Deflated);
    assert_eq!(m(&mut z, "original.png"), zip::CompressionMethod::Stored);
}

#[test]
fn viewer_decodes_a_project_as_its_render() {
    let bytes = valid();
    let img = rv_core::decode::decode(&bytes, None, &DecodeOptions::default()).unwrap();
    assert_eq!(img.format, rv_core::formats::Format::Project);
    let doc = sample_doc(W, H);
    assert_eq!((img.size.w, img.size.h), doc.canvas_size());
    let src = DocSource::new(
        rv_core::decode::decode(&tiny_png(W, H), None, &DecodeOptions::default()).unwrap(),
        Arc::from(Vec::new()),
    );
    assert_eq!(
        rv_core::color::rgba8(img.pixels),
        rv_core::color::rgba8(flatten(&doc, &src).pixels)
    );
    assert!(rv_core::formats::is_supported_path(std::path::Path::new("a.rvm")));
}

#[test]
fn unknown_fields_ignored_missing_defaults() {
    let png = tiny_png(W, H);
    let doc = format!(r#"{{"image_size":{{"w":{W},"h":{H}}},"next_id":1,"future_field":[1,2,3]}}"#);
    let p = read(&with(doc.as_bytes(), None)).unwrap();
    assert!(p.doc.shapes.is_empty() && p.doc.crop.is_none() && p.doc.orient == rv_core::orient::Orient::IDENTITY);
    assert_eq!(p.original, png);
}

// ---- file size
#[test]
fn rule1_file_size_limit() {
    let d = std::env::temp_dir().join(format!("rview-project-big-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let p = d.join("big.rvm");
    let f = std::fs::File::create(&p).unwrap();
    f.set_len(project::MAX_FILE + 1).unwrap(); // sparse: nothing is written
    drop(f);
    let r = project::read_file(&p, &DecodeOptions::default());
    let _ = std::fs::remove_dir_all(&d);
    assert!(matches!(r, Err(ProjectError::TooLarge)), "{:?}", r.err());
}

// ---- rule 2: ZIP structure, methods
#[test]
fn rule2_structure_and_methods() {
    corrupt("not_a_zip", "");
    corrupt("truncated", "");
    for name in ["bzip2", "encrypted"] {
        let r = read(&case(name));
        assert!(matches!(r, Err(ProjectError::Unsupported(_))), "{name}: {:?}", r.err());
    }
    // local header name differing from the central directory
    let v = valid();
    let mut z = v.clone();
    let local = headers(&z, LOCAL)[1];
    let n = u16::from_le_bytes([z[local + 26], z[local + 27]]) as usize;
    z[local + 30..local + 30 + n].copy_from_slice(&b"documenX.json"[..n]);
    assert!(read(&z).is_err(), "mismatched local header accepted");
}

// ---- rule 3: exactly the three entries, safe names
#[test]
fn rule3_entries() {
    corrupt("four_entries", "4 entries");
    corrupt("two_entries", "2 entries");
    corrupt("traversal", "not allowed");
    corrupt("backslash", "not allowed");
    corrupt("directory", "");
    corrupt("bad_ext", "unexpected");
    corrupt("nested_project", "unexpected");
    assert!(read(&case("duplicate")).is_err(), "duplicate names accepted");
}

// ---- rule 4: decompressed sizes, not trusting the headers
#[test]
fn rule4_size_limits() {
    corrupt("manifest_too_big", "manifest.json is larger");
    corrupt("len_mismatch", "original size");
    // document.json above 64 MiB (stored, so the ratio rule doesn't fire first)
    let mut doc = b"{\"pad\":\"".to_vec();
    doc.resize(64 * 1024 * 1024 + 16, b' ');
    let png = tiny_png(W, H);
    let z = build(&[
        ("manifest.json", &manifest(png.len()), true),
        ("document.json", &doc, false),
        ("original.png", &png, false),
    ]);
    match read(&z) {
        Err(ProjectError::Corrupt(m)) => assert!(m.contains("document.json is larger"), "{m}"),
        r => panic!("{:?}", r.err()),
    }
    // a declared uncompressed size that lies (smaller than the data)
    let v = valid();
    let mut z = v.clone();
    for i in headers(&z, CENTRAL) {
        z[i + 24..i + 28].copy_from_slice(&3u32.to_le_bytes());
    }
    for i in headers(&z, LOCAL) {
        z[i + 22..i + 26].copy_from_slice(&3u32.to_le_bytes());
    }
    assert!(read(&z).is_err(), "lying sizes accepted");
}

// ---- rule 5: bomb
#[test]
fn rule5_zip_bomb() {
    corrupt("bomb", "ratio");
}

// ---- rule 6: CRC
#[test]
fn rule6_crc() {
    corrupt("crc", "");
}

// ---- rule 7: semantic validation
#[test]
fn rule7_semantics() {
    assert!(matches!(read(&case("too_new")), Err(ProjectError::TooNew)));
    corrupt("other_format", "format");
    corrupt("bad_json", "document.json");
    corrupt("deep_json", "document.json"); // serde_json recursion limit
    corrupt("bad_ids", "object IDs");
    corrupt("size_mismatch", "image size");
    corrupt("crop_outside", "crop");
    corrupt("far_away", "out of range");
    corrupt("too_wide", "out of range");

    // counts, checked on documents too big for a test archive
    let base = DocState::new(100, 100);
    let mut many = base.clone();
    let dot = |id| {
        Arc::new(Shape {
            id: ObjectId(id),
            geom: Geom::Line {
                a: PointF { x: 0.0, y: 0.0 },
                b: PointF { x: 5.0, y: 5.0 },
            },
            style: Style::default_for(ShapeKind::Line),
        })
    };
    many.shapes = (1..=100_001).map(dot).collect();
    many.next_id = 100_002;
    let msg = |d: &DocState| match project::validate(d, 100, 100) {
        Err(ProjectError::Corrupt(m)) => m,
        r => panic!("{r:?}"),
    };
    assert!(msg(&many).contains("too many objects"));
    let stroke = |id, n: usize| {
        Arc::new(Shape {
            id: ObjectId(id),
            geom: Geom::Pen {
                points: Arc::from(vec![PointF { x: 1.0, y: 1.0 }; n]),
            },
            style: Style::default_for(ShapeKind::Pen),
        })
    };
    let mut long = base.clone();
    long.shapes = vec![stroke(1, 1_000_001)];
    long.next_id = 2;
    assert!(msg(&long).contains("out of range"));
    let mut total = base.clone();
    total.shapes = (1..=11).map(|i| stroke(i, 1_000_000)).collect();
    total.next_id = 12;
    assert!(msg(&total).contains("too many points"));
    let mut ok = base.clone();
    ok.shapes = (1..=10).map(|i| stroke(i, 1_000_000)).collect();
    ok.next_id = 11;
    project::validate(&ok, 100, 100).unwrap();
    let mut rot = base.clone();
    rot.orient.quarter_turns = 4;
    assert!(msg(&rot).contains("orientation"));
    let mut blur = base;
    blur.redactions = vec![Arc::new(Redaction {
        id: ObjectId(1),
        area: Area::Rect {
            rect: RectF {
                x: 0.0,
                y: 0.0,
                w: 5.0,
                h: 5.0,
            },
            rotation: 0.0,
        },
        kind: RedactKind::Pixelate { block: 0 },
    })];
    blur.next_id = 2;
    assert!(msg(&blur).contains("redaction 1"));
}

// ---- rule 8: the original goes through the normal decoder and its limits
#[test]
fn rule8_original_decoded() {
    assert!(matches!(read(&case("bad_original")), Err(ProjectError::Original(_))));
    // a PNG header claiming 100 000 × 100 000 (10 GP > 2 GP)
    let mut png = tiny_png(W, H);
    png[16..20].copy_from_slice(&100_000u32.to_be_bytes());
    png[20..24].copy_from_slice(&100_000u32.to_be_bytes());
    let z = build(&[
        ("manifest.json", &manifest(png.len()), true),
        ("document.json", &doc_json(&sample_doc(W, H)), true),
        ("original.png", &png, false),
    ]);
    assert!(matches!(read(&z), Err(ProjectError::Original(_))));
}

#[test]
fn every_seed_case_is_rejected_without_panic() {
    for (name, bytes) in cases() {
        let r = std::panic::catch_unwind(|| read(&bytes).is_ok());
        assert_eq!(r.ok(), Some(name == "valid"), "{name}");
    }
}
