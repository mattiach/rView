//! Decoders from the `image` crate (JPEG, PNG, GIF, WebP, TIFF, BMP, ICO, TGA, PNM, QOI, HDR, EXR, DDS).

use super::{DecodeError, DecodedImage, Pixels, SizeU, SourceColor, SourceMeta, check_size, corrupt};
use crate::formats::Format;
use image::{ColorType, DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use std::io::Cursor;

pub(crate) fn image_format(f: Format) -> Option<ImageFormat> {
    Some(match f {
        Format::Jpeg => ImageFormat::Jpeg,
        Format::Png => ImageFormat::Png,
        Format::Gif => ImageFormat::Gif,
        Format::Webp => ImageFormat::WebP,
        Format::Tiff => ImageFormat::Tiff,
        Format::Bmp => ImageFormat::Bmp,
        Format::Ico | Format::Cur => ImageFormat::Ico,
        Format::Tga => ImageFormat::Tga,
        Format::Pnm => ImageFormat::Pnm,
        Format::Qoi => ImageFormat::Qoi,
        Format::Hdr => ImageFormat::Hdr,
        Format::Exr => ImageFormat::OpenExr,
        Format::Dds => ImageFormat::Dds,
        _ => return None,
    })
}

/// Displayed (oriented) size from the header only.
pub fn probe(bytes: &[u8], format: Format) -> Option<(u32, u32)> {
    let mut dec = ImageReader::with_format(Cursor::new(bytes), image_format(format)?)
        .into_decoder()
        .ok()?;
    let (w, h) = dec.dimensions();
    let o = dec.orientation().ok()?;
    let swap = matches!(
        o,
        image::metadata::Orientation::Rotate90
            | image::metadata::Orientation::Rotate270
            | image::metadata::Orientation::Rotate90FlipH
            | image::metadata::Orientation::Rotate270FlipH
    );
    Some(if swap { (h, w) } else { (w, h) })
}

pub fn decode(bytes: &[u8], format: Format) -> Result<DecodedImage, DecodeError> {
    let fmt = image_format(format).ok_or(DecodeError::Unsupported)?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), fmt);
    // Our own 2 GP limit is checked from the header below; image's default 512 MiB cap is too low.
    reader.no_limits();
    let mut dec = reader.into_decoder().map_err(corrupt)?;
    let (w, h) = dec.dimensions();
    check_size(w, h)?;

    let mut icc = dec.icc_profile().ok().flatten();
    if icc.is_none() && format == Format::Tiff {
        icc = tiff_icc(bytes); // capability fallback: image 0.25.10 doesn't return tag 34675
    }
    let exif = dec.exif_metadata().ok().flatten();
    let has_xmp = dec.xmp_metadata().ok().flatten().is_some();
    let has_iptc = dec.iptc_metadata().ok().flatten().is_some();
    let orientation = dec.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);

    let mut img = DynamicImage::from_decoder(dec).map_err(corrupt)?;
    img.apply_orientation(orientation);

    let hdr = matches!(img.color(), ColorType::Rgb32F | ColorType::Rgba32F);
    let deep = matches!(
        img.color(),
        ColorType::L16 | ColorType::La16 | ColorType::Rgb16 | ColorType::Rgba16
    );
    let size = SizeU {
        w: img.width(),
        h: img.height(),
    };
    let pixels = if hdr {
        Pixels::Rgba16(linear_float_to_srgb16(img.into_rgba32f().into_raw()))
    } else if deep {
        Pixels::Rgba16(img.into_rgba16().into_raw())
    } else {
        Pixels::Rgba8(img.into_rgba8().into_raw())
    };

    Ok(DecodedImage {
        format,
        size,
        pixels,
        color: SourceColor {
            assumed: icc.is_none(),
            icc: icc.map(Into::into),
            hdr,
        },
        meta: SourceMeta {
            exif_read: true,
            exif,
            has_xmp,
            has_iptc,
        },
    })
}

fn tiff_icc(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut d = tiff::decoder::Decoder::new(Cursor::new(bytes)).ok()?;
    d.get_tag_u8_vec(tiff::tags::Tag::IccProfile).ok()
}

/// HDR/EXR are linear Rec.709: clamp to [0, 1] and sRGB-encode (shown as SDR).
fn linear_float_to_srgb16(px: Vec<f32>) -> Vec<u16> {
    px.chunks_exact(4)
        .flat_map(|p| {
            let e = |c: f32| (crate::color::srgb_encode(c.clamp(0.0, 1.0)) * 65535.0 + 0.5) as u16;
            [e(p[0]), e(p[1]), e(p[2]), (p[3].clamp(0.0, 1.0) * 65535.0 + 0.5) as u16]
        })
        .collect()
}
