//! WBMP, XBM and XPM. Small formats without a Rust decoder
//! crate: parsed here, bounded like every other decoder (size checked from the header before any
//! allocation, never a panic on malformed input). Output: RGBA8, sRGB, no ICC/EXIF.

use super::{DecodeError, DecodedImage, Pixels, SizeU, SourceColor, SourceMeta, check_size, corrupt};
use crate::formats::Format;

fn image(format: Format, w: u32, h: u32, rgba: Vec<u8>) -> DecodedImage {
    DecodedImage {
        format,
        size: SizeU { w, h },
        pixels: Pixels::Rgba8(rgba),
        color: SourceColor {
            icc: None,
            assumed: true,
            hdr: false,
        },
        meta: SourceMeta::default(),
    }
}

// ---------------------------------------------------------------- WBMP

/// Multi-byte integer (7 bits per byte, high bit = more).
fn mbi(b: &[u8], at: &mut usize) -> Option<u32> {
    let mut v: u32 = 0;
    for _ in 0..5 {
        let c = *b.get(*at)?;
        *at += 1;
        v = v.checked_mul(128)?.checked_add((c & 0x7F) as u32)?;
        if c & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// (width, height, data offset) of a type-0 WBMP.
fn wbmp_header(b: &[u8]) -> Option<(u32, u32, usize)> {
    let mut at = 0;
    if mbi(b, &mut at)? != 0 {
        return None; // only type 0 (B/W, no compression) exists in practice
    }
    // fixed header: extension headers are not used by type 0
    if *b.get(at)? & 0x80 != 0 {
        return None;
    }
    at += 1;
    let w = mbi(b, &mut at)?;
    let h = mbi(b, &mut at)?;
    Some((w, h, at))
}

pub fn probe(bytes: &[u8], format: Format) -> Option<(u32, u32)> {
    match format {
        Format::Wbmp => wbmp_header(bytes).map(|(w, h, _)| (w, h)),
        Format::Xbm => xbm_size(&String::from_utf8_lossy(&bytes[..bytes.len().min(4096)])),
        Format::Xpm => xpm_strings(bytes)
            .ok()?
            .first()
            .and_then(|s| xpm_values(s))
            .map(|v| (v.0, v.1)),
        _ => None,
    }
}

fn wbmp(b: &[u8]) -> Result<DecodedImage, DecodeError> {
    let (w, h, at) = wbmp_header(b).ok_or_else(|| corrupt("Not a WBMP image"))?;
    check_size(w, h)?;
    let row = (w as usize).div_ceil(8);
    let data = b
        .get(at..)
        .filter(|d| d.len() >= row * h as usize)
        .ok_or_else(|| corrupt("WBMP data is truncated"))?;
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h as usize {
        for x in 0..w as usize {
            // 1 = white, 0 = black
            let bit = data[y * row + x / 8] >> (7 - x % 8) & 1;
            let v = if bit == 1 { 255 } else { 0 };
            out.extend_from_slice(&[v, v, v, 255]);
        }
    }
    Ok(image(Format::Wbmp, w, h, out))
}

// ---------------------------------------------------------------- XBM

fn define(text: &str, suffix: &str) -> Option<u32> {
    text.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        (it.next()? == "#define" && it.next()?.ends_with(suffix)).then_some(())?;
        it.next()?.parse().ok()
    })
}

fn xbm_size(text: &str) -> Option<(u32, u32)> {
    Some((define(text, "_width")?, define(text, "_height")?))
}

fn xbm(b: &[u8]) -> Result<DecodedImage, DecodeError> {
    let text = std::str::from_utf8(b).map_err(|_| corrupt("XBM is not text"))?;
    let (w, h) = xbm_size(text).ok_or_else(|| corrupt("XBM without width/height"))?;
    check_size(w, h)?;
    let body = text
        .find('{')
        .and_then(|a| text[a + 1..].find('}').map(|z| &text[a + 1..a + 1 + z]))
        .ok_or_else(|| corrupt("XBM without data"))?;
    let row = (w as usize).div_ceil(8);
    let need = row * h as usize;
    let mut bytes = Vec::with_capacity(need.min(1 << 24));
    for tok in body.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        let v = tok
            .strip_prefix("0x")
            .or_else(|| tok.strip_prefix("0X"))
            .map(|h| u16::from_str_radix(h, 16))
            .unwrap_or_else(|| tok.parse())
            .map_err(|_| corrupt(format!("XBM value {tok:?}")))?;
        // X10 files use 16-bit values: two bytes, low first
        if v > 0xFF {
            bytes.extend_from_slice(&[v as u8, (v >> 8) as u8]);
        } else {
            bytes.push(v as u8);
        }
        if bytes.len() >= need {
            break;
        }
    }
    if bytes.len() < need {
        return Err(corrupt("XBM data is truncated"));
    }
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h as usize {
        for x in 0..w as usize {
            // least significant bit first; 1 = foreground (black) on white
            let bit = bytes[y * row + x / 8] >> (x % 8) & 1;
            let v = if bit == 1 { 0 } else { 255 };
            out.extend_from_slice(&[v, v, v, 255]);
        }
    }
    Ok(image(Format::Xbm, w, h, out))
}

// ---------------------------------------------------------------- XPM

/// The quoted strings of an XPM3 file (C syntax), or the lines of an XPM2 one.
fn xpm_strings(b: &[u8]) -> Result<Vec<String>, DecodeError> {
    let text = std::str::from_utf8(b).map_err(|_| corrupt("XPM is not text"))?;
    if let Some(rest) = text.strip_prefix("! XPM2") {
        return Ok(rest.lines().skip(1).map(str::to_string).collect());
    }
    if !text.trim_start().starts_with("/* XPM */") {
        return Err(corrupt("Not an XPM image"));
    }
    let mut out = Vec::new();
    let (mut chars, mut in_comment) = (text.chars().peekable(), false);
    while let Some(c) = chars.next() {
        match c {
            '/' if !in_comment && chars.peek() == Some(&'*') => {
                chars.next();
                in_comment = true;
            }
            '*' if in_comment && chars.peek() == Some(&'/') => {
                chars.next();
                in_comment = false;
            }
            '"' if !in_comment => {
                let mut s = String::new();
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => s.extend(chars.next()),
                        Some(c) => s.push(c),
                        None => return Err(corrupt("XPM string not closed")),
                    }
                }
                out.push(s);
            }
            _ => {}
        }
    }
    Ok(out)
}

/// width height ncolors chars-per-pixel.
fn xpm_values(s: &str) -> Option<(u32, u32, usize, usize)> {
    let v: Vec<u64> = s.split_whitespace().take(4).filter_map(|t| t.parse().ok()).collect();
    let [w, h, n, cpp] = v[..] else { return None };
    ((1..=8).contains(&cpp) && (1..=1 << 20).contains(&n)).then_some((
        w.try_into().ok()?,
        h.try_into().ok()?,
        n as usize,
        cpp as usize,
    ))
}

/// A colour of the `c` key: #rgb / #rrggbb / #rrrrggggbbbb, None, or a few X11 names.
fn xpm_color(spec: &str) -> Option<[u8; 4]> {
    let s = spec.trim();
    if s.eq_ignore_ascii_case("none") {
        return Some([0, 0, 0, 0]);
    }
    if let Some(h) = s.strip_prefix('#') {
        let n = h.len() / 3;
        if h.len() % 3 != 0 || !(1..=4).contains(&n) {
            return None;
        }
        let ch = |i: usize| u32::from_str_radix(&h[i * n..(i + 1) * n], 16).ok();
        let max = (1u32 << (4 * n)) - 1;
        let to8 = |v: u32| ((v * 255 + max / 2) / max) as u8;
        return Some([to8(ch(0)?), to8(ch(1)?), to8(ch(2)?), 255]);
    }
    let named = match s.to_ascii_lowercase().replace(' ', "").as_str() {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 255, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "cyan" => [0, 255, 255],
        "magenta" => [255, 0, 255],
        "gray" | "grey" => [190, 190, 190],
        "darkgray" | "darkgrey" => [169, 169, 169],
        "lightgray" | "lightgrey" => [211, 211, 211],
        "orange" => [255, 165, 0],
        "brown" => [165, 42, 42],
        "purple" => [160, 32, 240],
        "navy" => [0, 0, 128],
        _ => return None,
    };
    Some([named[0], named[1], named[2], 255])
}

fn xpm(b: &[u8]) -> Result<DecodedImage, DecodeError> {
    let strings = xpm_strings(b)?;
    let (w, h, n, cpp) = strings
        .first()
        .and_then(|s| xpm_values(s))
        .ok_or_else(|| corrupt("XPM without a valid header"))?;
    check_size(w, h)?;
    if strings.len() < 1 + n + h as usize {
        return Err(corrupt("XPM is truncated"));
    }
    let mut colors = std::collections::HashMap::with_capacity(n);
    for line in &strings[1..1 + n] {
        let key: String = line.chars().take(cpp).collect();
        let rest: String = line.chars().skip(cpp).collect();
        // "<chars> c #rrggbb" (other keys: m, g, g4, s); the c value may be several words
        let words: Vec<&str> = rest.split_whitespace().collect();
        let mut col = None;
        let mut i = 0;
        while i < words.len() {
            if matches!(words[i], "c" | "m" | "g" | "g4" | "s") {
                let k = words[i];
                let mut j = i + 1;
                while j < words.len() && !matches!(words[j], "c" | "m" | "g" | "g4" | "s") {
                    j += 1;
                }
                let v = words[i + 1..j].join(" ");
                if k == "c" || col.is_none() && k != "s" {
                    col = xpm_color(&v).or(col);
                }
                i = j;
            } else {
                i += 1;
            }
        }
        // an unknown colour name: shown as black rather than refusing the file
        colors.insert(key, col.unwrap_or([0, 0, 0, 255]));
    }
    let rows = &strings[1 + n..1 + n + h as usize];
    // the rows prove the size before anything is allocated for it
    if rows.iter().any(|r| r.chars().count() < w as usize * cpp) {
        return Err(corrupt("XPM row is too short"));
    }
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for line in rows {
        let px: Vec<char> = line.chars().collect();
        for x in 0..w as usize {
            let key: String = px[x * cpp..(x + 1) * cpp].iter().collect();
            let c = colors
                .get(&key)
                .ok_or_else(|| corrupt("XPM pixel with an undefined colour"))?;
            out.extend_from_slice(c);
        }
    }
    Ok(image(Format::Xpm, w, h, out))
}

pub fn decode(bytes: &[u8], format: Format) -> Result<DecodedImage, DecodeError> {
    match format {
        Format::Wbmp => wbmp(bytes),
        Format::Xbm => xbm(bytes),
        Format::Xpm => xpm(bytes),
        _ => Err(DecodeError::Unsupported),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(d: &DecodedImage) -> Vec<u8> {
        crate::color::rgba8(d.pixels.clone())
    }

    #[test]
    fn wbmp_decodes() {
        // 10×2: row 0 = 1010101010, row 1 = all black
        let b = [0, 0, 10, 2, 0b1010_1010, 0b1000_0000, 0, 0];
        let d = wbmp(&b).unwrap();
        assert_eq!((d.size.w, d.size.h), (10, 2));
        let p = px(&d);
        assert_eq!(&p[..8], &[255, 255, 255, 255, 0, 0, 0, 255]);
        assert_eq!(&p[40..44], &[0, 0, 0, 255]);
        assert!(wbmp(&b[..6]).is_err(), "truncated");
        assert!(wbmp(&[0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F, 1]).is_err(), "too large");
        assert_eq!(probe(&b, Format::Wbmp), Some((10, 2)));
    }

    #[test]
    fn xbm_decodes() {
        let t =
            b"#define t_width 9\n#define t_height 2\nstatic unsigned char t_bits[] = {\n 0x01, 0x01, 0xff, 0x00 };\n";
        let d = xbm(t).unwrap();
        assert_eq!((d.size.w, d.size.h), (9, 2));
        let p = px(&d);
        assert_eq!(&p[..4], &[0, 0, 0, 255], "bit 0 of the first byte = first pixel, black");
        assert_eq!(&p[4..8], &[255, 255, 255, 255]);
        assert_eq!(&p[32..36], &[0, 0, 0, 255], "pixel 8 = bit 0 of the second byte");
        assert!(xbm(b"#define a_width 8\n#define a_height 9\nstatic char a[] = { 0x00 };").is_err());
        assert_eq!(Format::sniff(t), Some(Format::Xbm));
    }

    /// Mutated inputs never panic (a short fuzz run per format, deterministic).
    #[test]
    fn mutations_never_panic() {
        let seeds: [(&[u8], Format); 3] = [
            (&[0, 0, 10, 2, 0b1010_1010, 0b1000_0000, 0, 0], Format::Wbmp),
            (
                b"#define t_width 9\n#define t_height 2\nstatic char t[] = { 0x01, 0x01, 0xff, 0x00 };",
                Format::Xbm,
            ),
            (
                b"/* XPM */ \"3 2 3 1\", \". c None\", \"# c #FF0000\", \"o c gray\", \".#o\", \"oo#\"",
                Format::Xpm,
            ),
        ];
        let mut s = 0x9E37_79B9u32;
        let mut rnd = || {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            s as usize
        };
        for (seed, f) in seeds {
            for _ in 0..20_000 {
                let mut v = seed.to_vec();
                for _ in 0..1 + rnd() % 4 {
                    let i = rnd() % v.len().max(1);
                    match rnd() % 3 {
                        0 if !v.is_empty() => v[i] = rnd() as u8,
                        1 if !v.is_empty() => v.truncate(i),
                        _ => v.insert(i.min(v.len()), b"0123456789 ,\"#x"[rnd() % 15]),
                    }
                }
                let _ = decode(&v, f);
                let _ = probe(&v, f);
            }
        }
    }

    #[test]
    fn xpm_decodes() {
        let t = b"/* XPM */\nstatic char *x[] = {\n/* comment */\n\"3 2 3 1\",\n\". c None\",\n\"# c #FF0000\",\n\"o c dark gray\",\n\".#o\",\n\"oo#\"\n};\n";
        let d = xpm(t).unwrap();
        assert_eq!((d.size.w, d.size.h), (3, 2));
        let p = px(&d);
        assert_eq!(&p[..12], &[0, 0, 0, 0, 255, 0, 0, 255, 169, 169, 169, 255]);
        assert_eq!(Format::sniff(t), Some(Format::Xpm));
        assert!(
            xpm(b"/* XPM */ \"2 1 1 1\", \". c #fff\", \".\"").is_err(),
            "row too short"
        );
        assert!(
            xpm(b"/* XPM */ \"1 1 1 1\", \". c #fff\", \"x\"").is_err(),
            "undefined colour"
        );
        assert!(xpm(b"/* XPM */ \"99999 99999 1 1\"").is_err());
    }
}
