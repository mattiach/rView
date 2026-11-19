//! Export of a Markup document: the flattened render, encoded in memory as
//! PNG (8/16 bit), JPEG, WebP lossless, TIFF (8/16 bit) or BMP, with the document's ICC profile
//! and the EXIF block of `metadata`. Writing the file is `atomic`'s job.

use crate::decode::{DecodedImage, Pixels};
use crate::edit::redact::{Patch, PatchCache};
use crate::edit::{Adjustments, DocState};
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
    /// The other formats: transparent pixels flattened on `background` too (alpha dropped).
    pub flatten: bool,
    /// Resolution written to the file, pixels unchanged; None = the source EXIF's, by preset.
    pub dpi: Option<u32>,
    /// Output size in pixels, resampled (`resample`); None = the rendered size.
    pub size: Option<(u32, u32)>,
    pub preset: MetadataPreset,
}

impl ExportOptions {
    /// The size of the file written for a `w`×`h` render (what `verify` checks).
    pub fn out_size(&self, w: u32, h: u32) -> (u32, u32) {
        self.size.unwrap_or((w, h))
    }
}

/// Highest resolution every format can store (JPEG's JFIF density is 16 bit).
pub const MAX_DPI: u32 = 65535;

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
    /// The last adjusted bases (`base`), most recent first.
    pub base_cache: Arc<Mutex<Bases>>,
}

/// Adjusted images by their adjustments, most recent first.
pub type Bases = Vec<(Adjustments, Arc<Pixels>)>;

/// Adjusted bases kept: the one on screen and the one being computed for it.
const BASES: usize = 2;

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
            base_cache: Arc::default(),
        }
    }

    /// The document's base for `a`: the source itself when neutral, else the adjusted pixels
    /// (cached; computed once for the preview and the export).
    pub fn base(&self, a: &Adjustments) -> Arc<Pixels> {
        if a.is_neutral() {
            return self.pixels.clone();
        }
        let lock = || self.base_cache.lock().unwrap_or_else(|e| e.into_inner());
        {
            let mut c = lock();
            if let Some(i) = c.iter().position(|(k, _)| k == a) {
                let hit = c.remove(i);
                c.insert(0, hit.clone());
                return hit.1;
            }
        }
        // computed without the lock: the preview reading a cached base never waits for it
        let p = Arc::new(crate::edit::adjust::apply(&self.pixels, self.w, a));
        let mut c = lock();
        c.retain(|(k, _)| k != a);
        c.insert(0, (*a, p.clone()));
        c.truncate(BASES);
        p
    }
}

impl DocSource {
    /// The redaction patches of `doc` (in the document space), through the cache.
    pub fn patches(&self, doc: &DocState) -> Vec<Arc<Patch>> {
        if doc.redactions.is_empty() {
            return Vec::new();
        }
        let colors = crate::color::srgb_to_doc(self.icc.as_deref());
        let base = self.base(&doc.adjust);
        let mut c = self.patch_cache.lock().unwrap_or_else(|e| e.into_inner());
        crate::edit::redact::patches(doc, self.w, self.h, (&*base).into(), &*colors, &mut c)
    }
}

/// The flattened document, straight, in the document space.
pub struct Flat {
    pub w: u32,
    pub h: u32,
    pub pixels: Pixels,
    pub icc: Option<Arc<[u8]>>,
}

impl Flat {
    /// The source itself, without a document (Convert): its pixels are moved, not rendered.
    pub fn of(src: DocSource) -> Flat {
        Flat {
            w: src.w,
            h: src.h,
            pixels: Arc::try_unwrap(src.pixels).unwrap_or_else(|p| (*p).clone()),
            icc: src.icc,
        }
    }
}

/// Render the document at full resolution (the source of truth).
pub fn flatten(doc: &DocState, src: &DocSource) -> Flat {
    let colors = crate::color::srgb_to_doc(src.icc.as_deref());
    let patches = src.patches(doc);
    let base = src.base(&doc.adjust);
    let (w, h, pixels) = crate::edit::render::render_pixels_with(doc, src.w, src.h, &base, &*colors, &patches);
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
    #[error("Invalid resolution: {0} DPI (it must be between 1 and {MAX_DPI})")]
    Dpi(u32),
    #[error("Invalid size: {0} × {1} pixels")]
    Size(u32, u32),
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

/// Straight pixels over the opaque `bg` (encoded values of the document space), in place.
fn flatten_on(px: &mut Pixels, bg: [u8; 3]) {
    match px {
        Pixels::Rgba8(v) => {
            for p in v.chunks_exact_mut(4) {
                let a = p[3] as u32;
                for (c, b) in p.iter_mut().zip(bg) {
                    *c = ((*c as u32 * a + b as u32 * (255 - a) + 127) / 255) as u8;
                }
                p[3] = 255;
            }
        }
        Pixels::Rgba16(v) => {
            for p in v.chunks_exact_mut(4) {
                let a = p[3] as u32;
                for (c, b) in p.iter_mut().zip(bg) {
                    *c = ((*c as u32 * a + b as u32 * 257 * (65535 - a) + 32767) / 65535) as u16;
                }
                p[3] = 65535;
            }
        }
    }
}

/// Pixels per metre (PNG pHYs, BMP header).
fn per_metre(dpi: u32) -> u32 {
    (dpi as f64 / 0.0254).round() as u32
}

/// Encode the flattened document. `exif` = the source's EXIF block (filtered by the preset).
pub fn encode(flat: &Flat, exif: Option<&[u8]>, o: &ExportOptions) -> Result<Vec<u8>, ExportError> {
    use image::{ExtendedColorType as C, ImageEncoder};
    if let Some(d) = o.dpi
        && !(1..=MAX_DPI).contains(&d)
    {
        return Err(ExportError::Dpi(d));
    }
    let resized;
    let flat = match o.size {
        Some(s) if s != (flat.w, flat.h) => {
            if !crate::resample::allowed(flat.w, flat.h, s.0, s.1) {
                return Err(ExportError::Size(s.0, s.1));
            }
            resized = Flat {
                w: s.0,
                h: s.1,
                pixels: crate::resample::resize(&flat.pixels, (flat.w, flat.h), s),
                icc: flat.icc.clone(),
            };
            &resized
        }
        _ => flat,
    };
    let (w, h) = (flat.w, flat.h);
    let mut icc = flat.icc.clone();
    let background = || {
        let [r, g, b] = o.background;
        let c = crate::color::srgb_to_doc(flat.icc.as_deref())(crate::edit::Color::rgb(r, g, b));
        [c.r, c.g, c.b]
    };
    let flattened;
    let pixels = if o.flatten && o.format != ExportFormat::Jpeg && !opaque(&flat.pixels) {
        let mut p = flat.pixels.clone();
        flatten_on(&mut p, background());
        flattened = p;
        &flattened
    } else {
        &flat.pixels
    };
    let solid = opaque(pixels);
    let mut out = Vec::new();
    match o.format {
        ExportFormat::Png => {
            let e = image::codecs::png::PngEncoder::new(&mut out);
            match (pixels, solid) {
                (Pixels::Rgba8(v), false) => e.write_image(v, w, h, C::Rgba8),
                (Pixels::Rgba8(v), true) => e.write_image(&rgb(v), w, h, C::Rgb8),
                (Pixels::Rgba16(v), false) => e.write_image(&u16_bytes(v), w, h, C::Rgba16),
                (Pixels::Rgba16(v), true) => e.write_image(&u16_bytes(&rgb(v)), w, h, C::Rgb16),
            }
            .map_err(enc)?;
        }
        ExportFormat::Jpeg => {
            // flattened on the background in encoded values of the document space
            let mut px = Pixels::Rgba8(crate::color::rgba8(pixels.clone()));
            flatten_on(&mut px, background());
            let Pixels::Rgba8(px) = px else { unreachable!() };
            let mut e = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, o.jpeg_quality.clamp(1, 100));
            if let Some(d) = o.dpi {
                e.set_pixel_density(image::codecs::jpeg::PixelDensity::dpi(d as u16));
            }
            e.write_image(&rgb(&px), w, h, C::Rgb8).map_err(enc)?;
        }
        ExportFormat::Webp => {
            let px = crate::color::rgba8(pixels.clone());
            let e = image::codecs::webp::WebPEncoder::new_lossless(&mut out);
            if solid {
                e.write_image(&rgb(&px), w, h, C::Rgb8)
            } else {
                e.write_image(&px, w, h, C::Rgba8)
            }
            .map_err(enc)?;
        }
        ExportFormat::Tiff => out = tiff(w, h, icc.as_deref(), pixels, solid, o.dpi)?,
        ExportFormat::Bmp => {
            // no ICC in BMP: pixels converted to sRGB
            let mut px = crate::color::rgba8(pixels.clone());
            crate::color::to_srgb_in_place(&mut px, icc.as_deref());
            icc = None;
            let mut e = image::codecs::bmp::BmpEncoder::new(&mut out);
            if solid {
                e.encode(&rgb(&px), w, h, C::Rgb8)
            } else {
                e.encode(&px, w, h, C::Rgba8)
            }
            .map_err(enc)?;
            if let Some(d) = o.dpi {
                // horizontal and vertical resolution of the info header
                let ppm = per_metre(d).to_le_bytes();
                out[38..42].copy_from_slice(&ppm);
                out[42..46].copy_from_slice(&ppm);
            }
        }
    }
    let exif = match o.format {
        ExportFormat::Png | ExportFormat::Jpeg | ExportFormat::Webp => {
            crate::metadata::export_exif(exif, o.preset, w, h, o.dpi)
        }
        _ => None,
    };
    let phys = o.dpi.filter(|_| o.format == ExportFormat::Png);
    if icc.is_none() && exif.is_none() && phys.is_none() || matches!(o.format, ExportFormat::Tiff | ExportFormat::Bmp) {
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
    if let (Some(d), DynImage::Png(png)) = (phys, &mut parts) {
        let ppm = per_metre(d).to_be_bytes();
        let data = [&ppm[..], &ppm[..], &[1]].concat();
        // after IHDR: pHYs must come before the image data
        png.chunks_mut()
            .insert(1, img_parts::png::PngChunk::new(*b"pHYs", Bytes::from(data)));
    }
    let mut res = Vec::new();
    parts.encoder().write_to(&mut res).map_err(enc)?;
    Ok(res)
}

fn tiff(
    w: u32,
    h: u32,
    icc: Option<&[u8]>,
    pixels: &Pixels,
    solid: bool,
    dpi: Option<u32>,
) -> Result<Vec<u8>, ExportError> {
    use tiff::encoder::{Compression, DeflateLevel, Rational, TiffEncoder, colortype};
    use tiff::tags::{ResolutionUnit, Tag};
    let mut out = Cursor::new(Vec::new());
    let mut e = TiffEncoder::new(&mut out)
        .map_err(enc)?
        .with_compression(Compression::Deflate(DeflateLevel::Balanced));
    macro_rules! write {
        ($ct:ty, $data:expr, $alpha:expr) => {{
            let mut img = e.new_image::<$ct>(w, h).map_err(enc)?;
            if let Some(d) = dpi {
                let t = img.encoder();
                t.write_tag(Tag::ResolutionUnit, ResolutionUnit::Inch.to_u16())
                    .map_err(enc)?;
                t.write_tag(Tag::XResolution, Rational { n: d, d: 1 }).map_err(enc)?;
                t.write_tag(Tag::YResolution, Rational { n: d, d: 1 }).map_err(enc)?;
            }
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
    match (pixels, solid) {
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
