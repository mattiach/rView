//! Show File Info: rows computed by `rows`, drawn in a non-modal window that follows the
//! file on screen while it is open. The rows are grouped in tabs; each value can be copied whole
//! (long ones are shortened on screen only).

use crate::i18n::{t, tf};
use rv_core::details::{AlphaMode, Details, Layout, ResolutionUnit, Transparency};
use rv_core::loader::ImageInfo;
use std::path::Path;
use std::time::SystemTime;

/// The Info window's tabs, in order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    General,
    Image,
    Camera,
    Metadata,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::General, Tab::Image, Tab::Camera, Tab::Metadata];

    fn label(self) -> String {
        t(match self {
            Tab::General => "info-tab-general",
            Tab::Image => "info-tab-image",
            Tab::Camera => "info-tab-camera",
            Tab::Metadata => "info-tab-metadata",
        })
    }
}

/// (tab, message id of the label, value). The label is translated when drawn.
pub type Row = (Tab, &'static str, String);

/// Size and times of the file on disk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileFacts {
    pub bytes: u64,
    pub modified: SystemTime,
    pub created: Option<SystemTime>,
}

/// What the Info rows are made of.
pub struct Facts<'a> {
    pub path: &'a Path,
    /// Size declared by the file, before view rotations.
    pub size: (u32, u32),
    pub info: &'a ImageInfo,
    /// Frames of an animation, once known.
    pub frames: Option<usize>,
    /// None if the file can't be read any more.
    pub file: Option<FileFacts>,
    /// Read from the file on a worker; None until then.
    pub details: Option<&'a Details>,
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

    fn number(&self, n: u64) -> String {
        grouped(n, &self.thousands, &self.grouping)
    }

    /// `v` with `places` decimals and the locale's decimal separator.
    fn decimal(&self, v: f64, places: usize) -> String {
        format!("{v:.places$}").replace('.', &self.decimal)
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
    let exact = tf("info-bytes", &[("n", &l.number(bytes))]);
    if u == 0 {
        return exact;
    }
    format!("{} {} ({exact})", l.decimal(v, 2), units[u])
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// "4:3" (W/gcd : H/gcd).
pub fn aspect(w: u32, h: u32) -> String {
    let g = gcd(w, h).max(1);
    format!("{}:{}", w / g, h / g)
}

fn yes_no(v: bool) -> String {
    t(if v { "info-yes" } else { "info-no" })
}

/// A resolution value: whole numbers without decimals ("300"), others with two ("299,72").
fn dpi(v: f64, l: &Locale) -> String {
    if (v - v.round()).abs() < 0.005 {
        format!("{}", v.round())
    } else {
        l.decimal(v, 2)
    }
}

/// The rows of every tab. Rows whose value is unknown are left out.
pub fn rows(f: &Facts, l: &Locale) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let mut add = |tab, key, value: Option<String>| {
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            rows.push((tab, key, v));
        }
    };
    let d = f.details;
    let x = d.map(|d| &d.exif);
    let (w, h) = f.size;
    use Tab::*;

    // General
    let name = f.path.file_name().map(|n| n.to_string_lossy().into_owned());
    add(General, "info-name", name);
    add(
        General,
        "info-type",
        Some(tf("info-type-value", &[("format", f.info.format.name())])),
    );
    add(
        General,
        "info-location",
        f.path.parent().map(|p| p.display().to_string()),
    );
    if let Some(file) = f.file {
        add(General, "info-size", Some(size_text(file.bytes, l)));
        add(General, "info-created", file.created.map(l.date_time));
        add(General, "info-modified", Some((l.date_time)(file.modified)));
    }
    add(General, "info-dimensions", Some(format!("{w} x {h}")));
    add(
        General,
        "info-megapixels",
        Some(format!("{} MP", l.decimal(w as f64 * h as f64 / 1e6, 1))),
    );
    // as the image is shown: EXIF orientations 5–8 turn it by a quarter
    let turned = x.and_then(|x| x.orientation).is_some_and(|o| (5..=8).contains(&o));
    let (sw, sh) = if turned { (h, w) } else { (w, h) };
    let orientation = match sw.cmp(&sh) {
        std::cmp::Ordering::Greater => "info-orientation-landscape",
        std::cmp::Ordering::Less => "info-orientation-portrait",
        std::cmp::Ordering::Equal => "info-orientation-square",
    };
    add(General, "info-orientation", Some(t(orientation)));
    add(General, "info-aspect", Some(aspect(sw, sh)));
    add(General, "info-frames", f.frames.map(|n| n.to_string()));

    // Image
    let bits = d
        .and_then(|d| d.bits)
        .unwrap_or(u16::from(f.info.bit_depth))
        .to_string();
    let depth = match (d.is_some_and(|d| d.float), f.info.hdr) {
        (true, _) => tf("info-depth-float", &[("bits", &bits)]),
        (false, true) => tf("info-depth-hdr", &[("bits", &bits)]),
        (false, false) => tf("info-depth-value", &[("bits", &bits)]),
    };
    add(Image, "info-depth", Some(depth));
    let layout = d.and_then(|d| d.layout);
    add(
        Image,
        "info-channels",
        layout.map(|l| {
            t(match l {
                Layout::Gray => "info-channels-gray",
                Layout::GrayAlpha => "info-channels-gray-alpha",
                Layout::Rgb => "info-channels-rgb",
                Layout::Rgba => "info-channels-rgba",
                Layout::Indexed { alpha: false } => "info-channels-indexed",
                Layout::Indexed { alpha: true } => "info-channels-indexed-alpha",
                Layout::Cmyk => "info-channels-cmyk",
            })
        }),
    );
    add(Image, "info-has-alpha", layout.map(|l| yes_no(l.has_alpha())));
    add(
        Image,
        "info-transparency",
        d.and_then(|d| d.transparency).map(|tr| {
            t(match tr {
                Transparency::None => "info-transparency-none",
                Transparency::Binary => "info-transparency-binary",
                Transparency::Partial => "info-transparency-partial",
            })
        }),
    );
    add(
        Image,
        "info-alpha-mode",
        d.and_then(|d| d.alpha_mode).map(|m| {
            t(match m {
                AlphaMode::Straight => "info-alpha-straight",
                AlphaMode::Premultiplied => "info-alpha-premultiplied",
            })
        }),
    );
    add(
        Image,
        "info-unique-colors",
        d.and_then(|d| d.unique_colors).map(|n| l.number(n)),
    );
    // fixed texts of the decoder side; an ICC description is data and stays as it is
    let profile = match f.info.profile.as_str() {
        "sRGB (assumed)" => t("info-profile-assumed"),
        "Embedded ICC profile" => t("info-profile-unnamed"),
        p => p.to_string(),
    };
    add(Image, "info-profile", Some(profile));
    let space = match (d.and_then(|d| d.icc_space), x.and_then(|x| x.color_space)) {
        (Some(s), _) => Some(s.to_string()),
        (None, Some(1)) => Some("sRGB".to_string()),
        (None, Some(0xFFFF)) => Some(t("info-color-space-uncalibrated")),
        (None, _) if layout == Some(Layout::Cmyk) => Some("CMYK".to_string()),
        (None, _) if d.is_some() => Some(t("info-profile-assumed")),
        _ => None,
    };
    add(Image, "info-color-space", space);
    add(
        Image,
        "info-icc-size",
        d.and_then(|d| d.icc_size).map(|n| size_text(n as u64, l)),
    );
    if let Some(r) = d.and_then(|d| d.resolution) {
        let value = if r.unit == ResolutionUnit::None {
            tf("info-resolution-aspect", &[("x", &dpi(r.x, l)), ("y", &dpi(r.y, l))])
        } else {
            tf("info-resolution-value", &[("x", &dpi(r.x, l)), ("y", &dpi(r.y, l))])
        };
        add(Image, "info-resolution", Some(value));
        let unit = match r.unit {
            ResolutionUnit::Inch => "info-unit-inch",
            ResolutionUnit::Centimetre => "info-unit-cm",
            ResolutionUnit::None => "info-unit-none",
        };
        add(Image, "info-resolution-unit", Some(t(unit)));
    }
    add(Image, "info-jpeg-sampling", d.and_then(|d| d.jpeg_sampling.clone()));
    let chunks = d.map(|d| {
        d.png_chunks
            .iter()
            .map(|(k, n)| if *n > 1 { format!("{k} ×{n}") } else { k.clone() })
            .collect::<Vec<_>>()
            .join(", ")
    });
    add(Image, "info-png-chunks", chunks);

    // Camera
    if let Some(x) = x {
        add(Camera, "info-camera-make", x.make.clone());
        add(Camera, "info-camera-model", x.model.clone());
        add(Camera, "info-lens", x.lens.clone());
        add(Camera, "info-date-taken", x.date_taken.clone());
        add(Camera, "info-exposure", x.exposure.clone());
        add(Camera, "info-f-number", x.f_number.clone());
        add(Camera, "info-iso", x.iso.clone());
        add(Camera, "info-focal-length", x.focal_length.clone());
        add(Camera, "info-flash", x.flash.clone());
        add(
            Camera,
            "info-gps",
            x.gps.map(|(lat, lon)| format!("{lat:.6}, {lon:.6}")),
        );
    }

    // Metadata
    let exif = match f.info.exif {
        Some((true, true)) => t("info-exif-gps"),
        Some((true, false)) => t("info-exif-yes"),
        Some((false, _)) => t("info-exif-no"),
        None => t("info-exif-unread"),
    };
    add(Metadata, "info-exif", Some(exif));
    if let Some(x) = x {
        add(Metadata, "info-software", x.software.clone());
        add(Metadata, "info-copyright", x.copyright.clone());
        add(Metadata, "info-artist", x.artist.clone());
    }
    if let Some(d) = d {
        add(Metadata, "info-iptc", Some(yes_no(d.iptc)));
        add(Metadata, "info-xmp", Some(yes_no(d.xmp)));
        let thumb = match d.thumbnail {
            Some(Some((tw, th))) => tf("info-thumbnail-size", &[("w", &tw.to_string()), ("h", &th.to_string())]),
            Some(None) => t("info-yes"),
            None => t("info-no"),
        };
        add(Metadata, "info-thumbnail", Some(thumb));
    }
    rows
}

/// Widths of the label column and of the whole window (points), at most: both shrink to fit a
/// small viewer window.
const LABEL_WIDTH: f32 = 142.0;
const WIDTH: f32 = 440.0;

/// The Info window; `open` is cleared when the user closes it. `pending`: the file's details are
/// still being read (the tabs that need them say so).
pub fn show(ctx: &egui::Context, open: &mut bool, rows: &[Row], pending: bool) {
    use super::icons::{self, Icon};
    let id = egui::Id::unique("file info");
    let mut tab = ctx
        .data(|d| d.get_temp::<usize>(id))
        .unwrap_or(0)
        .min(Tab::ALL.len() - 1);
    let copied_id = id.with("copied");
    let mut copied = ctx.data(|d| d.get_temp::<(&'static str, String)>(copied_id));
    let width = (ctx.content_rect().width() - 48.0).clamp(220.0, WIDTH);
    let label_width = LABEL_WIDTH.min(width * 0.38);
    egui::Window::new(t("info-title"))
        .id(id)
        .title_bar(false)
        // follows its content: a tab with fewer rows makes it shorter
        .auto_sized()
        .default_pos(egui::pos2(24.0, 24.0))
        .show(ctx, |ui| {
            ui.set_width(width);
            // header: the controls first, as on macOS, then the title
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if icons::button(ui, Icon::Close, false, t("button-close")).clicked() {
                    *open = false;
                }
                ui.add_space(6.0);
                ui.label(egui::RichText::new(t("info-title")).strong().size(15.0));
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for (i, tb) in Tab::ALL.iter().enumerate() {
                    ui.selectable_value(&mut tab, i, tb.label());
                }
            });
            ui.separator();
            let current = Tab::ALL[tab];
            let shown: Vec<&Row> = rows.iter().filter(|r| r.0 == current).collect();
            let max_height = (ctx.content_rect().height() - 160.0).max(160.0);
            egui::ScrollArea::vertical().max_height(max_height).show(ui, |ui| {
                for (_, key, value) in &shown {
                    ui.horizontal(|ui| {
                        let label = egui::vec2(label_width, ui.spacing().interact_size.y);
                        ui.allocate_ui_with_layout(label, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.set_min_size(label);
                            ui.add(egui::Label::new(egui::RichText::new(t(key)).weak()).truncate());
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let done = copied.as_ref().is_some_and(|(k, v)| k == key && v == value);
                            let tip = t(if done { "info-copied" } else { "info-copy" });
                            if icons::button(ui, Icon::Copy, done, tip).clicked() {
                                ui.ctx().copy_text(value.clone());
                                copied = Some((key, value.clone()));
                            }
                            // shortened with "…" when too long; the whole text on hover
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.add(egui::Label::new(value.as_str()).truncate());
                            });
                        });
                    });
                }
                let hint = if pending && current != Tab::General {
                    Some("info-reading")
                } else if shown.is_empty() {
                    Some(match current {
                        Tab::Camera => "info-no-camera",
                        _ => "info-nothing",
                    })
                } else {
                    None
                };
                if let Some(h) = hint {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if pending {
                            ui.spinner();
                        }
                        ui.label(egui::RichText::new(t(h)).weak());
                    });
                }
            });
        });
    ctx.data_mut(|d| {
        d.insert_temp(id, tab);
        match copied {
            Some(c) => {
                d.insert_temp(copied_id, c);
            }
            None => d.remove::<(&'static str, String)>(copied_id),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rv_core::details::{ExifFacts, Resolution};
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
    fn sizes_and_aspect() {
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
        assert_eq!(aspect(4000, 3000), "4:3");
        assert_eq!(aspect(1920, 1080), "16:9");
        assert_eq!(aspect(7, 5), "7:5");
        assert_eq!(dpi(300.0, &l), "300");
        assert_eq!(dpi(299.72, &l), "299,72");
    }

    fn info() -> ImageInfo {
        ImageInfo {
            format: Format::Jpeg,
            profile: "sRGB (assumed)".into(),
            profile_assumed: true,
            hdr: false,
            file_size: 10,
            bit_depth: 8,
            exif: Some((true, true)),
        }
    }

    fn facts<'a>(info: &'a ImageInfo, details: Option<&'a Details>) -> Facts<'a> {
        Facts {
            path: Path::new(r"C:\Foto\gita.jpg"),
            size: (6000, 4000),
            info,
            frames: None,
            file: Some(FileFacts {
                bytes: 2048,
                modified: SystemTime::UNIX_EPOCH,
                created: Some(SystemTime::UNIX_EPOCH),
            }),
            details,
        }
    }

    fn get<'a>(r: &'a [Row], k: &str) -> Option<(Tab, &'a str)> {
        r.iter().find(|(_, n, _)| *n == k).map(|(tab, _, v)| (*tab, v.as_str()))
    }

    #[test]
    fn rows_before_the_details() {
        let info = info();
        let r = rows(&facts(&info, None), &locale());
        assert_eq!(get(&r, "info-name"), Some((Tab::General, "gita.jpg")));
        assert_eq!(get(&r, "info-type"), Some((Tab::General, "JPEG image")));
        assert_eq!(get(&r, "info-location"), Some((Tab::General, r"C:\Foto")));
        assert_eq!(get(&r, "info-size"), Some((Tab::General, "2,00 KiB (2.048 bytes)")));
        assert_eq!(get(&r, "info-created"), Some((Tab::General, "13/11/2026 14:00")));
        assert_eq!(get(&r, "info-dimensions"), Some((Tab::General, "6000 x 4000")));
        assert_eq!(get(&r, "info-megapixels"), Some((Tab::General, "24,0 MP")));
        assert_eq!(get(&r, "info-orientation"), Some((Tab::General, "Landscape")));
        assert_eq!(get(&r, "info-aspect"), Some((Tab::General, "3:2")));
        assert_eq!(get(&r, "info-frames"), None);
        assert_eq!(get(&r, "info-depth"), Some((Tab::Image, "8 bits per channel")));
        assert_eq!(get(&r, "info-exif"), Some((Tab::Metadata, "Yes, with GPS position")));
        assert_eq!(get(&r, "info-channels"), None, "known only with the details");
        assert!(r.iter().all(|(tab, ..)| *tab != Tab::Camera));
    }

    #[test]
    fn rows_with_the_details() {
        let info = info();
        let d = Details {
            layout: Some(Layout::Rgb),
            bits: Some(8),
            transparency: Some(Transparency::None),
            unique_colors: Some(1_234_567),
            resolution: Some(Resolution {
                x: 300.0,
                y: 300.0,
                unit: ResolutionUnit::Inch,
            }),
            jpeg_sampling: Some("4:2:0".into()),
            exif_present: true,
            exif: ExifFacts {
                make: Some("Canon".into()),
                f_number: Some("f/2.8".into()),
                gps: Some((45.4642, 9.19)),
                artist: Some("A. Author".into()),
                // shot in portrait: the 6000 x 4000 frame is shown turned
                orientation: Some(6),
                ..Default::default()
            },
            thumbnail: Some(Some((160, 120))),
            ..Default::default()
        };
        let r = rows(&facts(&info, Some(&d)), &locale());
        assert_eq!(get(&r, "info-orientation"), Some((Tab::General, "Portrait")));
        assert_eq!(get(&r, "info-aspect"), Some((Tab::General, "2:3")));
        assert_eq!(get(&r, "info-channels"), Some((Tab::Image, "RGB")));
        assert_eq!(get(&r, "info-has-alpha"), Some((Tab::Image, "No")));
        assert_eq!(get(&r, "info-alpha-mode"), None, "no alpha");
        assert_eq!(get(&r, "info-unique-colors"), Some((Tab::Image, "1.234.567")));
        assert_eq!(get(&r, "info-color-space"), Some((Tab::Image, "sRGB (assumed)")));
        assert_eq!(get(&r, "info-resolution"), Some((Tab::Image, "300 x 300 DPI")));
        assert_eq!(get(&r, "info-resolution-unit"), Some((Tab::Image, "Inch")));
        assert_eq!(get(&r, "info-jpeg-sampling"), Some((Tab::Image, "4:2:0")));
        assert_eq!(get(&r, "info-png-chunks"), None, "no chunks: not a PNG");
        assert_eq!(get(&r, "info-camera-make"), Some((Tab::Camera, "Canon")));
        assert_eq!(get(&r, "info-f-number"), Some((Tab::Camera, "f/2.8")));
        assert_eq!(get(&r, "info-gps"), Some((Tab::Camera, "45.464200, 9.190000")));
        assert_eq!(get(&r, "info-lens"), None, "unknown rows are left out");
        assert_eq!(get(&r, "info-artist"), Some((Tab::Metadata, "A. Author")));
        assert_eq!(get(&r, "info-iptc"), Some((Tab::Metadata, "No")));
        assert_eq!(get(&r, "info-thumbnail"), Some((Tab::Metadata, "Yes, 160 x 120")));
    }
}
