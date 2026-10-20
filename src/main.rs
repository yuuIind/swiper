//! swiper — a minimal, keyboard-driven image sorter for dataset curation.
//!
//! Images are shown one at a time; pressing a target's hotkey moves the image
//! into that target's folder and the next image (already decoded and uploaded
//! to the GPU in the background) appears immediately.

// No console window when started by double-click (release builds).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod files;
mod setup;
mod sorter;
mod theme;

use config::{DEFAULT_AHEAD, DEFAULT_MAX_SIDE};
use eframe::egui;
use setup::Setup;
use sorter::Sorter;
use std::path::PathBuf;

const USAGE: &str = "\
usage: swiper [<image-folder> [KEY=DEST ...]] [--max-side N] [--ahead N]

  Without arguments (e.g. double-clicked) swiper opens its setup screen.

  KEY=DEST      bind a key to a destination folder, e.g. 1=D:/data/cat  a=sorted/dog
                KEY is a single letter/digit or a key name (F1, Delete, Enter, ...).
                Targets are remembered in <image-folder>/.swiper/ so next time
                `swiper <image-folder>` is enough.
  --max-side N  downscale images for display to at most N px (default 1920)
  --ahead N     number of images to preload ahead (default 8)

Default target: Delete -> <image-folder>/_deleted   (override with Delete=PATH)

keys while sorting:
  <target key>         move image to target, show next
  Space                skip (leave in place), show next
  Left / Right         browse previous / next image
  Backspace / Ctrl+Z   undo last move
  Home                 jump to first unsorted image
  mouse wheel          zoom (double-click: 1:1 / fit, drag: pan, Esc: fit)";

struct Args {
    source: Option<PathBuf>,
    bindings: Vec<(egui::Key, PathBuf)>,
    max_side: u32,
    ahead: usize,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let mut out = Args {
        source: None,
        bindings: Vec::new(),
        max_side: DEFAULT_MAX_SIDE,
        ahead: DEFAULT_AHEAD,
    };

    while let Some(a) = args.next() {
        let mut num = |name: &str| -> Result<usize, String> {
            args.next()
                .and_then(|v| v.parse().ok())
                .filter(|&v| v > 0)
                .ok_or_else(|| format!("{name} needs a positive number"))
        };
        match a.as_str() {
            "-h" | "--help" => return Err(String::new()),
            "--max-side" => out.max_side = num("--max-side")? as u32,
            "--ahead" => out.ahead = num("--ahead")?,
            _ if a.starts_with("--") => return Err(format!("unknown option {a}")),
            _ => match a.split_once('=') {
                // `C:\x` style paths never contain '=' before the drive letter.
                Some((k, p)) if !k.is_empty() && !k.contains(['/', '\\']) => {
                    out.bindings.push((config::parse_key(k)?, PathBuf::from(p)));
                }
                _ if out.source.is_none() => out.source = Some(PathBuf::from(a)),
                _ => return Err(format!("unexpected argument '{a}'")),
            },
        }
    }
    Ok(out)
}

/// Lets `eprintln!` reach the terminal when started from one, despite the GUI subsystem.
#[cfg(windows)]
fn attach_console() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

enum Screen {
    Setup(Setup),
    Sort(Box<Sorter>),
}

struct Root {
    screen: Screen,
    logo: egui::TextureHandle,
}

fn set_title(ctx: &egui::Context, sorter: Option<&Sorter>) {
    let title = match sorter {
        Some(s) => format!("swiper — {}", s.cfg.source.display()),
        None => "swiper".to_owned(),
    };
    ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
}

impl eframe::App for Root {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        match &mut self.screen {
            Screen::Setup(setup) => {
                if let Some(cfg) = setup.ui(ui, &self.logo) {
                    match Sorter::new(&ctx, cfg) {
                        Ok(s) => {
                            set_title(&ctx, Some(&s));
                            self.screen = Screen::Sort(Box::new(s));
                        }
                        Err(e) => setup.error = Some(e),
                    }
                }
            }
            Screen::Sort(sorter) => {
                if sorter.ui(ui) {
                    let (src, max_side, ahead) = (
                        sorter.cfg.source.clone(),
                        sorter.cfg.max_side,
                        sorter.cfg.ahead,
                    );
                    sorter.finish();
                    self.screen = Screen::Setup(Setup::new(Some(&src), max_side, ahead, None));
                    set_title(&ctx, None);
                }
            }
        }
    }
}

fn main() {
    #[cfg(windows)]
    attach_console();

    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("error: {e}\n");
            }
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };

    // With a folder (and targets given now or saved earlier) go straight to sorting.
    let start = args.source.as_ref().and_then(|src| {
        let mut bindings = args.bindings.clone();
        if bindings.is_empty() {
            bindings = config::saved_bindings(src);
            if bindings.is_empty() {
                return None;
            }
        }
        if !bindings.iter().any(|(k, _)| *k == egui::Key::Delete) {
            bindings.push((egui::Key::Delete, config::default_delete_dir(src)));
        }
        Some(config::build_config(
            src,
            bindings,
            args.max_side,
            args.ahead,
        ))
    });

    let icon = load_icon();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([720.0, 480.0])
            .with_title("swiper")
            .with_icon(icon.clone()),
        ..Default::default()
    };
    let result = eframe::run_native(
        "swiper",
        options,
        Box::new(move |cc| {
            let ctx = &cc.egui_ctx;
            theme::apply(ctx);
            let logo = ctx.load_texture(
                "logo",
                egui::ColorImage::from_rgba_unmultiplied(
                    [icon.width as usize, icon.height as usize],
                    &icon.rgba,
                ),
                egui::TextureOptions::LINEAR,
            );
            let setup = |err| {
                Screen::Setup(Setup::new(
                    args.source.as_deref(),
                    args.max_side,
                    args.ahead,
                    err,
                ))
            };
            let screen = match start {
                Some(Ok(cfg)) => match Sorter::new(ctx, cfg) {
                    Ok(s) => {
                        set_title(ctx, Some(&s));
                        Screen::Sort(Box::new(s))
                    }
                    Err(e) => setup(Some(e)),
                },
                Some(Err(e)) => setup(Some(e)),
                None => setup(None),
            };
            Ok(Box::new(Root { screen, logo }))
        }),
    );
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// The 256 px logo, used for the window/taskbar icon and the setup screen.
fn load_icon() -> egui::IconData {
    let img = image::load_from_memory_with_format(
        include_bytes!("../assets/icon-256.png"),
        image::ImageFormat::Png,
    )
    .expect("embedded icon is a valid PNG")
    .into_rgba8();
    let (width, height) = img.dimensions();
    egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    }
}
