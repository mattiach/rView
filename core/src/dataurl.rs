//! Base64 images (RFC 4648) and `data:` URLs (RFC 2397): Paste and Open URL read them, Copy as
//! Base64 writes them. The bytes decide the format; a data URL's media type is only a hint.

use crate::formats::Format;
use base64::Engine;
use base64::engine::{GeneralPurpose, general_purpose};

/// Largest decoded or encoded file.
pub const MAX_BYTES: usize = 256 << 20;

/// Padding optional; the URL-safe alphabet is mapped to this one before decoding.
const ENGINE: GeneralPurpose = GeneralPurpose::new(&base64::alphabet::STANDARD, general_purpose::PAD_INDIFFERENT);

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum DataUrlError {
    #[error("The text is not valid Base64 ({0}).")]
    Invalid(String),
    #[error("Only Base64 data URLs are supported (data:<type>;base64,<data>).")]
    NotBase64,
    #[error("The data is larger than {} MiB.", MAX_BYTES >> 20)]
    TooLarge,
}

fn is_data_url(t: &str) -> bool {
    t.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("data:"))
}

fn alphabet(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'-' | b'_')
}

/// Line breaks and tabs only: a space means prose, not Base64 (data URLs allow spaces too).
fn line_space(b: u8) -> bool {
    matches!(b, b'\r' | b'\n' | b'\t')
}

/// What Paste opens: a `data:` URL, or text made only of Base64 that starts like a known image
/// format (so a word or a sentence is never taken for an image).
pub fn is_image_text(text: &str) -> bool {
    let t = text.trim();
    if is_data_url(t) {
        return true;
    }
    if t.len() < 16 || !t.bytes().all(|b| alphabet(b) || b == b'=' || line_space(b)) {
        return false;
    }
    let head: Vec<u8> = t.bytes().filter(|b| alphabet(*b)).take(64).map(standard).collect();
    let head = &head[..head.len() / 4 * 4];
    ENGINE.decode(head).is_ok_and(|b| Format::sniff(&b).is_some())
}

fn standard(b: u8) -> u8 {
    match b {
        b'-' => b'+',
        b'_' => b'/',
        b => b,
    }
}

/// Whether `chars` Base64 characters decode to at most MAX_BYTES.
fn fits(chars: usize) -> bool {
    chars / 4 * 3 + (chars % 4).saturating_sub(1) <= MAX_BYTES
}

/// The bytes of a data URL or of raw Base64 (whitespace ignored), and the URL's media type.
/// The size is checked before anything is allocated.
pub fn decode(text: &str) -> Result<(Vec<u8>, Option<String>), DataUrlError> {
    let t = text.trim();
    let (mime, payload) = if is_data_url(t) {
        let (meta, data) = t[5..].split_once(',').ok_or(DataUrlError::Invalid("no comma".into()))?;
        let mut parts = meta.split(';').map(str::trim);
        let mime = parts.next().filter(|m| !m.is_empty()).map(str::to_ascii_lowercase);
        if !parts.any(|p| p.eq_ignore_ascii_case("base64")) {
            return Err(DataUrlError::NotBase64);
        }
        (mime, data)
    } else {
        (None, t)
    };
    if !fits(payload.bytes().filter(|b| !b.is_ascii_whitespace()).count()) {
        return Err(DataUrlError::TooLarge);
    }
    let clean: Vec<u8> = payload
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .map(standard)
        .collect();
    let bytes = ENGINE
        .decode(&clean)
        .map_err(|e| DataUrlError::Invalid(e.to_string()))?;
    if bytes.is_empty() {
        return Err(DataUrlError::Invalid("no data".into()));
    }
    Ok((bytes, mime))
}

/// Media type of a format; `application/octet-stream` for those without a registered one.
pub fn mime(f: Format) -> &'static str {
    match f {
        Format::Jpeg => "image/jpeg",
        Format::Png => "image/png",
        Format::Gif => "image/gif",
        Format::Webp => "image/webp",
        Format::Tiff => "image/tiff",
        Format::Bmp => "image/bmp",
        Format::Ico | Format::Cur => "image/x-icon",
        Format::Svg => "image/svg+xml",
        Format::Avif => "image/avif",
        Format::Heif => "image/heif",
        Format::Jxl => "image/jxl",
        Format::Jxr => "image/jxr",
        Format::Exr => "image/x-exr",
        Format::Tga => "image/x-tga",
        Format::Qoi => "image/qoi",
        _ => "application/octet-stream",
    }
}

/// `data:<type>;base64,<data>` of a file.
pub fn encode(bytes: &[u8], format: Format) -> Result<String, DataUrlError> {
    if bytes.len() > MAX_BYTES {
        return Err(DataUrlError::TooLarge);
    }
    Ok(format!("data:{};base64,{}", mime(format), ENGINE.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_4648_vectors() {
        for (plain, coded) in [
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(decode(coded).unwrap().0, plain.as_bytes(), "{coded}");
            assert_eq!(
                decode(coded.trim_end_matches('=')).unwrap().0,
                plain.as_bytes(),
                "unpadded"
            );
            assert_eq!(ENGINE.encode(plain), coded);
        }
    }

    #[test]
    fn data_urls_whitespace_and_alphabets() {
        let bytes = [0xFBu8, 0xFF, 0xBF, 1, 2];
        let std = ENGINE.encode(bytes); // "+/+/AQI="
        let safe = std.replace('+', "-").replace('/', "_");
        for text in [
            format!("data:image/png;base64,{std}"),
            format!("  DATA:Image/PNG ; charset=x ; Base64 ,{}\r\n", std),
            format!("data:;base64,{safe}"),
            format!("{}\r\n{}", &std[..3], &std[3..]),
        ] {
            let (b, _) = decode(&text).unwrap_or_else(|e| panic!("{text:?}: {e}"));
            assert_eq!(b, bytes, "{text:?}");
        }
        assert_eq!(
            decode(&format!("data:image/png;base64,{std}")).unwrap().1.as_deref(),
            Some("image/png")
        );
        assert_eq!(decode(&format!("data:;base64,{std}")).unwrap().1, None);
    }

    #[test]
    fn invalid_input() {
        assert_eq!(decode("data:image/svg+xml,<svg/>"), Err(DataUrlError::NotBase64));
        assert!(matches!(decode("data:image/png;base64"), Err(DataUrlError::Invalid(_))));
        assert!(matches!(decode("Zm9v!mFy"), Err(DataUrlError::Invalid(_))));
        assert!(
            matches!(decode("Zm9vY"), Err(DataUrlError::Invalid(_))),
            "one char too many"
        );
        assert!(
            matches!(decode("Zg=a"), Err(DataUrlError::Invalid(_))),
            "padding inside"
        );
        assert!(
            matches!(decode("data:;base64,"), Err(DataUrlError::Invalid(_))),
            "empty"
        );
    }

    #[test]
    fn size_checked_before_decoding() {
        let chars = MAX_BYTES / 3 * 4; // MAX_BYTES = 3k + 1: two more characters (one byte) still fit
        assert!(fits(chars) && fits(chars + 2) && !fits(chars + 4) && !fits(chars + 3));
        assert_eq!(
            encode(&vec![0; MAX_BYTES + 1], Format::Png),
            Err(DataUrlError::TooLarge)
        );
    }

    #[test]
    fn what_paste_takes_for_an_image() {
        let png = encode(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR", Format::Png).unwrap();
        assert!(png.starts_with("data:image/png;base64,iVBORw0KGgo"));
        assert!(is_image_text(&png));
        let raw = png.split_once(',').unwrap().1;
        assert!(is_image_text(raw) && is_image_text(&format!("{}\n{}", &raw[..10], &raw[10..])));
        assert!(
            is_image_text("data:text/plain;base64,SGVsbG8="),
            "a data URL always: the opener reports it"
        );
        for text in [
            "Supercalifragilisticexpialidocious",
            "hello world, plain text",
            "Zm9vYmFyZm9vYmFy",
            "",
        ] {
            assert!(!is_image_text(text), "{text:?}");
        }
    }
}
