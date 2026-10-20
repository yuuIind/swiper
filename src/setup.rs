//! Start screen: pick the image folder, manage targets and their hotkeys.

use crate::config::{self, Config, RESERVED};
use crate::files::list_images;
use eframe::egui::{self, Key};
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

impl Setup {
    pub fn new(source: Option<&Path>, max_side: u32, ahead: usize, error: Option<String>) -> Self {
        let mut s = Self {
            source: source.map(|p| p.display().to_string()).unwrap_or_default(),
            rows: vec![Row { key: Some(Key::Delete), path: String::new() }],
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
                    Row { key: Some(key), path }
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
            self.error = Some(format!("'{}' is reserved for navigation, pick another key", key.name()));
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
            let key = r.key.ok_or_else(|| format!("target {} has no key", i + 1))?;
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
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<Config> {
        if let Some(row) = self.capturing {
            let pressed = ui.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key, pressed: true, .. } => Some(*key),
                    _ => None,
                })
            });
            if let Some(key) = pressed {
                self.assign_key(row, key);
            }
        }

        let mut start = None;
        egui::CentralPanel::default_margins().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("swiper");
                ui.add_space(8.0);

                ui.strong("Image folder");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.source)
                            .hint_text("folder with the images to sort")
                            .desired_width(520.0),
                    );
                    if ui.button("Browse…").clicked() {
                        if let Some(p) = pick_folder(&self.source) {
                            self.source = p;
                        }
                    }
                });
                self.refresh();
                if !self.info.is_empty() {
                    ui.weak(&self.info);
                }

                ui.add_space(12.0);
                ui.strong("Targets");
                ui.weak("Click a key button, then press the hotkey you want for that target.");
                let mut remove = None;
                egui::Grid::new("targets").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
                    for (i, row) in self.rows.iter_mut().enumerate() {
                        let label = match (self.capturing == Some(i), row.key) {
                            (true, _) => "press a key…".to_owned(),
                            (false, Some(k)) => k.symbol_or_name().to_owned(),
                            (false, None) => "set key".to_owned(),
                        };
                        if ui.add(egui::Button::new(label).min_size(egui::vec2(90.0, 0.0))).clicked() {
                            self.capturing = Some(i);
                        }
                        let hint = if row.key == Some(Key::Delete) {
                            "<image folder>\\_deleted"
                        } else {
                            "destination folder"
                        };
                        ui.add_sized([420.0, 20.0], egui::TextEdit::singleline(&mut row.path).hint_text(hint));
                        if ui.button("Browse…").clicked() {
                            if let Some(p) = pick_folder(&row.path) {
                                row.path = p;
                            }
                        }
                        if ui.button("Remove").clicked() {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                });
                if let Some(i) = remove {
                    self.rows.remove(i);
                    self.capturing = None;
                }
                if ui.button("+ Add target").clicked() {
                    if let Some(p) = pick_folder(&self.source) {
                        self.rows.push(Row { key: None, path: p });
                        self.capturing = Some(self.rows.len() - 1);
                    }
                }

                ui.add_space(12.0);
                egui::CollapsingHeader::new("Performance").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Preview size");
                        ui.add(egui::DragValue::new(&mut self.max_side).range(256..=8192).speed(16).suffix(" px"));
                        ui.weak("(zooming loads full resolution on demand)");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Preload");
                        ui.add(egui::DragValue::new(&mut self.ahead).range(1..=32).suffix(" images"));
                    });
                });

                ui.add_space(12.0);
                if let Some(e) = &self.error {
                    ui.colored_label(egui::Color32::from_rgb(255, 110, 110), e);
                }
                if ui.add(egui::Button::new(egui::RichText::new("Start sorting").strong()).min_size(egui::vec2(160.0, 32.0))).clicked() {
                    match self.try_start() {
                        Ok(cfg) => start = Some(cfg),
                        Err(e) => self.error = Some(e),
                    }
                }

                if !self.recent.is_empty() {
                    ui.add_space(16.0);
                    ui.separator();
                    ui.strong("Recent projects");
                    let mut forget = None;
                    for p in &self.recent {
                        ui.horizontal(|ui| {
                            if ui.link(p.display().to_string()).clicked() {
                                self.source = p.display().to_string();
                            }
                            if ui.small_button("forget").clicked() {
                                forget = Some(p.clone());
                            }
                        });
                    }
                    if let Some(p) = forget {
                        config::forget_recent(&p);
                        self.recent.retain(|r| *r != p);
                    }
                }
            });
        });
        start
    }
}
