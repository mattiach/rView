//! Real-file checks. Origin and licence of every file: tests/fixtures/README.md.
//! References are decoded by an independent decoder (libjxl conformance refs, libavif, libjpeg,
//! libheif), so each test compares two decoders on the same bitstream. A missing file prints
//! `SKIPPED: fixtures/<name> not present` (nothing passes silently).

use rv_core::anim::{Player, is_animated, source_frame};
use rv_core::color::ViewSpace;
use rv_core::decode::{self, DecodeError, DecodeOptions, DecodedImage, Pixels};
use rv_core::formats::Format;
use rv_core::loader::ImageInfo;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn path(name: &str) -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn fixture(name: &str) -> Option<Vec<u8>> {
    match std::fs::read(path(name)) {
        Ok(b) => Some(b),
        Err(_) => {
            println!("SKIPPED: fixtures/{name} not present");
            None
        }
    }
}

fn decode_ext(bytes: &[u8], ext: &str) -> Result<DecodedImage, DecodeError> {
    decode::decode(bytes, Format::from_ext(ext), &DecodeOptions::default())
}

fn rgba8(d: &DecodedImage) -> Vec<u8> {
    match &d.pixels {
        Pixels::Rgba8(v) => v.clone(),
        Pixels::Rgba16(v) => v.iter().map(|&s| ((s as u32 * 255 + 32767) / 65535) as u8).collect(),
    }
}

fn reference(name: &str) -> image::RgbaImage {
    image::open(path(name))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .into_rgba8()
}

/// (max, mean) absolute difference over RGB.
fn diff(a: &[u8], b: &[u8]) -> (i32, f64) {
    assert_eq!(a.len(), b.len());
    let (mut max, mut sum, mut n) = (0, 0u64, 0u64);
    for (p, q) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        for c in 0..3 {
            let d = (p[c] as i32 - q[c] as i32).abs();
            max = max.max(d);
            sum += d as u64;
            n += 1;
        }
    }
    (max, sum as f64 / n as f64)
}

fn compare(name: &str, ours: &DecodedImage, refname: &str, max_ok: i32, mean_ok: f64) {
    let r = reference(refname);
    assert_eq!(
        (ours.size.w, ours.size.h),
        r.dimensions(),
        "{name}: size (orientation) vs reference"
    );
    let (max, mean) = diff(&rgba8(ours), r.as_raw());
    println!(
        "FIXTURE {name}: {}x{} vs {refname}: max diff {max}, mean {mean:.3}",
        ours.size.w, ours.size.h
    );
    assert!(
        max <= max_ok && mean <= mean_ok,
        "{name}: max {max} (≤ {max_ok}), mean {mean:.3} (≤ {mean_ok})"
    );
}

fn frames_of(bytes: Vec<u8>, format: Format) -> usize {
    let mut p = Player::spawn(bytes.into(), format, ViewSpace::Srgb, Instant::now(), Arc::new(|| {}));
    let t = Instant::now();
    loop {
        if let Some(n) = p.frame_count() {
            return n;
        }
        assert!(p.error.is_none(), "{:?}", p.error);
        assert!(t.elapsed() < Duration::from_secs(120), "frames never complete");
        std::thread::sleep(Duration::from_millis(2));
    }
}

// ---------- JPEG XL: libjxl conformance (reference decoded by libjxl) ----------

#[test]
fn jxl_still_grayscale_matches_conformance_reference() {
    let Some(b) = fixture("still.jxl") else { return };
    let d = decode_ext(&b, "jxl").expect("decodes");
    assert!(d.color.icc.is_some());
    compare("still.jxl", &d, "still.jxl.ref.png", 1, 0.5);
    println!("FIXTURE still.jxl profile: {}", ImageInfo::of(&d).profile);
}

/// ISOBMFF box: size, type, payload.
fn jxl_box(ty: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut b = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    b.extend_from_slice(ty);
    b.extend_from_slice(payload);
    b
}

/// The conformance codestream in a JPEG XL container with an `Exif` box (Orientation = 6, Make):
/// the EXIF is read as metadata, its orientation is not applied (the codestream's is).
#[test]
fn jxl_exif_box_read_not_applied() {
    let Some(code) = fixture("still.jxl") else { return };
    assert_eq!(&code[..2], [0xFF, 0x0A], "bare codestream");
    // little-endian TIFF: IFD0 with Orientation (0x0112) = 6 and Make (0x010F) = "rView"
    let mut tiff = b"II*\0\x08\0\0\0\x02\0".to_vec();
    tiff.extend_from_slice(&[0x0F, 0x01, 2, 0, 6, 0, 0, 0, 38, 0, 0, 0]);
    tiff.extend_from_slice(&[0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0]);
    tiff.extend_from_slice(&[0, 0, 0, 0]);
    tiff.extend_from_slice(b"rView\0");
    let mut exif_payload = 0u32.to_be_bytes().to_vec(); // TIFF header offset
    exif_payload.extend_from_slice(&tiff);
    let mut file = jxl_box(b"JXL ", &[0x0D, 0x0A, 0x87, 0x0A]);
    file.extend(jxl_box(b"ftyp", b"jxl \0\0\0\0jxl "));
    file.extend(jxl_box(b"Exif", &exif_payload));
    file.extend(jxl_box(b"jxlc", &code));

    let plain = decode_ext(&code, "jxl").expect("codestream decodes");
    assert!(plain.meta.exif_read && plain.meta.exif.is_none());
    let d = decode_ext(&file, "jxl").expect("container decodes");
    assert_eq!(
        d.meta.exif.as_deref(),
        Some(&tiff[..]),
        "EXIF block = the box's TIFF data"
    );
    let exif = exif::Reader::new().read_raw(tiff.clone()).expect("valid TIFF");
    assert!(exif.get_field(exif::Tag::Make, exif::In::PRIMARY).is_some());
    assert_eq!(
        (d.size.w, d.size.h),
        (plain.size.w, plain.size.h),
        "Orientation 6 not applied"
    );
    assert_eq!(rgba8(&d), rgba8(&plain));
    println!("FIXTURE still.jxl in a container with Exif: read, orientation not re-applied");
}

#[test]
fn jxl_orientation_applied_once_matches_reference() {
    let Some(b) = fixture("oriented.jxl") else { return };
    let d = decode_ext(&b, "jxl").expect("decodes");
    compare("oriented.jxl", &d, "oriented.jxl.ref.png", 2, 0.5);
}

#[test]
fn jxl_animation_frames_and_durations() {
    let Some(b) = fixture("anim.jxl") else { return };
    assert!(is_animated(&b, Format::Jxl));
    let spec: serde_json::Value = serde_json::from_slice(&std::fs::read(path("anim.jxl.test.json")).unwrap()).unwrap();
    let expected: Vec<f64> = spec["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["duration"].as_f64().unwrap())
        .collect();
    // durations of every frame, as the player sees them
    for (i, want) in expected.iter().enumerate() {
        let (f, _) = source_frame(&b, Format::Jxl, i).unwrap_or_else(|e| panic!("frame {i}: {e}"));
        let ms = (want * 1000.0).round() as u64;
        let want = if ms <= 10 { 100 } else { ms }; // browser clamp, rv_core::anim
        assert_eq!(f.delay, Duration::from_millis(want), "frame {i} duration");
    }
    // frame pixels vs the reference GIF of the conformance suite (palette-quantised: loose)
    let gif =
        image::codecs::gif::GifDecoder::new(std::io::Cursor::new(std::fs::read(path("anim.jxl.ref.gif")).unwrap()))
            .unwrap();
    use image::AnimationDecoder;
    let refs: Vec<image::RgbaImage> = gif.into_frames().map(|f| f.unwrap().into_buffer()).collect();
    assert_eq!(refs.len(), expected.len(), "reference frame count");
    let mut worst = (0, 0.0f64);
    for (i, r) in refs.iter().enumerate().step_by(5) {
        let (f, _) = source_frame(&b, Format::Jxl, i).unwrap();
        assert_eq!((f.w, f.h), r.dimensions());
        let (max, mean) = diff(&f.rgba, r.as_raw());
        worst = (worst.0.max(max), worst.1.max(mean));
    }
    println!(
        "FIXTURE anim.jxl: {} frames, durations as test.json; vs ref.gif (every 5th frame): max {}, mean {:.3}",
        expected.len(),
        worst.0,
        worst.1
    );
    assert!(worst.1 <= 2.0, "frames differ from the reference: {worst:?}");
    assert_eq!(frames_of(b, Format::Jxl), expected.len());
}

#[test]
fn jxl_preview_only_for_progressive_files() {
    // non-progressive (1 pass) > 24 MP: no preview, the loader shows the loading indicator instead
    if let Some(b) = fixture("large.jxl") {
        let full = decode_ext(&b, "jxl").expect("decodes");
        assert!(full.size.w as u64 * full.size.h as u64 > 24_000_000, "must be > 24 MP");
        assert!(
            decode::decode_preview(&b, Format::Jxl, 2048).is_none(),
            "1-pass file: no preview"
        );
        println!(
            "FIXTURE large.jxl: {}x{}, single pass → no preview",
            full.size.w, full.size.h
        );
    }
    // progressive (libjxl conformance): early render with the full geometry, close to the final image
    if let Some(b) = fixture("progressive.jxl") {
        let t = Instant::now();
        let full = decode_ext(&b, "jxl").expect("decodes");
        let t_full = t.elapsed();
        let t = Instant::now();
        let p = decode::decode_preview(&b, Format::Jxl, 2048).expect("progressive file: preview");
        let t_preview = t.elapsed();
        assert_eq!(
            (p.size.w, p.size.h),
            (full.size.w, full.size.h),
            "preview has the full geometry"
        );
        let (_, mean) = diff(&rgba8(&p), &rgba8(&full));
        println!(
            "FIXTURE progressive.jxl: preview {t_preview:?} vs full {t_full:?} ({} build), mean difference {mean:.1}",
            if cfg!(debug_assertions) { "debug" } else { "release" }
        );
        assert!(mean < 20.0, "preview too far from the final image: {mean:.1}");
    }
}

// ---------- WIC formats (Windows codecs) ----------

fn wic(name: &str, ext: &str) -> Option<DecodedImage> {
    let b = fixture(name)?;
    match decode_ext(&b, ext) {
        Ok(d) => Some(d),
        Err(DecodeError::CodecMissing { codec, .. }) => {
            println!("SKIPPED: {name}: Windows codec missing: {codec}");
            None
        }
        Err(e) => panic!("{name}: {e}"),
    }
}

#[test]
fn avif_decodes_through_wic() {
    let Some(d) = wic("photo.avif", "avif") else { return };
    assert_eq!(d.format, Format::Avif);
    let r = reference("photo.avif.ref.png");
    assert_eq!((d.size.w, d.size.h), r.dimensions());
    let (max, mean) = diff(&rgba8(&d), r.as_raw());
    println!(
        "FIXTURE photo.avif: {}x{} vs libavif: max {max}, mean {mean:.2} (BT.601 file, see the ignored test)",
        d.size.w, d.size.h
    );
    assert!(mean <= 6.0, "far from libavif: mean {mean:.2}");
}

/// KNOWN ISSUE: like HEIF, the Windows AV1/AVIF decoder converts BT.601 (nclx matrix 6,
/// libavif's default) with BT.709: on flat quadrants red (255,0,1) becomes (255,26,0).
#[test]
#[ignore = "known WIC issue: AVIF BT.601 matrix decoded as BT.709"]
fn avif_colours_match_libavif() {
    let Some(d) = wic("photo.avif", "avif") else { return };
    compare("photo.avif", &d, "photo.avif.ref.png", 40, 1.5);
}

#[test]
fn dng_from_a_camera() {
    let Some(d) = wic("photo.dng", "dng") else { return };
    assert_eq!(
        d.format,
        Format::Raw,
        "a DNG must reach the RAW codec, not the TIFF decoder"
    );
    let px = rgba8(&d);
    let n = (px.len() / 4) as f64;
    let mean = px
        .chunks_exact(4)
        .map(|p| (p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.0)
        .sum::<f64>()
        / n;
    let var = px
        .chunks_exact(4)
        .map(|p| ((p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.0 - mean).powi(2))
        .sum::<f64>()
        / n;
    println!(
        "FIXTURE photo.dng: {}x{} {:?}, mean luma {mean:.1}, std {:.1}",
        d.size.w,
        d.size.h,
        d.format,
        var.sqrt()
    );
    assert!(
        d.size.w > 1000 && d.size.h > 700,
        "the raw image, not the embedded thumbnail"
    );
    assert!(
        mean > 10.0 && mean < 245.0 && var.sqrt() > 10.0,
        "a real image, not a flat frame"
    );
}

/// Mean colour of the inside of each quadrant (8 px away from the colour edges).
fn quadrant_means(px: &[u8], w: u32, h: u32) -> [[i32; 3]; 4] {
    let mut out = [[0; 3]; 4];
    for (q, (x0, y0)) in [(0, 0), (w / 2, 0), (0, h / 2), (w / 2, h / 2)].into_iter().enumerate() {
        let (mut sum, mut n) = ([0i64; 3], 0i64);
        for y in y0 + 8..y0 + h / 2 - 8 {
            for x in x0 + 8..x0 + w / 2 - 8 {
                let i = ((y * w + x) * 4) as usize;
                for c in 0..3 {
                    sum[c] += px[i + c] as i64;
                }
                n += 1;
            }
        }
        out[q] = sum.map(|v| (v / n) as i32);
    }
    out
}

/// Index of the strongest channel of each quadrant: R, G, B or "white" (3). Independent of the
/// colour matrix, so it checks the orientation alone.
fn quadrant_hues(m: [[i32; 3]; 4]) -> [usize; 4] {
    m.map(|c| {
        if c.iter().all(|&v| v > 200) {
            3
        } else {
            (0..3).max_by_key(|&i| c[i]).unwrap()
        }
    })
}

#[test]
fn heic_all_eight_orientations_match_libheif() {
    for o in 1..=8 {
        let name = format!("orient{o}.heic");
        let Some(d) = wic(&name, "heic") else { return };
        let r = reference(&format!("{name}.ref.png"));
        assert_eq!(
            (d.size.w, d.size.h),
            r.dimensions(),
            "{name}: displayed size (rotation)"
        );
        let (a, b) = (
            quadrant_means(&rgba8(&d), d.size.w, d.size.h),
            quadrant_means(r.as_raw(), d.size.w, d.size.h),
        );
        println!(
            "FIXTURE {name}: {}x{}, quadrants {:?} (libheif {:?})",
            d.size.w,
            d.size.h,
            quadrant_hues(a),
            quadrant_hues(b)
        );
        assert_eq!(
            quadrant_hues(a),
            quadrant_hues(b),
            "{name}: orientation differs from libheif"
        );
    }
}

fn heic_colours(name: &str) -> Option<i32> {
    let d = wic(name, "heic")?;
    let r = reference(&format!("{name}.ref.png"));
    let (a, b) = (
        quadrant_means(&rgba8(&d), d.size.w, d.size.h),
        quadrant_means(r.as_raw(), d.size.w, d.size.h),
    );
    let worst = a
        .iter()
        .zip(&b)
        .flat_map(|(p, q)| p.iter().zip(q).map(|(x, y)| (x - y).abs()))
        .max()
        .unwrap();
    println!("FIXTURE {name}: quadrant colours vs libheif within {worst}: ours {a:?}, libheif {b:?}");
    Some(worst)
}

#[test]
fn heic_bt709_colours_match_libheif() {
    if let Some(worst) = heic_colours("heic_bt709.heic") {
        assert!(worst <= 3, "BT.709 HEIC colours differ by {worst}");
    }
}

/// KNOWN ISSUE: the Windows HEIF decoder ignores nclx matrix_coefficients = 6 (BT.601) and
/// converts with BT.709: red (254,0,0) comes out as (255,25,0). WIC offers no planar YCbCr output
/// for HEIF, so rView cannot redo the conversion. Run with --ignored.
#[test]
#[ignore = "known WIC issue: HEIF BT.601 matrix decoded as BT.709"]
fn heic_bt601_colours_match_libheif() {
    if let Some(worst) = heic_colours("heic_bt601.heic") {
        assert!(worst <= 3, "BT.601 HEIC colours differ by {worst}");
    }
}

// ---------- Rust decoders on files from other encoders ----------

#[test]
fn cmyk_jpeg_matches_libjpeg() {
    let Some(b) = fixture("cmyk.jpg") else { return };
    let d = decode_ext(&b, "jpg").expect("CMYK JPEG decodes");
    compare("cmyk.jpg", &d, "cmyk.jpg.ref.png", 3, 0.5);
}

fn webp_frames(exact: bool) {
    let Some(b) = fixture("anim.webp") else { return };
    assert!(is_animated(&b, Format::Webp));
    let colours: [[u8; 3]; 3] = [[255, 0, 0], [0, 255, 0], [0, 0, 255]];
    for (i, (ms, c)) in [40u64, 60, 80].into_iter().zip(colours).enumerate() {
        let (f, _) = source_frame(&b, Format::Webp, i).unwrap();
        assert_eq!((f.w, f.h, f.delay), (64, 48, Duration::from_millis(ms)), "frame {i}");
        let worst = f.rgba[0..3]
            .iter()
            .zip(c)
            .map(|(&a, b)| (a as i32 - b as i32).abs())
            .max()
            .unwrap();
        // frame 0 is never composited; later frames go through image-webp's blending
        let tol = if exact || i == 0 { 0 } else { 1 };
        assert!(
            worst <= tol,
            "frame {i}: {:?} vs {c:?} (libwebp gives exact values)",
            &f.rgba[0..3]
        );
    }
    assert_eq!(frames_of(b, Format::Webp), 3);
    println!(
        "FIXTURE anim.webp: 3 frames, 40/60/80 ms, colours {}",
        if exact {
            "exact"
        } else {
            "within 1 (see the ignored test)"
        }
    );
}

#[test]
fn animated_webp_frames_durations_colours() {
    webp_frames(false);
}

/// KNOWN ISSUE, upstream: image-webp 0.2.4 composites animation frames with a rounding
/// error, (0,255,0) → (0,254,0); libwebp (Pillow) decodes the same file exactly. Not visible.
#[test]
#[ignore = "upstream image-webp 0.2.4: composited frames off by 1"]
fn animated_webp_exact() {
    webp_frames(true);
}

#[test]
fn svgz_same_as_svg() {
    let Some(b) = fixture("quadrants.svgz") else { return };
    let d = decode::decode(&b, Some(Format::Svg), &DecodeOptions { svg_target: 64 }).expect("svgz decodes");
    let px = rgba8(&d);
    let at = |x: u32, y: u32| px[((y * d.size.w + x) * 4) as usize..][..3].to_vec();
    assert_eq!((d.size.w, d.size.h), (64, 32));
    assert_eq!(
        (at(8, 8), at(56, 8), at(8, 24), at(56, 24)),
        (vec![255, 0, 0], vec![0, 255, 0], vec![0, 0, 255], vec![255, 255, 255])
    );
    println!("FIXTURE quadrants.svgz: 64x32, quadrants exact");
}
