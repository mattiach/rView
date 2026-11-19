//! Export: the decoded result equals the render, ICC as the table says,
//! 16-bit output, and the metadata rules (no thumbnails/MPF/APP13/XMP, Orientation 1, presets).

mod common;

use common::*;
use img_parts::Bytes;
use rv_core::decode::{DecodeOptions, Pixels, decode};
use rv_core::edit::*;
use rv_core::export::{self, DocSource, ExportFormat, ExportOptions, flatten, verify};
use rv_core::settings::MetadataPreset;
use std::sync::Arc;

fn source(bytes: Vec<u8>) -> DocSource {
    let img = decode(&bytes, None, &DecodeOptions::default()).unwrap();
    DocSource::new(img, Arc::from(bytes))
}

/// Quadrants with a transparent band and a half-transparent one.
fn rgba_image(w: u32, h: u32) -> image::RgbaImage {
    let mut img = quadrants(w, h);
    for (x, y, p) in img.enumerate_pixels_mut() {
        if y < 3 {
            p.0[3] = 0;
        } else if x < 4 {
            p.0[3] = 128;
        }
    }
    img
}

fn doc_for(w: u32, h: u32) -> DocState {
    let mut d = DocState::new(w, h);
    for g in [
        Geom::Rect {
            rect: RectF {
                x: 4.0,
                y: 4.0,
                w: 14.0,
                h: 9.0,
            },
            rotation: 0.2,
            corner_radius: 2.0,
        },
        Geom::Arrow {
            a: PointF { x: 2.0, y: 20.0 },
            b: PointF { x: 28.0, y: 6.0 },
            heads: ArrowHeads::End,
            bend: 0.0,
        },
    ] {
        let mut st = Style::default_for(g.kind());
        st.fill = Some(Color {
            r: 0,
            g: 160,
            b: 255,
            a: 200,
        });
        st.opacity = 0.8;
        let s = Shape {
            id: d.peek_id(),
            geom: g,
            style: st,
        };
        d = d.apply(&Edit::AddShape(s)).unwrap();
    }
    d.apply(&Edit::SetCrop(Some(RectU {
        x: 1,
        y: 0,
        w: w - 2,
        h: h - 1,
    })))
    .unwrap()
    .apply(&Edit::Rotate { quarter_turns: 1 })
    .unwrap()
}

fn opts(format: ExportFormat) -> ExportOptions {
    ExportOptions {
        format,
        jpeg_quality: 95,
        background: [255, 255, 255],
        preset: MetadataPreset::StripLocation,
    }
}

#[test]
fn lossless_formats_decode_to_the_render_with_icc() {
    let icc = icc_p3();
    let src = source(encode(&rgba_image(32, 24), Enc::Png, Some(icc.clone()), None));
    assert_eq!(src.icc.as_deref(), Some(&icc[..]), "document space = source profile");
    let doc = doc_for(32, 24);
    let flat = flatten(&doc, &src);
    let expected = rv_core::color::rgba8(flat.pixels.clone());
    for f in [
        ExportFormat::Png,
        ExportFormat::Webp,
        ExportFormat::Tiff,
        ExportFormat::Bmp,
    ] {
        let out = export::encode(&flat, src.exif.as_deref(), &opts(f)).unwrap();
        assert!(verify(&out, f, flat.w, flat.h), "{f:?} verify");
        let back = decode(&out, None, &DecodeOptions::default()).unwrap();
        assert_eq!((back.size.w, back.size.h), (flat.w, flat.h), "{f:?}");
        let px = rv_core::color::rgba8(back.pixels);
        if f == ExportFormat::Bmp {
            // no ICC in BMP: converted to sRGB
            assert!(back.color.icc.is_none());
            let mut srgb = expected.clone();
            rv_core::color::to_srgb_in_place(&mut srgb, Some(&icc));
            assert_eq!(px, srgb, "BMP pixels = render in sRGB");
        } else {
            assert_eq!(back.color.icc.as_deref(), Some(&icc[..]), "{f:?} ICC");
            assert_eq!(px, expected, "{f:?} pixels = render");
        }
    }
    // shape colours converted sRGB → document space: a P3 export differs from an sRGB one
    let srgb_src = source(encode(&rgba_image(32, 24), Enc::Png, None, None));
    let a = rv_core::color::rgba8(flatten(&doc, &srgb_src).pixels);
    assert_ne!(a, expected);
}

#[test]
fn opaque_exports_and_jpeg_background() {
    let mut img = rgba_image(32, 24);
    let src = source(encode(&img, Enc::Png, None, None));
    let doc = DocState::new(32, 24);
    let flat = flatten(&doc, &src);
    let o = ExportOptions {
        background: [0, 0, 255],
        ..opts(ExportFormat::Jpeg)
    };
    let out = export::encode(&flat, None, &o).unwrap();
    assert!(verify(&out, ExportFormat::Jpeg, 32, 24));
    let back = rv_core::color::rgba8(decode(&out, None, &DecodeOptions::default()).unwrap().pixels);
    // the transparent band became the background
    let p = &back[(32 + 20) * 4..][..3];
    assert!(p[0] < 20 && p[1] < 20 && p[2] > 230, "{p:?}");
    // an opaque image: written without alpha, decodes to the same pixels
    for (_, y, p) in img.enumerate_pixels_mut() {
        p.0[3] = 255;
        let _ = y;
    }
    let flat = flatten(&doc, &source(encode(&img, Enc::Png, None, None)));
    for f in [
        ExportFormat::Png,
        ExportFormat::Webp,
        ExportFormat::Tiff,
        ExportFormat::Bmp,
    ] {
        let out = export::encode(&flat, None, &opts(f)).unwrap();
        let back = decode(&out, None, &DecodeOptions::default()).unwrap();
        assert_eq!(rv_core::color::rgba8(back.pixels), img.as_raw().clone(), "{f:?}");
    }
}

#[test]
fn sixteen_bit_png_and_tiff() {
    let (w, h) = (20u32, 12u32);
    let raw: Vec<u16> = (0..w * h)
        .flat_map(|i| {
            [
                (i * 211) as u16,
                (i * 4099) as u16,
                1000 + i as u16,
                if i % 7 == 0 { 30000 } else { 65535 },
            ]
        })
        .collect();
    let mut png = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut png),
        &raw.iter().flat_map(|s| s.to_ne_bytes()).collect::<Vec<u8>>(),
        w,
        h,
        image::ExtendedColorType::Rgba16,
    )
    .unwrap();
    let src = source(png);
    assert!(matches!(*src.pixels, Pixels::Rgba16(_)));
    let mut doc = DocState::new(w, h);
    let s = Shape {
        id: doc.peek_id(),
        geom: Geom::Line {
            a: PointF { x: 0.0, y: 0.0 },
            b: PointF { x: 19.0, y: 11.0 },
        },
        style: Style {
            opacity: 0.6,
            ..Style::default_for(ShapeKind::Line)
        },
    };
    doc = doc.apply(&Edit::AddShape(s)).unwrap();
    let flat = flatten(&doc, &src);
    let Pixels::Rgba16(expected) = &flat.pixels else {
        panic!("16-bit render")
    };
    // pixels away from the line keep their exact 16-bit values
    let Pixels::Rgba16(orig) = &*src.pixels else {
        unreachable!()
    };
    assert_eq!(
        expected[(11 * w as usize) * 4..][..4],
        orig[(11 * w as usize) * 4..][..4]
    );
    assert_ne!(expected, orig, "the line is drawn");
    for f in [ExportFormat::Png, ExportFormat::Tiff] {
        let out = export::encode(&flat, None, &opts(f)).unwrap();
        let back = decode(&out, None, &DecodeOptions::default()).unwrap();
        match back.pixels {
            Pixels::Rgba16(v) => assert_eq!(&v, expected, "{f:?}"),
            _ => panic!("{f:?}: not 16 bit"),
        }
    }
    // and the 8-bit view of the 16-bit render matches the 8-bit render of the 8-bit source within 1
    let src8 = DocSource {
        pixels: Arc::new(Pixels::Rgba8(rv_core::color::rgba8((*src.pixels).clone()))),
        ..src.clone()
    };
    let a = rv_core::color::rgba8(flat.pixels);
    let b = rv_core::color::rgba8(flatten(&doc, &src8).pixels);
    assert!(close(&a, &b, 2), "16-bit and 8-bit renders agree");
}

// ---------------------------------------------------------------- metadata

/// EXIF with Orientation 6, Make, Artist, a body serial, a MakerNote, GPS and an IFD1 thumbnail.
fn rich_exif() -> Vec<u8> {
    use exif::{Field, In, Tag, Value};
    let ascii = |s: &str| Value::Ascii(vec![s.as_bytes().to_vec()]);
    let f = |tag, ifd_num, value| Field { tag, ifd_num, value };
    let fields = [
        f(Tag::Orientation, In::PRIMARY, Value::Short(vec![6])),
        f(Tag::Make, In::PRIMARY, ascii("Cam")),
        f(Tag::Artist, In::PRIMARY, ascii("Someone")),
        f(Tag::BodySerialNumber, In::PRIMARY, ascii("SN123")),
        f(Tag::MakerNote, In::PRIMARY, Value::Undefined(vec![9; 16], 0)),
        f(Tag::GPSLatitudeRef, In::PRIMARY, ascii("N")),
        f(Tag::GPSLatitude, In::PRIMARY, Value::Rational(vec![(45, 1).into(); 3])),
        f(Tag::Compression, In::THUMBNAIL, Value::Short(vec![6])),
    ];
    let mut w = exif::experimental::Writer::new();
    for x in &fields {
        w.push_field(x);
    }
    let thumb = encode(&quadrants(8, 8), Enc::Jpeg, None, None);
    w.set_jpeg(&thumb, In::THUMBNAIL);
    let mut out = std::io::Cursor::new(Vec::new());
    w.write(&mut out, false).unwrap();
    out.into_inner()
}

/// A JPEG with every kind of embedded preview the export must drop.
fn camera_jpeg() -> Vec<u8> {
    use img_parts::ImageEXIF;
    use img_parts::jpeg::{Jpeg, JpegSegment};
    let base = encode(&quadrants(32, 24), Enc::Jpeg, None, None);
    let mut j = Jpeg::from_bytes(Bytes::from(base)).unwrap();
    j.set_exif(Some(Bytes::from(rich_exif())));
    let seg = |m, s: &[u8]| JpegSegment::new_with_contents(m, Bytes::copy_from_slice(s));
    let segs = j.segments_mut();
    segs.insert(1, seg(0xE0, b"JFXX\0\x10thumbnail"));
    segs.insert(2, seg(0xE1, b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta/>"));
    segs.insert(3, seg(0xED, b"Photoshop 3.0\08BIM\x04\x0c thumb"));
    segs.insert(4, seg(0xE2, b"MPF\0II*\0 second image"));
    let mut out = Vec::new();
    j.encoder().write_to(&mut out).unwrap();
    out
}

fn exif_of(file: &[u8]) -> Option<exif::Exif> {
    exif::Reader::new()
        .read_from_container(&mut std::io::Cursor::new(file))
        .ok()
}

#[test]
fn metadata_rules_and_presets() {
    use exif::{In, Tag};
    let cam = camera_jpeg();
    let src = source(cam.clone());
    assert_eq!((src.w, src.h), (24, 32), "orientation 6 applied once");
    assert!(exif_of(&cam).is_some_and(|e| e.fields().any(|f| f.ifd_num == In::THUMBNAIL)));
    let doc = DocState::new(24, 32);
    let flat = flatten(&doc, &src);
    for preset in MetadataPreset::ALL.iter().copied() {
        for f in [ExportFormat::Jpeg, ExportFormat::Png, ExportFormat::Webp] {
            let o = ExportOptions { preset, ..opts(f) };
            let out = export::encode(&flat, src.exif.as_deref(), &o).unwrap();
            let what = format!("{f:?} {preset:?}");
            // decoded upright, not rotated again
            let back = decode(&out, None, &DecodeOptions::default()).unwrap();
            assert_eq!((back.size.w, back.size.h), (24, 32), "{what}");
            if f == ExportFormat::Jpeg {
                let j = img_parts::jpeg::Jpeg::from_bytes(Bytes::from(out.clone())).unwrap();
                let has = |m: u8, prefix: &[u8]| j.segments_by_marker(m).any(|s| s.contents().starts_with(prefix));
                assert!(!has(0xE0, b"JFXX"), "{what}: JFXX");
                assert!(!has(0xE1, b"http://ns.adobe.com/xap"), "{what}: XMP");
                assert!(j.segment_by_marker(0xED).is_none(), "{what}: APP13");
                assert!(!has(0xE2, b"MPF"), "{what}: MPF");
            }
            let e = exif_of(&out);
            if preset == MetadataPreset::StripAll {
                assert!(e.is_none(), "{what}: EXIF with strip-all");
                continue;
            }
            let e = e.unwrap_or_else(|| panic!("{what}: no EXIF"));
            let tag = |t| e.get_field(t, In::PRIMARY);
            assert!(!e.fields().any(|x| x.ifd_num == In::THUMBNAIL), "{what}: IFD1");
            assert_eq!(
                tag(Tag::Orientation).and_then(|x| x.value.get_uint(0)),
                Some(1),
                "{what}"
            );
            assert_eq!(
                tag(Tag::PixelXDimension).and_then(|x| x.value.get_uint(0)),
                Some(24),
                "{what}"
            );
            assert!(tag(Tag::MakerNote).is_none(), "{what}: MakerNote");
            assert!(tag(Tag::Make).is_some(), "{what}: Make kept");
            let gps = tag(Tag::GPSLatitude).is_some();
            let serial = tag(Tag::BodySerialNumber).is_some() || tag(Tag::Artist).is_some();
            assert_eq!(gps, preset == MetadataPreset::Keep, "{what}: GPS");
            assert_eq!(serial, preset == MetadataPreset::Keep, "{what}: identifying tags");
        }
    }
    // TIFF and BMP never carry EXIF
    for f in [ExportFormat::Tiff, ExportFormat::Bmp] {
        let o = ExportOptions {
            preset: MetadataPreset::Keep,
            ..opts(f)
        };
        let out = export::encode(&flat, src.exif.as_deref(), &o).unwrap();
        let back = decode(&out, None, &DecodeOptions::default()).unwrap();
        assert!(back.meta.exif.is_none_or(|e| {
            !exif::Reader::new()
                .read_raw(e)
                .is_ok_and(|x| x.get_field(Tag::GPSLatitude, In::PRIMARY).is_some())
        }));
    }
}

#[test]
fn export_formats_of_sources() {
    use rv_core::formats::Format;
    assert_eq!(ExportFormat::of(Format::Jpeg), Some(ExportFormat::Jpeg));
    assert_eq!(ExportFormat::of(Format::Heif), None, "HEIC: Export proposes PNG");
    assert_eq!(ExportFormat::of(Format::Gif), None);
    assert_eq!(ExportFormat::of(Format::Project), None);
}
