//! ICNS: largest PNG/RLE entry (JPEG 2000 entries are not supported by the `icns` crate).

use super::{DecodeError, DecodedImage, Pixels, SizeU, SourceColor, SourceMeta, check_size, corrupt};
use crate::formats::Format;
use icns::{IconFamily, PixelFormat};
use std::io::Cursor;

pub fn decode(bytes: &[u8]) -> Result<DecodedImage, DecodeError> {
    let family = IconFamily::read(Cursor::new(bytes)).map_err(corrupt)?;
    let mut types = family.available_icons();
    types.sort_by_key(|t| std::cmp::Reverse(t.pixel_width()));
    // Try from the largest; some entries may be JPEG 2000 and fail.
    let mut last_err = DecodeError::Corrupt("No usable icon in ICNS file".into());
    for t in types {
        match family.get_icon_with_type(t) {
            Ok(img) => {
                let img = img.convert_to(PixelFormat::RGBA);
                check_size(img.width(), img.height())?;
                return Ok(DecodedImage {
                    format: Format::Icns,
                    size: SizeU {
                        w: img.width(),
                        h: img.height(),
                    },
                    pixels: Pixels::Rgba8(img.into_data().into_vec()),
                    color: SourceColor {
                        icc: None,
                        assumed: true,
                        hdr: false,
                    },
                    meta: SourceMeta::default(),
                });
            }
            Err(e) => last_err = corrupt(e),
        }
    }
    Err(last_err)
}
