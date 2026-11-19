//! Open URL: dialog, download on a worker with progress and cancel, temporary file
//! `%TEMP%\rView.XXXXXX.<ext>`.

use rv_core::formats::{EXTENSIONS, Format};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Larger downloads are refused (a temp file is not the place for them).
pub const MAX_BYTES: u64 = 1 << 30;

pub struct OpenUrl {
    pub text: String,
    first: bool,
}

impl OpenUrl {
    pub fn new(text: String) -> OpenUrl {
        OpenUrl { text, first: true }
    }

    pub(super) fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        ui.label(crate::i18n::t("openurl-label"));
        let all = (0, self.text.chars().count());
        super::text_field(ui, &mut self.text, &mut self.first, all)
    }
}

pub struct Progress {
    pub url: String,
    pub received: u64,
    pub total: Option<u64>,
}

impl Progress {
    pub(super) fn ui(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new(&self.url).small());
        let mib = |b: u64| b as f64 / (1 << 20) as f64;
        match self.total {
            Some(t) if t > 0 => {
                ui.add(egui::ProgressBar::new(self.received as f32 / t as f32).text(format!(
                    "{:.1} / {:.1} MiB",
                    mib(self.received),
                    mib(t)
                )));
            }
            _ => {
                ui.add(
                    egui::ProgressBar::new(0.0)
                        .animate(true)
                        .text(format!("{:.1} MiB", mib(self.received))),
                );
            }
        }
    }
}

/// An http(s) URL (also what Paste accepts as a URL).
pub fn is_url(s: &str) -> bool {
    let s = s.trim();
    let lower = s.get(..8).unwrap_or(s).to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://")) && !s.contains(char::is_whitespace)
}

/// Extension of the temporary file: the URL's own if it is a known image extension, else from
/// the content (magic bytes), else from the Content-Type; "tmp" if nothing matches (the loader
/// then reports the format as unsupported).
pub fn extension(url: &str, content_type: Option<&str>, head: &[u8]) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let from_url = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase())
        .filter(|e| Format::from_ext(e).is_some());
    let first_ext = |f: Format| EXTENSIONS.iter().find(|(_, g)| *g == f).map(|(e, _)| e.to_string());
    let from_bytes = Format::sniff(head).and_then(|f| match f {
        Format::Svg if head.starts_with(&[0x1F, 0x8B]) => Some("svgz".into()),
        f => first_ext(f),
    });
    let from_type = content_type.and_then(|t| {
        let sub = t.split(';').next()?.trim().strip_prefix("image/")?.to_ascii_lowercase();
        let e = match sub.as_str() {
            "svg+xml" => "svg",
            "x-icon" | "vnd.microsoft.icon" => "ico",
            other => other,
        };
        Format::from_ext(e).map(|_| e.to_string())
    });
    from_url.or(from_bytes).or(from_type).unwrap_or_else(|| "tmp".into())
}

/// `%TEMP%\rView.XXXXXX.<ext>` that doesn't exist yet.
pub fn temp_path(dir: &Path, ext: &str) -> PathBuf {
    let mut rng = rv_core::folder::Rng::from_entropy();
    loop {
        let tag: String = (0..6)
            .map(|_| {
                let c = (rng.next_u64() % 36) as u8;
                (if c < 10 { b'0' + c } else { b'a' + c - 10 }) as char
            })
            .collect();
        let p = dir.join(format!("rView.{tag}.{ext}"));
        if !p.exists() {
            return p;
        }
    }
}

/// HTTP client of Open URL and the update check: Windows TLS (SChannel) with the system's root
/// certificates, the system proxy, a generic user agent.
pub fn agent() -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    ureq::Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::NativeTls)
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .timeout_connect(Some(std::time::Duration::from_secs(20)))
        .user_agent(concat!("rView/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Download `url` into a new temporary file. `progress(received, total)` is called while
/// reading; `cancel` is checked between chunks (a cancelled download leaves no file).
pub fn download(url: &str, cancel: &AtomicBool, mut progress: impl FnMut(u64, Option<u64>)) -> Result<PathBuf, String> {
    let resp = agent().get(url).call().map_err(|e| e.to_string())?;
    let header = |n: &str| resp.headers().get(n).and_then(|v| v.to_str().ok()).map(str::to_string);
    let content_type = header("content-type");
    let total = header("content-length").and_then(|v| v.parse::<u64>().ok());
    if total.is_some_and(|t| t > MAX_BYTES) {
        return Err(crate::i18n::tf(
            "download-too-large",
            &[("mib", &(MAX_BYTES >> 20).to_string())],
        ));
    }
    let mut reader = resp.into_body().into_with_config().limit(MAX_BYTES).reader();
    let mut data = Vec::with_capacity(total.unwrap_or(0).min(64 << 20) as usize);
    let mut buf = vec![0u8; 256 << 10];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        progress(data.len() as u64, total);
    }
    let ext = extension(url, content_type.as_deref(), &data[..data.len().min(1024)]);
    save_temp(&data, &ext)
}

/// `data` written to a new temporary file (removed again if the write fails).
pub fn save_temp(data: &[u8], ext: &str) -> Result<PathBuf, String> {
    let path = temp_path(&std::env::temp_dir(), ext);
    let mut f = std::fs::File::create_new(&path).map_err(|e| e.to_string())?;
    f.write_all(data).map_err(|e| {
        let _ = std::fs::remove_file(&path);
        e.to_string()
    })?;
    Ok(path)
}

/// Base64 or a data URL (Paste, Open URL) decoded into a temporary file with the extension of
/// its content; an unknown content is refused, nothing is written.
pub fn save_base64(text: &str) -> Result<PathBuf, String> {
    let (data, mime) = rv_core::dataurl::decode(text).map_err(|e| e.to_string())?;
    let ext = extension("", mime.as_deref(), &data[..data.len().min(1024)]);
    if ext == "tmp" {
        return Err(crate::i18n::t("base64-not-image"));
    }
    save_temp(&data, &ext)
}

/// The start of a long text, for a message.
pub fn excerpt(s: &str) -> String {
    match s.char_indices().nth(80) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_extensions_temp_names() {
        assert!(is_url("https://example.com/a.png"));
        assert!(is_url("  HTTP://x/y  "));
        assert!(!is_url("ftp://x/y") && !is_url("C:\\a.png") && !is_url("https://a b"));
        let png = b"\x89PNG\r\n\x1a\n....";
        assert_eq!(
            extension("https://x/a.JPG?w=100", None, png),
            "jpg",
            "URL extension wins"
        );
        assert_eq!(
            extension("https://x/img?id=3", Some("image/webp"), png),
            "png",
            "then the bytes"
        );
        assert_eq!(
            extension("https://x/img", Some("image/webp; q=1"), b"????"),
            "webp",
            "then the type"
        );
        assert_eq!(extension("https://x/img", Some("image/svg+xml"), b""), "svg");
        assert_eq!(extension("https://x/page.html", Some("text/html"), b"<html>"), "tmp");
        let dir = std::env::temp_dir();
        let p = temp_path(&dir, "png");
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("rView.") && name.ends_with(".png") && name.len() == "rView.xxxxxx.png".len());
        assert!(!p.exists());
        assert_eq!(excerpt("short"), "short");
        assert_eq!(excerpt(&"é".repeat(100)).chars().count(), 81);
    }

    #[test]
    fn base64_to_temp_file() {
        let mut png = Vec::new();
        image::RgbaImage::new(2, 2)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        // the content decides: a PNG labelled as JPEG is saved as .png
        let url = rv_core::dataurl::encode(&png, Format::Png)
            .unwrap()
            .replace("image/png", "image/jpeg");
        let p = save_base64(&url).unwrap();
        assert_eq!(p.extension().unwrap(), "png");
        assert_eq!(std::fs::read(&p).unwrap(), png);
        std::fs::remove_file(&p).unwrap();
        // not an image: refused, and invalid Base64 reports why
        assert!(save_base64("data:text/plain;base64,SGVsbG8=").is_err());
        assert!(
            save_base64("data:image/png;base64,***")
                .unwrap_err()
                .contains("not valid Base64")
        );
    }
}
