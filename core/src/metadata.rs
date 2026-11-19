//! EXIF of exported images. The export never copies the source file's segments: the
//! encoder writes a new file and only the block built here (plus the ICC) is added, so embedded
//! thumbnails in other places (JFXX, APP13/IPTC, MPF, XMP) can't come along. In the EXIF itself:
//! no IFD1 (the thumbnail), Orientation = 1, pixel dimensions of the export, no MakerNote, no XMP
//! packet; the preset then removes the location and identifying tags, or everything.

use crate::settings::MetadataPreset;
use exif::{Context, Field, In, Tag, Value};

/// HostComputer (0x013C), not named by kamadak-exif 0.6.
const HOST_COMPUTER: Tag = Tag(Context::Tiff, 0x013C);

/// TIFF-context tags of IFD0 that describe the photo, not the pixel layout of the source file
/// (a TIFF source's EXIF is its IFD0: strip offsets, compression… must not be copied).
const KEPT_TIFF: &[Tag] = &[
    Tag::ImageDescription,
    Tag::Make,
    Tag::Model,
    Tag::XResolution,
    Tag::YResolution,
    Tag::ResolutionUnit,
    Tag::Software,
    Tag::DateTime,
    Tag::Artist,
    HOST_COMPUTER,
    Tag::Copyright,
];

/// Removed by `strip-location` (besides the whole GPS IFD).
const IDENTIFYING: &[Tag] = &[
    Tag::BodySerialNumber,
    Tag::LensSerialNumber,
    Tag::CameraOwnerName,
    Tag::ImageUniqueID,
    Tag::Artist,
    HOST_COMPUTER,
    Tag::UserComment,
];

/// Rewritten here, never copied.
const REWRITTEN: &[Tag] = &[
    Tag::Orientation,
    Tag::PixelXDimension,
    Tag::PixelYDimension,
    Tag::MakerNote,
];

/// The EXIF block (TIFF structure, as `image` returns it and img-parts inserts it) for an export of
/// `w`×`h` pixels, or None (strip-all, no source EXIF, or a source EXIF that can't be parsed).
pub fn export_exif(source: Option<&[u8]>, preset: MetadataPreset, w: u32, h: u32) -> Option<Vec<u8>> {
    if preset == MetadataPreset::StripAll {
        return None;
    }
    let parsed = match exif::Reader::new().read_raw(source?.to_vec()) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("source EXIF not readable, not exported: {e}");
            return None;
        }
    };
    let keep = |f: &Field| {
        f.ifd_num == In::PRIMARY
            && !REWRITTEN.contains(&f.tag)
            && match f.tag.context() {
                Context::Tiff => KEPT_TIFF.contains(&f.tag),
                Context::Gps => preset == MetadataPreset::Keep,
                _ => true, // Exif, Interop
            }
            && !(preset == MetadataPreset::StripLocation && IDENTIFYING.contains(&f.tag))
    };
    let mut fields: Vec<Field> = parsed.fields().filter(|f| keep(f)).cloned().collect();
    let primary = |tag, value| Field {
        tag,
        ifd_num: In::PRIMARY,
        value,
    };
    fields.push(primary(Tag::Orientation, Value::Short(vec![1])));
    fields.push(primary(Tag::PixelXDimension, Value::Long(vec![w])));
    fields.push(primary(Tag::PixelYDimension, Value::Long(vec![h])));
    let mut wr = exif::experimental::Writer::new();
    for f in &fields {
        wr.push_field(f);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    match wr.write(&mut out, parsed.little_endian()) {
        Ok(()) => Some(out.into_inner()),
        Err(e) => {
            log::warn!("EXIF not written: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> Vec<u8> {
        let ascii = |s: &str| Value::Ascii(vec![s.as_bytes().to_vec()]);
        let f = |tag, ifd_num, value| Field { tag, ifd_num, value };
        let fields = [
            f(Tag::Orientation, In::PRIMARY, Value::Short(vec![6])),
            f(Tag::Make, In::PRIMARY, ascii("Cam")),
            f(Tag::Artist, In::PRIMARY, ascii("Someone")),
            f(Tag::StripOffsets, In::PRIMARY, Value::Long(vec![8])),
            f(Tag::BodySerialNumber, In::PRIMARY, ascii("123")),
            f(Tag::MakerNote, In::PRIMARY, Value::Undefined(vec![1, 2, 3], 0)),
            f(Tag::ExposureTime, In::PRIMARY, Value::Rational(vec![(1, 100).into()])),
            f(Tag::GPSLatitudeRef, In::PRIMARY, ascii("N")),
            f(Tag::GPSLatitude, In::PRIMARY, Value::Rational(vec![(45, 1).into(); 3])),
            f(Tag::Compression, In::THUMBNAIL, Value::Short(vec![6])),
        ];
        let mut w = exif::experimental::Writer::new();
        for x in &fields {
            w.push_field(x);
        }
        let thumb = [0xFFu8, 0xD8, 0xFF, 0xD9];
        w.set_jpeg(&thumb, In::THUMBNAIL);
        let mut out = std::io::Cursor::new(Vec::new());
        w.write(&mut out, false).unwrap();
        out.into_inner()
    }

    fn tags(b: &[u8]) -> Vec<(Tag, In)> {
        let e = exif::Reader::new().read_raw(b.to_vec()).unwrap();
        e.fields().map(|f| (f.tag, f.ifd_num)).collect()
    }

    #[test]
    fn presets() {
        let src = source();
        assert!(tags(&src).iter().any(|t| t.1 == In::THUMBNAIL), "fixture has IFD1");
        assert!(export_exif(Some(&src), MetadataPreset::StripAll, 10, 20).is_none());
        assert!(export_exif(None, MetadataPreset::Keep, 10, 20).is_none());

        let keep = export_exif(Some(&src), MetadataPreset::Keep, 10, 20).unwrap();
        let e = exif::Reader::new().read_raw(keep.clone()).unwrap();
        let uint = |t| e.get_field(t, In::PRIMARY).and_then(|f| f.value.get_uint(0));
        assert_eq!(uint(Tag::Orientation), Some(1));
        assert_eq!(
            (uint(Tag::PixelXDimension), uint(Tag::PixelYDimension)),
            (Some(10), Some(20))
        );
        let t = tags(&keep);
        for gone in [Tag::MakerNote, Tag::StripOffsets, Tag::JPEGInterchangeFormat] {
            assert!(!t.iter().any(|x| x.0 == gone), "{gone} kept");
        }
        assert!(!t.iter().any(|x| x.1 == In::THUMBNAIL), "IFD1 kept");
        for kept in [
            Tag::Make,
            Tag::Artist,
            Tag::BodySerialNumber,
            Tag::ExposureTime,
            Tag::GPSLatitude,
        ] {
            assert!(t.iter().any(|x| x.0 == kept), "{kept} lost with keep");
        }

        let loc = tags(&export_exif(Some(&src), MetadataPreset::StripLocation, 10, 20).unwrap());
        for gone in [
            Tag::GPSLatitude,
            Tag::GPSLatitudeRef,
            Tag::Artist,
            Tag::BodySerialNumber,
        ] {
            assert!(!loc.iter().any(|x| x.0 == gone), "{gone} kept by strip-location");
        }
        assert!(loc.iter().any(|x| x.0 == Tag::Make) && loc.iter().any(|x| x.0 == Tag::ExposureTime));
    }
}
