//! The sorting screen: shows images, handles hotkeys, preloads ahead, moves
//! files in the background and lets you zoom into the current image.

use crate::config::Config;
use crate::files::{list_images, move_file, natural_cmp, read_log, unique_dest};
use eframe::egui;
use image::{DynamicImage, ImageDecoder, ImageReader, metadata::Orientation};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

const BEHIND: usize = 2;
/// Maximum magnification, in screen pixels per image pixel.
const MAX_PIXEL_ZOOM: f32 = 16.0;

#[derive(Clone, Copy, PartialEq)]
enum State {
    Pending,
    Skipped,
    Moved,
}

struct Item {
    name: String,
    state: State,
}

// ---------------------------------------------------------------------------
// File-operation worker: all moves happen off the UI thread, strictly in order.

enum Op {
    Move { idx: usize, name: String, dir: PathBuf },
    Undo { idx: usize, target: Option<usize>, name: String },
    Skip { name: String },
}

enum Report {
    MoveFailed(usize, String),
    UndoFailed(usize, Option<usize>, String),
    Error(String),
}

fn spawn_file_worker(
    source: PathBuf,
    log_path: PathBuf,
    mut moved: HashMap<String, PathBuf>,
    reports: Sender<Report>,
    ctx: egui::Context,
) -> (Sender<Op>, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<Op>();
    let handle = std::thread::spawn(move || {
        let mut log = match OpenOptions::new().create(true).append(true).open(&log_path) {
            Ok(f) => Some(f),
            Err(e) => {
                let _ = reports.send(Report::Error(format!("cannot open log: {e}")));
                None
            }
        };
        let report = |r: Report| {
            let _ = reports.send(r);
            ctx.request_repaint();
        };
        for op in rx {
            let line = match op {
                Op::Move { idx, name, dir } => {
                    let dst = unique_dest(&dir, &name);
                    match move_file(&source.join(&name), &dst) {
                        Ok(()) => {
                            let l = format!("M\t{name}\t{}\n", dst.display());
                            moved.insert(name, dst);
                            l
                        }
                        Err(e) => {
                            report(Report::MoveFailed(idx, format!("moving {name}: {e}")));
                            continue;
                        }
                    }
                }
                Op::Undo { idx, target, name } => {
                    let Some(dst) = moved.get(&name) else {
                        report(Report::UndoFailed(idx, target, format!("undo {name}: unknown destination")));
                        continue;
                    };
                    match move_file(dst, &source.join(&name)) {
                        Ok(()) => {
                            moved.remove(&name);
                            format!("U\t{name}\n")
                        }
                        Err(e) => {
                            report(Report::UndoFailed(idx, target, format!("undo {name}: {e}")));
                            continue;
                        }
                    }
                }
                Op::Skip { name } => format!("S\t{name}\n"),
            };
            if let Some(f) = log.as_mut() {
                if let Err(e) = f.write_all(line.as_bytes()) {
                    report(Report::Error(format!("writing log: {e}")));
                }
            }
        }
    });
    (tx, handle)
}

// ---------------------------------------------------------------------------
// Decoder pool: decodes + downscales upcoming images on all spare cores.

struct Job {
    idx: usize,
    path: PathBuf,
    max_side: u32,
    full: bool,
}

struct Decoded {
    idx: usize,
    full: bool,
    result: Result<(egui::ColorImage, [u32; 2]), String>,
}

/// Returns the (possibly downscaled) image and its original, upright size.
fn decode(path: &Path, max_side: u32) -> Result<(egui::ColorImage, [u32; 2]), String> {
    let err = |e: &dyn std::fmt::Display| e.to_string();
    let mut dec = ImageReader::open(path)
        .map_err(|e| err(&e))?
        .with_guessed_format()
        .map_err(|e| err(&e))?
        .into_decoder()
        .map_err(|e| err(&e))?;
    let orientation = dec.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(dec).map_err(|e| err(&e))?;
    let mut orig = [img.width(), img.height()];
    if matches!(
        orientation,
        Orientation::Rotate90 | Orientation::Rotate270 | Orientation::Rotate90FlipH | Orientation::Rotate270FlipH
    ) {
        orig.swap(0, 1);
    }
    if img.width() > max_side || img.height() > max_side {
        img = img.thumbnail(max_side, max_side);
    }
    img.apply_orientation(orientation);
    let rgba = img.into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok((egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()), orig))
}

fn spawn_decoders(wanted: Arc<Mutex<HashSet<usize>>>, out: Sender<Decoded>, ctx: egui::Context) -> Sender<Job> {
    let (tx, rx) = mpsc::channel::<Job>();
    let rx = Arc::new(Mutex::new(rx));
    let n = std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(1).clamp(1, 8));
    for _ in 0..n {
        let (rx, wanted, out, ctx) = (rx.clone(), wanted.clone(), out.clone(), ctx.clone());
        std::thread::spawn(move || loop {
            let Ok(job) = rx.lock().unwrap().recv() else { return };
            // The user may have raced past this image already.
            if !wanted.lock().unwrap().contains(&job.idx) {
                continue;
            }
            let result = decode(&job.path, job.max_side);
            if out.send(Decoded { idx: job.idx, full: job.full, result }).is_err() {
                return;
            }
            ctx.request_repaint();
        });
    }
    tx
}

// ---------------------------------------------------------------------------

enum Slot {
    Loading,
    Ready(egui::TextureHandle, [u32; 2]),
    Failed(String),
}

pub struct Sorter {
    pub cfg: Config,
    items: Vec<Item>,
    cursor: Option<usize>,
    // (item, target it was sent to this session — None for moves from earlier sessions)
    undo: Vec<(usize, Option<usize>)>,
    counts: Vec<usize>,
    cache: HashMap<usize, Slot>,
    /// Full-resolution texture of the current image, loaded when zooming in.
    hires: Option<(usize, Slot)>,
    wanted: Arc<Mutex<HashSet<usize>>>,
    jobs: Sender<Job>,
    decoded: Receiver<Decoded>,
    ops: Sender<Op>,
    worker: Option<JoinHandle<()>>,
    reports: Receiver<Report>,
    message: Option<String>,
    /// 1 = fit to window.
    zoom: f32,
    pan: egui::Vec2,
    view_idx: Option<usize>,
}

impl Sorter {
    pub fn new(ctx: &egui::Context, cfg: Config) -> Result<Self, String> {
        let log_path = cfg.state_dir.join("log.tsv");
        let mut history = read_log(&log_path);
        let names = list_images(&cfg.source).map_err(|e| format!("reading folder: {e}"))?;

        // A file that is back in the source folder is not "moved", whatever the log says.
        for n in &names {
            history.moved.remove(n);
        }
        history.undo.retain(|n| history.moved.contains_key(n));

        let mut items: Vec<Item> = names
            .into_iter()
            .map(|name| {
                let state = if history.skipped.contains(&name) { State::Skipped } else { State::Pending };
                Item { name, state }
            })
            .chain(history.moved.keys().map(|n| Item { name: n.clone(), state: State::Moved }))
            .collect();
        items.sort_by(|a, b| natural_cmp(&a.name, &b.name));

        let index: HashMap<&str, usize> =
            items.iter().enumerate().map(|(i, it)| (it.name.as_str(), i)).collect();
        let undo = history.undo.iter().filter_map(|n| index.get(n.as_str()).map(|&i| (i, None))).collect();

        let (dec_tx, dec_rx) = mpsc::channel();
        let (rep_tx, rep_rx) = mpsc::channel();
        let wanted = Arc::new(Mutex::new(HashSet::new()));
        let jobs = spawn_decoders(wanted.clone(), dec_tx, ctx.clone());
        let (ops, worker) = spawn_file_worker(cfg.source.clone(), log_path, history.moved, rep_tx, ctx.clone());

        let mut s = Self {
            counts: vec![0; cfg.targets.len()],
            cfg,
            items,
            cursor: None,
            undo,
            cache: HashMap::new(),
            hires: None,
            wanted,
            jobs,
            decoded: dec_rx,
            ops,
            worker: Some(worker),
            reports: rep_rx,
            message: None,
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            view_idx: None,
        };
        s.cursor = s.first_open();
        Ok(s)
    }

    fn next_where(&self, from: usize, f: impl Fn(State) -> bool) -> Option<usize> {
        (from..self.items.len()).find(|&i| f(self.items[i].state))
    }

    fn prev_open(&self, from: usize) -> Option<usize> {
        (0..from).rev().find(|&i| self.items[i].state != State::Moved)
    }

    /// First pending image, falling back to the first skipped one.
    fn first_open(&self) -> Option<usize> {
        self.next_where(0, |s| s == State::Pending).or_else(|| self.next_where(0, |s| s == State::Skipped))
    }

    fn advance(&mut self, from: usize) {
        self.cursor = self.next_where(from + 1, |s| s == State::Pending);
    }

    fn handle_key(&mut self, key: egui::Key, mods: egui::Modifiers, repeat: bool) {
        use egui::Key;
        if (key == Key::Z && mods.command) || key == Key::Backspace {
            self.undo_last();
            return;
        }
        match key {
            Key::Escape => self.reset_zoom(),
            Key::ArrowRight => {
                let from = self.cursor.map_or(self.items.len(), |c| c + 1);
                if let Some(i) = self.next_where(from, |s| s != State::Moved) {
                    self.cursor = Some(i);
                }
            }
            Key::ArrowLeft => {
                let from = self.cursor.unwrap_or(self.items.len());
                if let Some(i) = self.prev_open(from) {
                    self.cursor = Some(i);
                }
            }
            Key::Home => self.cursor = self.first_open(),
            _ if repeat || mods.command || mods.alt => {}
            Key::Space => {
                if let Some(c) = self.cursor {
                    self.message = None;
                    self.items[c].state = State::Skipped;
                    let _ = self.ops.send(Op::Skip { name: self.items[c].name.clone() });
                    self.advance(c);
                }
            }
            _ => {
                let Some(t) = self.cfg.targets.iter().position(|t| t.key == key) else { return };
                let Some(c) = self.cursor else { return };
                self.message = None;
                self.items[c].state = State::Moved;
                self.undo.push((c, Some(t)));
                self.counts[t] += 1;
                let dir = self.cfg.targets[t].dir.clone();
                let _ = self.ops.send(Op::Move { idx: c, name: self.items[c].name.clone(), dir });
                self.advance(c);
            }
        }
    }

    fn undo_last(&mut self) {
        let Some((idx, target)) = self.undo.pop() else {
            self.message = Some("nothing to undo".into());
            return;
        };
        self.message = None;
        self.items[idx].state = State::Pending;
        if let Some(t) = target {
            self.counts[t] -= 1;
        }
        let _ = self.ops.send(Op::Undo { idx, target, name: self.items[idx].name.clone() });
        // The texture may still be cached; drop it only if it failed earlier.
        if matches!(self.cache.get(&idx), Some(Slot::Failed(_))) {
            self.cache.remove(&idx);
        }
        self.cursor = Some(idx);
    }

    fn poll_workers(&mut self, ctx: &egui::Context) {
        while let Ok(r) = self.reports.try_recv() {
            match r {
                Report::MoveFailed(idx, msg) => {
                    self.items[idx].state = State::Pending;
                    if let Some(pos) = self.undo.iter().rposition(|&(i, _)| i == idx) {
                        if let (_, Some(t)) = self.undo.remove(pos) {
                            self.counts[t] -= 1;
                        }
                    }
                    self.message = Some(msg);
                }
                Report::UndoFailed(idx, target, msg) => {
                    self.items[idx].state = State::Moved;
                    self.undo.push((idx, target));
                    if let Some(t) = target {
                        self.counts[t] += 1;
                    }
                    if self.cursor == Some(idx) {
                        self.advance(idx);
                    }
                    self.message = Some(msg);
                }
                Report::Error(msg) => self.message = Some(msg),
            }
        }
        while let Ok(d) = self.decoded.try_recv() {
            let slot = match d.result {
                Ok((img, orig)) => {
                    let tex = ctx.load_texture(format!("img{}", d.idx), img, egui::TextureOptions::LINEAR);
                    Slot::Ready(tex, orig)
                }
                Err(e) => Slot::Failed(e),
            };
            if d.full {
                if matches!(self.hires, Some((i, _)) if i == d.idx) {
                    self.hires = Some((d.idx, slot));
                }
            } else if self.cache.contains_key(&d.idx) {
                self.cache.insert(d.idx, slot);
            }
        }
    }

    /// Keep the current image, a few behind and `ahead` in front decoded; evict the rest.
    fn schedule(&mut self) {
        let mut order = Vec::with_capacity(self.cfg.ahead + BEHIND + 2);
        if let Some(c) = self.cursor {
            order.push(c);
            let mut i = c;
            for _ in 0..BEHIND {
                match self.prev_open(i) {
                    Some(p) => {
                        order.push(p);
                        i = p;
                    }
                    None => break,
                }
            }
            let mut i = c + 1;
            for _ in 0..self.cfg.ahead {
                match self.next_where(i, |s| s == State::Pending) {
                    Some(n) => {
                        order.push(n);
                        i = n + 1;
                    }
                    None => break,
                }
            }
        }
        // Keep (but never load) the most recently moved image so undo is instant.
        let keep = self.undo.last().map(|&(i, _)| i).filter(|i| self.cache.contains_key(i));

        if self.hires.as_ref().is_some_and(|(i, _)| Some(*i) != self.cursor) {
            self.hires = None;
        }
        let wanted: HashSet<usize> = order.iter().copied().chain(keep).collect();
        self.cache.retain(|k, _| wanted.contains(k));
        *self.wanted.lock().unwrap() = wanted;
        for idx in order {
            if !self.cache.contains_key(&idx) {
                self.cache.insert(idx, Slot::Loading);
                let path = self.cfg.source.join(&self.items[idx].name);
                let _ = self.jobs.send(Job { idx, path, max_side: self.cfg.max_side, full: false });
            }
        }
    }

    fn request_hires(&mut self, idx: usize, max_texture_side: usize) {
        if self.hires.is_none() {
            self.hires = Some((idx, Slot::Loading));
            let path = self.cfg.source.join(&self.items[idx].name);
            let _ = self.jobs.send(Job { idx, path, max_side: max_texture_side as u32, full: true });
        }
    }

    fn reset_zoom(&mut self) {
        self.zoom = 1.0;
        self.pan = egui::Vec2::ZERO;
    }

    /// Zoom by `factor`, keeping the image point under `anchor` fixed on screen.
    fn zoom_at(&mut self, anchor: egui::Pos2, center: egui::Pos2, factor: f32, max_zoom: f32) {
        let new = (self.zoom * factor).clamp(1.0, max_zoom.max(1.0));
        let f = new / self.zoom;
        let a = anchor - center;
        self.pan = a - (a - self.pan) * f;
        self.zoom = new;
    }

    /// Draws the image with zoom/pan interaction.
    fn image_view(&mut self, ui: &mut egui::Ui, rect: egui::Rect, idx: usize, orig: [u32; 2]) {
        if self.view_idx != Some(idx) {
            self.view_idx = Some(idx);
            self.reset_zoom();
        }
        let orig = egui::vec2(orig[0] as f32, orig[1] as f32);
        let ppp = ui.ctx().pixels_per_point();
        // Points per original image pixel when fitted to the window.
        let fit = (rect.width() / orig.x).min(rect.height() / orig.y);
        let max_zoom = MAX_PIXEL_ZOOM / ppp / fit;

        let resp = ui.interact(rect, ui.scope_id().with("image"), egui::Sense::click_and_drag());
        if let Some(p) = resp.hover_pos() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (scroll * 0.003).exp();
            if factor != 1.0 {
                self.zoom_at(p, rect.center(), factor, max_zoom);
            }
        }
        if resp.double_clicked() {
            if self.zoom > 1.0 {
                self.reset_zoom();
            } else if let Some(p) = resp.interact_pointer_pos() {
                // Jump to 1:1 pixels (or 2x for images smaller than the window).
                let one_to_one = 1.0 / ppp / fit;
                self.zoom_at(p, rect.center(), one_to_one.max(2.0), max_zoom);
            }
        }
        if resp.dragged() && self.zoom > 1.0 {
            self.pan += resp.drag_delta();
        }
        if self.zoom > 1.0 && resp.hovered() {
            ui.ctx().set_cursor_icon(if resp.dragged() {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            });
        }

        // Keep the image covering the view; no panning into the void.
        let size = orig * fit * self.zoom;
        let slack = ((size - rect.size()) * 0.5).max(egui::Vec2::ZERO);
        self.pan = self.pan.clamp(-slack, slack);
        let img_rect = egui::Rect::from_center_size(rect.center() + self.pan, size);

        let Some(Slot::Ready(low, _)) = self.cache.get(&idx) else { return };
        let low = low.clone();
        // Swap in the full-resolution texture once the preview runs out of pixels.
        if size.x * ppp > low.size()[0] as f32 * 1.05 && (low.size()[0] as f32) < orig.x {
            let max_tex = ui.input(|i| i.max_texture_side);
            self.request_hires(idx, max_tex);
        }
        let tex = match &self.hires {
            Some((i, Slot::Ready(t, _))) if *i == idx => t.clone(),
            _ => low,
        };
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        ui.painter().with_clip_rect(rect).image(tex.id(), img_rect, uv, egui::Color32::WHITE);

        if self.zoom > 1.0 {
            let loading = matches!(self.hires, Some((i, Slot::Loading)) if i == idx);
            let text = format!("{:.0}%{}", fit * self.zoom * ppp * 100.0, if loading { "  loading full res…" } else { "" });
            let pos = rect.right_bottom() - egui::vec2(10.0, 10.0);
            let font = egui::FontId::proportional(14.0);
            let galley = ui.painter().layout_no_wrap(text, font, egui::Color32::WHITE);
            let bg = egui::Rect::from_min_size(pos - galley.size() - egui::vec2(6.0, 4.0), galley.size() + egui::vec2(12.0, 8.0));
            ui.painter().rect_filled(bg, 4.0, egui::Color32::from_black_alpha(160));
            ui.painter().galley(bg.min + egui::vec2(6.0, 4.0), galley, egui::Color32::WHITE);
        }
    }

    /// Draws the sorting screen. Returns true when the user wants the setup screen.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        let ctx = ui.ctx().clone();
        let keys: Vec<_> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, repeat, modifiers, .. } => {
                        Some((*key, *modifiers, *repeat))
                    }
                    _ => None,
                })
                .collect()
        });
        self.poll_workers(&ctx);
        for (k, m, r) in keys {
            self.handle_key(k, m, r);
        }
        self.schedule();

        let total = self.items.len();
        let moved = self.items.iter().filter(|i| i.state == State::Moved).count();
        let skipped = self.items.iter().filter(|i| i.state == State::Skipped).count();
        let pending = total - moved - skipped;
        let done = if total == 0 { 1.0 } else { (moved + skipped) as f32 / total as f32 };
        let weak = ui.visuals().weak_text_color();
        let mut leave = false;

        egui::Panel::top("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                leave = ui.button("Setup").on_hover_text("Change folder or targets").clicked();
                ui.strong(format!("{} / {total}  ({:.1}%)", moved + skipped, done * 100.0));
                ui.label(format!("{pending} left · {skipped} skipped"));
                if let Some(c) = self.cursor {
                    ui.separator();
                    ui.label(&self.items[c].name);
                    if let Some(Slot::Ready(_, [w, h])) = self.cache.get(&c) {
                        ui.colored_label(weak, format!("{w}×{h}"));
                    }
                    if self.items[c].state == State::Skipped {
                        ui.colored_label(egui::Color32::YELLOW, "skipped");
                    }
                }
            });
            ui.add(egui::ProgressBar::new(done).desired_height(4.0));
        });

        egui::Panel::bottom("keys").show(ui, |ui| {
            if let Some(msg) = &self.message {
                ui.colored_label(egui::Color32::from_rgb(255, 110, 110), msg);
            }
            ui.horizontal_wrapped(|ui| {
                for (t, n) in self.cfg.targets.iter().zip(&self.counts) {
                    ui.strong(format!("[{}]", t.key.symbol_or_name()));
                    ui.label(format!("{} ({n})", t.label));
                    ui.add_space(8.0);
                }
                ui.separator();
                ui.colored_label(
                    weak,
                    "Space skip · Left/Right browse · Backspace undo · Home first unsorted · \
                     wheel/double-click zoom, drag pan, Esc fit",
                );
            });
        });

        egui::CentralPanel::no_frame().show(ui, |ui| {
            let rect = ui.max_rect();
            ui.painter().rect_filled(rect, 0.0, egui::Color32::from_gray(18));
            let center = |ui: &mut egui::Ui, text: String| {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    text,
                    egui::FontId::proportional(18.0),
                    egui::Color32::GRAY,
                );
            };
            let Some(c) = self.cursor else {
                if pending + skipped == 0 {
                    center(ui, "All images sorted.".into());
                } else {
                    center(ui, format!("End of list — {skipped} skipped. Press Home to revisit."));
                }
                return;
            };
            match self.cache.get(&c) {
                Some(Slot::Ready(_, orig)) => {
                    let orig = *orig;
                    self.image_view(ui, rect, c, orig);
                }
                Some(Slot::Failed(e)) => center(ui, format!("Cannot decode: {e}")),
                _ => center(ui, "loading…".into()),
            }
        });
        leave
    }
}

impl Sorter {
    /// Finish all queued moves (and their log lines); the sorter is unusable afterwards.
    pub fn finish(&mut self) {
        drop(std::mem::replace(&mut self.ops, mpsc::channel().0));
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Sorter {
    fn drop(&mut self) {
        self.finish();
    }
}
