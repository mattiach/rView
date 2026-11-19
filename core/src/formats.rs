//! Known formats, extensions, magic-byte sniffing and file-dialog filters.

use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Format {
    Jpeg,
    Png,
    Gif,
    Webp,
    Tiff,
    Bmp,
    Ico,
    Cur,
    Icns,
    Tga,
    Pnm,
    Qoi,
    Hdr,
    Exr,
    Dds,
    Jxl,
    Svg,
    Heif,
    Avif,
    Jxr,
    Raw,
    /// rView Markup project: shown as its rendered document.
    Project,
    Wbmp,
    Xbm,
    Xpm,
}

/// (extension, format). Lower case, without dot.
pub const EXTENSIONS: &[(&str, Format)] = &[
    ("jpg", Format::Jpeg),
    ("jpeg", Format::Jpeg),
    ("jpe", Format::Jpeg),
    ("jfi", Format::Jpeg),
    ("jfif", Format::Jpeg),
    ("png", Format::Png),
    ("gif", Format::Gif),
    ("webp", Format::Webp),
    ("tif", Format::Tiff),
    ("tiff", Format::Tiff),
    ("bmp", Format::Bmp),
    ("ico", Format::Ico),
    ("cur", Format::Cur),
    ("icns", Format::Icns),
    ("tga", Format::Tga),
    ("pbm", Format::Pnm),
    ("pgm", Format::Pnm),
    ("ppm", Format::Pnm),
    ("pam", Format::Pnm),
    ("qoi", Format::Qoi),
    ("hdr", Format::Hdr),
    ("exr", Format::Exr),
    ("dds", Format::Dds),
    ("jxl", Format::Jxl),
    ("svg", Format::Svg),
    ("svgz", Format::Svg),
    ("heic", Format::Heif),
    ("heics", Format::Heif),
    ("heif", Format::Heif),
    ("heifs", Format::Heif),
    ("hif", Format::Heif),
    ("avif", Format::Avif),
    ("jxr", Format::Jxr),
    ("wdp", Format::Jxr),
    ("dng", Format::Raw),
    ("cr2", Format::Raw),
    ("cr3", Format::Raw),
    ("nef", Format::Raw),
    ("nrw", Format::Raw),
    ("arw", Format::Raw),
    ("orf", Format::Raw),
    ("rw2", Format::Raw),
    ("raf", Format::Raw),
    ("pef", Format::Raw),
    ("srw", Format::Raw),
    ("rvm", Format::Project),
    ("wbmp", Format::Wbmp),
    ("xbm", Format::Xbm),
    ("xpm", Format::Xpm),
];

impl Format {
    pub fn from_ext(ext: &str) -> Option<Format> {
        EXTENSIONS
            .iter()
            .find(|(e, _)| e.eq_ignore_ascii_case(ext))
            .map(|&(_, f)| f)
    }

    pub fn from_path(path: &Path) -> Option<Format> {
        Format::from_ext(path.extension()?.to_str()?)
    }

    /// Detect from magic bytes. `None` if unknown (caller falls back to the extension).
    pub fn sniff(b: &[u8]) -> Option<Format> {
        let starts = |m: &[u8]| b.starts_with(m);
        let ftyp_brand = (b.len() >= 12 && &b[4..8] == b"ftyp").then(|| &b[8..12]);
        Some(match () {
            _ if starts(&[0xFF, 0xD8, 0xFF]) => Format::Jpeg,
            _ if starts(&[b'P', b'K', 3, 4]) => Format::Project,
            _ if starts(b"\x89PNG\r\n\x1a\n") => Format::Png,
            _ if starts(b"GIF87a") || starts(b"GIF89a") => Format::Gif,
            _ if starts(b"RIFF") && b.get(8..12) == Some(b"WEBP") => Format::Webp,
            _ if starts(b"II*\0") || starts(b"MM\0*") => Format::Tiff,
            _ if starts(b"BM") => Format::Bmp,
            _ if starts(&[0, 0, 1, 0]) => Format::Ico,
            _ if starts(&[0, 0, 2, 0]) => Format::Cur,
            _ if starts(b"icns") => Format::Icns,
            _ if starts(b"qoif") => Format::Qoi,
            _ if b.len() > 2 && b[0] == b'P' && (b'1'..=b'7').contains(&b[1]) && b[2].is_ascii_whitespace() => {
                Format::Pnm
            }
            _ if starts(b"#?RADIANCE") || starts(b"#?RGBE") => Format::Hdr,
            _ if starts(&[0x76, 0x2F, 0x31, 0x01]) => Format::Exr,
            _ if starts(b"DDS ") => Format::Dds,
            _ if starts(&[0xFF, 0x0A]) || starts(b"\0\0\0\x0CJXL \r\n\x87\n") => Format::Jxl,
            _ if starts(&[0x1F, 0x8B]) => Format::Svg, // gzip: only svgz is plausible here
            _ if starts(b"II\xBC") => Format::Jxr,
            _ if matches!(ftyp_brand, Some(b"avif" | b"avis")) => Format::Avif,
            _ if matches!(
                ftyp_brand,
                Some(b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" | b"mif1" | b"msf1")
            ) =>
            {
                Format::Heif
            }
            _ if matches!(ftyp_brand, Some(b"crx ")) => Format::Raw,
            _ if looks_like_svg(b) => Format::Svg,
            // text formats (WBMP has no signature: by extension only)
            _ if starts(b"/* XPM */") || starts(b"! XPM2") => Format::Xpm,
            _ if starts(b"#define") && b.windows(6).take(256).any(|w| w == b"_width") => Format::Xbm,
            _ => return None,
        })
    }

    /// Display name for Info / errors.
    pub fn name(self) -> &'static str {
        match self {
            Format::Jpeg => "JPEG",
            Format::Png => "PNG",
            Format::Gif => "GIF",
            Format::Webp => "WebP",
            Format::Tiff => "TIFF",
            Format::Bmp => "BMP",
            Format::Ico => "ICO",
            Format::Cur => "CUR",
            Format::Icns => "ICNS",
            Format::Tga => "TGA",
            Format::Pnm => "PNM",
            Format::Qoi => "QOI",
            Format::Hdr => "Radiance HDR",
            Format::Exr => "OpenEXR",
            Format::Dds => "DDS",
            Format::Jxl => "JPEG XL",
            Format::Svg => "SVG",
            Format::Heif => "HEIF",
            Format::Avif => "AVIF",
            Format::Jxr => "JPEG XR",
            Format::Raw => "RAW",
            Format::Project => "rView Project",
            Format::Wbmp => "WBMP",
            Format::Xbm => "XBM",
            Format::Xpm => "XPM",
        }
    }
}

fn looks_like_svg(b: &[u8]) -> bool {
    let head = &b[..b.len().min(1024)];
    let s = String::from_utf8_lossy(head);
    let s = s.trim_start_matches('\u{feff}').trim_start();
    (s.starts_with("<?xml") || s.starts_with("<svg") || s.starts_with("<!--")) && s.contains("<svg")
}

/// True if the path has a known image extension and is not an AppleDouble `._` file.
pub fn is_supported_path(path: &Path) -> bool {
    let hidden_apple = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("._"));
    !hidden_apple && Format::from_path(path).is_some()
}

/// Extensions for the Open dialog filter ("Supported Images").
pub fn dialog_extensions() -> Vec<&'static str> {
    EXTENSIONS.iter().map(|&(e, _)| e).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_case_insensitive_and_apple_double() {
        assert_eq!(Format::from_ext("JPG"), Some(Format::Jpeg));
        assert!(is_supported_path(Path::new("C:/x/a.PNG")));
        assert!(!is_supported_path(Path::new("C:/x/._a.png")));
        assert!(!is_supported_path(Path::new("C:/x/a.pdf")));
    }

    /// dist/win/rview.iss registers every extension and has the crate's version.
    #[test]
    fn installer_lists_every_extension() {
        let iss = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../dist/win/rview.iss")).unwrap();
        for (e, f) in EXTENSIONS {
            let prog = if *f == Format::Project {
                "rView.Project"
            } else {
                "rView.Image"
            };
            let line =
                format!(r#"Subkey: "Software\Classes\.{e}\OpenWithProgids"; ValueType: string; ValueName: "{prog}""#);
            assert!(iss.contains(&line), "rview.iss lacks .{e}");
        }
        assert!(iss.contains(&format!("#define AppVersion \"{}\"", env!("CARGO_PKG_VERSION"))));
    }

    #[test]
    fn sniff_basic() {
        assert_eq!(Format::sniff(b"\x89PNG\r\n\x1a\n...."), Some(Format::Png));
        assert_eq!(Format::sniff(b"\0\0\0\x18ftypheic\0\0\0\0"), Some(Format::Heif));
        assert_eq!(Format::sniff(b"\0\0\0\x18ftypavif\0\0\0\0"), Some(Format::Avif));
        assert_eq!(Format::sniff(b"<?xml version='1.0'?><svg>"), Some(Format::Svg));
        assert_eq!(Format::sniff(b"hello"), None);
    }
}
