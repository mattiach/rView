//! `.rvm` Markup projects: a ZIP with
//! `manifest.json`, `document.json` (the `DocState`) and `original.<ext>`, the original file's
//! exact bytes. Read entirely in memory as untrusted input: nothing is ever written to
//! disk, every limit is checked while reading (not from the headers), and any violation is a
//! `ProjectError` — never a partial load, never a panic.

use crate::decode::{DecodeOptions, DecodedImage};
use crate::edit::{Area, DocState, Geom, RedactKind};
use crate::formats::{EXTENSIONS, Format};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Write};

pub const EXTENSION: &str = "rvm";
pub const FORMAT_ID: &str = "rview-markup";
pub const VERSION: u32 = 1;

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;
const GIB: u64 = 1024 * MIB;
pub const MAX_FILE: u64 = 2 * GIB + 64 * MIB;
const MAX_MANIFEST: u64 = 64 * KIB;
const MAX_DOCUMENT: u64 = 64 * MIB;
const MAX_ORIGINAL: u64 = 2 * GIB;
/// decompressed/compressed above 1 MiB.
const MAX_RATIO: u64 = 100;
const MAX_OBJECTS: usize = 100_000;
const MAX_POINTS: usize = 1_000_000;
const MAX_TOTAL_POINTS: usize = 10_000_000;
const MAX_TEXT: usize = 100_000;
/// Font sizes the Inspector offers (image px).
pub const TEXT_SIZES: std::ops::RangeInclusive<f32> = 4.0..=500.0;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("The project file is larger than the 2 GB limit.")]
    TooLarge,
    #[error("The project file is damaged or not an rView project ({0}).")]
    Corrupt(String),
    #[error("The project uses a feature rView does not support ({0}).")]
    Unsupported(String),
    #[error("The project was created with a newer version of rView. Update rView to open it.")]
    TooNew,
    #[error("The project's image could not be read: {0}")]
    Original(crate::decode::DecodeError),
    #[error("Cannot read the project file: {0}")]
    Io(#[from] std::io::Error),
}

fn corrupt(s: impl Into<String>) -> ProjectError {
    ProjectError::Corrupt(s.into())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub app: String,
    /// ISO 8601, UTC.
    #[serde(default)]
    pub created: String,
    pub original_name: String,
    pub original_len: u64,
}

/// A project read and validated: the document (revision 0, `History::from_loaded` gives it one),
/// the original file and its decoded pixels.
pub struct Project {
    pub manifest: Manifest,
    pub doc: DocState,
    pub original: Vec<u8>,
    pub format: Format,
    pub image: DecodedImage,
}

/// Read a `.rvm` from disk, refusing files above the limit before reading them.
pub fn read_file(path: &std::path::Path, opts: &DecodeOptions) -> Result<Project, ProjectError> {
    let f = std::fs::File::open(path)?;
    if f.metadata()?.len() > MAX_FILE {
        return Err(ProjectError::TooLarge);
    }
    let mut bytes = Vec::new();
    f.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    read(&bytes, opts)
}

fn zip_err(e: zip::result::ZipError) -> ProjectError {
    use zip::result::ZipError;
    match e {
        ZipError::UnsupportedArchive(s) => ProjectError::Unsupported(s.to_string()),
        e => corrupt(e.to_string()),
    }
}

/// Entry name allowed by rule 3 (before matching the three expected names).
fn safe_name(n: &str) -> bool {
    !n.is_empty() && !n.contains(['/', '\\', ':']) && !n.contains("..") && !n.chars().any(char::is_control)
}

/// The format of `original.<ext>`, for a supported image extension (a project can't hold one).
fn original_format(name: &str) -> Option<Format> {
    let ext = name.strip_prefix("original.")?;
    EXTENSIONS
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, f)| *f)
        .filter(|f| *f != Format::Project)
}

/// Read one entry to the end (the CRC is checked there), at most `limit` bytes.
fn read_entry(e: &mut zip::read::ZipFile<'_, Cursor<&[u8]>>, limit: u64) -> Result<Vec<u8>, ProjectError> {
    let deflate = e.compression() == zip::CompressionMethod::Deflated;
    let packed = e.compressed_size().max(1);
    let name = e.name().map_err(zip_err)?.into_owned();
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = e.read(&mut buf).map_err(|err| corrupt(format!("{name}: {err}")))?;
        if n == 0 {
            return Ok(out);
        }
        out.extend_from_slice(&buf[..n]);
        let len = out.len() as u64;
        if len > limit {
            return Err(corrupt(format!("{name} is larger than allowed")));
        }
        if deflate && len > MIB && len > MAX_RATIO * packed {
            return Err(corrupt(format!("{name}: compression ratio above {MAX_RATIO}:1")));
        }
    }
}

/// Parse and validate a project held in memory.
pub fn read(bytes: &[u8], opts: &DecodeOptions) -> Result<Project, ProjectError> {
    if bytes.len() as u64 > MAX_FILE {
        return Err(ProjectError::TooLarge);
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(zip_err)?;
    if zip.len() != 3 {
        return Err(corrupt(format!("{} entries instead of 3", zip.len())));
    }
    let (mut manifest, mut document, mut original) = (None, None, None);
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).map_err(zip_err)?;
        let name = e.name().map_err(zip_err)?.into_owned();
        if !safe_name(&name) || e.is_dir() {
            return Err(corrupt(format!("entry {name:?} not allowed")));
        }
        let method: u16 = match e.compression() {
            zip::CompressionMethod::Stored => 0,
            zip::CompressionMethod::Deflated => 8,
            m => return Err(ProjectError::Unsupported(format!("compression {m}"))),
        };
        // the local header must say what the central directory says (name, method)
        let at = usize::try_from(e.header_start()).unwrap_or(usize::MAX);
        let local = at.checked_add(30).and_then(|end| bytes.get(at..end));
        let same = local.is_some_and(|l| {
            let n = u16::from_le_bytes([l[26], l[27]]) as usize;
            l[..4] == [b'P', b'K', 3, 4]
                && u16::from_le_bytes([l[8], l[9]]) == method
                && bytes.get(at + 30..at + 30 + n) == Some(e.name_raw())
        });
        if !same {
            return Err(corrupt(format!("local header of {name:?} differs from the directory")));
        }
        let (slot, limit) = match name.as_str() {
            "manifest.json" => (&mut manifest, MAX_MANIFEST),
            "document.json" => (&mut document, MAX_DOCUMENT),
            n if original_format(n).is_some() => (&mut original, MAX_ORIGINAL),
            _ => return Err(corrupt(format!("unexpected entry {name:?}"))),
        };
        if slot.is_some() {
            return Err(corrupt(format!("duplicate entry {name:?}")));
        }
        *slot = Some((name.clone(), read_entry(&mut e, limit)?));
    }
    let (Some((_, m)), Some((_, d)), Some((oname, original))) = (manifest, document, original) else {
        return Err(corrupt("missing entry"));
    };
    let manifest: Manifest = serde_json::from_slice(&m).map_err(|e| corrupt(format!("manifest.json: {e}")))?;
    if manifest.format != FORMAT_ID {
        return Err(corrupt(format!("format {:?}", manifest.format)));
    }
    if manifest.version > VERSION {
        return Err(ProjectError::TooNew);
    }
    if manifest.version == 0 {
        return Err(corrupt("version 0"));
    }
    if manifest.original_len != original.len() as u64 {
        return Err(corrupt("original size differs from the manifest"));
    }
    let doc: DocState = serde_json::from_slice(&d).map_err(|e| corrupt(format!("document.json: {e}")))?;
    let format = original_format(&oname).expect("checked above");
    // rule 8: the normal decoder, with its limits (2 GP from the header)
    let image = crate::decode::decode(&original, Some(format), opts).map_err(ProjectError::Original)?;
    validate(&doc, image.size.w, image.size.h)?;
    Ok(Project {
        manifest,
        doc,
        original,
        format: image.format,
        image,
    })
}

/// Semantic checks of rule 7.
pub fn validate(doc: &DocState, w: u32, h: u32) -> Result<(), ProjectError> {
    let bad = |s: &str| Err(corrupt(format!("document: {s}")));
    if (doc.image_size.w, doc.image_size.h) != (w, h) {
        return bad("image size differs from the original");
    }
    if !doc.ids_valid() {
        return bad("object IDs");
    }
    if doc.orient.quarter_turns > 3 {
        return bad("orientation");
    }
    if let Some(c) = doc.crop
        && (c.w == 0 || c.h == 0 || c.x as u64 + c.w as u64 > w as u64 || c.y as u64 + c.h as u64 > h as u64)
    {
        return bad("crop outside the image");
    }
    if doc.shapes.len() + doc.redactions.len() > MAX_OBJECTS {
        return bad("too many objects");
    }
    // coordinates finite and within ±4× the image size (rotations: a few turns at most)
    let (lx, ly) = (4.0 * w as f32, 4.0 * h as f32);
    let xy = |x: f32, y: f32| x.is_finite() && y.is_finite() && x.abs() <= lx && y.abs() <= ly;
    let rect = |r: &crate::edit::RectF| xy(r.x, r.y) && xy(r.w, r.h);
    let rot = |r: f32| r.is_finite() && r.abs() <= 8.0 * std::f32::consts::PI;
    let mut total = 0usize;
    for s in &doc.shapes {
        let ok = crate::edit::apply::check_geom(&s.geom).is_ok()
            && crate::edit::apply::check_style(&s.style).is_ok()
            && s.style.width <= 40.0
            && match &s.geom {
                Geom::Rect {
                    rect: r,
                    rotation,
                    corner_radius,
                } => rect(r) && rot(*rotation) && (0.0..=100.0).contains(corner_radius),
                Geom::Ellipse { rect: r, rotation } => rect(r) && rot(*rotation),
                Geom::Line { a, b } | Geom::Arrow { a, b, .. } => xy(a.x, a.y) && xy(b.x, b.y),
                Geom::Text {
                    rect: r,
                    rotation,
                    text,
                    font,
                    ..
                } => {
                    rect(r)
                        && rot(*rotation)
                        && text.chars().count() <= MAX_TEXT
                        && (TEXT_SIZES).contains(&font.size)
                        && (100..=900).contains(&font.weight)
                        && font.family.chars().count() <= 256
                        && !font.family.chars().any(char::is_control)
                }
                Geom::Loupe { center, radius, zoom } => {
                    xy(center.x, center.y) && *radius <= lx.max(ly) && (2.0..=4.0).contains(zoom)
                }
                Geom::Pen { points } | Geom::Highlight { points } => {
                    total += points.len();
                    points.len() <= MAX_POINTS && points.iter().all(|p| xy(p.x, p.y))
                }
            };
        if !ok {
            return bad(&format!("shape {} out of range", s.id.0));
        }
    }
    if total > MAX_TOTAL_POINTS {
        return bad("too many points");
    }
    for r in &doc.redactions {
        let (Area::Rect { rect: a, rotation } | Area::Ellipse { rect: a, rotation }) = &r.area;
        let kind_ok = match r.kind {
            RedactKind::Blur { radius } => radius.is_finite() && (1.0..=100.0).contains(&radius),
            RedactKind::Pixelate { block } => (2..=100).contains(&block),
            RedactKind::Solid { .. } => true,
        };
        if crate::edit::apply::check_area(&r.area).is_err() || !rect(a) || !rot(*rotation) || !kind_ok {
            return bad(&format!("redaction {} out of range", r.id.0));
        }
    }
    Ok(())
}

/// ISO 8601 UTC of a time (no time-zone database needed).
fn iso8601(t: std::time::SystemTime) -> String {
    let s = t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (s.div_euclid(86_400), s.rem_euclid(86_400));
    // civil from days (H. Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// The `.rvm` bytes: the JSON entries deflated, the original stored as it is.
pub fn write(doc: &DocState, original: &[u8], original_name: &str, format: Format) -> Result<Vec<u8>, ProjectError> {
    use zip::write::SimpleFileOptions;
    let ext = original_name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .filter(|e| original_format(&format!("original.{e}")) == Some(format))
        .or_else(|| {
            EXTENSIONS
                .iter()
                .find(|(_, f)| *f == format)
                .map(|(e, _)| e.to_string())
        })
        .ok_or_else(|| ProjectError::Unsupported(format!("{} originals", format.name())))?;
    let manifest = Manifest {
        format: FORMAT_ID.into(),
        version: VERSION,
        app: format!("rView {}", env!("CARGO_PKG_VERSION")),
        created: iso8601(std::time::SystemTime::now()),
        original_name: original_name.into(),
        original_len: original.len() as u64,
    };
    let deflate = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(original.len() as u64 >= u32::MAX as u64);
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let io = |e: zip::result::ZipError| corrupt(e.to_string());
    z.start_file("manifest.json", deflate).map_err(io)?;
    z.write_all(&serde_json::to_vec(&manifest).map_err(|e| corrupt(e.to_string()))?)?;
    z.start_file("document.json", deflate).map_err(io)?;
    z.write_all(&serde_json::to_vec(doc).map_err(|e| corrupt(e.to_string()))?)?;
    z.start_file(format!("original.{ext}"), stored).map_err(io)?;
    z.write_all(original)?;
    Ok(z.finish().map_err(io)?.into_inner())
}

#[cfg(test)]
mod tests {
    #[test]
    fn iso_dates() {
        use std::time::{Duration, UNIX_EPOCH};
        assert_eq!(super::iso8601(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        let t = UNIX_EPOCH + Duration::from_secs(1_794_571_445); // 2026-11-13
        assert_eq!(super::iso8601(t), "2026-11-13T12:04:05Z");
        let leap = UNIX_EPOCH + Duration::from_secs(951_782_400); // 2000-02-29
        assert_eq!(super::iso8601(leap), "2000-02-29T00:00:00Z");
    }
}
