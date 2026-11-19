//! Decoding per format, orientation, errors, provisional colour pipeline.
//! Fixtures are generated in memory; formats without a pure-Rust/WIC encoder print SKIPPED.

mod common;
use common::*;
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use rv_core::decode::{self, DecodeError, DecodeOptions, DecodedImage, Pixels};
use rv_core::formats::Format;

fn dec(bytes: &[u8], ext: Option<Format>) -> DecodedImage {
    decode::decode(bytes, ext, &DecodeOptions::default()).unwrap()
}

fn rgba8(d: &DecodedImage) -> Vec<u8> {
    match &d.pixels {
        Pixels::Rgba8(v) => v.clone(),
        Pixels::Rgba16(v) => v.iter().map(|&s| (s >> 8) as u8).collect(),
    }
}

/// Check size and the four quadrant colours (sampled at quadrant centres).
fn check_quadrants(d: &DecodedImage, w: u32, h: u32, tol: i32) {
    assert_eq!((d.size.w, d.size.h), (w, h), "{:?}", d.format);
    let px = rgba8(d);
    let at = |x: u32, y: u32| &px[((y * w + x) * 4) as usize..][..4];
    let (qx, qy) = (w / 4, h / 4);
    for (x, y, c) in [
        (qx, qy, RED),
        (w - qx, qy, GREEN),
        (qx, h - qy, BLUE),
        (w - qx, h - qy, WHITE),
    ] {
        assert!(
            close(at(x, y), &c, tol),
            "{:?} at ({x},{y}) = {:?}, want {c:?}",
            d.format,
            at(x, y)
        );
    }
}

fn enc_with(f: impl FnOnce(&mut Vec<u8>, &RgbaImage)) -> Vec<u8> {
    let mut out = Vec::new();
    f(&mut out, &quadrants(32, 16));
    out
}

#[test]
fn rust_decoders() {
    let img = quadrants(32, 16);
    let rgb = image::DynamicImage::ImageRgba8(img.clone()).into_rgb8();
    let cases: Vec<(Format, Vec<u8>, i32)> = vec![
        (Format::Png, encode(&img, Enc::Png, None, None), 0),
        (Format::Jpeg, encode(&img, Enc::Jpeg, None, None), 12),
        (Format::Webp, encode(&img, Enc::Webp, None, None), 0),
        (Format::Tiff, encode(&img, Enc::Tiff, None, None), 0),
        (
            Format::Gif,
            enc_with(|o, i| {
                image::codecs::gif::GifEncoder::new(o)
                    .encode_frame(image::Frame::new(i.clone()))
                    .unwrap()
            }),
            0,
        ),
        (
            Format::Bmp,
            enc_with(|o, i| {
                image::codecs::bmp::BmpEncoder::new(o)
                    .write_image(i, 32, 16, ExtendedColorType::Rgba8)
                    .unwrap()
            }),
            0,
        ),
        (
            Format::Ico,
            enc_with(|o, i| {
                image::codecs::ico::IcoEncoder::new(o)
                    .write_image(i, 32, 16, ExtendedColorType::Rgba8)
                    .unwrap()
            }),
            0,
        ),
        (
            Format::Tga,
            enc_with(|o, i| {
                image::codecs::tga::TgaEncoder::new(o)
                    .write_image(i, 32, 16, ExtendedColorType::Rgba8)
                    .unwrap()
            }),
            0,
        ),
        (
            Format::Qoi,
            enc_with(|o, i| {
                image::codecs::qoi::QoiEncoder::new(o)
                    .write_image(i, 32, 16, ExtendedColorType::Rgba8)
                    .unwrap()
            }),
            0,
        ),
        (
            Format::Pnm,
            {
                let mut o = Vec::new();
                image::codecs::pnm::PnmEncoder::new(&mut o)
                    .write_image(&rgb, 32, 16, ExtendedColorType::Rgb8)
                    .unwrap();
                o
            },
            0,
        ),
    ];
    for (f, bytes, tol) in cases {
        // TGA has no magic bytes: the viewer passes the extension's format as hint.
        let d = dec(&bytes, (f == Format::Tga).then_some(Format::Tga));
        assert_eq!(d.format, f);
        check_quadrants(&d, 32, 16, tol);
        println!("DECODE {f:?}: OK");
    }
}

#[test]
fn png_16bit_and_alpha() {
    let img16 =
        image::ImageBuffer::<image::Rgba<u16>, _>::from_fn(4, 4, |x, _| image::Rgba([x as u16 * 20000, 0, 0, 65535]));
    let mut o = Vec::new();
    image::codecs::png::PngEncoder::new(&mut o)
        .write_image(bytemuck_u16(img16.as_raw()), 4, 4, ExtendedColorType::Rgba16)
        .unwrap();
    let d = dec(&o, None);
    let Pixels::Rgba16(v) = &d.pixels else {
        panic!("expected 16-bit")
    };
    assert_eq!(v[4], 20000);
    assert!(d.color.assumed);

    let alpha = RgbaImage::from_fn(8, 1, |x, _| image::Rgba([255, 255, 255, (x * 36) as u8]));
    let d = dec(&encode(&alpha, Enc::Png, None, None), None);
    assert_eq!(rgba8(&d)[7 * 4 + 3], 252); // straight alpha preserved
    assert_eq!(rgba8(&d)[7 * 4], 255);
}

fn bytemuck_u16(v: &[u16]) -> &[u8] {
    // SAFETY: u16 → u8 view of the same memory, alignment of u8 is 1.
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2) }
}

#[test]
fn float_formats_shown_as_srgb() {
    // linear 0.2140 ≈ sRGB 0.5 (128)
    let f = image::Rgb32FImage::from_pixel(4, 4, image::Rgb([0.2140, 1.5, 0.0]));
    let mut hdr = Vec::new();
    image::codecs::hdr::HdrEncoder::new(&mut hdr)
        .encode(&f.pixels().copied().collect::<Vec<_>>(), 4, 4)
        .unwrap();
    let mut exr = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb32F(f)
        .write_to(&mut exr, image::ImageFormat::OpenExr)
        .unwrap();
    for (fmt, bytes) in [(Format::Hdr, hdr), (Format::Exr, exr.into_inner())] {
        let d = dec(&bytes, None);
        assert_eq!(d.format, fmt);
        assert!(d.color.hdr);
        let px = rgba8(&d);
        assert!(close(&px[0..4], &[127, 255, 0, 255], 2), "{fmt:?} {:?}", &px[0..4]);
    }
}

#[test]
fn dds_dxt1() {
    // 4×4 DXT1, one block, solid red (RGB565 0xF800).
    let mut b = b"DDS ".to_vec();
    let mut hdr = [0u32; 31];
    hdr[0] = 124; // size
    hdr[1] = 0x1 | 0x2 | 0x4 | 0x1000 | 0x80000; // CAPS|HEIGHT|WIDTH|PIXELFORMAT|LINEARSIZE
    hdr[2] = 4; // height
    hdr[3] = 4; // width
    hdr[4] = 8; // linear size
    hdr[18] = 32; // ddspf.size
    hdr[19] = 0x4; // DDPF_FOURCC
    hdr[20] = u32::from_le_bytes(*b"DXT1");
    hdr[26] = 0x1000; // DDSCAPS_TEXTURE
    for v in hdr {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&[0x00, 0xF8, 0x00, 0xF8, 0, 0, 0, 0]);
    let d = dec(&b, None);
    assert_eq!((d.size.w, d.size.h), (4, 4));
    assert!(close(&rgba8(&d)[0..4], &RED, 4));
}

#[test]
fn icns() {
    let data = quadrants(32, 32).into_raw();
    let mut fam = icns::IconFamily::new();
    fam.add_icon(&icns::Image::from_data(icns::PixelFormat::RGBA, 32, 32, data).unwrap())
        .unwrap();
    let mut out = Vec::new();
    fam.write(&mut out).unwrap();
    check_quadrants(&dec(&out, None), 32, 32, 0);
}

#[test]
fn svg() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10">
        <rect width="10" height="5" fill="#f00"/><rect x="10" width="10" height="5" fill="#0f0"/>
        <rect y="5" width="10" height="5" fill="#00f"/><rect x="10" y="5" width="10" height="5" fill="#fff"/></svg>"##;
    let d = decode::decode(svg, None, &DecodeOptions { svg_target: 64 }).unwrap();
    check_quadrants(&d, 64, 32, 0);
    println!("DECODE Svg: OK (svgz: SKIPPED here, no gzip encoder in deps; covered by fixtures.rs svgz_same_as_svg)");
}

#[test]
fn wic_formats() {
    use windows::Win32::Graphics::Imaging::{GUID_ContainerFormatHeif, GUID_ContainerFormatWmp};
    for (name, guid, tol) in [
        ("Heif", GUID_ContainerFormatHeif, 24),
        ("Jxr", GUID_ContainerFormatWmp, 8),
    ] {
        match wic_encode(&quadrants(64, 32), guid) {
            None => println!("DECODE {name}: SKIPPED: WIC codec not installed"),
            Some(b) => {
                check_quadrants(&dec(&b, None), 64, 32, tol);
                println!("DECODE {name}: OK");
            }
        }
    }
    println!(
        "DECODE Jxl: SKIPPED here: no JXL encoder for a synthetic fixture; covered by the jxl_* tests in fixtures.rs"
    );
    println!(
        "DECODE Avif/Raw: SKIPPED here: no encoder for a synthetic fixture; covered by fixtures.rs (photo.avif, photo.dng)"
    );
}

#[test]
fn jpeg_exif_orientation_all_8() {
    // (exif, original corner shown at displayed TL, at displayed TR)
    let table = [
        (1, RED, GREEN),
        (2, GREEN, RED),
        (3, WHITE, BLUE),
        (4, BLUE, WHITE),
        (5, RED, BLUE),
        (6, BLUE, RED),
        (7, WHITE, GREEN),
        (8, GREEN, WHITE),
    ];
    let img = quadrants(64, 32);
    for (v, tl, tr) in table {
        let d = dec(&encode(&img, Enc::Jpeg, None, Some(exif_orientation(v))), None);
        let (w, h) = if v >= 5 { (32, 64) } else { (64, 32) };
        assert_eq!((d.size.w, d.size.h), (w, h), "exif {v}");
        let px = rgba8(&d);
        let at = |x: u32, y: u32| px[((y * w + x) * 4) as usize..][..4].to_vec();
        assert!(close(&at(4, 4), &tl, 12), "exif {v} TL {:?}", at(4, 4));
        assert!(close(&at(w - 5, 4), &tr, 12), "exif {v} TR {:?}", at(w - 5, 4));
    }
}

#[test]
fn tiff_based_raw_goes_to_the_raw_codec() {
    // camera raws (DNG, NEF, CR2, …) start with a TIFF header: the extension decides, so the TIFF
    // decoder never shows their thumbnail and the file is never labelled TIFF.
    let tiff = encode(&quadrants(32, 16), Enc::Tiff, None, None);
    assert_eq!(
        decode::probe(&tiff, Some(Format::Raw)).map(|h| h.format),
        Some(Format::Raw)
    );
    assert_eq!(
        decode::probe(&tiff, Some(Format::Tiff)).map(|h| h.format),
        Some(Format::Tiff)
    );
    // content still wins for other formats: a ".dng" that is really a JPEG decodes as JPEG
    let jpg = encode(&quadrants(32, 16), Enc::Jpeg, None, None);
    assert_eq!(dec(&jpg, Some(Format::Raw)).format, Format::Jpeg);
    match decode::decode(&tiff, Some(Format::Raw), &DecodeOptions::default()) {
        Ok(d) => assert_eq!((d.format, d.size.w, d.size.h), (Format::Raw, 32, 16)),
        Err(DecodeError::CodecMissing { .. }) => println!("SKIPPED: RAW codec missing"),
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn errors_without_panic() {
    let png = encode(&quadrants(32, 16), Enc::Png, None, None);
    let jpg = encode(&quadrants(32, 16), Enc::Jpeg, None, None);
    assert!(matches!(
        decode::decode(b"not an image", None, &Default::default()),
        Err(DecodeError::Unsupported)
    ));
    assert!(decode::decode(&png[..png.len() / 2], None, &Default::default()).is_err());
    assert!(decode::decode(&jpg[..40], None, &Default::default()).is_err());
    assert!(decode::decode(b"<svg", Some(Format::Svg), &Default::default()).is_err());
    // BMP header claiming 100000×100000 → refused from the header, before allocating.
    let mut bmp = b"BM".to_vec();
    bmp.extend_from_slice(&[0; 8]);
    bmp.extend_from_slice(&54u32.to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&100_000i32.to_le_bytes());
    bmp.extend_from_slice(&100_000i32.to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&24u16.to_le_bytes());
    bmp.extend_from_slice(&[0; 24]);
    assert!(decode::decode(&bmp, None, &Default::default()).is_err());
    // Our own 2 GP check (here through an SVG rasterised at 100000 px per side).
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>"#;
    let e = decode::decode(svg, None, &DecodeOptions { svg_target: 100_000 }).unwrap_err();
    assert!(matches!(e, DecodeError::TooLarge { .. }), "{e:?}");
    assert_eq!(e.code(), 3);
}

#[test]
fn icc_embedded_is_converted_to_srgb() {
    let px = RgbaImage::from_pixel(2, 2, image::Rgba([200, 100, 100, 255]));
    let d = dec(&encode(&px, Enc::Png, Some(icc_p3()), None), None);
    assert!(!d.color.assumed);
    let view = rv_core::color::prepare_view(&d);
    // P3 is wider: the same values in sRGB must be more saturated (more red, less green/blue).
    assert!(view[0] > 200 && view[1] < 100 && view[2] < 100, "{:?}", &view[0..4]);
    // No profile → untouched.
    let d = dec(&encode(&px, Enc::Png, None, None), None);
    assert_eq!(&rv_core::color::prepare_view(&d)[0..4], &[200, 100, 100, 255]);
}
