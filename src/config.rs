//! Project configuration: targets (key → folder), per-project persistence and
//! the list of recently opened projects.

use eframe::egui::Key;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_MAX_SIDE: u32 = 1920;
pub const DEFAULT_AHEAD: usize = 8;

/// Keys used for navigation and therefore not bindable to targets.
pub const RESERVED: &[Key] =
    &[Key::ArrowLeft, Key::ArrowRight, Key::Space, Key::Backspace, Key::Home, Key::Escape];

pub struct Target {
    pub key: Key,
    pub label: String,
    pub dir: PathBuf,
}

pub struct Config {
    pub source: PathBuf,
    pub state_dir: PathBuf,
    pub targets: Vec<Target>,
    pub max_side: u32,
    pub ahead: usize,
}

pub fn parse_key(s: &str) -> Result<Key, String> {
    let k = Key::from_name(s).ok_or_else(|| format!("unknown key '{s}'"))?;
    if RESERVED.contains(&k) {
        return Err(format!("key '{s}' is reserved for navigation"));
    }
    Ok(k)
}

pub fn default_delete_dir(source: &Path) -> PathBuf {
    source.join("_deleted")
}

/// Targets saved by the previous session in `<source>/.swiper/targets.txt`.
pub fn saved_bindings(source: &Path) -> Vec<(Key, PathBuf)> {
    let Ok(text) = fs::read_to_string(source.join(".swiper").join("targets.txt")) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| l.split_once('='))
        .filter_map(|(k, p)| Some((parse_key(k).ok()?, PathBuf::from(p))))
        .collect()
}

/// Validates the project, creates all folders and remembers the targets.
pub fn build_config(
    source: &Path,
    bindings: Vec<(Key, PathBuf)>,
    max_side: u32,
    ahead: usize,
) -> Result<Config, String> {
    let source = std::path::absolute(source).map_err(|e| e.to_string())?;
    if !source.is_dir() {
        return Err(format!("{} is not a folder", source.display()));
    }
    if bindings.is_empty() {
        return Err("add at least one target".into());
    }
    let state_dir = source.join(".swiper");
    fs::create_dir_all(&state_dir).map_err(|e| format!("creating {}: {e}", state_dir.display()))?;

    let mut targets: Vec<Target> = Vec::new();
    for (key, dir) in bindings {
        if targets.iter().any(|t| t.key == key) {
            return Err(format!("key '{}' is bound twice", key.name()));
        }
        let dir = std::path::absolute(&dir).map_err(|e| e.to_string())?;
        if dir == source {
            return Err("a target cannot be the image folder itself".into());
        }
        fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        let label = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        targets.push(Target { key, label, dir });
    }

    let saved: String =
        targets.iter().map(|t| format!("{}={}\n", t.key.name(), t.dir.display())).collect();
    let targets_file = state_dir.join("targets.txt");
    fs::write(&targets_file, saved).map_err(|e| format!("writing {}: {e}", targets_file.display()))?;
    remember_recent(&source);

    Ok(Config { source, state_dir, targets, max_side, ahead })
}

// ---------------------------------------------------------------------------
// Recent projects, stored in %APPDATA%/swiper/recent.txt

const MAX_RECENT: usize = 10;

fn recent_file() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("swiper").join("recent.txt"))
}

pub fn recent_projects() -> Vec<PathBuf> {
    let Some(text) = recent_file().and_then(|f| fs::read_to_string(f).ok()) else {
        return Vec::new();
    };
    text.lines().filter(|l| !l.is_empty()).map(PathBuf::from).collect()
}

fn remember_recent(source: &Path) {
    let Some(file) = recent_file() else { return };
    let mut list = recent_projects();
    list.retain(|p| p != source);
    list.insert(0, source.to_path_buf());
    list.truncate(MAX_RECENT);
    let text: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(file, text);
}

pub fn forget_recent(source: &Path) {
    let Some(file) = recent_file() else { return };
    let mut list = recent_projects();
    list.retain(|p| p != source);
    let text: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
    let _ = fs::write(file, text);
}
