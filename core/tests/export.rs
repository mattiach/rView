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
        flatten: false,
        dpi: None,
        size: None,
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

// ---------------------------------------------------------------- convert

/// A file converted: decoded, its source encoded without a document.
fn convert(file: &[u8], o: &ExportOptions) -> Result<Vec<u8>, export::ExportError> {
    let src = source(file.to_vec());
    let exif = src.exif.clone();
    export::encode(&export::Flat::of(src), exif.as_deref(), o)
}

fn decoded8(file: &[u8]) -> (u32, u32, Vec<u8>) {
    let d = decode(file, None, &DecodeOptions::default()).unwrap();
    (d.size.w, d.size.h, rv_core::color::rgba8(d.pixels))
}

/// Smooth gradients: lossy codecs can't reproduce them exactly.
fn gradient(w: u32, h: u32) -> image::RgbaImage {
    image::RgbaImage::from_fn(w, h, |x, y| {
        image::Rgba([(x * 7) as u8, (y * 5) as u8, ((x + y) * 3) as u8, 255])
    })
}

fn mean_diff(a: &[u8], b: &[u8]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (*x as i32 - *y as i32).abs() as f64)
        .sum::<f64>()
        / a.len().max(1) as f64
}

#[test]
fn conversion_paths() {
    use rv_core::formats::Format;
    let img = rgba_image(32, 24);
    let png = encode(&img, Enc::Png, None, None);
    let jpeg = encode(&gradient(32, 24), Enc::Jpeg, None, None);
    let webp = encode(&img, Enc::Webp, None, None);
    let jpeg_px = decoded8(&jpeg).2;
    for (name, file, to) in [
        ("PNG→JPEG", &png, ExportFormat::Jpeg),
        ("JPEG→PNG", &jpeg, ExportFormat::Png),
        ("PNG→WebP", &png, ExportFormat::Webp),
        ("JPEG→WebP", &jpeg, ExportFormat::Webp),
        ("WebP→PNG", &webp, ExportFormat::Png),
    ] {
        let out = convert(file, &opts(to)).unwrap();
        assert!(verify(&out, to, 32, 24), "{name}: verify");
        assert_eq!(Format::sniff(&out), Some(to.format()), "{name}: format");
        let (w, h, px) = decoded8(&out);
        assert_eq!((w, h), (32, 24), "{name}: size");
        match (file == &jpeg, to) {
            // lossless targets keep the decoded source exactly, transparency included
            (true, _) => assert_eq!(px, jpeg_px, "{name}"),
            (false, ExportFormat::Jpeg) => {}
            (false, _) => assert_eq!(&px, img.as_raw(), "{name}: pixels and alpha"),
        }
    }
    // PNG with transparency → JPEG: flattened on the background, opaque
    let o = ExportOptions {
        background: [255, 0, 0],
        ..opts(ExportFormat::Jpeg)
    };
    let px = decoded8(&convert(&png, &o).unwrap()).2;
    assert!(px.chunks_exact(4).all(|p| p[3] == 255), "JPEG is opaque");
    let p = &px[(32 + 20) * 4..][..3];
    assert!(
        p[0] > 230 && p[1] < 25 && p[2] < 25,
        "transparent band = background: {p:?}"
    );
}

#[test]
fn jpeg_quality_is_lossy_even_at_100() {
    let img = gradient(64, 48);
    let png = encode(&img, Enc::Png, None, None);
    let at = |q| {
        let o = ExportOptions {
            jpeg_quality: q,
            ..opts(ExportFormat::Jpeg)
        };
        convert(&png, &o).unwrap()
    };
    let (q100, q50, q1) = (at(100), at(50), at(1));
    assert!(q100.len() > q50.len() && q50.len() > q1.len(), "size follows quality");
    let err = |f: &[u8]| mean_diff(&decoded8(f).2, img.as_raw());
    assert!(err(&q100) < err(&q50) && err(&q50) < err(&q1), "error follows quality");
    assert!(err(&q100) > 0.0, "quality 100 is not lossless");
    // out-of-range qualities are clamped, never an error
    let o = ExportOptions {
        jpeg_quality: 0,
        ..opts(ExportFormat::Jpeg)
    };
    assert_eq!(convert(&png, &o).unwrap(), q1);
}

#[test]
fn conversion_keeps_metadata_presets() {
    use exif::{In, Tag};
    let cam = camera_jpeg();
    for preset in MetadataPreset::ALL.iter().copied() {
        for to in [ExportFormat::Png, ExportFormat::Webp, ExportFormat::Jpeg] {
            let out = convert(&cam, &ExportOptions { preset, ..opts(to) }).unwrap();
            let e = exif_of(&out);
            let what = format!("{to:?} {preset:?}");
            if preset == MetadataPreset::StripAll {
                assert!(e.is_none(), "{what}");
                continue;
            }
            let e = e.unwrap_or_else(|| panic!("{what}: no EXIF"));
            assert!(e.get_field(Tag::Make, In::PRIMARY).is_some(), "{what}: Make");
            let gps = e.get_field(Tag::GPSLatitude, In::PRIMARY).is_some();
            assert_eq!(gps, preset == MetadataPreset::Keep, "{what}: GPS");
            assert!(!e.fields().any(|x| x.ifd_num == In::THUMBNAIL), "{what}: IFD1");
        }
    }
}

#[test]
fn conversion_failures() {
    assert!(decode(b"not an image", None, &DecodeOptions::default()).is_err());
    // wider than JPEG (65535) and WebP (16384) can store: an error, not a panic
    let w = 70_000;
    let flat = export::Flat {
        w,
        h: 1,
        pixels: Pixels::Rgba8(vec![128; w as usize * 4]),
        icc: None,
    };
    for f in [ExportFormat::Jpeg, ExportFormat::Webp] {
        assert!(
            matches!(
                export::encode(&flat, None, &opts(f)),
                Err(export::ExportError::Encode(_))
            ),
            "{f:?}"
        );
    }
}

// ---------------------------------------------------------------- background

/// Opaque, fully transparent and half-transparent pixels in one row.
fn alpha_row() -> image::RgbaImage {
    image::RgbaImage::from_fn(3, 1, |x, _| {
        image::Rgba(match x {
            0 => [10, 200, 30, 255],
            1 => [99, 99, 99, 0],
            _ => [200, 100, 0, 128],
        })
    })
}

fn over(c: u8, a: u8, bg: u8) -> u8 {
    ((c as u32 * a as u32 + bg as u32 * (255 - a as u32) + 127) / 255) as u8
}

#[test]
fn background_flattens_every_format() {
    let png = encode(&alpha_row(), Enc::Png, None, None);
    let bg = [0, 64, 255];
    let expected = [
        [10, 200, 30, 255],
        [bg[0], bg[1], bg[2], 255],
        [over(200, 128, bg[0]), over(100, 128, bg[1]), over(0, 128, bg[2]), 255],
    ];
    for f in [
        ExportFormat::Png,
        ExportFormat::Webp,
        ExportFormat::Tiff,
        ExportFormat::Bmp,
    ] {
        let o = ExportOptions {
            background: bg,
            flatten: true,
            ..opts(f)
        };
        let px = decoded8(&convert(&png, &o).unwrap()).2;
        assert_eq!(px, expected.concat(), "{f:?} flattened");
        // not requested: transparency kept
        let px = decoded8(&convert(&png, &opts(f)).unwrap()).2;
        assert_eq!(&px, alpha_row().as_raw(), "{f:?} alpha kept");
    }
    // a fully transparent image becomes the background
    let clear = encode(&image::RgbaImage::new(4, 4), Enc::Png, None, None);
    let o = ExportOptions {
        background: bg,
        flatten: true,
        ..opts(ExportFormat::Png)
    };
    let px = decoded8(&convert(&clear, &o).unwrap()).2;
    assert!(px.chunks_exact(4).all(|p| p == [bg[0], bg[1], bg[2], 255]));
    // an opaque image is unchanged by the option
    let solid = encode(&quadrants(8, 8), Enc::Png, None, None);
    assert_eq!(
        convert(&solid, &o).unwrap(),
        convert(&solid, &opts(ExportFormat::Png)).unwrap()
    );
}

#[test]
fn background_keeps_jpeg_and_sixteen_bit() {
    // JPEG always flattens: the option changes nothing
    let png = encode(&rgba_image(32, 24), Enc::Png, None, None);
    let o = ExportOptions {
        background: [0, 0, 255],
        ..opts(ExportFormat::Jpeg)
    };
    let flag = ExportOptions { flatten: true, ..o };
    assert_eq!(convert(&png, &o).unwrap(), convert(&png, &flag).unwrap());
    // 16-bit: flattened at 16 bit, written without alpha
    let raw: Vec<u16> = vec![60000, 30000, 0, 65535, 1000, 2000, 3000, 0, 40000, 20000, 10000, 32768];
    let mut file = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut file),
        &raw.iter().flat_map(|s| s.to_ne_bytes()).collect::<Vec<u8>>(),
        3,
        1,
        image::ExtendedColorType::Rgba16,
    )
    .unwrap();
    let o = ExportOptions {
        background: [255, 0, 0],
        flatten: true,
        ..opts(ExportFormat::Png)
    };
    let back = decode(&convert(&file, &o).unwrap(), None, &DecodeOptions::default()).unwrap();
    let Pixels::Rgba16(v) = back.pixels else {
        panic!("not 16 bit")
    };
    let mix = |c: u32, a: u32, bg: u32| ((c * a + bg * 257 * (65535 - a) + 32767) / 65535) as u16;
    assert_eq!(
        v,
        vec![
            60000,
            30000,
            0,
            65535,
            65535,
            0,
            0,
            65535,
            mix(40000, 32768, 255),
            mix(20000, 32768, 0),
            mix(10000, 32768, 0),
            65535
        ]
    );
}

#[test]
fn background_in_the_document_space() {
    // a P3 document: the sRGB background is converted like shape colours
    let icc = icc_p3();
    let png = encode(&alpha_row(), Enc::Png, Some(icc.clone()), None);
    let o = ExportOptions {
        background: [255, 0, 0],
        flatten: true,
        ..opts(ExportFormat::Png)
    };
    let out = convert(&png, &o).unwrap();
    let back = decode(&out, None, &DecodeOptions::default()).unwrap();
    assert_eq!(back.color.icc.as_deref(), Some(&icc[..]), "profile kept");
    let px = rv_core::color::rgba8(back.pixels);
    let red = rv_core::color::srgb_to_doc(Some(&icc))(Color::rgb(255, 0, 0));
    assert_eq!(&px[4..8], &[red.r, red.g, red.b, 255]);
    assert_ne!((red.r, red.g, red.b), (255, 0, 0), "P3 red differs from sRGB red");
}

// ---------------------------------------------------------------- DPI

/// Pixels per inch as the file stores them (File Info for JPEG, PNG and EXIF; TIFF tags).
fn resolution(file: &[u8]) -> Option<(f64, f64)> {
    if rv_core::formats::Format::sniff(file) == Some(rv_core::formats::Format::Tiff) {
        use tiff::tags::Tag;
        let mut d = tiff::decoder::Decoder::new(std::io::Cursor::new(file)).ok()?;
        let unit = d.get_tag_u32(Tag::ResolutionUnit).ok()?;
        let rat = |v: tiff::decoder::ifd::Value| match v {
            tiff::decoder::ifd::Value::Rational(n, d) => Some(n as f64 / d as f64),
            _ => None,
        };
        let x = rat(d.get_tag(Tag::XResolution).ok()?)?;
        let y = rat(d.get_tag(Tag::YResolution).ok()?)?;
        return (unit == 2).then_some((x, y));
    }
    let d = rv_core::details::read(file, None, &DecodeOptions::default());
    d.resolution.map(|r| (r.x, r.y))
}

fn bmp_ppm(file: &[u8]) -> (i32, i32) {
    let at = |i: usize| i32::from_le_bytes(file[i..i + 4].try_into().unwrap());
    (at(38), at(42))
}

#[test]
fn dpi_is_metadata_only() {
    let img = rgba_image(32, 24);
    let png = encode(&img, Enc::Png, None, None);
    for dpi in [72, 96, 150, 300, 1, 1234, export::MAX_DPI] {
        for f in ExportFormat::ALL.iter().copied() {
            let what = format!("{f:?} {dpi}");
            let plain = convert(&png, &opts(f)).unwrap();
            let o = ExportOptions {
                dpi: Some(dpi),
                size: None,
                preset: MetadataPreset::Keep,
                ..opts(f)
            };
            let out = convert(&png, &o).unwrap();
            assert!(verify(&out, f, 32, 24), "{what}: same size");
            assert_eq!(decoded8(&out), decoded8(&plain), "{what}: same pixels");
            if f == ExportFormat::Bmp {
                let ppm = (dpi as f64 / 0.0254).round() as i32;
                assert_eq!(bmp_ppm(&out), (ppm, ppm), "{what}");
                assert_eq!(bmp_ppm(&plain), (0, 0), "{what}: none by default");
                continue;
            }
            // PNG stores pixels per metre: within rounding
            let (x, y) = resolution(&out).unwrap_or_else(|| panic!("{what}: no resolution"));
            assert!(
                (x - dpi as f64).abs() < 0.02 && (y - dpi as f64).abs() < 0.02,
                "{what}: {x} {y}"
            );
        }
    }
}

#[test]
fn dpi_survives_metadata_presets() {
    let cam = camera_jpeg();
    for preset in MetadataPreset::ALL.iter().copied() {
        for f in ExportFormat::ALL.iter().copied() {
            let what = format!("{f:?} {preset:?}");
            let o = ExportOptions {
                dpi: Some(300),
                size: None,
                preset,
                ..opts(f)
            };
            let out = convert(&cam, &o).unwrap();
            if f == ExportFormat::Bmp {
                assert_eq!(bmp_ppm(&out).0, 11811, "{what}");
                continue;
            }
            // WebP keeps a resolution only in EXIF: none with strip-all
            if f == ExportFormat::Webp && preset == MetadataPreset::StripAll {
                assert!(resolution(&out).is_none() && exif_of(&out).is_none(), "{what}");
                continue;
            }
            let (x, _) = resolution(&out).unwrap_or_else(|| panic!("{what}: no resolution"));
            assert!((x - 300.0).abs() < 0.02, "{what}: {x}");
            // the EXIF, when written, agrees with the native field
            if let Some(e) = exif_of(&out).filter(|_| f != ExportFormat::Tiff) {
                let r = e.get_field(exif::Tag::XResolution, exif::In::PRIMARY).map(|r| &r.value);
                assert!(
                    matches!(r, Some(exif::Value::Rational(v)) if v[0].num == 300 && v[0].denom == 1),
                    "{what}: EXIF {r:?}"
                );
            }
        }
    }
}

#[test]
fn dpi_invalid_values() {
    let png = encode(&rgba_image(8, 8), Enc::Png, None, None);
    for bad in [0, export::MAX_DPI + 1, u32::MAX] {
        for f in ExportFormat::ALL {
            let o = ExportOptions {
                dpi: Some(bad),
                ..opts(*f)
            };
            assert!(
                matches!(convert(&png, &o), Err(export::ExportError::Dpi(d)) if d == bad),
                "{f:?} {bad}"
            );
        }
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

// ---------------------------------------------------------------- resize

#[test]
fn resize_every_format_and_entry() {
    use exif::{In, Tag};
    // a document (crop + rotation + shapes), the camera file converted, and every format
    let src = source(encode(&rgba_image(32, 24), Enc::Png, None, None));
    let flat = flatten(&doc_for(32, 24), &src);
    for f in ExportFormat::ALL.iter().copied() {
        for size in [(11, 15), (46, 60), (1, 1), (23, 31)] {
            let o = ExportOptions {
                size: Some(size),
                ..opts(f)
            };
            let out = export::encode(&flat, None, &o).unwrap();
            assert_eq!(o.out_size(flat.w, flat.h), size);
            assert!(verify(&out, f, size.0, size.1), "{f:?} {size:?}");
        }
    }
    let cam = camera_jpeg();
    for f in [ExportFormat::Jpeg, ExportFormat::Png, ExportFormat::Webp] {
        let o = ExportOptions {
            size: Some((12, 16)),
            preset: MetadataPreset::Keep,
            ..opts(f)
        };
        let e = exif_of(&convert(&cam, &o).unwrap()).unwrap();
        let dim = |t| e.get_field(t, In::PRIMARY).and_then(|x| x.value.get_uint(0));
        assert_eq!(
            (dim(Tag::PixelXDimension), dim(Tag::PixelYDimension)),
            (Some(12), Some(16)),
            "{f:?}: EXIF size is the resized one"
        );
    }
}

#[test]
fn resize_same_size_is_untouched_and_colour_is_kept() {
    let png = encode(&gradient(32, 24), Enc::Png, Some(icc_p3()), None);
    let same = ExportOptions {
        size: Some((32, 24)),
        ..opts(ExportFormat::Png)
    };
    assert_eq!(
        convert(&png, &same).unwrap(),
        convert(&png, &opts(ExportFormat::Png)).unwrap()
    );
    let half = ExportOptions {
        size: Some((16, 12)),
        ..opts(ExportFormat::Png)
    };
    let out = convert(&png, &half).unwrap();
    let back = decode(&out, None, &DecodeOptions::default()).unwrap();
    assert_eq!(back.color.icc.as_deref(), Some(&icc_p3()[..]), "profile kept");
    // a smooth gradient halved stays close to the source sampled at the same places
    let (w, _, src) = decoded8(&png);
    let (_, _, small) = decoded8(&out);
    let mut expect = Vec::new();
    for y in 0..12 {
        for x in 0..16 {
            for c in 0..4 {
                let at = |dx: usize, dy: usize| src[((2 * y + dy) * w as usize + 2 * x + dx) * 4 + c] as u32;
                expect.push(((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8);
            }
        }
    }
    assert!(mean_diff(&small, &expect) < 2.0, "{}", mean_diff(&small, &expect));
}

#[test]
fn resize_keeps_alpha_edges_clean_and_sixteen_bit() {
    // opaque white on the left, transparent black on the right: no dark fringe when shrunk
    let mut img = image::RgbaImage::new(40, 4);
    for (x, _, p) in img.enumerate_pixels_mut() {
        p.0 = if x < 20 { [255, 255, 255, 255] } else { [0, 0, 0, 0] };
    }
    let o = ExportOptions {
        size: Some((7, 1)),
        ..opts(ExportFormat::Png)
    };
    let (_, _, px) = decoded8(&convert(&encode(&img, Enc::Png, None, None), &o).unwrap());
    assert!(
        px.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255),
        "a soft edge: {px:?}"
    );
    for p in px.chunks_exact(4).filter(|p| p[3] > 0) {
        assert_eq!(&p[..3], &[255, 255, 255], "no fringe: {px:?}");
    }
    // 16 bit in, 16 bit out, with precision below 8 bit kept
    let raw: Vec<u16> = (0..8u16).flat_map(|i| [1000 + i, 2001, 3002, 65535]).collect();
    let mut file = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut file),
        &raw.iter().flat_map(|s| s.to_ne_bytes()).collect::<Vec<u8>>(),
        8,
        1,
        image::ExtendedColorType::Rgba16,
    )
    .unwrap();
    let o = ExportOptions {
        size: Some((4, 1)),
        ..opts(ExportFormat::Png)
    };
    let back = decode(&convert(&file, &o).unwrap(), None, &DecodeOptions::default()).unwrap();
    assert_eq!((back.size.w, back.size.h), (4, 1));
    let Pixels::Rgba16(v) = back.pixels else {
        panic!("not 16 bit")
    };
    for p in v.chunks_exact(4) {
        assert!((999..=1008).contains(&p[0]) && p[1] == 2001 && p[2] == 3002, "{v:?}");
    }
}

#[test]
fn resize_limits() {
    let png = encode(&quadrants(8, 8), Enc::Png, None, None);
    for bad in [(0, 8), (8, 0), (16385, 16385)] {
        let o = ExportOptions {
            size: Some(bad),
            ..opts(ExportFormat::Png)
        };
        assert!(
            matches!(convert(&png, &o), Err(export::ExportError::Size(w, h)) if (w, h) == bad),
            "{bad:?}"
        );
    }
    assert!(
        rv_core::resample::allowed(100_000, 100_000, 50_000, 50_000),
        "shrinking is always allowed"
    );
    assert!(rv_core::resample::allowed(8, 8, 16384, 16384), "up to 2^28 pixels");
}
