//! Export of a Markup document: the flattened render, encoded in memory as
//! PNG (8/16 bit), JPEG, WebP lossless, TIFF (8/16 bit) or BMP, with the document's ICC profile
//! and the EXIF block of `metadata`. Writing the file is `atomic`'s job.

use crate::decode::{DecodedImage, Pixels};
use crate::edit::DocState;
use crate::edit::redact::{Patch, PatchCache};
use crate::formats::Format;
use crate::settings::MetadataPreset;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportFormat {
    Png,
    Jpeg,
    Webp,
    Tiff,
    Bmp,
}

impl ExportFormat {
    pub const ALL: &'static [ExportFormat] = &[
        ExportFormat::Png,
        ExportFormat::Jpeg,
        ExportFormat::Webp,
        ExportFormat::Tiff,
        ExportFormat::Bmp,
    ];

    /// The export format of a source format; None = not exportable as itself (Export offers PNG,
    /// Overwrite Original is off).
    pub fn of(f: Format) -> Option<ExportFormat> {
        Some(match f {
            Format::Png => ExportFormat::Png,
            Format::Jpeg => ExportFormat::Jpeg,
            Format::Webp => ExportFormat::Webp,
            Format::Tiff => ExportFormat::Tiff,
            Format::Bmp => ExportFormat::Bmp,
            _ => return None,
        })
    }

    pub fn format(self) -> Format {
        match self {
            ExportFormat::Png => Format::Png,
            ExportFormat::Jpeg => Format::Jpeg,
            ExportFormat::Webp => Format::Webp,
            ExportFormat::Tiff => Format::Tiff,
            ExportFormat::Bmp => Format::Bmp,
        }
    }

    /// Extensions for the save dialog filter, the first one is proposed.
    pub fn exts(self) -> &'static [&'static str] {
        match self {
            ExportFormat::Png => &["png"],
            ExportFormat::Jpeg => &["jpg", "jpeg"],
            ExportFormat::Webp => &["webp"],
            ExportFormat::Tiff => &["tif", "tiff"],
            ExportFormat::Bmp => &["bmp"],
        }
    }

    pub fn name(self) -> &'static str {
        self.format().name()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExportOptions {
    pub format: ExportFormat,
    /// JPEG only, 1–100.
    pub jpeg_quality: u8,
    /// JPEG only: what transparent pixels are flattened on (sRGB).
    pub background: [u8; 3],
    pub preset: MetadataPreset,
}

/// The document's source in its colour space: straight pixels at full precision, an RGB
/// ICC profile (None = sRGB), the source EXIF, and the original file's bytes.
#[derive(Clone, Debug)]
pub struct DocSource {
    pub w: u32,
    pub h: u32,
    pub pixels: Arc<Pixels>,
    pub icc: Option<Arc<[u8]>>,
    pub exif: Option<Arc<[u8]>>,
    pub format: Format,
    pub bytes: Arc<[u8]>,
    /// Redaction patches, shared by the preview and the export of this source.
    pub patch_cache: Arc<Mutex<PatchCache>>,
}

impl DocSource {
    /// The document space is the source's RGB profile. Gray (and CMYK, already converted without
    /// ICC by the decoder) sources become sRGB: an RGB file can't carry those profiles.
    pub fn new(img: DecodedImage, bytes: Arc<[u8]>) -> DocSource {
        let (mut pixels, mut icc) = (img.pixels, img.color.icc);
        if icc.as_deref().is_some_and(|p| !crate::color::icc_is_rgb(p)) {
            // A 16-bit gray-profile source is reduced to 8 bit here (qcms path is 8-bit)
            let mut v = crate::color::rgba8(pixels);
            crate::color::to_srgb_in_place(&mut v, icc.as_deref());
            pixels = Pixels::Rgba8(v);
            icc = None;
        }
        DocSource {
            w: img.size.w,
            h: img.size.h,
            pixels: Arc::new(pixels),
            icc,
            exif: img.meta.exif.map(Into::into),
            format: img.format,
            bytes,
            patch_cache: Arc::default(),
        }
    }
}

impl DocSource {
    /// The redaction patches of `doc` (in the document space), through the cache.
    pub fn patches(&self, doc: &DocState) -> Vec<Arc<Patch>> {
        if doc.redactions.is_empty() {
            return Vec::new();
        }
        let colors = crate::color::srgb_to_doc(self.icc.as_deref());
        let mut c = self.patch_cache.lock().unwrap_or_else(|e| e.into_inner());
        crate::edit::redact::patches(doc, self.w, self.h, (&*self.pixels).into(), &*colors, &mut c)
    }
}

/// The flattened document, straight, in the document space.
pub struct Flat {
    pub w: u32,
    pub h: u32,
    pub pixels: Pixels,
    pub icc: Option<Arc<[u8]>>,
}

/// Render the document at full resolution (the source of truth).
pub fn flatten(doc: &DocState, src: &DocSource) -> Flat {
    let colors = crate::color::srgb_to_doc(src.icc.as_deref());
    let patches = src.patches(doc);
    let (w, h, pixels) = crate::edit::render::render_pixels_with(doc, src.w, src.h, &src.pixels, &*colors, &patches);
    Flat {
        w,
        h,
        pixels,
        icc: src.icc.clone(),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("The image could not be encoded: {0}")]
    Encode(String),
}

fn enc(e: impl std::fmt::Display) -> ExportError {
    ExportError::Encode(e.to_string())
}

fn opaque(px: &Pixels) -> bool {
    match px {
        Pixels::Rgba8(v) => v.chunks_exact(4).all(|p| p[3] == 255),
        Pixels::Rgba16(v) => v.chunks_exact(4).all(|p| p[3] == 65535),
    }
}

/// Drop the alpha channel.
fn rgb<T: Copy>(v: &[T]) -> Vec<T> {
    v.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect()
}

fn u16_bytes(v: &[u16]) -> Vec<u8> {
    v.iter().flat_map(|s| s.to_ne_bytes()).collect()
}

/// Encode the flattened document. `exif` = the source's EXIF block (filtered by the preset).
pub fn encode(flat: &Flat, exif: Option<&[u8]>, o: &ExportOptions) -> Result<Vec<u8>, ExportError> {
    use image::{ExtendedColorType as C, ImageEncoder};
    let (w, h) = (flat.w, flat.h);
    let mut icc = flat.icc.clone();
    let solid = opaque(&flat.pixels);
    let mut out = Vec::new();
    match o.format {
        ExportFormat::Png => {
            let e = image::codecs::png::PngEncoder::new(&mut out);
            match (&flat.pixels, solid) {
                (Pixels::Rgba8(v), false) => e.write_image(v, w, h, C::Rgba8),
                (Pixels::Rgba8(v), true) => e.write_image(&rgb(v), w, h, C::Rgb8),
                (Pixels::Rgba16(v), false) => e.write_image(&u16_bytes(v), w, h, C::Rgba16),
                (Pixels::Rgba16(v), true) => e.write_image(&u16_bytes(&rgb(v)), w, h, C::Rgb16),
            }
            .map_err(enc)?;
        }
        ExportFormat::Jpeg => {
            // flattened on the background in encoded values of the document space
            let to_doc = crate::color::srgb_to_doc(icc.as_deref());
            let [r, g, b] = o.background;
            let bg = to_doc(crate::edit::Color::rgb(r, g, b));
            let bg = [bg.r as u32, bg.g as u32, bg.b as u32];
            let px = crate::color::rgba8(flat.pixels.clone());
            let flat_rgb: Vec<u8> = px
                .chunks_exact(4)
                .flat_map(|p| {
                    let a = p[3] as u32;
                    let mix = |c: u8, k: usize| ((c as u32 * a + bg[k] * (255 - a) + 127) / 255) as u8;
                    [mix(p[0], 0), mix(p[1], 1), mix(p[2], 2)]
                })
                .collect();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, o.jpeg_quality.clamp(1, 100))
                .write_image(&flat_rgb, w, h, C::Rgb8)
                .map_err(enc)?;
        }
        ExportFormat::Webp => {
            let px = crate::color::rgba8(flat.pixels.clone());
            let e = image::codecs::webp::WebPEncoder::new_lossless(&mut out);
            if solid {
                e.write_image(&rgb(&px), w, h, C::Rgb8)
            } else {
                e.write_image(&px, w, h, C::Rgba8)
            }
            .map_err(enc)?;
        }
        ExportFormat::Tiff => out = tiff(flat, solid)?,
        ExportFormat::Bmp => {
            // no ICC in BMP: pixels converted to sRGB
            let mut px = crate::color::rgba8(flat.pixels.clone());
            crate::color::to_srgb_in_place(&mut px, icc.as_deref());
            icc = None;
            let mut e = image::codecs::bmp::BmpEncoder::new(&mut out);
            if solid {
                e.encode(&rgb(&px), w, h, C::Rgb8)
            } else {
                e.encode(&px, w, h, C::Rgba8)
            }
            .map_err(enc)?;
        }
    }
    let exif = match o.format {
        ExportFormat::Png | ExportFormat::Jpeg | ExportFormat::Webp => {
            crate::metadata::export_exif(exif, o.preset, w, h)
        }
        _ => None,
    };
    if icc.is_none() && exif.is_none() || matches!(o.format, ExportFormat::Tiff | ExportFormat::Bmp) {
        return Ok(out);
    }
    use img_parts::{Bytes, DynImage, ImageEXIF, ImageICC};
    let mut parts = DynImage::from_bytes(Bytes::from(out))
        .map_err(enc)?
        .ok_or_else(|| enc("encoder output not recognised"))?;
    if let Some(p) = icc {
        parts.set_icc_profile(Some(Bytes::copy_from_slice(&p)));
    }
    if let Some(e) = exif {
        parts.set_exif(Some(Bytes::from(e)));
    }
    let mut res = Vec::new();
    parts.encoder().write_to(&mut res).map_err(enc)?;
    Ok(res)
}

fn tiff(flat: &Flat, solid: bool) -> Result<Vec<u8>, ExportError> {
    use tiff::encoder::{Compression, DeflateLevel, TiffEncoder, colortype};
    use tiff::tags::Tag;
    let mut out = Cursor::new(Vec::new());
    let mut e = TiffEncoder::new(&mut out)
        .map_err(enc)?
        .with_compression(Compression::Deflate(DeflateLevel::Balanced));
    let (w, h, icc) = (flat.w, flat.h, flat.icc.as_deref());
    macro_rules! write {
        ($ct:ty, $data:expr, $alpha:expr) => {{
            let mut img = e.new_image::<$ct>(w, h).map_err(enc)?;
            if let Some(p) = icc {
                img.encoder().write_tag(Tag::IccProfile, p).map_err(enc)?;
            }
            if $alpha {
                // 2 = unassociated (straight) alpha
                img.encoder().write_tag(Tag::ExtraSamples, 2u16).map_err(enc)?;
            }
            img.write_data($data).map_err(enc)?;
        }};
    }
    match (&flat.pixels, solid) {
        (Pixels::Rgba8(v), false) => write!(colortype::RGBA8, v, true),
        (Pixels::Rgba8(v), true) => write!(colortype::RGB8, &rgb(v), false),
        (Pixels::Rgba16(v), false) => write!(colortype::RGBA16, v, true),
        (Pixels::Rgba16(v), true) => write!(colortype::RGB16, &rgb(v), false),
    }
    Ok(out.into_inner())
}

/// The verification step of the atomic write: the file read back has the
/// expected format and size.
pub fn verify(bytes: &[u8], format: ExportFormat, w: u32, h: u32) -> bool {
    crate::decode::probe(bytes, None).is_some_and(|hd| hd.format == format.format() && (hd.size.w, hd.size.h) == (w, h))
}
