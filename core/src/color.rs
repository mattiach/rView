//! Colour pipeline for the view: source profile → view space with qcms
//! (RGB and gray profiles), then premultiplication in linear light.
//!
//! Advanced Color: the assumptions AC-1…AC-3 (`docs/capabilities.md`) are NOT confirmed, so only
//! the provisional behaviour exists: Auto = sRGB, never the monitor profile. Nothing here depends on
//! the monitor, which is why no re-check on monitor change is needed until AC-1 is confirmed.

use crate::decode::{DecodedImage, Pixels};
use crate::settings::ColorSpaceConversion;
use std::sync::OnceLock;

pub fn srgb_encode(l: f32) -> f32 {
    if l <= 0.003_130_8 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

pub fn srgb_decode(e: f32) -> f32 {
    if e <= 0.040_45 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }
}

/// Target the view buffers are converted to.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum ViewSpace {
    #[default]
    Srgb,
    /// `colorspaceconversion = 0`: raw values, no conversion.
    Raw,
    DisplayP3,
}

impl ViewSpace {
    /// Part of the view-cache key (changing space invalidates only other-space entries).
    pub fn id(self) -> u32 {
        self as u32
    }
}

/// Provisional for Auto (Advanced Color unknown → sRGB).
pub fn view_space(setting: ColorSpaceConversion) -> ViewSpace {
    match setting {
        ColorSpaceConversion::Disabled => ViewSpace::Raw,
        // Monitor-profile conversion only once AC-1 is confirmed.
        ColorSpaceConversion::Auto | ColorSpaceConversion::Srgb => ViewSpace::Srgb,
        ColorSpaceConversion::DisplayP3 => ViewSpace::DisplayP3,
    }
}

/// ICC data colour space signature (bytes 16..20 of the header).
fn icc_space(icc: &[u8]) -> Option<&[u8]> {
    icc.get(16..20)
}

pub fn icc_is_rgb(icc: &[u8]) -> bool {
    icc_space(icc) == Some(b"RGB ")
}

/// Profile description for Info: `desc` (ICC v2) or `mluc` (v4) tag.
pub fn icc_description(icc: &[u8]) -> Option<String> {
    let be32 = |o: usize| {
        icc.get(o..o + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    let count = be32(128)?;
    let (off, len) = (0..count.min(1000)).find_map(|i| {
        let e = 132 + i * 12;
        (icc.get(e..e + 4)? == b"desc").then(|| Some((be32(e + 4)?, be32(e + 8)?)))?
    })?;
    let tag = icc.get(off..off.checked_add(len)?)?;
    let text = match tag.get(0..4)? {
        b"desc" => {
            let n = u32::from_be_bytes(tag.get(8..12)?.try_into().ok()?) as usize;
            String::from_utf8_lossy(tag.get(12..12 + n)?)
                .trim_end_matches('\0')
                .to_string()
        }
        b"mluc" => {
            let (n, o) = (
                u32::from_be_bytes(tag.get(20..24)?.try_into().ok()?) as usize,
                u32::from_be_bytes(tag.get(24..28)?.try_into().ok()?) as usize,
            );
            let units: Vec<u16> = tag
                .get(o..o + n)?
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units).trim_end_matches('\0').to_string()
        }
        _ => return None,
    };
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn output_profile(space: ViewSpace) -> Box<qcms::Profile> {
    let mut p = match space {
        ViewSpace::DisplayP3 => {
            let xy = |x, y| qcms::CIE_xyY { x, y, Y: 1.0 };
            let table: Vec<u16> = (0..1024)
                .map(|i| (srgb_decode(i as f32 / 1023.0) * 65535.0 + 0.5) as u16)
                .collect();
            qcms::Profile::new_rgb_with_table(
                xy(0.3127, 0.3290),
                qcms::CIE_xyYTRIPLE {
                    red: xy(0.680, 0.320),
                    green: xy(0.265, 0.690),
                    blue: xy(0.150, 0.060),
                },
                &table,
            )
            .expect("valid Display P3 primaries")
        }
        _ => qcms::Profile::new_sRGB(),
    };
    p.precache_output_transform();
    p
}

enum Kind {
    Identity,
    Rgb(qcms::Transform),
    /// Gray profile: the gray channel (R of the expanded RGBA) + alpha go through GrayA8 → RGBA8.
    Gray(qcms::Transform),
}

/// Source profile → view space, built once per image (and reused for every animation frame).
pub struct ViewTransform(Kind);

impl ViewTransform {
    pub fn new(icc: Option<&[u8]>, space: ViewSpace) -> ViewTransform {
        use qcms::{DataType, Intent, Profile, Transform};
        if space == ViewSpace::Raw {
            return ViewTransform(Kind::Identity);
        }
        let src = icc.and_then(|icc| {
            let p = Profile::new_from_slice(icc, false);
            if p.is_none() {
                log::warn!("invalid ICC profile, assuming sRGB");
            }
            Some((p?, icc_space(icc) == Some(b"GRAY")))
        });
        // CMYK/Lab/… profiles: the decoder already produced RGB without ICC → sRGB.
        let src = src.filter(|(p, gray)| *gray || !p.is_sRGB());
        let out = output_profile(space);
        let kind = match src {
            Some((p, true)) => {
                Transform::new_to(&p, &out, DataType::GrayA8, DataType::RGBA8, Intent::Perceptual).map(Kind::Gray)
            }
            Some((p, false)) => Transform::new(&p, &out, DataType::RGBA8, Intent::Perceptual).map(Kind::Rgb),
            None if space == ViewSpace::Srgb => Some(Kind::Identity),
            None => Transform::new(&Profile::new_sRGB(), &out, DataType::RGBA8, Intent::Perceptual).map(Kind::Rgb),
        };
        ViewTransform(kind.unwrap_or_else(|| {
            log::warn!("ICC profile not transformable, assuming sRGB");
            Kind::Identity
        }))
    }

    /// Straight RGBA8 in place.
    pub fn apply(&self, rgba: &mut [u8]) {
        match &self.0 {
            Kind::Identity => {}
            Kind::Rgb(t) => t.apply(rgba),
            Kind::Gray(t) => {
                let ga: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[3]]).collect();
                t.convert(&ga, rgba);
            }
        }
    }
}

fn to_rgba8(px: Pixels) -> Vec<u8> {
    match px {
        Pixels::Rgba8(v) => v,
        // round(v · 255 / 65535)
        Pixels::Rgba16(v) => v.iter().map(|&s| ((s as u32 * 255 + 32767) / 65535) as u8).collect(),
    }
}

/// `lut[a][c] = enc(dec(c) · a)`: premultiplication in linear light, re-encoded as sRGB, so that
/// sampling an `Rgba8UnormSrgb` texture yields correct linear premultiplied colour.
fn premul_lut() -> &'static [[u8; 256]; 256] {
    static LUT: OnceLock<Box<[[u8; 256]; 256]>> = OnceLock::new();
    LUT.get_or_init(|| {
        let mut lut = Box::new([[0u8; 256]; 256]);
        let dec: Vec<f32> = (0..256).map(|c| srgb_decode(c as f32 / 255.0)).collect();
        for (a, row) in lut.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                *v = (srgb_encode(dec[c] * a as f32 / 255.0) * 255.0 + 0.5) as u8;
            }
        }
        lut
    })
}

/// Opaque pixels are skipped (fast path for images without alpha).
pub fn premultiply_linear(rgba: &mut [u8]) {
    let lut = premul_lut();
    for p in rgba.chunks_exact_mut(4) {
        let a = p[3];
        if a != 255 {
            let row = &lut[a as usize];
            p[0] = row[p[0] as usize];
            p[1] = row[p[1] as usize];
            p[2] = row[p[2] as usize];
        }
    }
}

/// Straight RGBA8 in the source space → view buffer (converted, premultiplied).
pub fn finish_view(mut rgba: Vec<u8>, t: &ViewTransform) -> Vec<u8> {
    t.apply(&mut rgba);
    premultiply_linear(&mut rgba);
    rgba
}

/// Source → view buffer in sRGB (kept for callers that don't choose a view space).
pub fn prepare_view(img: &DecodedImage) -> Vec<u8> {
    into_view(img.clone(), ViewSpace::Srgb)
}

/// Source → view buffer ready for upload, consuming the image (8-bit pixels converted in place).
pub fn into_view(img: DecodedImage, space: ViewSpace) -> Vec<u8> {
    let t = ViewTransform::new(img.color.icc.as_deref(), space);
    finish_view(to_rgba8(img.pixels), &t)
}

/// Pixels as straight RGBA8 (16-bit samples rounded).
pub fn rgba8(px: Pixels) -> Vec<u8> {
    to_rgba8(px)
}

/// sRGB → the document space of `icc` (shape colours in the export). Identity
/// without a profile, or when qcms can't use the profile as an output (warned).
pub fn srgb_to_doc(icc: Option<&[u8]>) -> Box<dyn Fn(crate::edit::Color) -> crate::edit::Color + Send + Sync> {
    use qcms::{DataType, Intent, Profile, Transform};
    let t = icc.and_then(|icc| {
        let mut out = Profile::new_from_slice(icc, false)?;
        if out.is_sRGB() {
            return None;
        }
        out.precache_output_transform();
        let t = Transform::new(&Profile::new_sRGB(), &out, DataType::RGBA8, Intent::Perceptual);
        if t.is_none() {
            log::warn!("document profile not usable as output: shape colours stay sRGB");
        }
        t
    });
    match t {
        None => Box::new(|c| c),
        Some(t) => Box::new(move |c| {
            let mut px = [c.r, c.g, c.b, 255];
            t.apply(&mut px);
            crate::edit::Color {
                r: px[0],
                g: px[1],
                b: px[2],
                a: c.a,
            }
        }),
    }
}

/// Straight RGBA8 in the space of `icc` → sRGB in place (formats without ICC, e.g. BMP).
pub fn to_srgb_in_place(rgba: &mut [u8], icc: Option<&[u8]>) {
    ViewTransform::new(icc, ViewSpace::Srgb).apply(rgba);
}

/// Source → straight-alpha sRGB RGBA8 (Copy to the clipboard, other apps expect sRGB).
pub fn into_srgb_straight(img: DecodedImage) -> Vec<u8> {
    let t = ViewTransform::new(img.color.icc.as_deref(), ViewSpace::Srgb);
    let mut rgba = to_rgba8(img.pixels);
    t.apply(&mut rgba);
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_roundtrip() {
        for i in 0..=255 {
            let e = i as f32 / 255.0;
            assert!((srgb_encode(srgb_decode(e)) - e).abs() < 1e-5);
        }
    }

    #[test]
    fn premultiply_values() {
        let mut px = [255, 128, 0, 255, 255, 255, 255, 0, 255, 255, 255, 128];
        premultiply_linear(&mut px);
        assert_eq!(&px[0..4], &[255, 128, 0, 255]); // opaque: unchanged
        assert_eq!(&px[4..8], &[0, 0, 0, 0]); // transparent: black
        // white at 50 % alpha: linear 0.502 → encoded ≈ 0.736 → 188
        assert_eq!(px[8], 188);
        assert_eq!(px[11], 128);
    }

    #[test]
    fn sixteen_to_eight_rounding() {
        assert_eq!(to_rgba8(Pixels::Rgba16(vec![0, 257, 65535, 128])), vec![0, 1, 255, 0]);
    }

    #[test]
    fn view_space_table_provisional() {
        assert_eq!(view_space(ColorSpaceConversion::Disabled), ViewSpace::Raw);
        assert_eq!(view_space(ColorSpaceConversion::Auto), ViewSpace::Srgb);
        assert_eq!(view_space(ColorSpaceConversion::Srgb), ViewSpace::Srgb);
        assert_eq!(view_space(ColorSpaceConversion::DisplayP3), ViewSpace::DisplayP3);
        let ids: std::collections::HashSet<u32> = [ViewSpace::Srgb, ViewSpace::Raw, ViewSpace::DisplayP3]
            .iter()
            .map(|s| s.id())
            .collect();
        assert_eq!(ids.len(), 3);
    }
}
