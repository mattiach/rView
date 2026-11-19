//! JPEG XL via jxl-oxide (orientation applied by the renderer).

use super::{DecodeError, DecodedImage, Pixels, SizeU, SourceColor, SourceMeta, check_size, corrupt, expand_to_rgba};
use crate::formats::Format;
use jxl_oxide::JxlImage;
use std::io::Cursor;

/// Oriented size from the first bytes (header only).
pub fn probe(bytes: &[u8]) -> Option<(u32, u32)> {
    let image = init_prefix(bytes, 64 << 10)?;
    Some((image.width(), image.height()))
}

/// Initialise from a prefix of the file, growing it until the headers fit.
fn init_prefix(bytes: &[u8], mut n: usize) -> Option<JxlImage> {
    loop {
        let mut uninit = JxlImage::builder().build_uninit();
        uninit.feed_bytes(&bytes[..n.min(bytes.len())]).ok()?;
        match uninit.try_init().ok()? {
            jxl_oxide::InitializeResult::Initialized(img) => return Some(img),
            jxl_oxide::InitializeResult::NeedMoreData(_) if n < bytes.len() => n *= 2,
            jxl_oxide::InitializeResult::NeedMoreData(_) => return None,
        }
    }
}

/// Progressive preview: render what the first 1/8 of the file contains (LF / early passes),
/// full resolution, 8 bit.
// Measured on real files (see below). Runs as its own job, parallel to the
// full decode.
pub fn decode_preview(bytes: &[u8]) -> Option<DecodedImage> {
    let mut image = init_prefix(bytes, (bytes.len() / 8).max(64 << 10))?;
    if image.pixel_format().has_black() {
        return None;
    }
    // Only files encoded in several passes have something meaningful early: measured on a
    // non-progressive 25 MP file the partial render cost 94 % of the full decode and showed
    // undecoded regions (mean error 70); on the libjxl progressive conformance file it is ~35 %
    // faster (194 vs 300 ms) with a usable result (mean difference 13).
    // Frames loaded from the prefix: an LF frame or a multi-pass frame means progressive coding.
    let progressive = (0..).map_while(|i| image.frame(i)).any(|f| {
        let h = f.header();
        h.passes.num_passes > 1 || h.frame_type == jxl_oxide::frame::FrameType::LfFrame
    });
    if !progressive {
        return None;
    }
    let render = image.render_loading_frame().ok()?;
    let mut stream = render.stream();
    let (w, h, ch) = (stream.width(), stream.height(), stream.channels() as usize);
    let mut buf = vec![0u8; w as usize * h as usize * ch];
    stream.write_to_buffer(&mut buf);
    // gray JXL keeps its gray profile: the view pipeline converts gray ICC
    let icc = Some(image.rendered_icc());
    Some(DecodedImage {
        format: Format::Jxl,
        size: SizeU { w, h },
        pixels: Pixels::Rgba8(expand_to_rgba(&buf, ch, u8::MAX)),
        color: SourceColor {
            assumed: icc.is_none(),
            icc: icc.map(Into::into),
            hdr: image.hdr_type().is_some(),
        },
        meta: SourceMeta::default(),
    })
}

pub fn decode(bytes: &[u8]) -> Result<DecodedImage, DecodeError> {
    let image = JxlImage::builder().read(Cursor::new(bytes)).map_err(corrupt)?;
    check_size(image.width(), image.height())?;
    if image.pixel_format().has_black() {
        return Err(DecodeError::Corrupt("CMYK JPEG XL is not supported".into()));
    }
    let deep = image.image_header().metadata.bit_depth.bits_per_sample() > 8;
    let render = image.render_frame(0).map_err(corrupt)?;
    let mut stream = render.stream();
    let (w, h, ch) = (stream.width(), stream.height(), stream.channels() as usize);
    let n = w as usize * h as usize * ch;
    let pixels = if deep {
        let mut buf = vec![0u16; n];
        stream.write_to_buffer(&mut buf);
        Pixels::Rgba16(expand_to_rgba(&buf, ch, u16::MAX))
    } else {
        let mut buf = vec![0u8; n];
        stream.write_to_buffer(&mut buf);
        Pixels::Rgba8(expand_to_rgba(&buf, ch, u8::MAX))
    };
    // gray JXL keeps its gray profile: the view pipeline converts gray ICC
    let icc = Some(image.rendered_icc());
    Ok(DecodedImage {
        format: Format::Jxl,
        size: SizeU { w, h },
        pixels,
        color: SourceColor {
            assumed: icc.is_none(),
            icc: icc.map(Into::into),
            hdr: image.hdr_type().is_some(),
        },
        meta: exif_meta(&image),
    })
}

/// The container's `Exif` box as a TIFF block (metadata only: the orientation comes from the
/// codestream, the EXIF Orientation tag is never applied).
fn exif_meta(image: &JxlImage) -> SourceMeta {
    match image.aux_boxes().first_exif() {
        Ok(jxl_oxide::AuxBoxData::Data(e)) => SourceMeta {
            exif_read: true,
            exif: e.payload().get(e.tiff_header_offset() as usize..).map(<[u8]>::to_vec),
            ..Default::default()
        },
        Ok(jxl_oxide::AuxBoxData::NotFound) => SourceMeta {
            exif_read: true,
            ..Default::default()
        },
        _ => SourceMeta::default(),
    }
}
