//! Start screen: pick the image folder, manage targets and their hotkeys.

use crate::config::{self, Config, RESERVED};
use crate::files::list_images;
use crate::theme;
use eframe::egui::{self, Key, RichText};
use std::path::{Path, PathBuf};

struct Row {
    key: Option<Key>,
    /// Empty for the Delete row means `<image folder>/_deleted`.
    path: String,
}

pub struct Setup {
    source: String,
    rows: Vec<Row>,
    /// Row waiting for its hotkey to be pressed.
    capturing: Option<usize>,
    pub error: Option<String>,
    recent: Vec<PathBuf>,
    max_side: u32,
    ahead: usize,
    /// Source the rows/info below were last loaded for.
    loaded_for: String,
    info: String,
}

fn pick_folder(start: &str) -> Option<String> {
    let mut dialog = rfd::FileDialog::new();
    if Path::new(start).is_dir() {
        dialog = dialog.set_directory(start);
    }
    dialog.pick_folder().map(|p| p.display().to_string())
}

const ROW_H: f32 = 28.0;
const KEY_W: f32 = 100.0;
const BTN_W: f32 = 80.0;

impl Setup {
    pub fn new(source: Option<&Path>, max_side: u32, ahead: usize, error: Option<String>) -> Self {
        let mut s = Self {
            source: source.map(|p| p.display().to_string()).unwrap_or_default(),
            rows: vec![Row {
                key: Some(Key::Delete),
                path: String::new(),
            }],
            capturing: None,
            error,
            recent: config::recent_projects(),
            max_side,
            ahead,
            loaded_for: String::new(),
            info: String::new(),
        };
        s.refresh();
        s
    }

    /// Re-read folder info, and the saved targets if the folder has any.
    fn refresh(&mut self) {
        if self.loaded_for == self.source {
            return;
        }
        self.loaded_for = self.source.clone();
        let src = Path::new(&self.source);
        self.info = match list_images(src) {
            Ok(names) if !self.source.is_empty() => format!("{} images to sort", names.len()),
            _ => String::new(),
        };
        let saved = config::saved_bindings(src);
        if !saved.is_empty() {
            let default_del = config::default_delete_dir(src);
            self.rows = saved
                .into_iter()
                .map(|(key, dir)| {
                    let path = if key == Key::Delete && dir == default_del {
                        String::new()
                    } else {
                        dir.display().to_string()
                    };
                    Row {
                        key: Some(key),
                        path,
                    }
                })
                .collect();
        }
    }

    fn assign_key(&mut self, row: usize, key: Key) {
        if key == Key::Escape {
            self.capturing = None;
            return;
        }
        if RESERVED.contains(&key) {
            self.error = Some(format!(
                "'{}' is reserved for navigation, pick another key",
                key.name()
            ));
            return;
        }
        // Steal the key from any other row instead of complaining.
        for r in &mut self.rows {
            if r.key == Some(key) {
                r.key = None;
            }
        }
        self.rows[row].key = Some(key);
        self.capturing = None;
        self.error = None;
    }

    fn try_start(&self) -> Result<Config, String> {
        let source = PathBuf::from(self.source.trim());
        if self.source.trim().is_empty() {
            return Err("choose an image folder".into());
        }
        let mut bindings = Vec::new();
        for (i, r) in self.rows.iter().enumerate() {
            let key = r
                .key
                .ok_or_else(|| format!("target {} has no key", i + 1))?;
            let dir = match r.path.trim() {
                "" if key == Key::Delete => config::default_delete_dir(&source),
                "" => return Err(format!("target {} ({}) has no folder", i + 1, key.name())),
                p => PathBuf::from(p),
            };
            bindings.push((key, dir));
        }
        config::build_config(&source, bindings, self.max_side, self.ahead)
    }

    /// Draws the screen; returns a config once the user presses Start.
    pub fn ui(&mut self, ui: &mut egui::Ui, logo: &egui::TextureHandle) -> Option<Config> {
        if let Some(row) = self.capturing {
            let pressed = ui.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key {
                        key, pressed: true, ..
                    } => Some(*key),
                    _ => None,
                })
            });
            if let Some(key) = pressed {
                self.assign_key(row, key);
            }
        }

        let mut start = None;
        let frame = egui::Frame::new()
            .fill(theme::PANEL)
            .inner_margin(egui::Margin::symmetric(24, 20));
        egui::CentralPanel::no_frame().frame(frame).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    // Centered column with a comfortable maximum width.
                    let width = ui.available_width().min(860.0);
                    let pad = ((ui.available_width() - width) / 2.0).max(0.0);
                    ui.horizontal(|ui| {
                        ui.add_space(pad);
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            Self::header(ui, logo);
                            ui.add_space(16.0);
                            self.folder_card(ui);
                            ui.add_space(12.0);
                            self.targets_card(ui);
                            ui.add_space(12.0);
                            self.performance_card(ui);
                            ui.add_space(16.0);

                            ui.horizontal(|ui| {
                                if theme::primary_button(ui, "Start sorting").clicked() {
                                    match self.try_start() {
                                        Ok(cfg) => start = Some(cfg),
                                        Err(e) => self.error = Some(e),
                                    }
                                }
                                if let Some(e) = &self.error {
                                    ui.add_space(8.0);
                                    ui.colored_label(theme::DANGER, e);
                                }
                            });

                            if !self.recent.is_empty() {
                                ui.add_space(20.0);
                                self.recent_card(ui);
                            }
                            ui.add_space(12.0);
                        });
                    });
                });
        });
        start
    }

    fn header(ui: &mut egui::Ui, logo: &egui::TextureHandle) {
        ui.horizontal(|ui| {
            ui.add(egui::Image::new((logo.id(), egui::vec2(56.0, 56.0))));
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.add_space(6.0);
                ui.label(
                    RichText::new("swiper")
                        .size(26.0)
                        .strong()
                        .color(theme::TEXT),
                );
                ui.label(
                    RichText::new("Sort image datasets with a single key press")
                        .color(theme::MUTED),
                );
            });
        });
    }

    fn folder_card(&mut self, ui: &mut egui::Ui) {
        theme::card(ui, |ui| {
            theme::section(ui, "Image folder");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let w = ui.available_width() - BTN_W - 8.0;
                ui.add_sized(
                    [w, ROW_H],
                    egui::TextEdit::singleline(&mut self.source)
                        .hint_text("folder with the images to sort")
                        .align(egui::Align2::LEFT_CENTER),
                );
                if ui
                    .add_sized([BTN_W, ROW_H], egui::Button::new("Browse…"))
                    .clicked()
                {
                    if let Some(p) = pick_folder(&self.source) {
                        self.source = p;
                    }
                }
            });
            self.refresh();
            if !self.info.is_empty() {
                ui.label(RichText::new(&self.info).color(theme::ACCENT));
            }
        });
    }

    fn targets_card(&mut self, ui: &mut egui::Ui) {
        theme::card(ui, |ui| {
            theme::section(ui, "Targets");
            ui.weak("Click a key, then press the hotkey you want for that target.");
            ui.add_space(6.0);
            let mut remove = None;
            let path_w = (ui.available_width() - KEY_W - 2.0 * BTN_W - 3.0 * 8.0).max(160.0);
            for (i, row) in self.rows.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let (label, fill, color) = match (self.capturing == Some(i), row.key) {
                        (true, _) => ("press a key…".to_owned(), theme::ACCENT_DIM, theme::TEXT),
                        (false, Some(k)) => (
                            k.symbol_or_name().to_owned(),
                            theme::target_color(k),
                            theme::BG,
                        ),
                        (false, None) => ("set key".to_owned(), theme::SURFACE_HI, theme::WARN),
                    };
                    let key_btn =
                        egui::Button::new(RichText::new(label).monospace().strong().color(color))
                            .fill(fill);
                    if ui.add_sized([KEY_W, ROW_H], key_btn).clicked() {
                        self.capturing = Some(i);
                    }
                    let hint = if row.key == Some(Key::Delete) {
                        "<image folder>\\_deleted"
                    } else {
                        "destination folder"
                    };
                    ui.add_sized(
                        [path_w, ROW_H],
                        egui::TextEdit::singleline(&mut row.path)
                            .hint_text(hint)
                            .align(egui::Align2::LEFT_CENTER),
                    );
                    if ui
                        .add_sized([BTN_W, ROW_H], egui::Button::new("Browse…"))
                        .clicked()
                    {
                        if let Some(p) = pick_folder(&row.path) {
                            row.path = p;
                        }
                    }
                    let rm = egui::Button::new(RichText::new("Remove").color(theme::DANGER));
                    if ui.add_sized([BTN_W, ROW_H], rm).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                self.rows.remove(i);
                self.capturing = None;
            }
            ui.add_space(4.0);
            if ui
                .button(RichText::new("+  Add target").color(theme::ACCENT))
                .clicked()
            {
                if let Some(p) = pick_folder(&self.source) {
                    self.rows.push(Row { key: None, path: p });
                    self.capturing = Some(self.rows.len() - 1);
                }
            }
        });
    }

    fn performance_card(&mut self, ui: &mut egui::Ui) {
        theme::card(ui, |ui| {
            theme::section(ui, "Performance");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Preview size");
                ui.add(
                    egui::DragValue::new(&mut self.max_side)
                        .range(256..=8192)
                        .speed(16)
                        .suffix(" px"),
                );
                ui.add_space(16.0);
                ui.label("Preload");
                ui.add(
                    egui::DragValue::new(&mut self.ahead)
                        .range(1..=32)
                        .suffix(" images"),
                );
            });
            ui.weak("Zooming in loads the full-resolution file on demand.");
        });
    }

    fn recent_card(&mut self, ui: &mut egui::Ui) {
        theme::card(ui, |ui| {
            theme::section(ui, "Recent projects");
            ui.add_space(4.0);
            let mut forget = None;
            for p in &self.recent {
                ui.horizontal(|ui| {
                    if ui.link(p.display().to_string()).clicked() {
                        self.source = p.display().to_string();
                    }
                    if ui
                        .small_button(RichText::new("forget").color(theme::MUTED))
                        .clicked()
                    {
                        forget = Some(p.clone());
                    }
                });
            }
            if let Some(p) = forget {
                config::forget_recent(&p);
                self.recent.retain(|r| *r != p);
            }
        });
    }
}
