//! Synthetic fixtures shared by the integration tests (generated in memory, nothing on disk).
#![allow(dead_code)]

use image::{ExtendedColorType, ImageEncoder, RgbaImage};

pub const RED: [u8; 4] = [255, 0, 0, 255];
pub const GREEN: [u8; 4] = [0, 255, 0, 255];
pub const BLUE: [u8; 4] = [0, 0, 255, 255];
pub const WHITE: [u8; 4] = [255, 255, 255, 255];

/// w×h image split in quadrants: TL red, TR green, BL blue, BR white.
pub fn quadrants(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        image::Rgba(match (x < w / 2, y < h / 2) {
            (true, true) => RED,
            (false, true) => GREEN,
            (true, false) => BLUE,
            (false, false) => WHITE,
        })
    })
}

pub fn close(a: &[u8], b: &[u8], tol: i32) -> bool {
    a.iter().zip(b).all(|(&x, &y)| (x as i32 - y as i32).abs() <= tol)
}

fn s15(v: f64) -> [u8; 4] {
    ((v * 65536.0).round() as i32).to_be_bytes()
}

/// `curv` tag with a pure gamma.
pub fn curv_gamma(gamma: f64) -> Vec<u8> {
    let mut c = b"curv\0\0\0\0".to_vec();
    c.extend_from_slice(&1u32.to_be_bytes());
    c.extend_from_slice(&((gamma * 256.0).round() as u16).to_be_bytes());
    c.extend_from_slice(&[0, 0]); // pad to 4
    c
}

/// `curv` tag sampling the sRGB transfer function (1024 entries).
pub fn curv_srgb() -> Vec<u8> {
    let mut c = b"curv\0\0\0\0".to_vec();
    c.extend_from_slice(&1024u32.to_be_bytes());
    for i in 0..1024 {
        let v = srgb_dec(i as f64 / 1023.0);
        c.extend_from_slice(&((v * 65535.0).round() as u16).to_be_bytes());
    }
    c
}

pub fn srgb_dec(e: f64) -> f64 {
    if e <= 0.04045 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }
}

pub fn srgb_enc(l: f64) -> f64 {
    if l <= 0.0031308 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

fn xyz_tag(v: [f64; 3]) -> Vec<u8> {
    let mut t = b"XYZ \0\0\0\0".to_vec();
    for c in v {
        t.extend_from_slice(&s15(c));
    }
    t
}

/// Assemble an ICC v2 display profile (D50 PCS) for data colour space `space`.
fn icc_build(space: &[u8; 4], mut tags: Vec<(&[u8; 4], Vec<u8>)>) -> Vec<u8> {
    let mut desc = b"desc\0\0\0\0".to_vec();
    desc.extend_from_slice(&5u32.to_be_bytes());
    desc.extend_from_slice(b"test\0");
    desc.extend_from_slice(&[0; 4 + 4 + 2 + 1 + 67]);
    while !desc.len().is_multiple_of(4) {
        desc.push(0);
    }
    let d50 = [0.9642, 1.0, 0.8249];
    tags.insert(0, (b"wtpt", xyz_tag(d50)));
    tags.insert(0, (b"desc", desc));
    let mut table = (tags.len() as u32).to_be_bytes().to_vec();
    let mut data = Vec::new();
    let base = 128 + 4 + 12 * tags.len();
    for (sig, t) in &tags {
        table.extend_from_slice(*sig);
        table.extend_from_slice(&((base + data.len()) as u32).to_be_bytes());
        table.extend_from_slice(&(t.len() as u32).to_be_bytes());
        data.extend_from_slice(t);
        while !data.len().is_multiple_of(4) {
            data.push(0);
        }
    }
    let total = 128 + table.len() + data.len();
    let mut h = vec![0u8; 128];
    h[0..4].copy_from_slice(&(total as u32).to_be_bytes());
    h[8..12].copy_from_slice(&0x0210_0000u32.to_be_bytes());
    h[12..16].copy_from_slice(b"mntr");
    h[16..20].copy_from_slice(space);
    h[20..24].copy_from_slice(b"XYZ ");
    h[36..40].copy_from_slice(b"acsp");
    for (i, c) in d50.iter().enumerate() {
        h[68 + i * 4..72 + i * 4].copy_from_slice(&s15(*c));
    }
    [h, table, data].concat()
}

/// RGB matrix/TRC profile with the given D50 colorants and curve.
pub fn icc_rgb(rxyz: [f64; 3], gxyz: [f64; 3], bxyz: [f64; 3], curv: Vec<u8>) -> Vec<u8> {
    icc_build(
        b"RGB ",
        vec![
            (b"rXYZ", xyz_tag(rxyz)),
            (b"gXYZ", xyz_tag(gxyz)),
            (b"bXYZ", xyz_tag(bxyz)),
            (b"rTRC", curv.clone()),
            (b"gTRC", curv.clone()),
            (b"bTRC", curv),
        ],
    )
}

/// Gray profile (kTRC).
pub fn icc_gray(curv: Vec<u8>) -> Vec<u8> {
    icc_build(b"GRAY", vec![(b"kTRC", curv)])
}

/// Minimal ICC v2 matrix/TRC display profile with a pure gamma curve.
pub fn icc_matrix(rxyz: [f64; 3], gxyz: [f64; 3], bxyz: [f64; 3], gamma: f64) -> Vec<u8> {
    icc_rgb(rxyz, gxyz, bxyz, curv_gamma(gamma))
}

/// Display P3 colorants (Bradford-adapted to D50).
pub const P3_D50: [[f64; 3]; 3] = [
    [0.5151, 0.2412, -0.0011],
    [0.2920, 0.6922, 0.0419],
    [0.1571, 0.0666, 0.7841],
];
/// Adobe RGB (1998) colorants (D50).
pub const ADOBE_D50: [[f64; 3]; 3] = [
    [0.6097, 0.3111, 0.0195],
    [0.2052, 0.6257, 0.0609],
    [0.1492, 0.0632, 0.7446],
];

/// Display P3 primaries, gamma 2.2 (close enough to the sRGB TRC for the capability tests).
pub fn icc_p3() -> Vec<u8> {
    icc_matrix(P3_D50[0], P3_D50[1], P3_D50[2], 2.2)
}

/// Raw TIFF-structured EXIF (no "Exif\0\0" prefix) with only the Orientation tag.
pub fn exif_orientation(v: u16) -> Vec<u8> {
    let mut e = b"MM\0*\0\0\0\x08".to_vec();
    e.extend_from_slice(&1u16.to_be_bytes()); // 1 entry
    e.extend_from_slice(&0x0112u16.to_be_bytes()); // Orientation
    e.extend_from_slice(&3u16.to_be_bytes()); // SHORT
    e.extend_from_slice(&1u32.to_be_bytes()); // count
    e.extend_from_slice(&v.to_be_bytes());
    e.extend_from_slice(&[0, 0]);
    e.extend_from_slice(&0u32.to_be_bytes()); // next IFD
    e
}

/// Encode with one of image's encoders, optionally with ICC and EXIF.
pub fn encode(img: &RgbaImage, enc: Enc, icc: Option<Vec<u8>>, exif: Option<Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::new();
    let (w, h) = img.dimensions();
    macro_rules! go {
        ($e:expr, $data:expr, $ty:expr) => {{
            let mut e = $e;
            if let Some(i) = icc {
                e.set_icc_profile(i).expect("icc unsupported");
            }
            if let Some(x) = exif {
                e.set_exif_metadata(x).expect("exif unsupported");
            }
            e.write_image($data, w, h, $ty).unwrap();
        }};
    }
    let rgb = image::DynamicImage::ImageRgba8(img.clone()).into_rgb8();
    match enc {
        Enc::Png => go!(
            image::codecs::png::PngEncoder::new(&mut out),
            img,
            ExtendedColorType::Rgba8
        ),
        Enc::Jpeg => go!(
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 95),
            &rgb,
            ExtendedColorType::Rgb8
        ),
        Enc::Webp => go!(
            image::codecs::webp::WebPEncoder::new_lossless(&mut out),
            img,
            ExtendedColorType::Rgba8
        ),
        Enc::Tiff => go!(
            image::codecs::tiff::TiffEncoder::new(std::io::Cursor::new(&mut out)),
            img,
            ExtendedColorType::Rgba8
        ),
    }
    out
}

#[derive(Clone, Copy, Debug)]
pub enum Enc {
    Png,
    Jpeg,
    Webp,
    Tiff,
}

/// Encode RGBA8 with a WIC encoder (container GUID). `None` if the encoder is not installed.
pub fn wic_encode(img: &RgbaImage, container: windows::core::GUID) -> Option<Vec<u8>> {
    use windows::Win32::Graphics::Imaging::*;
    use windows::Win32::System::Com::*;
    let path = std::env::temp_dir().join(format!("rview-wic-{}-{:?}.bin", std::process::id(), container));
    let (w, h) = img.dimensions();
    // SAFETY: test-only COM calls with valid buffers.
    let r = unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        (|| -> windows::core::Result<()> {
            let f: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let enc = f.CreateEncoder(&container, std::ptr::null())?;
            let stream = f.CreateStream()?;
            let wpath: Vec<u16> = path.as_os_str().encode_wide_nul();
            stream.InitializeFromFilename(
                windows::core::PCWSTR(wpath.as_ptr()),
                0x4000_0000, /* GENERIC_WRITE */
            )?;
            enc.Initialize(&stream, WICBitmapEncoderNoCache)?;
            let mut frame = None;
            let mut bag = None;
            enc.CreateNewFrame(&mut frame, &mut bag)?;
            let frame = frame.unwrap();
            frame.Initialize(bag.as_ref())?;
            frame.SetSize(w, h)?;
            let mut fmt = GUID_WICPixelFormat32bppRGBA;
            frame.SetPixelFormat(&mut fmt)?;
            if fmt != GUID_WICPixelFormat32bppRGBA {
                // Encoder wants another layout: go through a converter from an in-memory bitmap.
                let bmp = f.CreateBitmapFromMemory(w, h, &GUID_WICPixelFormat32bppRGBA, w * 4, img.as_raw())?;
                let conv = f.CreateFormatConverter()?;
                conv.Initialize(
                    &bmp,
                    &fmt,
                    WICBitmapDitherTypeNone,
                    None,
                    0.0,
                    WICBitmapPaletteTypeCustom,
                )?;
                frame.WriteSource(&conv, std::ptr::null())?;
            } else {
                frame.WritePixels(h, w * 4, img.as_raw())?;
            }
            frame.Commit()?;
            enc.Commit()?;
            Ok(())
        })()
    };
    let out = r.ok().and_then(|_| std::fs::read(&path).ok());
    let _ = std::fs::remove_file(&path);
    out
}

trait WideNul {
    fn encode_wide_nul(&self) -> Vec<u16>;
}
impl WideNul for std::ffi::OsStr {
    fn encode_wide_nul(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain([0]).collect()
    }
}
