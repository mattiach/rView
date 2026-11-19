//! Decoding to a normalised `DecodedImage`: RGBA, straight alpha, orientation applied.

mod icns;
mod image_rs;
mod jxl;
mod legacy;
mod svg;
pub mod wic;

use crate::formats::Format;
use std::sync::Arc;

/// Refuse images above 2 gigapixels, checked from the header before decoding.
pub const MAX_PIXELS: u64 = 2_000_000_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SizeU {
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Debug)]
pub enum Pixels {
    Rgba8(Vec<u8>),
    Rgba16(Vec<u16>),
}

#[derive(Clone, Debug, Default)]
pub struct SourceColor {
    /// Embedded ICC profile; `None` = sRGB.
    pub icc: Option<Arc<[u8]>>,
    /// True when no profile was found and sRGB is assumed.
    pub assumed: bool,
    /// HDR / float content shown as SDR.
    pub hdr: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SourceMeta {
    /// The decoder looked for EXIF (`exif` = None then really means "none"); SVG, icns and WIC
    /// containers kamadak-exif doesn't know (JPEG XR) don't read it.
    pub exif_read: bool,
    pub exif: Option<Vec<u8>>,
    pub has_xmp: bool,
    pub has_iptc: bool,
}

#[derive(Clone, Debug)]
pub struct DecodedImage {
    pub format: Format,
    pub size: SizeU,
    pub pixels: Pixels,
    pub color: SourceColor,
    pub meta: SourceMeta,
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("Unsupported or unrecognised image format")]
    Unsupported,
    #[error("{0}")]
    Corrupt(String),
    #[error("Image is too large ({w} x {h}); the limit is 2 gigapixels")]
    TooLarge { w: u32, h: u32 },
    #[error("The {codec} codec is not installed. Get it from the Microsoft Store: {store_url}")]
    CodecMissing {
        codec: &'static str,
        store_url: &'static str,
    },
}

impl DecodeError {
    /// Numeric code shown as "(Error <n>)" in the viewer.
    pub fn code(&self) -> u32 {
        match self {
            DecodeError::Unsupported => 1,
            DecodeError::Corrupt(_) => 2,
            DecodeError::TooLarge { .. } => 3,
            DecodeError::CodecMissing { .. } => 4,
        }
    }
}

pub(crate) fn corrupt(e: impl std::fmt::Display) -> DecodeError {
    DecodeError::Corrupt(e.to_string())
}

pub(crate) fn check_size(w: u32, h: u32) -> Result<(), DecodeError> {
    if w == 0 || h == 0 {
        return Err(DecodeError::Corrupt("Image has zero size".into()));
    }
    if w as u64 * h as u64 > MAX_PIXELS {
        return Err(DecodeError::TooLarge { w, h });
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub struct DecodeOptions {
    /// Target for the longest side of vector images (largest monitor side).
    pub svg_target: u32,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        DecodeOptions { svg_target: 2048 }
    }
}

/// Cheap header facts: format and displayed size (orientation applied).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Header {
    pub format: Format,
    pub size: SizeU,
}

/// Read only the header. `None` when the format has no cheap header path (SVG/ICNS, or unknown).
/// Format from the content, except TIFF-based camera raws (DNG, NEF, CR2, ARW, …): their magic is
/// plain TIFF, and the TIFF decoder would show their embedded thumbnail (or label them TIFF, which
/// later phases treat as overwritable). A `.dng` that is really a JPEG stays a JPEG.
fn resolve_format(bytes: &[u8], ext_format: Option<Format>) -> Option<Format> {
    match (Format::sniff(bytes), ext_format) {
        (Some(Format::Tiff), Some(Format::Raw)) => Some(Format::Raw),
        (sniffed, ext) => sniffed.or(ext),
    }
}

pub fn probe(bytes: &[u8], ext_format: Option<Format>) -> Option<Header> {
    let format = resolve_format(bytes, ext_format)?;
    let (w, h) = match format {
        Format::Jxl => jxl::probe(bytes)?,
        Format::Heif | Format::Avif | Format::Jxr | Format::Raw => wic::probe(bytes)?,
        Format::Svg | Format::Icns | Format::Project => return None,
        Format::Wbmp | Format::Xbm | Format::Xpm => legacy::probe(bytes, format)?,
        _ => image_rs::probe(bytes, format).or_else(|| wic::probe(bytes))?,
    };
    Some(Header {
        format,
        size: SizeU { w, h },
    })
}

/// Reduced-cost preview for big files: JPEG via WIC DCT scaling to ≥ `min_side`, JPEG XL
/// from the first progressive passes. `None` = no cheap preview for this format.
pub fn decode_preview(bytes: &[u8], format: Format, min_side: u32) -> Option<DecodedImage> {
    match format {
        Format::Jpeg => wic::decode_jpeg_scaled(bytes, min_side).ok(),
        Format::Jxl => jxl::decode_preview(bytes),
        _ => None,
    }
}

/// Decode a whole file held in memory. `ext_format` comes from the file extension and is used
/// only when the magic bytes are not recognised.
pub fn decode(bytes: &[u8], ext_format: Option<Format>, opts: &DecodeOptions) -> Result<DecodedImage, DecodeError> {
    let format = resolve_format(bytes, ext_format).ok_or(DecodeError::Unsupported)?;
    match format {
        Format::Jxl => jxl::decode(bytes),
        Format::Svg => svg::decode(bytes, opts.svg_target),
        Format::Icns => icns::decode(bytes),
        Format::Project => decode_project(bytes, opts),
        Format::Wbmp | Format::Xbm | Format::Xpm => legacy::decode(bytes, format),
        Format::Heif | Format::Avif | Format::Jxr | Format::Raw => wic::decode(bytes, format),
        // Rust decoders first; WIC as fallback for exotic JPEG/TIFF/DDS and for CUR.
        Format::Jpeg | Format::Tiff | Format::Dds | Format::Cur => {
            image_rs::decode(bytes, format).or_else(|e| match e {
                DecodeError::TooLarge { .. } => Err(e),
                _ => wic::decode(bytes, format).map_err(|_| e),
            })
        }
        _ => image_rs::decode(bytes, format),
    }
}

/// A `.rvm` is shown as its flattened document, in the document's colour space.
fn decode_project(bytes: &[u8], opts: &DecodeOptions) -> Result<DecodedImage, DecodeError> {
    use crate::project::ProjectError;
    let p = crate::project::read(bytes, opts).map_err(|e| match e {
        ProjectError::Original(e) => e,
        ProjectError::TooLarge => DecodeError::Corrupt(e.to_string()),
        e => DecodeError::Corrupt(e.to_string()),
    })?;
    let src = crate::export::DocSource::new(p.image, Arc::from(Vec::new()));
    let flat = crate::export::flatten(&p.doc, &src);
    Ok(DecodedImage {
        format: Format::Project,
        size: SizeU { w: flat.w, h: flat.h },
        pixels: flat.pixels,
        color: SourceColor {
            assumed: flat.icc.is_none(),
            icc: flat.icc,
            hdr: false,
        },
        meta: SourceMeta::default(),
    })
}

/// Expand interleaved samples with `ch` channels (1 gray, 2 gray+alpha, 3 rgb, 4 rgba) to RGBA.
pub(crate) fn expand_to_rgba<T: Copy>(src: &[T], ch: usize, opaque: T) -> Vec<T> {
    if ch == 4 {
        return src.to_vec();
    }
    let mut out = Vec::with_capacity(src.len() / ch * 4);
    for px in src.chunks_exact(ch) {
        match ch {
            1 => out.extend_from_slice(&[px[0], px[0], px[0], opaque]),
            2 => out.extend_from_slice(&[px[0], px[0], px[0], px[1]]),
            _ => out.extend_from_slice(&[px[0], px[1], px[2], opaque]),
        }
    }
    out
}
