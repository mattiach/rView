//! Show File Info: rows computed by `rows`, drawn in a non-modal window that follows the
//! file on screen while it is open.

use crate::i18n::{t, tf};
use rv_core::loader::ImageInfo;
use std::path::Path;
use std::time::SystemTime;

/// What the Info rows are made of.
pub struct Facts<'a> {
    pub path: &'a Path,
    /// Size declared by the file, before view rotations.
    pub size: (u32, u32),
    pub info: &'a ImageInfo,
    /// Frames of an animation, once known.
    pub frames: Option<usize>,
    /// File size and modification time (None if the file can't be read any more).
    pub file: Option<(u64, SystemTime)>,
}

/// Locale-dependent pieces, so the rows can be tested without the user's settings.
pub struct Locale {
    pub decimal: String,
    pub thousands: String,
    /// Digit groups from the right, the last one repeating ("3;0" → [3]; Indian "3;2;0" → [3, 2]).
    pub grouping: Vec<usize>,
    pub date_time: fn(SystemTime) -> String,
}

impl Locale {
    pub fn user() -> Locale {
        let (decimal, thousands, grouping) = crate::platform::number_format();
        Locale {
            decimal,
            thousands,
            grouping: parse_grouping(&grouping),
            date_time: crate::platform::local_date_time,
        }
    }
}

/// `LOCALE_SGROUPING` ("3;0", "3;2;0", "3"): the group sizes from the right; the last one repeats
/// when the string ends with ";0", otherwise the digits left of the groups stay together.
pub fn parse_grouping(s: &str) -> Vec<usize> {
    let mut g: Vec<usize> = s.split(';').filter_map(|p| p.trim().parse().ok()).collect();
    match g.last() {
        Some(0) => {
            g.pop();
        }
        Some(_) => g.push(usize::MAX),
        None => g.push(3),
    }
    g.retain(|n| *n > 0);
    if g.is_empty() { vec![3] } else { g }
}

/// 1234567 → "1,234,567" with the locale's separator and grouping ("12,34,567" in India).
pub fn grouped(n: u64, sep: &str, grouping: &[usize]) -> String {
    let digits: Vec<char> = n.to_string().chars().collect();
    let mut groups = Vec::new();
    let mut end = digits.len();
    let mut i = 0;
    while end > 0 {
        let size = grouping.get(i).or(grouping.last()).copied().unwrap_or(3).min(end);
        groups.push(digits[end - size..end].iter().collect::<String>());
        end -= size;
        i += 1;
    }
    groups.reverse();
    groups.join(sep)
}

/// "1.18 MiB (1,234,567 bytes)": binary units with two decimals.
pub fn size_text(bytes: u64, l: &Locale) -> String {
    let units = ["bytes", "KiB", "MiB", "GiB", "TiB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < units.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    let exact = tf("info-bytes", &[("n", &grouped(bytes, &l.thousands, &l.grouping))]);
    if u == 0 {
        return exact;
    }
    format!("{} {} ({exact})", format!("{v:.2}").replace('.', &l.decimal), units[u])
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// "4000 x 3000 (12.0 MP)".
pub fn dimensions(w: u32, h: u32, l: &Locale) -> String {
    let mp = format!("{:.1}", w as f64 * h as f64 / 1e6).replace('.', &l.decimal);
    format!("{w} x {h} ({mp} MP)")
}

/// "4:3" (W/gcd : H/gcd).
pub fn aspect(w: u32, h: u32) -> String {
    let g = gcd(w, h).max(1);
    format!("{}:{}", w / g, h / g)
}

/// (message id of the label, value). The label is translated when drawn.
pub fn rows(f: &Facts, l: &Locale) -> Vec<(&'static str, String)> {
    let name = f
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let folder = f.path.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let (w, h) = f.size;
    let mut rows = vec![
        ("info-name", name),
        ("info-type", tf("info-type-value", &[("format", f.info.format.name())])),
        ("info-location", folder),
    ];
    if let Some((bytes, modified)) = f.file {
        rows.push(("info-size", size_text(bytes, l)));
        rows.push(("info-modified", (l.date_time)(modified)));
    }
    rows.push(("info-dimensions", dimensions(w, h, l)));
    rows.push(("info-aspect", aspect(w, h)));
    if let Some(n) = f.frames {
        rows.push(("info-frames", n.to_string()));
    }
    // fixed texts of the decoder side; an ICC description is data and stays as it is
    let profile = match f.info.profile.as_str() {
        "sRGB (assumed)" => t("info-profile-assumed"),
        "Embedded ICC profile" => t("info-profile-unnamed"),
        p => p.to_string(),
    };
    rows.push(("info-profile", profile));
    let bits = f.info.bit_depth.to_string();
    let depth_id = if f.info.hdr {
        "info-depth-hdr"
    } else {
        "info-depth-value"
    };
    rows.push(("info-depth", tf(depth_id, &[("bits", &bits)])));
    let exif = match f.info.exif {
        Some((true, true)) => t("info-exif-gps"),
        Some((true, false)) => t("info-exif-yes"),
        Some((false, _)) => t("info-exif-no"),
        None => t("info-exif-unread"),
    };
    rows.push(("info-exif", exif));
    rows
}

/// The Info window; `open` is cleared when the user closes it.
pub fn show(ctx: &egui::Context, open: &mut bool, rows: &[(&'static str, String)]) {
    use super::icons::{self, Icon};
    egui::Window::new(t("info-title"))
        .id(egui::Id::unique("file info"))
        .title_bar(false)
        .resizable(false)
        .default_pos(egui::pos2(24.0, 24.0))
        .show(ctx, |ui| {
            // header: the controls first, as on macOS, then the title
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if icons::button(ui, Icon::Close, false, t("button-close")).clicked() {
                    *open = false;
                }
                ui.add_space(6.0);
                ui.label(egui::RichText::new(t("info-title")).strong().size(15.0));
            });
            ui.separator();
            egui::Grid::new("file info grid")
                .num_columns(2)
                .spacing([16.0, 6.0])
                .show(ui, |ui| {
                    for (k, v) in rows {
                        ui.label(egui::RichText::new(t(k)).weak());
                        ui.label(v);
                        ui.end_row();
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rv_core::formats::Format;

    fn locale() -> Locale {
        Locale {
            decimal: ",".into(),
            thousands: ".".into(),
            grouping: vec![3],
            date_time: |_| "13/11/2026 14:00".into(),
        }
    }

    #[test]
    fn sizes_dimensions_aspect() {
        let l = locale();
        assert_eq!(grouped(0, ".", &[3]), "0");
        assert_eq!(grouped(999, ".", &[3]), "999");
        assert_eq!(grouped(1_234_567, ".", &[3]), "1.234.567");
        assert_eq!(grouped(1_234_567, ",", &parse_grouping("3;2;0")), "12,34,567");
        assert_eq!(grouped(1_234_567, ",", &parse_grouping("3")), "1234,567");
        assert_eq!(parse_grouping("3;0"), vec![3]);
        assert_eq!(parse_grouping(""), vec![3]);
        assert_eq!(size_text(512, &l), "512 bytes");
        assert_eq!(size_text(1_234_567, &l), "1,18 MiB (1.234.567 bytes)");
        assert_eq!(size_text(3 << 30, &l), "3,00 GiB (3.221.225.472 bytes)");
        assert_eq!(dimensions(4000, 3000, &l), "4000 x 3000 (12,0 MP)");
        assert_eq!(aspect(4000, 3000), "4:3");
        assert_eq!(aspect(1920, 1080), "16:9");
        assert_eq!(aspect(7, 5), "7:5");
    }

    #[test]
    fn rows_of_a_photo() {
        let info = ImageInfo {
            format: Format::Jpeg,
            profile: "sRGB (assumed)".into(),
            profile_assumed: true,
            hdr: false,
            file_size: 10,
            bit_depth: 8,
            exif: Some((true, true)),
        };
        let f = Facts {
            path: Path::new(r"C:\Foto\gita.jpg"),
            size: (6000, 4000),
            info: &info,
            frames: None,
            file: Some((2048, SystemTime::UNIX_EPOCH)),
        };
        let r = rows(&f, &locale());
        let get = |k: &str| r.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("info-name"), Some("gita.jpg"));
        assert_eq!(get("info-type"), Some("JPEG image"));
        assert_eq!(get("info-location"), Some(r"C:\Foto"));
        assert_eq!(get("info-size"), Some("2,00 KiB (2.048 bytes)"));
        assert_eq!(get("info-modified"), Some("13/11/2026 14:00"));
        assert_eq!(get("info-dimensions"), Some("6000 x 4000 (24,0 MP)"));
        assert_eq!(get("info-aspect"), Some("3:2"));
        assert_eq!(get("info-frames"), None);
        assert_eq!(get("info-exif"), Some("Yes, with GPS position"));
        assert_eq!(get("info-depth"), Some("8 bits per channel"));
    }
}
