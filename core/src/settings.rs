//! Settings: every key with its default, `%APPDATA%\rView\settings.toml`.
//! Enums are stored as integers. Loading is lenient per key: a missing or invalid key keeps its default, the
//! others still apply.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::path::{Path, PathBuf};

/// Serialize/deserialize a fieldless enum as its integer value.
macro_rules! int_enum {
    ($t:ident { $($v:ident = $n:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum $t {
            $($v = $n),+
        }

        impl $t {
            /// Every value, in order (settings dialog).
            pub const ALL: &'static [$t] = &[$($t::$v),+];
        }

        impl Serialize for $t {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_u8(*self as u8)
            }
        }

        impl<'de> Deserialize<'de> for $t {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                match u8::deserialize(d)? {
                    $($n => Ok($t::$v),)+
                    n => Err(D::Error::custom(format!("invalid {} value {n}", stringify!($t)))),
                }
            }
        }
    };
}

int_enum!(CropMode {
    Whole = 0,
    Height = 1,
    Width = 2,
});

int_enum!(ScrollZoom {
    Zoom = 0,
    Auto = 1,
    Scroll = 2,
});

// Name "rView - name", Full "rView - name - size".
int_enum!(TitleBarMode {
    Name = 1,
    Full = 3,
});

int_enum!(WindowResizeMode {
    Never = 0,
    WhenLaunching = 1,
    WhenOpeningImages = 2,
});

// `colorspaceconversion`.
int_enum!(ColorSpaceConversion {
    Disabled = 0,
    Auto = 1,
    Srgb = 2,
    DisplayP3 = 3,
});

int_enum!(SortMode {
    Name = 0,
    Modified = 1,
    Created = 2,
    Size = 3,
    Type = 4,
    Random = 5,
});

// `afterdelete`: what the window shows after its file was deleted.
int_enum!(AfterDelete {
    MoveBack = 0,
    DoNothing = 1,
    MoveForward = 2,
});

// Off, Adjacent ±1, Extended ±4.
int_enum!(PreloadingMode {
    Off = 0,
    Adjacent = 1,
    Extended = 2,
});

impl PreloadingMode {
    /// Files preloaded on each side of the current one.
    pub fn radius(self) -> usize {
        match self {
            PreloadingMode::Off => 0,
            PreloadingMode::Adjacent => 1,
            PreloadingMode::Extended => 4,
        }
    }

    /// View-cache budget before the 25 %-of-RAM cap.
    pub fn cache_budget(self) -> u64 {
        match self {
            PreloadingMode::Off => 0,
            PreloadingMode::Adjacent => 256 << 20,
            PreloadingMode::Extended => 2 << 30,
        }
    }
}

/// `markup.metadatapreset`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MetadataPreset {
    Keep,
    StripLocation,
    StripAll,
}

impl MetadataPreset {
    pub const ALL: &'static [MetadataPreset] = &[
        MetadataPreset::Keep,
        MetadataPreset::StripLocation,
        MetadataPreset::StripAll,
    ];
}

/// The `[markup]` keys (editable in the settings dialog).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MarkupSettings {
    #[serde(rename = "metadatapreset")]
    pub metadata_preset: MetadataPreset,
    /// 1–100.
    #[serde(rename = "jpegquality")]
    pub jpeg_quality: u8,
    /// Confirm before Overwrite Original (JPEG always asks).
    #[serde(rename = "askoverwrite")]
    pub ask_overwrite: bool,
    /// Last style used per shape kind; missing kinds use `Style::default_for`.
    #[serde(rename = "laststyle")]
    pub last_style: std::collections::BTreeMap<crate::edit::ShapeKind, crate::edit::Style>,
}

impl Default for MarkupSettings {
    fn default() -> Self {
        MarkupSettings {
            metadata_preset: MetadataPreset::StripLocation,
            jpeg_quality: 100,
            ask_overwrite: true,
            last_style: Default::default(),
        }
    }
}

/// "#rrggbb" in the file, RGB in memory.
mod hex_color {
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(c: &[u8; 3], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 3], D::Error> {
        let s = String::deserialize(d)?;
        super::parse_color(&s).ok_or_else(|| D::Error::custom(format!("invalid colour {s:?}")))
    }
}

/// "#rgb" / "#rrggbb" (case-insensitive).
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().strip_prefix('#')?;
    let v = |i: usize, n: usize| u8::from_str_radix(h.get(i..i + n)?, 16).ok();
    match h.len() {
        6 => Some([v(0, 2)?, v(2, 2)?, v(4, 2)?]),
        3 => Some([v(0, 1)? * 17, v(1, 1)? * 17, v(2, 1)? * 17]),
        _ => None,
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // --- Window ---
    #[serde(rename = "bgcolorenabled")]
    pub bg_color_enabled: bool,
    #[serde(rename = "bgcolor", with = "hex_color")]
    pub bg_color: [u8; 3],
    #[serde(rename = "titlebarmode")]
    pub title_bar_mode: TitleBarMode,
    #[serde(rename = "windowresizemode")]
    pub window_resize_mode: WindowResizeMode,
    /// % of the monitor size.
    #[serde(rename = "minwindowresizedpercentage")]
    pub min_window_resized_percentage: u32,
    #[serde(rename = "maxwindowresizedpercentage")]
    pub max_window_resized_percentage: u32,
    #[serde(rename = "menubarenabled")]
    pub menubar_enabled: bool,
    #[serde(rename = "fullscreendetails")]
    pub fullscreen_details: bool,
    // --- Image ---
    #[serde(rename = "filteringenabled")]
    pub filtering_enabled: bool,
    #[serde(rename = "scalingenabled")]
    pub scaling_enabled: bool,
    #[serde(rename = "scalingtwoenabled")]
    pub scaling_two_enabled: bool,
    /// Percent per zoom step.
    #[serde(rename = "scalefactor")]
    pub scale_factor: u32,
    #[serde(rename = "scrollzoom")]
    pub scroll_zoom: ScrollZoom,
    #[serde(rename = "fractionalzoom")]
    pub fractional_zoom: bool,
    #[serde(rename = "cursorzoom")]
    pub cursor_zoom: bool,
    #[serde(rename = "cropmode")]
    pub crop_mode: CropMode,
    #[serde(rename = "pastactualsizeenabled")]
    pub past_actual_size_enabled: bool,
    #[serde(rename = "colorspaceconversion")]
    pub color_space_conversion: ColorSpaceConversion,
    // --- Miscellaneous ---
    /// Language code ("en") or "system".
    pub language: String,
    #[serde(rename = "sortmode")]
    pub sort_mode: SortMode,
    #[serde(rename = "sortdescending")]
    pub sort_descending: bool,
    #[serde(rename = "preloadingmode")]
    pub preloading_mode: PreloadingMode,
    #[serde(rename = "loopfoldersenabled")]
    pub loop_folders_enabled: bool,
    #[serde(rename = "slideshowreversed")]
    pub slideshow_reversed: bool,
    /// Seconds.
    #[serde(rename = "slideshowtimer")]
    pub slideshow_timer: f64,
    #[serde(rename = "afterdelete")]
    pub after_delete: AfterDelete,
    /// Confirm Delete (Recycle Bin); "Do not ask again" turns it off.
    #[serde(rename = "askdelete")]
    pub ask_delete: bool,
    #[serde(rename = "allowmimecontentdetection")]
    pub allow_mime_content_detection: bool,
    /// Keep the Open Recent list.
    #[serde(rename = "saverecents")]
    pub save_recents: bool,
    #[serde(rename = "updatenotifications")]
    pub update_notifications: bool,
    #[serde(rename = "skiphidden")]
    pub skip_hidden: bool,
    pub markup: MarkupSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            bg_color_enabled: true,
            bg_color: [0x20, 0x20, 0x20],
            title_bar_mode: TitleBarMode::Full,
            window_resize_mode: WindowResizeMode::WhenLaunching,
            min_window_resized_percentage: 20,
            max_window_resized_percentage: 70,
            menubar_enabled: true,
            fullscreen_details: false,
            filtering_enabled: true,
            scaling_enabled: true,
            scaling_two_enabled: true,
            scale_factor: 25,
            scroll_zoom: ScrollZoom::Auto,
            fractional_zoom: false,
            cursor_zoom: true,
            crop_mode: CropMode::Whole,
            past_actual_size_enabled: true,
            color_space_conversion: ColorSpaceConversion::Auto,
            language: "system".into(),
            sort_mode: SortMode::Name,
            sort_descending: false,
            preloading_mode: PreloadingMode::Adjacent,
            loop_folders_enabled: true,
            slideshow_reversed: false,
            slideshow_timer: 5.0,
            after_delete: AfterDelete::MoveForward,
            ask_delete: true,
            allow_mime_content_detection: false,
            save_recents: true,
            update_notifications: false,
            skip_hidden: true,
            markup: MarkupSettings::default(),
        }
    }
}

impl Settings {
    /// Values outside their range are brought back into it (hand-edited files).
    pub fn sanitized(mut self) -> Settings {
        self.min_window_resized_percentage = self.min_window_resized_percentage.clamp(1, 100);
        self.max_window_resized_percentage = self
            .max_window_resized_percentage
            .clamp(self.min_window_resized_percentage, 100);
        self.scale_factor = self.scale_factor.clamp(1, 500);
        if !(self.slideshow_timer.is_finite() && self.slideshow_timer >= 0.1) {
            self.slideshow_timer = Settings::default().slideshow_timer;
        }
        self.markup.jpeg_quality = self.markup.jpeg_quality.clamp(1, 100);
        if self.language.trim().is_empty() {
            self.language = "system".into();
        }
        self
    }

    /// Parse a settings file: every valid key applies, every missing or invalid key keeps its
    /// default. Returns the settings and one warning per key that was ignored.
    pub fn from_toml(text: &str) -> (Settings, Vec<String>) {
        let mut warnings = Vec::new();
        let user: toml::Table = match toml::from_str(text) {
            Ok(t) => t,
            Err(e) => return (Settings::default(), vec![format!("not a valid TOML file: {e}")]),
        };
        let defaults = toml::Table::try_from(Settings::default()).expect("defaults serialize");
        let merged = overlay(defaults, user, "", &mut warnings);
        let s = Settings::deserialize(toml::Value::Table(merged)).unwrap_or_default();
        (s.sanitized(), warnings)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).expect("settings serialize")
    }
}

/// `base` with every key of `user` that still deserializes as Settings once applied.
fn overlay(base: toml::Table, user: toml::Table, prefix: &str, warnings: &mut Vec<String>) -> toml::Table {
    let mut out = base;
    for (k, v) in user {
        let name = format!("{prefix}{k}");
        match (out.get(&k), v) {
            (Some(toml::Value::Table(sub)), toml::Value::Table(usub)) => {
                let merged = overlay(sub.clone(), usub, &format!("{name}."), warnings);
                out.insert(k, toml::Value::Table(merged));
            }
            (None, _) => warnings.push(format!("unknown key {name}")),
            (Some(_), v) => {
                let mut candidate = out.clone();
                candidate.insert(k.clone(), v);
                // at the top level the whole struct is checked; nested tables are checked by
                // their caller's check when the table is reassembled
                if !prefix.is_empty() || Settings::deserialize(toml::Value::Table(candidate.clone())).is_ok() {
                    out = candidate;
                } else {
                    warnings.push(format!("invalid value for {name}, default kept"));
                }
            }
        }
    }
    if !prefix.is_empty() {
        // nested: keep only the keys that deserialize as MarkupSettings (the only table)
        let mut checked = toml::Table::new();
        for (k, v) in out.clone() {
            let mut candidate = checked.clone();
            candidate.insert(k.clone(), v);
            if MarkupSettings::deserialize(toml::Value::Table(candidate.clone())).is_ok() {
                checked = candidate;
            } else {
                warnings.push(format!("invalid value for {prefix}{k}, default kept"));
            }
        }
        return checked;
    }
    out
}

/// `%APPDATA%\rView\settings.toml`, next to state.toml.
pub fn default_path() -> Option<PathBuf> {
    crate::state::default_path().map(|p| p.with_file_name("settings.toml"))
}

/// Missing file → defaults; unreadable keys → defaults for those keys (logged).
pub fn load(path: &Path) -> Settings {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let (s, warnings) = Settings::from_toml(&text);
            for w in warnings {
                log::warn!("{}: {w}", path.display());
            }
            s
        }
        Err(_) => Settings::default(),
    }
}

/// Atomic write, creating the folder if needed.
pub fn save(path: &Path, s: &Settings) -> Result<(), crate::atomic::SaveError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    crate::atomic::write_atomic(path, s.to_toml().as_bytes(), |b| {
        toml::from_str::<toml::Table>(&String::from_utf8_lossy(b)).is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_qt_key_names() {
        let s = Settings {
            bg_color: [0x12, 0xab, 0xff],
            title_bar_mode: TitleBarMode::Name,
            after_delete: AfterDelete::MoveBack,
            markup: MarkupSettings {
                metadata_preset: MetadataPreset::StripAll,
                ..Default::default()
            },
            ..Default::default()
        };
        let text = s.to_toml();
        assert!(text.contains("bgcolor = \"#12abff\""), "{text}");
        assert!(text.contains("titlebarmode = 1"), "enums as integers: {text}");
        assert!(text.contains("afterdelete = 0"));
        assert!(text.contains("[markup]") && text.contains("metadatapreset = \"strip-all\""));
        let (back, warnings) = Settings::from_toml(&text);
        assert_eq!(back, s);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn lenient_per_key() {
        let text = r##"
bgcolor = "#fff"
titlebarmode = 9
scalefactor = "big"
sortdescending = true
futurekey = 1
slideshowtimer = 2
[markup]
jpegquality = 300
metadatapreset = "nonsense"
askoverwrite = false
"##;
        let (s, warnings) = Settings::from_toml(text);
        let d = Settings::default();
        assert_eq!(s.bg_color, [255, 255, 255], "#rgb accepted");
        assert_eq!(s.title_bar_mode, d.title_bar_mode, "out-of-range enum → default");
        assert_eq!(s.scale_factor, d.scale_factor, "wrong type → default");
        assert!(s.sort_descending, "valid keys still apply");
        assert_eq!(s.slideshow_timer, 2.0, "integer accepted for a float");
        assert_eq!(
            s.markup.jpeg_quality, d.markup.jpeg_quality,
            "300 doesn't fit u8 → default"
        );
        assert_eq!(s.markup.metadata_preset, d.markup.metadata_preset);
        assert!(!s.markup.ask_overwrite);
        for key in [
            "titlebarmode",
            "scalefactor",
            "futurekey",
            "markup.jpegquality",
            "markup.metadatapreset",
        ] {
            assert!(
                warnings.iter().any(|w| w.contains(key)),
                "warning for {key}: {warnings:?}"
            );
        }
        assert_eq!(Settings::from_toml("not [[ toml").0, d);
        assert_eq!(Settings::from_toml("").0, d);
    }

    #[test]
    fn sanitizing_and_file() {
        let (s, _) = Settings::from_toml(
            "minwindowresizedpercentage = 90\nmaxwindowresizedpercentage = 10\nslideshowtimer = -3\nlanguage = \"\"",
        );
        assert_eq!(
            (s.min_window_resized_percentage, s.max_window_resized_percentage),
            (90, 90)
        );
        assert!(s.slideshow_timer >= 0.1);
        assert_eq!(s.language, "system");
        let dir = std::env::temp_dir().join(format!("rview-settings-{}", std::process::id()));
        let p = dir.join("sub").join("settings.toml");
        assert_eq!(load(&p), Settings::default(), "missing file");
        let s = Settings {
            language: "it".into(),
            ..Default::default()
        };
        save(&p, &s).unwrap();
        assert_eq!(load(&p), s);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(default_path().is_some_and(|p| p.ends_with("rView\\settings.toml")));
        assert_eq!(parse_color("#21212"), None);
    }
}
