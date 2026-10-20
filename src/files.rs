//! File-system helpers: listing, natural sort, the action log and safe moves.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "gif", "tif", "tiff"];

/// "img2.jpg" < "img10.jpg", case-insensitive.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.as_bytes(), b.as_bytes());
    loop {
        match (x.first(), y.first()) {
            (None, None) => return a.cmp(b),
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let ex = x
                    .iter()
                    .position(|c| !c.is_ascii_digit())
                    .unwrap_or(x.len());
                let ey = y
                    .iter()
                    .position(|c| !c.is_ascii_digit())
                    .unwrap_or(y.len());
                let trim =
                    |s: &[u8]| -> usize { s.iter().position(|&c| c != b'0').unwrap_or(s.len()) };
                let (nx, ny) = (&x[trim(&x[..ex])..ex], &y[trim(&y[..ey])..ey]);
                let ord = nx.len().cmp(&ny.len()).then(nx.cmp(ny));
                if ord != Ordering::Equal {
                    return ord;
                }
                (x, y) = (&x[ex..], &y[ey..]);
            }
            (Some(c), Some(d)) => {
                let ord = c.to_ascii_lowercase().cmp(&d.to_ascii_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                (x, y) = (&x[1..], &y[1..]);
            }
        }
    }
}

/// Replayed `.swiper/log.tsv`: which files were moved where, which were skipped.
pub struct History {
    pub moved: HashMap<String, PathBuf>,
    pub undo: Vec<String>,
    pub skipped: HashSet<String>,
}

pub fn read_log(path: &Path) -> History {
    let mut h = History {
        moved: HashMap::new(),
        undo: Vec::new(),
        skipped: HashSet::new(),
    };
    let Ok(f) = fs::File::open(path) else {
        return h;
    };
    for line in BufReader::new(f).lines().map_while(Result::ok) {
        let mut parts = line.split('\t');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("M"), Some(name), Some(dst)) => {
                h.skipped.remove(name);
                h.moved.insert(name.to_owned(), PathBuf::from(dst));
                h.undo.push(name.to_owned());
            }
            (Some("U"), Some(name), _) => {
                h.moved.remove(name);
                if let Some(i) = h.undo.iter().rposition(|n| n == name) {
                    h.undo.remove(i);
                }
            }
            (Some("S"), Some(name), _) => {
                h.skipped.insert(name.to_owned());
            }
            _ => {}
        }
    }
    h
}

fn is_image(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

pub fn list_images(dir: &Path) -> std::io::Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() && is_image(&entry.path()) {
            if let Ok(name) = entry.file_name().into_string() {
                out.push(name);
            }
        }
    }
    Ok(out)
}

pub fn unique_dest(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (1..)
        .map(|i| dir.join(format!("{stem}_{i}{ext}")))
        .find(|p| !p.exists())
        .unwrap()
}

pub fn move_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    if dst.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "destination exists",
        ));
    }
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        // Different drive: fall back to copy + delete.
        Err(e) => match fs::copy(src, dst) {
            Ok(_) => fs::remove_file(src),
            Err(_) => Err(e),
        },
    }
}
