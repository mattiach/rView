//! Capability tests: risky decoders and platform features, plus serde `rc`, zip CRC, ReplaceFileW, Advanced
//! Color probe. Run with `cargo test -p rv-core --test capabilities -- --nocapture --test-threads 1`
//! and read the `CAPABILITY` lines.

mod common;
use common::*;
use image::{AnimationDecoder, ImageDecoder, ImageFormat, ImageReader};
use rv_core::decode::{self, DecodeOptions, Pixels};
use rv_core::formats::Format;
use std::io::Cursor;

fn report(name: &str, status: &str) {
    println!("CAPABILITY {name}: {status}");
}

fn read_icc(bytes: &[u8], f: ImageFormat) -> Option<Vec<u8>> {
    let mut r = ImageReader::with_format(Cursor::new(bytes), f).into_decoder().unwrap();
    r.icc_profile().unwrap()
}

#[test]
fn icc_read_via_image() {
    let img = quadrants(16, 16);
    let icc = icc_p3();
    for (enc, f) in [
        (Enc::Png, ImageFormat::Png),
        (Enc::Jpeg, ImageFormat::Jpeg),
        (Enc::Webp, ImageFormat::WebP),
        (Enc::Tiff, ImageFormat::Tiff),
    ] {
        let bytes = encode(&img, enc, Some(icc.clone()), None);
        let via_image = read_icc(&bytes, f).as_deref() == Some(&icc[..]);
        let d = decode::decode(&bytes, None, &DecodeOptions::default()).unwrap();
        let via_rview = d.color.icc.as_deref() == Some(&icc[..]);
        let status = match (via_image, via_rview) {
            (true, _) => "OK",
            (false, true) => "FAIL via image → fallback active (tag read with `tiff`), rView OK",
            _ => "FAIL",
        };
        report(&format!("icc_read_{enc:?}"), status);
        assert!(via_rview, "{enc:?}");
    }
}

#[test]
fn qcms_accepts_test_profile() {
    let ok = qcms::Profile::new_from_slice(&icc_p3(), false).is_some();
    report("qcms_parses_synthetic_p3", if ok { "OK" } else { "FAIL" });
    assert!(ok);
}

#[test]
fn orientation_png_webp_via_image() {
    // EXIF 6 = rotate 90° CW: a 32×16 image becomes 16×32 with original BL at TL.
    let img = quadrants(32, 16);
    for enc in [Enc::Png, Enc::Webp] {
        let bytes = encode(&img, enc, None, Some(exif_orientation(6)));
        let d = decode::decode(&bytes, None, &DecodeOptions::default()).unwrap();
        let Pixels::Rgba8(px) = &d.pixels else { panic!() };
        let ok = (d.size.w, d.size.h) == (16, 32) && close(&px[0..4], &BLUE, 2);
        report(&format!("apply_orientation_{enc:?}"), if ok { "OK" } else { "FAIL" });
        assert!(ok, "{enc:?}");
    }
}

#[test]
fn gif_loop_count() {
    let mut out = Vec::new();
    {
        let mut e = image::codecs::gif::GifEncoder::new(&mut out);
        e.set_repeat(image::codecs::gif::Repeat::Finite(3)).unwrap();
        let f = image::Frame::new(quadrants(4, 4));
        e.encode_frames([f.clone(), f]).unwrap();
    }
    let d = image::codecs::gif::GifDecoder::new(Cursor::new(&out)).unwrap();
    let lc = d.loop_count();
    let ok = matches!(lc, image::metadata::LoopCount::Finite(n) if n.get() == 3);
    report(
        "gif_loop_count",
        if ok { "OK" } else { "FAIL (fallback: infinite loop)" },
    );
    assert!(ok);
}

#[test]
fn kamadak_exif_writer() {
    use exif::{Field, In, Tag, Value};
    let orient = Field {
        tag: Tag::Orientation,
        ifd_num: In::PRIMARY,
        value: Value::Short(vec![6]),
    };
    let artist = Field {
        tag: Tag::Artist,
        ifd_num: In::PRIMARY,
        value: Value::Ascii(vec![b"someone".to_vec()]),
    };
    let mut w = exif::experimental::Writer::new();
    w.push_field(&orient);
    w.push_field(&artist);
    let mut buf = Cursor::new(Vec::new());
    w.write(&mut buf, false).unwrap();
    let parsed = exif::Reader::new().read_raw(buf.into_inner()).unwrap();
    let o = parsed
        .get_field(Tag::Orientation, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0));
    let ok = o == Some(6) && parsed.get_field(Tag::Artist, In::PRIMARY).is_some();
    report(
        "kamadak_exif_writer",
        if ok { "OK" } else { "FAIL (fallback: strip-all only)" },
    );
    assert!(ok);
}

#[test]
fn img_parts_insert_icc_exif() {
    use img_parts::{Bytes, DynImage, ImageEXIF, ImageICC};
    let img = quadrants(32, 16);
    let icc = icc_p3();
    for (enc, f) in [
        (Enc::Jpeg, ImageFormat::Jpeg),
        (Enc::Png, ImageFormat::Png),
        (Enc::Webp, ImageFormat::WebP),
    ] {
        let plain = encode(&img, enc, None, None);
        let mut parts = DynImage::from_bytes(Bytes::from(plain)).unwrap().unwrap();
        parts.set_icc_profile(Some(Bytes::from(icc.clone())));
        parts.set_exif(Some(Bytes::from(exif_orientation(6))));
        let mut out = Vec::new();
        parts.encoder().write_to(&mut out).unwrap();
        let icc_ok = read_icc(&out, f).as_deref() == Some(&icc[..]);
        let d = decode::decode(&out, None, &DecodeOptions::default()).unwrap();
        let ok = icc_ok && (d.size.w, d.size.h) == (16, 32);
        report(&format!("img_parts_icc_exif_{enc:?}"), if ok { "OK" } else { "FAIL" });
        assert!(ok, "{enc:?} icc_ok={icc_ok} size={:?}", d.size);
    }
}

#[test]
fn image_ico_decoder_reads_cur() {
    let mut ico = Vec::new();
    image::codecs::ico::IcoEncoder::new(&mut ico)
        .write_image(quadrants(16, 16).as_raw(), 16, 16, image::ExtendedColorType::Rgba8)
        .unwrap();
    ico[2] = 2; // ICONDIR type 2 = cursor
    let rust = image::load_from_memory_with_format(&ico, ImageFormat::Ico).is_ok();
    let ours = decode::decode(&ico, Some(Format::Cur), &DecodeOptions::default()).is_ok();
    report("cur_via_image_ico", if rust { "OK" } else { "FAIL (fallback: WIC)" });
    report("cur_decoded_by_rview", if ours { "OK" } else { "FAIL" });
    assert!(ours);
}
use image::ImageEncoder;

#[test]
fn jxl_exif_box() {
    report(
        "jxl_oxide_exif_box",
        "SKIPPED here: no JXL encoder for a synthetic fixture; covered by fixtures.rs jxl_exif_box_read_not_applied",
    );
}

#[test]
fn jpeg_cmyk() {
    report(
        "jpeg_cmyk_without_icc",
        "SKIPPED here: no CMYK JPEG encoder for a synthetic fixture; covered by fixtures.rs cmyk_jpeg_matches_libjpeg",
    );
}

#[test]
fn wic_scaled_jpeg() {
    let bytes = encode(&quadrants(2048, 1024), Enc::Jpeg, None, None);
    let t = std::time::Instant::now();
    let d = decode::wic::decode_jpeg_scaled(&bytes, 256).unwrap();
    let dt = t.elapsed();
    let Pixels::Rgba8(px) = &d.pixels else { panic!() };
    let w = d.size.w as usize;
    let ok =
        (d.size.w, d.size.h) == (256, 128) && close(&px[0..4], &RED, 8) && close(&px[(w - 1) * 4..w * 4], &GREEN, 8);
    report(
        "wic_scaled_jpeg_decode",
        &format!(
            "{} (2048x1024 → {}x{} in {dt:?})",
            if ok { "OK" } else { "FAIL" },
            d.size.w,
            d.size.h
        ),
    );
    assert!(ok);
}

#[test]
fn wic_heif_avif() {
    use windows::Win32::Graphics::Imaging::{GUID_ContainerFormatHeif, GUID_ContainerFormatWmp};
    for (name, guid) in [("heif", GUID_ContainerFormatHeif), ("jxr", GUID_ContainerFormatWmp)] {
        match wic_encode(&quadrants(64, 32), guid) {
            None => report(&format!("wic_{name}"), "SKIPPED: WIC encoder/codec not installed"),
            Some(bytes) => {
                let r = decode::decode(&bytes, None, &DecodeOptions::default());
                let ok = r.as_ref().is_ok_and(|d| (d.size.w, d.size.h) == (64, 32));
                report(&format!("wic_{name}"), if ok { "OK" } else { "FAIL" });
                assert!(ok, "{name}: {:?}", r.err());
            }
        }
    }
    report(
        "wic_avif",
        "SKIPPED here: Windows has no AVIF encoder for a synthetic fixture; covered by fixtures.rs avif_decodes_through_wic",
    );
}

#[test]
fn serde_rc() {
    use std::sync::Arc;
    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct S {
        a: Arc<String>,
        b: Arc<[u32]>,
    }
    let s = S {
        a: Arc::new("x".into()),
        b: Arc::from(vec![1, 2, 3]),
    };
    let j = serde_json::to_string(&s).unwrap();
    let ok = serde_json::from_str::<S>(&j).unwrap() == s;
    report("serde_rc_arc", if ok { "OK" } else { "FAIL" });
    assert!(ok);
}

#[test]
fn zip_detects_bad_crc() {
    use std::io::{Read, Write};
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        z.start_file("original.png", o).unwrap();
        z.write_all(b"HELLO-WORLD-PAYLOAD").unwrap();
        z.finish().unwrap();
    }
    let mut bytes = buf.into_inner();
    let pos = bytes.windows(5).position(|w| w == b"HELLO").unwrap();
    bytes[pos] = b'J'; // corrupt one data byte, CRC now wrong
    let mut ar = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();
    let res = ar.by_index(0).unwrap().read_to_end(&mut out);
    let ok = res.is_err();
    report(
        "zip_crc32_checked",
        if ok { "OK" } else { "FAIL (fallback: crc32fast)" },
    );
    assert!(ok);
}

#[test]
fn replace_file_and_trash_backup() {
    use std::os::windows::fs::MetadataExt;
    use windows::Win32::Storage::FileSystem::*;
    use windows::core::HSTRING;
    let dir = std::env::temp_dir().join(format!("rview-replace-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (dst, tmp, bak) = (
        dir.join("photo.jpg"),
        dir.join(".photo.jpg.qvtmp-1"),
        dir.join(".photo.jpg.qvbak-1"),
    );
    std::fs::write(&dst, b"old").unwrap();
    // SAFETY: valid NUL-terminated paths.
    unsafe {
        SetFileAttributesW(&HSTRING::from(dst.as_os_str()), FILE_ATTRIBUTE_NOT_CONTENT_INDEXED).unwrap();
    }
    let created = std::fs::metadata(&dst).unwrap().creation_time();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&tmp, b"new").unwrap();
    // SAFETY: valid paths; no exclude/reserved pointers.
    unsafe {
        ReplaceFileW(
            &HSTRING::from(dst.as_os_str()),
            &HSTRING::from(tmp.as_os_str()),
            &HSTRING::from(bak.as_os_str()),
            REPLACEFILE_IGNORE_MERGE_ERRORS,
            None,
            None,
        )
        .unwrap();
    }
    let md = std::fs::metadata(&dst).unwrap();
    let ok_replace = std::fs::read(&dst).unwrap() == b"new"
        && std::fs::read(&bak).unwrap() == b"old"
        && !tmp.exists()
        && md.file_attributes() & FILE_ATTRIBUTE_NOT_CONTENT_INDEXED.0 != 0
        && md.creation_time() == created;
    report(
        "replace_file_w_preserves_attrs_and_ctime",
        if ok_replace { "OK" } else { "FAIL" },
    );

    let trashed = trash::delete(&bak).is_ok() && !bak.exists();
    // the bin records the folder's long form; the temp folder may be given with 8.3 names
    // (GitHub runners: C:\Users\RUNNER~1\…)
    let long_dir = rv_core::folder::long_path(&dir);
    let restored = trashed
        && trash::os_limited::list().is_ok_and(|items| {
            let mine: Vec<_> = items
                .into_iter()
                .filter(|i| rv_core::folder::same_path_ordinal(&i.original_parent, &long_dir))
                .collect();
            !mine.is_empty() && trash::os_limited::restore_all(mine).is_ok()
        })
        && bak.exists();
    report(
        "backup_to_recycle_bin_and_restore",
        if restored { "OK" } else { "FAIL" },
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(ok_replace && restored);
}

/// AC-1 probe: what DisplayConfig reports for each active display. The hypothesis itself needs a
/// manual check (ACM on/off, HDR on).
#[test]
fn advanced_color_probe() {
    use windows::Win32::Devices::Display::*;
    #[repr(C)]
    struct AdvancedColorInfo2 {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
        value: u32,
        color_encoding: u32,
        bits_per_color_channel: u32,
        active_color_mode: u32,
    }
    // SAFETY: buffers sized by GetDisplayConfigBufferSizes; packets carry their own size.
    unsafe {
        let (mut np, mut nm) = (0, 0);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm).is_err() {
            report("advanced_color_probe", "SKIPPED: no display configuration (headless?)");
            return;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        let _ = QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut np,
            paths.as_mut_ptr(),
            &mut nm,
            modes.as_mut_ptr(),
            None,
        );
        for p in &paths[..np as usize] {
            let mut i1 = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO::default();
            i1.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO;
            i1.header.size = size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32;
            i1.header.adapterId = p.targetInfo.adapterId;
            i1.header.id = p.targetInfo.id;
            let r1 = DisplayConfigGetDeviceInfo(&mut i1.header);
            let mut i2 = AdvancedColorInfo2 {
                header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_TYPE(15),
                    size: size_of::<AdvancedColorInfo2>() as u32,
                    adapterId: p.targetInfo.adapterId,
                    id: p.targetInfo.id,
                },
                value: 0,
                color_encoding: 0,
                bits_per_color_channel: 0,
                active_color_mode: 0,
            };
            let r2 = DisplayConfigGetDeviceInfo(&mut i2.header);
            let b1 = i1.Anonymous.Anonymous._bitfield;
            report(
                &format!("advanced_color_probe target {}", p.targetInfo.id),
                &format!(
                    "INFO rc={r1} supported={} enabled={} wideEnforced={} forceDisabled={} bpc={}; \
                     INFO_2 rc={r2} value={:#x} activeColorMode={} (0 SDR, 1 WCG, 2 HDR)",
                    b1 & 1,
                    (b1 >> 1) & 1,
                    (b1 >> 2) & 1,
                    (b1 >> 3) & 1,
                    i1.bitsPerColorChannel,
                    i2.value,
                    i2.active_color_mode
                ),
            );
        }
    }
}
