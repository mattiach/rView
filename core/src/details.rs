//! File Info details read from the file itself: the pixel layout it stores, container facts
//! (PNG chunks, JPEG sampling, resolution, thumbnail), EXIF fields and pixel statistics.
//! `read` decodes the whole image (unique colours, transparency): call it off the UI thread.

use crate::decode::{self, DecodeOptions, DecodedImage, Pixels};
use crate::formats::Format;
use exif::{Exif, In, Tag, Value};
use std::io::Cursor;

/// Channels stored in the file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    Gray,
    GrayAlpha,
    Rgb,
    Rgba,
    /// Palette colours; `alpha`: the palette has transparent entries.
    Indexed {
        alpha: bool,
    },
    Cmyk,
}

impl Layout {
    pub fn has_alpha(self) -> bool {
        matches!(self, Layout::GrayAlpha | Layout::Rgba | Layout::Indexed { alpha: true })
    }
}

/// What the alpha values of the pixels actually do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transparency {
    /// Every pixel fully opaque.
    None,
    /// Only fully transparent and fully opaque pixels.
    Binary,
    /// Some pixels are semi-transparent.
    Partial,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlphaMode {
    Straight,
    Premultiplied,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResolutionUnit {
    Inch,
    Centimetre,
    /// Only the pixel aspect ratio is given.
    None,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Resolution {
    /// Pixels per inch (converted when the file counts per centimetre or metre).
    pub x: f64,
    pub y: f64,
    /// The unit the file uses.
    pub unit: ResolutionUnit,
}

/// EXIF fields as text (camera, authorship), empty when absent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExifFacts {
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub date_taken: Option<String>,
    pub exposure: Option<String>,
    pub f_number: Option<String>,
    pub iso: Option<String>,
    pub focal_length: Option<String>,
    pub flash: Option<String>,
    /// Decimal degrees (latitude, longitude), north and east positive.
    pub gps: Option<(f64, f64)>,
    pub software: Option<String>,
    pub copyright: Option<String>,
    pub artist: Option<String>,
    /// EXIF orientation 1–8.
    pub orientation: Option<u32>,
    /// EXIF ColorSpace: 1 = sRGB, 0xFFFF = uncalibrated.
    pub color_space: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Details {
    pub layout: Option<Layout>,
    /// Bits per channel stored in the file (1, 8, 16, 32…).
    pub bits: Option<u16>,
    pub float: bool,
    pub transparency: Option<Transparency>,
    pub alpha_mode: Option<AlphaMode>,
    /// Distinct RGB colours (16-bit images counted on their top 8 bits).
    pub unique_colors: Option<u64>,
    pub icc_size: Option<usize>,
    /// Colour space of the ICC profile ("RGB", "Grayscale", "CMYK", "Lab"…).
    pub icc_space: Option<&'static str>,
    pub resolution: Option<Resolution>,
    /// "4:2:0", "4:4:4", "Grayscale"…
    pub jpeg_sampling: Option<String>,
    /// PNG chunk types in file order, repeated chunks counted ("IDAT", 12).
    pub png_chunks: Vec<(String, usize)>,
    pub exif_present: bool,
    pub exif: ExifFacts,
    pub iptc: bool,
    pub xmp: bool,
    /// EXIF thumbnail: present, and its size when it can be read.
    pub thumbnail: Option<Option<(u32, u32)>>,
}

/// Everything above for a file. Decode failures leave the pixel facts empty; container facts
/// are still read.
pub fn read(bytes: &[u8], ext: Option<Format>, opts: &DecodeOptions) -> Details {
    let format = decode::resolve_format(bytes, ext);
    let img = decode::decode(bytes, ext, opts).ok();
    let mut d = Details::default();
    header_layout(bytes, format, &mut d);
    if let Some(img) = &img {
        pixel_facts(img, &mut d);
        d.icc_size = img.color.icc.as_ref().map(|p| p.len());
        d.icc_space = img.color.icc.as_deref().and_then(icc_space);
        d.iptc = img.meta.has_iptc;
        d.xmp = img.meta.has_xmp;
        if let Some(raw) = &img.meta.exif {
            read_exif(raw, &mut d);
        }
    }
    match format {
        Some(Format::Jpeg) => jpeg_facts(bytes, &mut d),
        Some(Format::Png) => png_facts(bytes, &mut d),
        _ => {}
    }
    d.alpha_mode = d.layout.filter(|l| l.has_alpha()).map(|_| alpha_mode(bytes, format));
    d
}

/// Layout and bits from the header, for the formats the `image` crate reads.
fn header_layout(bytes: &[u8], format: Option<Format>, d: &mut Details) {
    use image::{ColorType, ExtendedColorType as E, ImageDecoder};
    let Some(fmt) = format.and_then(decode::image_rs::image_format) else {
        return;
    };
    let Ok(dec) = image::ImageReader::with_format(Cursor::new(bytes), fmt).into_decoder() else {
        return;
    };
    let (ct, original) = (dec.color_type(), dec.original_color_type());
    d.float = matches!(ct, ColorType::Rgb32F | ColorType::Rgba32F);
    d.bits = Some(original.bits_per_pixel() / u16::from(original.channel_count().max(1)));
    d.layout = Some(match original {
        E::Cmyk8 | E::Cmyk16 => Layout::Cmyk,
        _ => match (ct.has_color(), ct.has_alpha()) {
            (false, false) => Layout::Gray,
            (false, true) => Layout::GrayAlpha,
            (true, false) => Layout::Rgb,
            (true, true) => Layout::Rgba,
        },
    });
}

fn pixel_facts(img: &DecodedImage, d: &mut Details) {
    // one pass over the pixels; a 2 MiB bitset of the 2^24 RGB values
    let mut seen = vec![0u64; 1 << 18];
    let (mut gray, mut clear, mut partial) = (true, false, false);
    let mut visit = |r: u8, g: u8, b: u8, a: f32| {
        let i = (usize::from(r) << 16) | (usize::from(g) << 8) | usize::from(b);
        seen[i >> 6] |= 1 << (i & 63);
        gray &= r == g && g == b;
        if a < 1.0 {
            clear = true;
            partial |= a > 0.0;
        }
    };
    match &img.pixels {
        Pixels::Rgba8(v) => v
            .chunks_exact(4)
            .for_each(|p| visit(p[0], p[1], p[2], f32::from(p[3]) / 255.0)),
        Pixels::Rgba16(v) => v.chunks_exact(4).for_each(|p| {
            let top = |c: u16| (c >> 8) as u8;
            visit(top(p[0]), top(p[1]), top(p[2]), f32::from(p[3]) / 65535.0)
        }),
    }
    d.unique_colors = Some(seen.iter().map(|w| u64::from(w.count_ones())).sum());
    d.transparency = Some(match (clear, partial) {
        (false, _) => Transparency::None,
        (true, false) => Transparency::Binary,
        (true, true) => Transparency::Partial,
    });
    // formats the header doesn't describe: what the decoded pixels show
    d.layout.get_or_insert(match (gray, clear) {
        (true, false) => Layout::Gray,
        (true, true) => Layout::GrayAlpha,
        (false, false) => Layout::Rgb,
        (false, true) => Layout::Rgba,
    });
    if d.bits.is_none() {
        d.bits = Some(match img.pixels {
            Pixels::Rgba8(_) => 8,
            Pixels::Rgba16(_) => 16,
        });
    }
}

/// Data colour space of an ICC profile (header bytes 16–19).
fn icc_space(icc: &[u8]) -> Option<&'static str> {
    Some(match icc.get(16..20)? {
        b"RGB " => "RGB",
        b"GRAY" => "Grayscale",
        b"CMYK" => "CMYK",
        b"Lab " => "Lab",
        b"XYZ " => "XYZ",
        b"YCbr" => "YCbCr",
        _ => return None,
    })
}

fn ascii(e: &Exif, tag: Tag) -> Option<String> {
    let f = e.get_field(tag, In::PRIMARY)?;
    let Value::Ascii(parts) = &f.value else { return None };
    let s = parts
        .iter()
        .map(|p| {
            String::from_utf8_lossy(p)
                .trim_matches(|c: char| c == '\0' || c.is_whitespace())
                .to_string()
        })
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    (!s.is_empty()).then_some(s)
}

/// kamadak-exif's own rendering, with units ("1/125 s", "f/2.8", "4.2 mm").
fn shown(e: &Exif, tag: Tag) -> Option<String> {
    let f = e.get_field(tag, In::PRIMARY)?;
    let s = f.display_value().with_unit(e).to_string();
    (!s.trim().is_empty()).then_some(s)
}

fn uint(e: &Exif, tag: Tag) -> Option<u32> {
    e.get_field(tag, In::PRIMARY)?.value.get_uint(0)
}

fn rational(e: &Exif, tag: Tag, ifd: In) -> Option<f64> {
    match &e.get_field(tag, ifd)?.value {
        Value::Rational(v) => v.first().map(|r| r.to_f64()),
        Value::SRational(v) => v.first().map(|r| r.to_f64()),
        _ => None,
    }
}

/// GPS degrees, minutes, seconds + N/S/E/W → signed decimal degrees.
fn gps_coord(e: &Exif, value: Tag, reference: Tag) -> Option<f64> {
    let Value::Rational(dms) = &e.get_field(value, In::PRIMARY)?.value else {
        return None;
    };
    let part = |i: usize| dms.get(i).map(|r| r.to_f64()).filter(|v| v.is_finite());
    let deg = part(0)? + part(1).unwrap_or(0.0) / 60.0 + part(2).unwrap_or(0.0) / 3600.0;
    let negative = ascii(e, reference).is_some_and(|r| r.starts_with(['S', 'W', 's', 'w']));
    Some(if negative { -deg } else { deg })
}

fn read_exif(raw: &[u8], d: &mut Details) {
    let Ok(e) = exif::Reader::new().read_raw(raw.to_vec()) else {
        return;
    };
    d.exif_present = true;
    let x = &mut d.exif;
    x.make = ascii(&e, Tag::Make);
    x.model = ascii(&e, Tag::Model);
    x.lens = ascii(&e, Tag::LensModel);
    x.date_taken = shown(&e, Tag::DateTimeOriginal).or_else(|| shown(&e, Tag::DateTime));
    x.exposure = shown(&e, Tag::ExposureTime);
    x.f_number = shown(&e, Tag::FNumber);
    x.iso = shown(&e, Tag::PhotographicSensitivity);
    x.focal_length = shown(&e, Tag::FocalLength);
    x.flash = shown(&e, Tag::Flash);
    x.gps = gps_coord(&e, Tag::GPSLatitude, Tag::GPSLatitudeRef).zip(gps_coord(
        &e,
        Tag::GPSLongitude,
        Tag::GPSLongitudeRef,
    ));
    x.software = ascii(&e, Tag::Software);
    x.copyright = ascii(&e, Tag::Copyright);
    x.artist = ascii(&e, Tag::Artist);
    x.orientation = uint(&e, Tag::Orientation);
    x.color_space = uint(&e, Tag::ColorSpace);
    if let (Some(rx), Some(ry)) = (
        rational(&e, Tag::XResolution, In::PRIMARY),
        rational(&e, Tag::YResolution, In::PRIMARY),
    ) && rx > 0.0
        && ry > 0.0
    {
        let (unit, per_inch) = match uint(&e, Tag::ResolutionUnit) {
            Some(3) => (ResolutionUnit::Centimetre, 2.54),
            Some(1) => (ResolutionUnit::None, 1.0),
            _ => (ResolutionUnit::Inch, 1.0),
        };
        d.resolution = Some(Resolution {
            x: rx * per_inch,
            y: ry * per_inch,
            unit,
        });
    }
    // IFD1 thumbnail: offset and length from the TIFF header of the EXIF block
    let thumb = |tag| e.get_field(tag, In::THUMBNAIL).and_then(|f| f.value.get_uint(0));
    if let (Some(at), Some(len)) = (
        thumb(Tag::JPEGInterchangeFormat),
        thumb(Tag::JPEGInterchangeFormatLength),
    ) {
        let jpeg = e.buf().get(at as usize..(at as usize).saturating_add(len as usize));
        d.thumbnail = Some(jpeg.and_then(|b| {
            image::ImageReader::with_format(Cursor::new(b), image::ImageFormat::Jpeg)
                .into_dimensions()
                .ok()
        }));
    }
}

fn jpeg_facts(bytes: &[u8], d: &mut Details) {
    let Ok(jpeg) = img_parts::jpeg::Jpeg::from_bytes(img_parts::Bytes::copy_from_slice(bytes)) else {
        return;
    };
    for s in jpeg.segments() {
        let c = s.contents();
        match s.marker() {
            // start of frame (not DHT C4, JPG C8, DAC CC): components and their sampling
            m @ 0xC0..=0xCF if !matches!(m, 0xC4 | 0xC8 | 0xCC) && d.jpeg_sampling.is_none() => {
                d.jpeg_sampling = jpeg_sampling(c);
                // the decoder converts CMYK to RGB and reports RGB: the frame header knows
                match c.get(5) {
                    Some(4) => d.layout = Some(Layout::Cmyk),
                    Some(1) => d.layout = Some(Layout::Gray),
                    _ => {}
                }
            }
            // JFIF density, unless EXIF already gave a resolution
            0xE0 if d.resolution.is_none() && c.len() >= 12 && c.starts_with(b"JFIF\0") => {
                let (x, y) = (u16::from_be_bytes([c[8], c[9]]), u16::from_be_bytes([c[10], c[11]]));
                let (unit, per_inch) = match c[7] {
                    1 => (ResolutionUnit::Inch, 1.0),
                    2 => (ResolutionUnit::Centimetre, 2.54),
                    _ => (ResolutionUnit::None, 1.0),
                };
                if x > 0 && y > 0 {
                    d.resolution = Some(Resolution {
                        x: f64::from(x) * per_inch,
                        y: f64::from(y) * per_inch,
                        unit,
                    });
                }
            }
            _ => {}
        }
    }
}

/// SOF segment → "4:2:0" etc. from the luma and chroma sampling factors.
fn jpeg_sampling(sof: &[u8]) -> Option<String> {
    let n = usize::from(*sof.get(5)?);
    let factor = |i: usize| sof.get(6 + i * 3 + 1).map(|f| (u32::from(f >> 4), u32::from(f & 15)));
    Some(match n {
        1 => "Grayscale".into(),
        3 => {
            let (y, cb, cr) = (factor(0)?, factor(1)?, factor(2)?);
            if cb != cr || cb.0 == 0 || cb.1 == 0 {
                return Some(format!("{}x{}, {}x{}, {}x{}", y.0, y.1, cb.0, cb.1, cr.0, cr.1));
            }
            match (y.0 / cb.0, y.1 / cb.1) {
                (1, 1) => "4:4:4".into(),
                (2, 1) => "4:2:2".into(),
                (2, 2) => "4:2:0".into(),
                (1, 2) => "4:4:0".into(),
                (4, 1) => "4:1:1".into(),
                (4, 2) => "4:1:0".into(),
                _ => format!("{}x{}, {}x{}, {}x{}", y.0, y.1, cb.0, cb.1, cr.0, cr.1),
            }
        }
        4 => "CMYK / YCCK".into(),
        _ => return None,
    })
}

fn png_facts(bytes: &[u8], d: &mut Details) {
    let Ok(png) = img_parts::png::Png::from_bytes(img_parts::Bytes::copy_from_slice(bytes)) else {
        return;
    };
    let mut transparent_palette = false;
    for c in png.chunks() {
        let kind = String::from_utf8_lossy(&c.kind()).into_owned();
        match d.png_chunks.last_mut() {
            Some((k, n)) if *k == kind => *n += 1,
            _ => d.png_chunks.push((kind.clone(), 1)),
        }
        let data = c.contents();
        match &c.kind() {
            b"tRNS" => transparent_palette = true,
            b"pHYs" if d.resolution.is_none() && data.len() >= 9 => {
                let px = |i: usize| f64::from(u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]));
                let (x, y) = (px(0), px(4));
                if x > 0.0 && y > 0.0 {
                    // per metre (unit 1) → per inch; unit 0 is an aspect ratio only
                    d.resolution = Some(if data[8] == 1 {
                        Resolution {
                            x: x * 0.0254,
                            y: y * 0.0254,
                            unit: ResolutionUnit::Centimetre,
                        }
                    } else {
                        Resolution {
                            x,
                            y,
                            unit: ResolutionUnit::None,
                        }
                    });
                }
            }
            _ => {}
        }
    }
    // IHDR colour type 3: palette (the decoder expands it to RGB / RGBA)
    if let Some(ihdr) = png.chunks_by_type(*b"IHDR").next().map(|c| c.contents().clone())
        && ihdr.get(9) == Some(&3)
    {
        d.layout = Some(Layout::Indexed {
            alpha: transparent_palette,
        });
        d.bits = ihdr.get(8).map(|b| u16::from(*b));
    }
}

/// How a file with alpha stores it: TIFF says so (ExtraSamples 1 = associated), OpenEXR is
/// premultiplied by definition, the other formats are straight.
fn alpha_mode(bytes: &[u8], format: Option<Format>) -> AlphaMode {
    match format {
        Some(Format::Exr) => AlphaMode::Premultiplied,
        Some(Format::Tiff) => {
            let associated = tiff::decoder::Decoder::new(Cursor::new(bytes))
                .ok()
                .and_then(|mut t| t.get_tag_u32(tiff::tags::Tag::ExtraSamples).ok());
            if associated == Some(1) {
                AlphaMode::Premultiplied
            } else {
                AlphaMode::Straight
            }
        }
        _ => AlphaMode::Straight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(img: image::DynamicImage) -> Vec<u8> {
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn rgba_png_layout_transparency_colours() {
        let mut img = image::RgbaImage::from_pixel(4, 2, image::Rgba([10, 20, 30, 255]));
        img.put_pixel(0, 0, image::Rgba([200, 0, 0, 0]));
        img.put_pixel(1, 0, image::Rgba([0, 200, 0, 128]));
        let d = read(&png(img.into()), Some(Format::Png), &DecodeOptions::default());
        assert_eq!(d.layout, Some(Layout::Rgba));
        assert_eq!(d.bits, Some(8));
        assert_eq!(d.transparency, Some(Transparency::Partial));
        assert_eq!(d.alpha_mode, Some(AlphaMode::Straight));
        assert_eq!(d.unique_colors, Some(3));
        assert_eq!(d.png_chunks.first().map(|c| c.0.as_str()), Some("IHDR"));
        assert_eq!(d.png_chunks.last().map(|c| c.0.as_str()), Some("IEND"));
        assert!(!d.exif_present && d.thumbnail.is_none());
    }

    #[test]
    fn gray_png_and_binary_alpha() {
        let g = image::GrayImage::from_fn(3, 3, |x, _| image::Luma([x as u8 * 50]));
        let d = read(&png(g.into()), Some(Format::Png), &DecodeOptions::default());
        assert_eq!(d.layout, Some(Layout::Gray));
        assert_eq!(d.transparency, Some(Transparency::None));
        assert_eq!(d.alpha_mode, None, "no alpha channel");
        assert_eq!(d.unique_colors, Some(3));
        let mut a = image::RgbaImage::from_pixel(2, 1, image::Rgba([1, 2, 3, 255]));
        a.put_pixel(0, 0, image::Rgba([1, 2, 3, 0]));
        let d = read(&png(a.into()), Some(Format::Png), &DecodeOptions::default());
        assert_eq!(d.transparency, Some(Transparency::Binary));
    }

    #[test]
    fn jpeg_sampling_and_jfif_density() {
        let img = image::RgbImage::from_pixel(16, 16, image::Rgb([90, 140, 200]));
        let mut out = Vec::new();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
        enc.set_pixel_density(image::codecs::jpeg::PixelDensity::dpi(300));
        enc.encode_image(&img).unwrap();
        let d = read(&out, Some(Format::Jpeg), &DecodeOptions::default());
        assert_eq!(
            d.jpeg_sampling.as_deref(),
            Some("4:4:4"),
            "image's encoder does not subsample"
        );
        assert_eq!(
            d.resolution,
            Some(Resolution {
                x: 300.0,
                y: 300.0,
                unit: ResolutionUnit::Inch
            })
        );
        assert_eq!(d.layout, Some(Layout::Rgb));
        assert_eq!(
            jpeg_sampling(&[8, 0, 16, 0, 16, 3, 1, 0x11, 0, 2, 0x11, 1, 3, 0x11, 1]).as_deref(),
            Some("4:4:4")
        );
        assert_eq!(
            jpeg_sampling(&[8, 0, 16, 0, 16, 3, 1, 0x21, 0, 2, 0x11, 1, 3, 0x11, 1]).as_deref(),
            Some("4:2:2")
        );
        assert_eq!(
            jpeg_sampling(&[8, 0, 16, 0, 16, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]).as_deref(),
            Some("4:2:0")
        );
        assert_eq!(
            jpeg_sampling(&[8, 0, 16, 0, 16, 1, 1, 0x11, 0]).as_deref(),
            Some("Grayscale")
        );
    }

    #[test]
    fn exif_camera_fields_gps_and_resolution() {
        use exif::experimental::Writer;
        use exif::{Field, Rational};
        let f = |tag, value| Field {
            tag,
            ifd_num: In::PRIMARY,
            value,
        };
        let r = |n, d| Rational { num: n, denom: d };
        let fields = [
            f(Tag::Make, Value::Ascii(vec![b"Canon".to_vec()])),
            f(Tag::Model, Value::Ascii(vec![b"EOS R6\0".to_vec()])),
            f(Tag::FNumber, Value::Rational(vec![r(28, 10)])),
            f(Tag::GPSLatitudeRef, Value::Ascii(vec![b"S".to_vec()])),
            f(Tag::GPSLatitude, Value::Rational(vec![r(33, 1), r(30, 1), r(0, 1)])),
            f(Tag::GPSLongitudeRef, Value::Ascii(vec![b"E".to_vec()])),
            f(Tag::GPSLongitude, Value::Rational(vec![r(151, 1), r(12, 1), r(36, 1)])),
            f(Tag::XResolution, Value::Rational(vec![r(118, 1)])),
            f(Tag::YResolution, Value::Rational(vec![r(118, 1)])),
            f(Tag::ResolutionUnit, Value::Short(vec![3])),
        ];
        let mut w = Writer::new();
        for x in &fields {
            w.push_field(x);
        }
        let mut raw = Cursor::new(Vec::new());
        w.write(&mut raw, false).unwrap();
        let mut d = Details::default();
        read_exif(&raw.into_inner(), &mut d);
        assert!(d.exif_present);
        assert_eq!(d.exif.make.as_deref(), Some("Canon"));
        assert_eq!(d.exif.model.as_deref(), Some("EOS R6"), "NUL padding trimmed");
        assert_eq!(d.exif.f_number.as_deref(), Some("f/2.8"));
        let (lat, lon) = d.exif.gps.unwrap();
        assert!((lat + 33.5).abs() < 1e-9 && (lon - 151.21).abs() < 1e-9, "{lat} {lon}");
        let res = d.resolution.unwrap();
        assert_eq!(res.unit, ResolutionUnit::Centimetre);
        assert!((res.x - 299.72).abs() < 1e-6, "118 px/cm = 299.72 DPI: {}", res.x);
    }

    #[test]
    fn icc_header_space() {
        let mut icc = vec![0u8; 128];
        icc[16..20].copy_from_slice(b"CMYK");
        assert_eq!(icc_space(&icc), Some("CMYK"));
        assert_eq!(icc_space(&icc[..10]), None);
    }
}
