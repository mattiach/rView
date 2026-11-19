//! SVG/SVGZ via resvg, rasterised in sRGB so that the longest side equals `target`.

use super::{DecodeError, DecodedImage, Pixels, SizeU, SourceColor, SourceMeta, check_size, corrupt};
use crate::formats::Format;
use resvg::{tiny_skia, usvg};
use std::sync::{Arc, OnceLock};

/// System fonts are loaded lazily, only for SVGs that contain text.
fn fontdb() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    })
    .clone()
}

pub fn decode(bytes: &[u8], target: u32) -> Result<DecodedImage, DecodeError> {
    let data = if bytes.starts_with(&[0x1F, 0x8B]) {
        std::borrow::Cow::Owned(usvg::decompress_svgz(bytes).map_err(corrupt)?)
    } else {
        std::borrow::Cow::Borrowed(bytes)
    };
    let mut opt = usvg::Options::default();
    if memchr_text(&data) {
        opt.fontdb = fontdb();
    }
    let tree = usvg::Tree::from_data(&data, &opt).map_err(corrupt)?;
    let s = tree.size();
    let scale = target.max(1) as f32 / s.width().max(s.height());
    let w = (s.width() * scale).round().max(1.0) as u32;
    let h = (s.height() * scale).round().max(1.0) as u32;
    check_size(w, h)?;
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or(DecodeError::TooLarge { w, h })?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia is premultiplied: convert to straight alpha.
    let pixels = pixmap.pixels().iter().flat_map(|p| {
        let c = p.demultiply();
        [c.red(), c.green(), c.blue(), c.alpha()]
    });
    Ok(DecodedImage {
        format: Format::Svg,
        size: SizeU { w, h },
        pixels: Pixels::Rgba8(pixels.collect()),
        color: SourceColor {
            icc: None,
            assumed: true,
            hdr: false,
        },
        meta: SourceMeta::default(),
    })
}

fn memchr_text(data: &[u8]) -> bool {
    data.windows(5).any(|w| w == b"<text")
}
