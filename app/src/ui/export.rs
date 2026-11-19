//! Export dialog: format, JPEG quality and background, metadata preset. The file
//! name is asked next, by the system Save dialog.

use crate::i18n::t;
use rv_core::export::{ExportFormat, ExportOptions};
use rv_core::settings::MetadataPreset;

pub struct Export {
    pub opts: ExportOptions,
    /// The document has redactions: strip-all is the default.
    pub redactions: bool,
}

pub fn preset_label(p: MetadataPreset) -> String {
    t(match p {
        MetadataPreset::Keep => "settings-metadata-keep",
        MetadataPreset::StripLocation => "settings-metadata-location",
        MetadataPreset::StripAll => "settings-metadata-all",
    })
}

impl Export {
    pub fn new(
        m: &rv_core::settings::MarkupSettings,
        source: Option<rv_core::formats::Format>,
        redactions: bool,
    ) -> Export {
        Export {
            opts: ExportOptions {
                format: source.and_then(ExportFormat::of).unwrap_or(ExportFormat::Png),
                jpeg_quality: m.jpeg_quality.clamp(1, 100),
                background: [255, 255, 255],
                preset: if redactions {
                    MetadataPreset::StripAll
                } else {
                    m.metadata_preset
                },
            },
            redactions,
        }
    }

    /// The driver picks the format by name ("PNG", "JPEG"…).
    pub fn set_format(&mut self, name: &str) {
        if let Some(f) = ExportFormat::ALL.iter().find(|f| f.name().eq_ignore_ascii_case(name)) {
            self.opts.format = *f;
        }
    }

    pub(super) fn ui(&mut self, ui: &mut egui::Ui) -> bool {
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
        let note = |ui: &mut egui::Ui, id: &str| {
            ui.add(egui::Label::new(egui::RichText::new(t(id)).small().weak()).wrap());
        };
        if o.format == ExportFormat::Jpeg {
            note(ui, "export-jpeg-note");
        }
        if matches!(o.format, ExportFormat::Tiff | ExportFormat::Bmp) {
            note(ui, "export-no-exif-note");
        }
        if self.redactions {
            note(ui, "export-redactions-note");
        }
        false
    }
}
