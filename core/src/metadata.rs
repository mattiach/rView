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

/// Replaced when the export sets a resolution.
const RESOLUTION: &[Tag] = &[Tag::XResolution, Tag::YResolution, Tag::ResolutionUnit];

/// Rewritten here, never copied.
const REWRITTEN: &[Tag] = &[
    Tag::Orientation,
    Tag::PixelXDimension,
    Tag::PixelYDimension,
    Tag::MakerNote,
];

/// The EXIF block (TIFF structure, as `image` returns it and img-parts inserts it) for an export of
/// `w`×`h` pixels, or None (strip-all, no source EXIF, or a source EXIF that can't be parsed).
/// `dpi` replaces the resolution tags, and gives a block of its own when the source has none.
pub fn export_exif(source: Option<&[u8]>, preset: MetadataPreset, w: u32, h: u32, dpi: Option<u32>) -> Option<Vec<u8>> {
    if preset == MetadataPreset::StripAll {
        return None;
    }
    let parsed = source.and_then(|s| match exif::Reader::new().read_raw(s.to_vec()) {
        Ok(p) => Some(p),
        Err(e) => {
            log::warn!("source EXIF not readable, not exported: {e}");
            None
        }
    });
    if parsed.is_none() && dpi.is_none() {
        return None;
    }
    let keep = |f: &Field| {
        f.ifd_num == In::PRIMARY
            && !REWRITTEN.contains(&f.tag)
            && match f.tag.context() {
                Context::Tiff => KEPT_TIFF.contains(&f.tag),
                Context::Gps => preset == MetadataPreset::Keep,
                _ => true, // Exif, Interop
            }
            && !(preset == MetadataPreset::StripLocation && IDENTIFYING.contains(&f.tag))
            && !(dpi.is_some() && RESOLUTION.contains(&f.tag))
    };
    let mut fields: Vec<Field> = parsed
        .iter()
        .flat_map(|p| p.fields())
        .filter(|f| keep(f))
        .cloned()
        .collect();
    let primary = |tag, value| Field {
        tag,
        ifd_num: In::PRIMARY,
        value,
    };
    fields.push(primary(Tag::Orientation, Value::Short(vec![1])));
    fields.push(primary(Tag::PixelXDimension, Value::Long(vec![w])));
    fields.push(primary(Tag::PixelYDimension, Value::Long(vec![h])));
    if let Some(d) = dpi {
        fields.push(primary(Tag::XResolution, Value::Rational(vec![(d, 1).into()])));
        fields.push(primary(Tag::YResolution, Value::Rational(vec![(d, 1).into()])));
        // 2 = inches
        fields.push(primary(Tag::ResolutionUnit, Value::Short(vec![2])));
    }
    let mut wr = exif::experimental::Writer::new();
    for f in &fields {
        wr.push_field(f);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    match wr.write(&mut out, parsed.as_ref().is_some_and(|p| p.little_endian())) {
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
        assert!(export_exif(Some(&src), MetadataPreset::StripAll, 10, 20, None).is_none());
        assert!(export_exif(None, MetadataPreset::Keep, 10, 20, None).is_none());

        let keep = export_exif(Some(&src), MetadataPreset::Keep, 10, 20, None).unwrap();
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

        let loc = tags(&export_exif(Some(&src), MetadataPreset::StripLocation, 10, 20, None).unwrap());
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

    fn resolution(b: &[u8]) -> (Option<u32>, Option<u32>, Option<u32>) {
        let e = exif::Reader::new().read_raw(b.to_vec()).unwrap();
        let rat = |t| {
            e.get_field(t, In::PRIMARY).and_then(|f| match &f.value {
                Value::Rational(v) => v.first().map(|r| r.num / r.denom),
                _ => None,
            })
        };
        let unit = e
            .get_field(Tag::ResolutionUnit, In::PRIMARY)
            .and_then(|f| f.value.get_uint(0));
        (rat(Tag::XResolution), rat(Tag::YResolution), unit)
    }

    #[test]
    fn dpi_replaces_the_resolution() {
        let f = |tag, value| Field {
            tag,
            ifd_num: In::PRIMARY,
            value,
        };
        let mut w = exif::experimental::Writer::new();
        let fields = [
            f(Tag::Make, Value::Ascii(vec![b"Cam".to_vec()])),
            f(Tag::XResolution, Value::Rational(vec![(72, 1).into()])),
            f(Tag::YResolution, Value::Rational(vec![(72, 1).into()])),
            f(Tag::ResolutionUnit, Value::Short(vec![2])),
        ];
        for x in &fields {
            w.push_field(x);
        }
        let mut src = std::io::Cursor::new(Vec::new());
        w.write(&mut src, true).unwrap();
        let src = src.into_inner();

        // no DPI: the source's resolution is copied
        let kept = export_exif(Some(&src), MetadataPreset::Keep, 10, 20, None).unwrap();
        assert_eq!(resolution(&kept), (Some(72), Some(72), Some(2)));
        for preset in [MetadataPreset::Keep, MetadataPreset::StripLocation] {
            let out = export_exif(Some(&src), preset, 10, 20, Some(300)).unwrap();
            assert_eq!(resolution(&out), (Some(300), Some(300), Some(2)), "{preset:?}");
            let t = tags(&out);
            assert_eq!(
                t.iter().filter(|x| x.0 == Tag::XResolution).count(),
                1,
                "one XResolution"
            );
            assert!(t.iter().any(|x| x.0 == Tag::Make), "other tags kept");
        }
        // no source EXIF: a block with only the layout and the resolution
        let own = export_exif(None, MetadataPreset::Keep, 10, 20, Some(150)).unwrap();
        assert_eq!(resolution(&own), (Some(150), Some(150), Some(2)));
        assert!(!tags(&own).iter().any(|x| x.0 == Tag::Make));
        // strip-all writes no EXIF at all
        assert!(export_exif(Some(&src), MetadataPreset::StripAll, 10, 20, Some(300)).is_none());
        assert!(export_exif(None, MetadataPreset::StripAll, 10, 20, Some(300)).is_none());
    }
}
