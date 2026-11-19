//! Settings window: tabs Window, Image, Miscellaneous (with the Markup section) and
//! Shortcuts; OK / Cancel / Apply / Restore Defaults. Works on a draft; nothing changes in the app
//! until Apply or OK.

use super::tool::ToolOut;
use crate::actions::{self, ACTIONS, Action, Enable};
use crate::i18n::{self, t, tf};
use rv_core::settings::*;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Window,
    Image,
    Misc,
    Shortcuts,
}

pub struct Options {
    pub draft: Settings,
    /// Effective shortcuts of every action (defaults or custom), by action key.
    pub keys: BTreeMap<String, Vec<String>>,
    pub tab: Tab,
    recording: Option<Action>,
}

/// Default keys of an action, normalised.
fn defaults(a: Action) -> Vec<String> {
    actions::info(a)
        .default_keys
        .iter()
        .filter_map(|k| crate::shortcuts::normalize(k))
        .collect()
}

/// Effective keys of every action from the custom map (missing = defaults).
pub fn effective(custom: &BTreeMap<String, Vec<String>>) -> BTreeMap<String, Vec<String>> {
    ACTIONS
        .iter()
        .map(|(a, i)| {
            let keys = match custom.get(i.key) {
                Some(list) => list.iter().filter_map(|k| crate::shortcuts::normalize(k)).collect(),
                None => defaults(*a),
            };
            (i.key.to_string(), keys)
        })
        .collect()
}

/// Only the actions whose keys differ from the defaults (what state.toml keeps).
pub fn custom_of(keys: &BTreeMap<String, Vec<String>>) -> BTreeMap<String, Vec<String>> {
    ACTIONS
        .iter()
        .filter_map(|(a, i)| {
            let k = keys.get(i.key)?;
            (*k != defaults(*a)).then(|| (i.key.to_string(), k.clone()))
        })
        .collect()
}

/// Sequences used by more than one action that can be enabled at the same time: action key →
/// the other actions' keys. A key shared between a Markup-only action and a viewer action is fine (Ctrl+S).
pub fn conflicts(keys: &BTreeMap<String, Vec<String>>) -> HashMap<String, Vec<String>> {
    let mut by_seq: HashMap<&str, Vec<Action>> = HashMap::new();
    for (a, i) in ACTIONS {
        for k in keys.get(i.key).into_iter().flatten() {
            by_seq.entry(k).or_default().push(*a);
        }
    }
    let markup = |a: Action| actions::info(a).enable == Enable::InMarkup;
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for list in by_seq.values() {
        for &a in list {
            for &b in list {
                if a != b && markup(a) == markup(b) {
                    out.entry(actions::info(a).key.to_string())
                        .or_default()
                        .push(actions::info(b).key.to_string());
                }
            }
        }
    }
    out
}

impl Options {
    pub fn new(settings: &Settings, custom: &BTreeMap<String, Vec<String>>) -> Options {
        Options {
            draft: settings.clone(),
            keys: effective(custom),
            tab: Tab::Window,
            recording: None,
        }
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    /// The key combination pressed while recording becomes the action's only shortcut.
    pub fn record(&mut self, seq: String) {
        if let Some(a) = self.recording.take() {
            self.keys.insert(actions::info(a).key.to_string(), vec![seq]);
        }
    }

    fn apply(&self) -> ToolOut {
        ToolOut::Apply {
            settings: self.draft.clone().sanitized(),
            shortcuts: custom_of(&self.keys),
        }
    }

    pub fn press(&mut self, button: &str) -> Vec<ToolOut> {
        let tab = match button {
            "tab-window" => Some(Tab::Window),
            "tab-image" => Some(Tab::Image),
            "tab-misc" => Some(Tab::Misc),
            "tab-shortcuts" => Some(Tab::Shortcuts),
            _ => None,
        };
        if let Some(t) = tab {
            self.tab = t;
            return vec![];
        }
        match button {
            "ok" => vec![self.apply(), ToolOut::Close],
            "apply" => vec![self.apply()],
            "defaults" => {
                self.draft = Settings::default();
                self.keys = effective(&BTreeMap::new());
                vec![]
            }
            _ => vec![ToolOut::Close], // cancel, window closed
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Vec<ToolOut> {
        let mut outs = vec![];
        ui.horizontal(|ui| {
            for (tab, id) in [
                (Tab::Window, "settings-tab-window"),
                (Tab::Image, "settings-tab-image"),
                (Tab::Misc, "settings-tab-misc"),
                (Tab::Shortcuts, "settings-tab-shortcuts"),
            ] {
                ui.selectable_value(&mut self.tab, tab, t(id));
            }
        });
        ui.separator();
        let bottom = 44.0;
        let height = (ui.available_height() - bottom).max(100.0);
        egui::ScrollArea::vertical()
            .max_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| match self.tab {
                Tab::Window => self.window_tab(ui),
                Tab::Image => self.image_tab(ui),
                Tab::Misc => self.misc_tab(ui),
                Tab::Shortcuts => self.shortcuts_tab(ui),
            });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(t("button-restore-defaults")).clicked() {
                outs.extend(self.press("defaults"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t("button-apply")).clicked() {
                    outs.extend(self.press("apply"));
                }
                if ui.button(t("button-cancel")).clicked() {
                    outs.extend(self.press("cancel"));
                }
                if ui.button(t("button-ok")).clicked() {
                    outs.extend(self.press("ok"));
                }
            });
        });
        outs
    }

    fn window_tab(&mut self, ui: &mut egui::Ui) {
        let d = &mut self.draft;
        grid(ui, "window", |ui| {
            row(ui, "settings-bgcolor", |ui| {
                ui.checkbox(&mut d.bg_color_enabled, t("settings-bgcolor-custom"));
                ui.add_enabled_ui(d.bg_color_enabled, |ui| {
                    super::color::edit_button(ui, &mut d.bg_color);
                });
            });
            row(ui, "settings-titlebarmode", |ui| {
                combo(
                    ui,
                    "titlebarmode",
                    &mut d.title_bar_mode,
                    &[
                        (TitleBarMode::Full, "settings-titlebar-full"),
                        (TitleBarMode::Name, "settings-titlebar-name"),
                    ],
                )
            });
            row(ui, "settings-windowresizemode", |ui| {
                combo(
                    ui,
                    "windowresizemode",
                    &mut d.window_resize_mode,
                    &[
                        (WindowResizeMode::Never, "settings-resize-never"),
                        (WindowResizeMode::WhenLaunching, "settings-resize-launching"),
                        (WindowResizeMode::WhenOpeningImages, "settings-resize-opening"),
                    ],
                )
            });
            row(ui, "settings-resize-limits", |ui| {
                ui.add(
                    egui::DragValue::new(&mut d.min_window_resized_percentage)
                        .range(1..=100)
                        .suffix(" %"),
                );
                ui.label("–");
                ui.add(
                    egui::DragValue::new(&mut d.max_window_resized_percentage)
                        .range(1..=100)
                        .suffix(" %"),
                );
            });
            check(ui, "settings-menubar", &mut d.menubar_enabled);
            check(ui, "settings-fullscreendetails", &mut d.fullscreen_details);
        });
    }

    fn image_tab(&mut self, ui: &mut egui::Ui) {
        let d = &mut self.draft;
        grid(ui, "image", |ui| {
            check(ui, "settings-filtering", &mut d.filtering_enabled);
            check(ui, "settings-scaling", &mut d.scaling_enabled);
            check(ui, "settings-scalingtwo", &mut d.scaling_two_enabled);
            row(ui, "settings-scalefactor", |ui| {
                ui.add(egui::DragValue::new(&mut d.scale_factor).range(1..=500).suffix(" %"));
            });
            row(ui, "settings-scrollzoom", |ui| {
                combo(
                    ui,
                    "scrollzoom",
                    &mut d.scroll_zoom,
                    &[
                        (ScrollZoom::Zoom, "settings-scroll-zoom"),
                        (ScrollZoom::Auto, "settings-scroll-auto"),
                        (ScrollZoom::Scroll, "settings-scroll-scroll"),
                    ],
                )
            });
            check(ui, "settings-fractionalzoom", &mut d.fractional_zoom);
            check(ui, "settings-cursorzoom", &mut d.cursor_zoom);
            row(ui, "settings-cropmode", |ui| {
                combo(
                    ui,
                    "cropmode",
                    &mut d.crop_mode,
                    &[
                        (CropMode::Whole, "settings-crop-whole"),
                        (CropMode::Height, "settings-crop-height"),
                        (CropMode::Width, "settings-crop-width"),
                    ],
                )
            });
            check(ui, "settings-pastactualsize", &mut d.past_actual_size_enabled);
            row(ui, "settings-colorspace", |ui| {
                combo(
                    ui,
                    "colorspace",
                    &mut d.color_space_conversion,
                    &[
                        (ColorSpaceConversion::Disabled, "settings-colorspace-disabled"),
                        (ColorSpaceConversion::Auto, "settings-colorspace-auto"),
                        (ColorSpaceConversion::Srgb, "settings-colorspace-srgb"),
                        (ColorSpaceConversion::DisplayP3, "settings-colorspace-p3"),
                    ],
                )
            });
        });
    }

    fn misc_tab(&mut self, ui: &mut egui::Ui) {
        let d = &mut self.draft;
        grid(ui, "misc", |ui| {
            row(ui, "settings-language", |ui| {
                let label = |code: &str| {
                    if code == "system" {
                        t("settings-language-system")
                    } else {
                        i18n::language_name(code)
                    }
                };
                egui::ComboBox::from_id_salt("language")
                    .selected_text(label(&d.language))
                    .show_ui(ui, |ui| {
                        for code in std::iter::once("system").chain(i18n::available()) {
                            ui.selectable_value(&mut d.language, code.to_string(), label(code));
                        }
                    });
            });
            row(ui, "settings-sortmode", |ui| {
                combo(
                    ui,
                    "sortmode",
                    &mut d.sort_mode,
                    &[
                        (SortMode::Name, "settings-sort-name"),
                        (SortMode::Modified, "settings-sort-modified"),
                        (SortMode::Created, "settings-sort-created"),
                        (SortMode::Size, "settings-sort-size"),
                        (SortMode::Type, "settings-sort-type"),
                        (SortMode::Random, "settings-sort-random"),
                    ],
                );
                ui.checkbox(&mut d.sort_descending, t("settings-sortdescending"));
            });
            row(ui, "settings-preloading", |ui| {
                combo(
                    ui,
                    "preloading",
                    &mut d.preloading_mode,
                    &[
                        (PreloadingMode::Off, "settings-preload-off"),
                        (PreloadingMode::Adjacent, "settings-preload-adjacent"),
                        (PreloadingMode::Extended, "settings-preload-extended"),
                    ],
                )
            });
            check(ui, "settings-loopfolders", &mut d.loop_folders_enabled);
            row(ui, "settings-slideshow", |ui| {
                ui.add(
                    egui::DragValue::new(&mut d.slideshow_timer)
                        .range(0.1..=3600.0)
                        .speed(0.1)
                        .suffix(" s"),
                );
                ui.checkbox(&mut d.slideshow_reversed, t("settings-slideshowreversed"));
            });
            row(ui, "settings-afterdelete", |ui| {
                combo(
                    ui,
                    "afterdelete",
                    &mut d.after_delete,
                    &[
                        (AfterDelete::MoveBack, "settings-afterdelete-back"),
                        (AfterDelete::DoNothing, "settings-afterdelete-nothing"),
                        (AfterDelete::MoveForward, "settings-afterdelete-forward"),
                    ],
                )
            });
            check(ui, "settings-askdelete", &mut d.ask_delete);
            check(ui, "settings-mime", &mut d.allow_mime_content_detection);
            check(ui, "settings-saverecents", &mut d.save_recents);
            check(ui, "settings-skiphidden", &mut d.skip_hidden);
        });
        ui.add_space(10.0);
        ui.label(egui::RichText::new(t("settings-markup")).strong());
        grid(ui, "markup", |ui| {
            row(ui, "settings-metadata", |ui| {
                combo(
                    ui,
                    "metadata",
                    &mut d.markup.metadata_preset,
                    &[
                        (MetadataPreset::Keep, "settings-metadata-keep"),
                        (MetadataPreset::StripLocation, "settings-metadata-location"),
                        (MetadataPreset::StripAll, "settings-metadata-all"),
                    ],
                )
            });
            row(ui, "settings-jpegquality", |ui| {
                ui.add(egui::Slider::new(&mut d.markup.jpeg_quality, 1..=100));
            });
            check(ui, "settings-askoverwrite", &mut d.markup.ask_overwrite);
        });
    }

    fn shortcuts_tab(&mut self, ui: &mut egui::Ui) {
        if let Some(a) = self.recording {
            ui.horizontal(|ui| {
                let name = i18n::plain(&actions::name(a));
                ui.label(egui::RichText::new(tf("settings-shortcut-recording", &[("action", &name)])).strong());
                if ui.button(t("settings-shortcut-clear")).clicked() {
                    self.keys.insert(actions::info(a).key.to_string(), vec![]);
                    self.recording = None;
                }
                if ui.button(t("button-cancel")).clicked() {
                    self.recording = None;
                }
            });
        } else {
            ui.label(egui::RichText::new(t("settings-shortcut-hint")).weak());
        }
        ui.add_space(4.0);
        let conflicts = conflicts(&self.keys);
        egui::Grid::new("shortcut table")
            .num_columns(2)
            .striped(true)
            .spacing([24.0, 4.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new(t("settings-shortcut-action")).strong());
                ui.label(egui::RichText::new(t("settings-shortcut-keys")).strong());
                ui.end_row();
                for (a, info) in ACTIONS {
                    let name = i18n::plain(&actions::name(*a));
                    let keys = self.keys.get(info.key).cloned().unwrap_or_default().join(", ");
                    let selected = self.recording == Some(*a);
                    let r = ui.selectable_label(selected, name);
                    let mut text = egui::RichText::new(if keys.is_empty() { "—".to_string() } else { keys });
                    if let Some(others) = conflicts.get(info.key) {
                        text = text.color(ui.visuals().error_fg_color);
                        let names: Vec<String> = others
                            .iter()
                            .filter_map(|k| actions::from_key(k))
                            .map(|o| i18n::plain(&actions::name(o)))
                            .collect();
                        let r2 = ui.label(text);
                        r2.on_hover_text(tf("settings-shortcut-conflict", &[("actions", &names.join(", "))]));
                    } else {
                        ui.label(text);
                    }
                    if r.double_clicked() {
                        self.recording = Some(*a);
                    }
                    ui.end_row();
                }
            });
    }
}

fn grid(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id).num_columns(2).spacing([20.0, 8.0]).show(ui, add);
}

fn row(ui: &mut egui::Ui, label_id: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.label(t(label_id));
    ui.horizontal(add);
    ui.end_row();
}

fn check(ui: &mut egui::Ui, label_id: &str, value: &mut bool) {
    ui.label("");
    ui.checkbox(value, t(label_id));
    ui.end_row();
}

fn combo<T: Copy + PartialEq>(ui: &mut egui::Ui, id: &str, value: &mut T, choices: &[(T, &str)]) {
    let current = choices
        .iter()
        .find(|(v, _)| v == value)
        .map(|(_, l)| t(l))
        .unwrap_or_default();
    egui::ComboBox::from_id_salt(id)
        .selected_text(current)
        .show_ui(ui, |ui| {
            for (v, label) in choices {
                ui.selectable_value(value, *v, t(label));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_shortcuts_and_conflicts() {
        let none = BTreeMap::new();
        let keys = effective(&none);
        assert_eq!(keys["nextfile"], ["Right"]);
        assert!(custom_of(&keys).is_empty(), "defaults are not stored");
        assert!(
            conflicts(&keys).is_empty(),
            "the default table has no conflict (Ctrl+S is fine)"
        );

        let mut keys = keys;
        keys.insert("nextfile".into(), vec!["Space".into()]);
        keys.insert("zoomin".into(), vec![]);
        keys.insert("mirror".into(), vec!["Space".into()]);
        let custom = custom_of(&keys);
        assert_eq!(custom.len(), 3);
        assert_eq!(effective(&custom), keys, "round trip through the stored map");
        let c = conflicts(&keys);
        assert_eq!(c["nextfile"], ["mirror"]);
        assert_eq!(c["mirror"], ["nextfile"]);

        let mut o = Options::new(&Settings::default(), &custom);
        o.recording = Some(Action::Flip);
        o.record("Ctrl+Shift+F".into());
        assert!(!o.is_recording());
        let out = o.press("ok");
        let ToolOut::Apply { shortcuts, .. } = &out[0] else {
            panic!()
        };
        assert_eq!(shortcuts["flip"], ["Ctrl+Shift+F"]);
        assert_eq!(out[1], ToolOut::Close);
        assert!(o.press("defaults").is_empty());
        assert!(custom_of(&o.keys).is_empty() && o.draft == Settings::default());
    }
}
