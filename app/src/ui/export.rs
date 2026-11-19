//! Export dialog (Markup Export, and Save Frame As for animations): format, JPEG quality and
//! background, metadata preset. The file name is asked next, by the system Save dialog.
//! Convert uses it for the file on screen; every target can flatten transparency on the
//! background, set the stored resolution and resize (in pixels, as a percentage, or to a print
//! size at the chosen DPI).

use crate::i18n::{t, tf};
use rv_core::export::{ExportFormat, ExportOptions, MAX_DPI};
use rv_core::settings::MetadataPreset;

/// What the options are for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// The Markup document.
    Document,
    /// Save Frame As: this frame (0-based) of the animation; a frame carries no metadata.
    Frame(usize),
    /// Convert: the file on screen, as decoded.
    File,
}

/// Resolution presets of the DPI list.
pub const DPI_PRESETS: &[u32] = &[72, 96, 150, 300];

/// Largest side the size fields accept (JPEG's limit; the other formats check their own).
pub const MAX_SIDE: u32 = 65535;

/// How the output size is given.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sizing {
    Original,
    Pixels,
    Percent,
    /// Width and height on paper at the DPI of the Resolution row (resampled to it).
    Print,
}

impl Sizing {
    const ALL: &[Sizing] = &[Sizing::Original, Sizing::Pixels, Sizing::Percent, Sizing::Print];

    fn label(self) -> String {
        t(match self {
            Sizing::Original => "export-size-original",
            Sizing::Pixels => "export-size-pixels",
            Sizing::Percent => "export-size-percent",
            Sizing::Print => "export-size-print",
        })
    }
}

pub struct Export {
    pub opts: ExportOptions,
    /// The document has redactions: strip-all is the default.
    pub redactions: bool,
    pub target: Target,
    /// The DPI is typed instead of picked from `DPI_PRESETS`.
    custom_dpi: bool,
    pub resize: Resize,
}

/// The Size rows' state; the result is `ExportOptions::size`.
pub struct Resize {
    /// Size of the render before resizing; (0, 0) = unknown, no resizing offered.
    pub source: (u32, u32),
    pub sizing: Sizing,
    /// Width and height change together.
    pub keep_ratio: bool,
    /// Print size shown in centimetres, else inches.
    cm: bool,
    /// Print size in inches: kept when the DPI changes (the pixels follow).
    print: (f64, f64),
}

/// `(w, h)` scaled by `f`, each side within 1..=MAX_SIDE.
fn scaled((w, h): (u32, u32), f: f64) -> (u32, u32) {
    let s = |v: u32| ((v as f64 * f).round() as u32).clamp(1, MAX_SIDE);
    (s(w), s(h))
}

impl Resize {
    /// The other side of `w` (`by_width`) or of `h`, in the source's proportions.
    fn follow(&self, (w, h): (u32, u32), by_width: bool) -> (u32, u32) {
        let (sw, sh) = (self.source.0 as f64, self.source.1 as f64);
        if by_width {
            scaled((w, (w as f64 * sh / sw).round() as u32), 1.0)
        } else {
            scaled(((h as f64 * sw / sh).round() as u32, h), 1.0)
        }
    }

    /// Pixels of the print size at `dpi`.
    fn print_pixels(&self, dpi: f64) -> (u32, u32) {
        let px = |inches: f64| (inches * dpi).round() as u32;
        scaled((px(self.print.0), px(self.print.1)), 1.0)
    }

    /// Switch to `s`; Print starts from the current size at the DPI (300 if none is set).
    fn set_sizing(&mut self, s: Sizing, o: &mut ExportOptions) {
        self.sizing = s;
        if s == Sizing::Print {
            let dpi = *o.dpi.get_or_insert(300) as f64;
            let (w, h) = o.size.unwrap_or(self.source);
            self.print = (w as f64 / dpi, h as f64 / dpi);
        }
    }

    /// The Size combo and, unless Original, its fields; writes `o.size` (and `o.dpi` for Print).
    fn rows(&mut self, ui: &mut egui::Ui, o: &mut ExportOptions) {
        ui.label(t("export-size"));
        egui::ComboBox::from_id_salt("export size")
            .selected_text(self.sizing.label())
            .show_ui(ui, |ui| {
                for s in Sizing::ALL {
                    if ui.selectable_label(self.sizing == *s, s.label()).clicked() && self.sizing != *s {
                        self.set_sizing(*s, o);
                    }
                }
            });
        ui.end_row();
        if self.sizing == Sizing::Original {
            o.size = None;
            return;
        }
        let (mut w, mut h) = o.size.unwrap_or(self.source);
        ui.label("");
        ui.horizontal(|ui| {
            match self.sizing {
                Sizing::Pixels => {
                    let cw = ui.add(egui::DragValue::new(&mut w).range(1..=MAX_SIDE)).changed();
                    ui.label("×");
                    let ch = ui
                        .add(egui::DragValue::new(&mut h).range(1..=MAX_SIDE).suffix(" px"))
                        .changed();
                    if self.keep_ratio && (cw || ch) {
                        (w, h) = self.follow((w, h), cw);
                    }
                }
                Sizing::Percent => {
                    let mut p = w as f64 / self.source.0 as f64 * 100.0;
                    let drag = egui::DragValue::new(&mut p)
                        .range(1.0..=1000.0)
                        .speed(0.5)
                        .max_decimals(1)
                        .suffix(" %");
                    if ui.add(drag).changed() {
                        (w, h) = scaled(self.source, p / 100.0);
                    }
                }
                Sizing::Print => {
                    let dpi = *o.dpi.get_or_insert(300) as f64;
                    let unit = if self.cm { 2.54 } else { 1.0 };
                    let (mut pw, mut ph) = (self.print.0 * unit, self.print.1 * unit);
                    let top = MAX_SIDE as f64 / dpi * unit;
                    fn field(v: &mut f64, top: f64) -> egui::DragValue<'_> {
                        egui::DragValue::new(v).range(0.01..=top).speed(0.05).max_decimals(2)
                    }
                    let cw = ui.add(field(&mut pw, top)).changed();
                    ui.label("×");
                    let ch = ui.add(field(&mut ph, top)).changed();
                    egui::ComboBox::from_id_salt("export unit")
                        .width(48.0)
                        .selected_text(if self.cm { "cm" } else { "in" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.cm, true, "cm");
                            ui.selectable_value(&mut self.cm, false, "in");
                        });
                    let (sw, sh) = (self.source.0 as f64, self.source.1 as f64);
                    if self.keep_ratio && cw {
                        ph = pw * sh / sw;
                    } else if self.keep_ratio && ch {
                        pw = ph * sw / sh;
                    }
                    if cw || ch {
                        self.print = (pw / unit, ph / unit);
                    }
                    (w, h) = self.print_pixels(dpi);
                }
                Sizing::Original => {}
            }
            if self.sizing != Sizing::Percent
                && ui.checkbox(&mut self.keep_ratio, t("export-keep-ratio")).changed()
                && self.keep_ratio
            {
                (w, h) = self.follow((w, h), true);
                let dpi = o.dpi.unwrap_or(300) as f64;
                self.print = (w as f64 / dpi, h as f64 / dpi);
            }
            if self.sizing != Sizing::Pixels {
                ui.label(tf(
                    "export-size-result",
                    &[("w", &w.to_string()), ("h", &h.to_string())],
                ));
            }
        });
        ui.end_row();
        o.size = Some((w, h));
    }
}

pub fn preset_label(p: MetadataPreset) -> String {
    t(match p {
        MetadataPreset::Keep => "settings-metadata-keep",
        MetadataPreset::StripLocation => "settings-metadata-location",
        MetadataPreset::StripAll => "settings-metadata-all",
    })
}

fn dpi_label(dpi: Option<u32>) -> String {
    match dpi {
        None => t("export-dpi-unchanged"),
        Some(d) => tf("export-dpi-value", &[("dpi", &d.to_string())]),
    }
}

impl Export {
    pub fn new(
        m: &rv_core::settings::MarkupSettings,
        source: Option<rv_core::formats::Format>,
        size: (u32, u32),
        redactions: bool,
    ) -> Export {
        Export {
            opts: ExportOptions {
                format: source.and_then(ExportFormat::of).unwrap_or(ExportFormat::Png),
                jpeg_quality: m.jpeg_quality.clamp(1, 100),
                background: [255, 255, 255],
                flatten: false,
                dpi: None,
                size: None,
                preset: if redactions {
                    MetadataPreset::StripAll
                } else {
                    m.metadata_preset
                },
            },
            redactions,
            target: Target::Document,
            custom_dpi: false,
            resize: Resize {
                source: size,
                sizing: Sizing::Original,
                keep_ratio: true,
                cm: true,
                print: (0.0, 0.0),
            },
        }
    }

    /// Save Frame As: PNG by default (lossless, transparency kept).
    pub fn frame(m: &rv_core::settings::MarkupSettings, n: usize, size: (u32, u32)) -> Export {
        let mut e = Export::new(m, None, size, false);
        e.target = Target::Frame(n);
        e
    }

    /// Convert: the source's format proposed, like Export.
    pub fn convert(
        m: &rv_core::settings::MarkupSettings,
        source: Option<rv_core::formats::Format>,
        size: (u32, u32),
    ) -> Export {
        let mut e = Export::new(m, source, size, false);
        e.target = Target::File;
        e
    }

    /// The driver sets the options as words: a format name ("PNG", "JPEG"…), `quality=<1–100>`,
    /// `background=#rrggbb`, `flatten`, `dpi=<n>`, `size=<w>x<h>` (pixels), `scale=<percent>`,
    /// `print=<w>x<h>` (centimetres at the DPI, 300 if none). No range check: the export
    /// validates them.
    pub fn set_format(&mut self, text: &str) {
        fn pair<T: std::str::FromStr>(v: &str) -> Option<(T, T)> {
            let (w, h) = v.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        }
        let r = &mut self.resize;
        for word in text.split_whitespace() {
            let o = &mut self.opts;
            match word.split_once('=') {
                Some(("quality", v)) => o.jpeg_quality = v.parse().unwrap_or(o.jpeg_quality),
                Some(("background", v)) => o.background = rv_core::settings::parse_color(v).unwrap_or(o.background),
                Some(("dpi", v)) => o.dpi = v.parse().ok(),
                Some(("size", v)) => {
                    o.size = pair(v);
                    r.sizing = if o.size.is_some() {
                        Sizing::Pixels
                    } else {
                        Sizing::Original
                    };
                }
                Some(("scale", v)) => {
                    if let Ok(p) = v.parse::<f64>() {
                        r.sizing = Sizing::Percent;
                        o.size = Some(scaled(r.source, p / 100.0));
                    }
                }
                Some(("print", v)) => {
                    if let Some((w, h)) = pair::<f64>(v) {
                        (r.sizing, r.print) = (Sizing::Print, (w / 2.54, h / 2.54));
                    }
                }
                None if word == "flatten" => o.flatten = true,
                _ => {
                    if let Some(f) = ExportFormat::ALL.iter().find(|f| f.name().eq_ignore_ascii_case(word)) {
                        o.format = *f;
                    }
                }
            }
        }
        if r.sizing == Sizing::Print {
            let dpi = *self.opts.dpi.get_or_insert(300) as f64;
            self.opts.size = Some(r.print_pixels(dpi));
        }
    }

    pub(super) fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        let frame = matches!(self.target, Target::Frame(_));
        let custom_dpi = &mut self.custom_dpi;
        let resize = &mut self.resize;
        let o = &mut self.opts;
        egui::Grid::new("export grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label(t("export-format"));
                egui::ComboBox::from_id_salt("export format")
                    .selected_text(o.format.name())
                    .show_ui(ui, |ui| {
                        for f in ExportFormat::ALL {
                            ui.selectable_value(&mut o.format, *f, f.name());
                        }
                    });
                ui.end_row();
                if o.format == ExportFormat::Jpeg {
                    ui.label(t("export-quality"));
                    ui.add(egui::Slider::new(&mut o.jpeg_quality, 1..=100));
                    ui.end_row();
                    ui.label(t("export-background"));
                    super::color::edit_button(ui, &mut o.background);
                    ui.end_row();
                } else {
                    ui.label(t("export-transparency"));
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut o.flatten, t("export-flatten"));
                        if o.flatten {
                            super::color::edit_button(ui, &mut o.background);
                        }
                    });
                    ui.end_row();
                }
                ui.label(t("export-resolution"));
                ui.horizontal(|ui| {
                    let selected = if *custom_dpi {
                        t("export-dpi-custom")
                    } else {
                        dpi_label(o.dpi)
                    };
                    egui::ComboBox::from_id_salt("export dpi")
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for dpi in std::iter::once(None).chain(DPI_PRESETS.iter().map(|d| Some(*d))) {
                                if ui
                                    .selectable_label(!*custom_dpi && o.dpi == dpi, dpi_label(dpi))
                                    .clicked()
                                {
                                    o.dpi = dpi;
                                    *custom_dpi = false;
                                }
                            }
                            if ui.selectable_label(*custom_dpi, t("export-dpi-custom")).clicked() {
                                *custom_dpi = true;
                                o.dpi = Some(o.dpi.unwrap_or(300));
                            }
                        });
                    if *custom_dpi && let Some(d) = &mut o.dpi {
                        ui.add(egui::DragValue::new(d).range(1..=MAX_DPI).suffix(" DPI"));
                    }
                });
                ui.end_row();
                if resize.source.0 > 0 && resize.source.1 > 0 {
                    resize.rows(ui, o);
                }
                if frame {
                    return;
                }
                ui.label(t("export-metadata"));
                egui::ComboBox::from_id_salt("export metadata")
                    .selected_text(preset_label(o.preset))
                    .show_ui(ui, |ui| {
                        for p in MetadataPreset::ALL {
                            ui.selectable_value(&mut o.preset, *p, preset_label(*p));
                        }
                    });
                ui.end_row();
            });
        let note = |ui: &mut egui::Ui, text: String| {
            ui.add(egui::Label::new(egui::RichText::new(text).small().weak()).wrap());
        };
        if o.format == ExportFormat::Jpeg {
            note(ui, t("export-jpeg-note"));
        } else {
            note(ui, tf("export-lossless-note", &[("format", o.format.name())]));
        }
        if !frame && matches!(o.format, ExportFormat::Tiff | ExportFormat::Bmp) {
            note(ui, t("export-no-exif-note"));
        }
        let r = &self.resize;
        if let Some((w, h)) = o.size
            && !rv_core::resample::allowed(r.source.0, r.source.1, w, h)
        {
            note(ui, t("export-size-too-large"));
        }
        if o.dpi.is_some() {
            note(
                ui,
                t(if r.sizing == Sizing::Print {
                    "export-print-note"
                } else {
                    "export-dpi-note"
                }),
            );
            if o.format == ExportFormat::Webp && (frame || o.preset == MetadataPreset::StripAll) {
                note(ui, t("export-dpi-webp-note"));
            }
        }
        if self.redactions {
            note(ui, t("export-redactions-note"));
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_words_and_defaults() {
        let m = rv_core::settings::MarkupSettings::default();
        let mut e = Export::convert(&m, Some(rv_core::formats::Format::Png), (1200, 800));
        assert_eq!(e.target, Target::File);
        assert_eq!(e.opts.format, ExportFormat::Png);
        assert_eq!(e.opts.jpeg_quality, 100, "quality 100 by default");
        assert!(
            !e.opts.flatten && e.opts.dpi.is_none(),
            "transparency and resolution unchanged"
        );
        e.set_format("jpeg quality=80 background=#102030 flatten dpi=150");
        assert_eq!(e.opts.format, ExportFormat::Jpeg);
        assert_eq!(e.opts.jpeg_quality, 80);
        assert_eq!(e.opts.background, [0x10, 0x20, 0x30]);
        assert!(e.opts.flatten);
        assert_eq!(e.opts.dpi, Some(150));
        // invalid colours keep the current one
        e.set_format("background=#zz0000 dpi=x");
        assert_eq!(e.opts.background, [0x10, 0x20, 0x30]);
        assert_eq!(e.opts.dpi, None);
        // a source that can't be exported as itself: PNG
        let heic = Export::convert(&m, Some(rv_core::formats::Format::Heif), (0, 0));
        assert_eq!(heic.opts.format, ExportFormat::Png);
    }

    #[test]
    fn sizes_from_pixels_percent_and_print() {
        let m = rv_core::settings::MarkupSettings::default();
        let mut e = Export::convert(&m, None, (1200, 800));
        assert_eq!(e.opts.size, None, "original size by default");
        e.set_format("size=600x300");
        assert_eq!((e.opts.size, e.resize.sizing), (Some((600, 300)), Sizing::Pixels));
        // proportions kept from either side, rounded, never 0
        assert_eq!(e.resize.follow((600, 1), true), (600, 400));
        assert_eq!(e.resize.follow((1, 100), false), (150, 100));
        assert_eq!(e.resize.follow((1, 1), true), (1, 1));
        e.set_format("scale=25");
        assert_eq!(e.opts.size, Some((300, 200)));
        e.set_format("scale=0.01");
        assert_eq!(e.opts.size, Some((1, 1)), "clamped to one pixel");
        // print: 10.16 × 5.08 cm = 4 × 2 in, at 150 DPI = 600 × 300 px; the DPI defaults to 300
        e.set_format("dpi=150 print=10.16x5.08");
        assert_eq!((e.opts.size, e.opts.dpi), (Some((600, 300)), Some(150)));
        e.set_format("size=off");
        e.opts.dpi = None;
        e.set_format("print=2.54x2.54");
        assert_eq!((e.opts.size, e.opts.dpi), (Some((300, 300)), Some(300)));
        e.set_format("size=off");
        assert_eq!((e.opts.size, e.resize.sizing), (None, Sizing::Original));
        // the print size stays when the DPI changes: the pixels follow
        let mut o = e.opts;
        e.resize.set_sizing(Sizing::Print, &mut o);
        assert_eq!(e.resize.print_pixels(300.0), (1200, 800));
        assert_eq!(e.resize.print_pixels(150.0), (600, 400));
    }
}
