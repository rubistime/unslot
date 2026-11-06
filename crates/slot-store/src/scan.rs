use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::gba::title_code;
use crate::platform::{Header, Platform};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Cart {
    pub platform: Platform,
    pub stem: String,
    pub rom: PathBuf,
    pub label: Option<PathBuf>,
    pub title: String,
    pub code: String,
    pub shell: Option<crate::ShellChoice>,
}

impl Cart {
    /// What the shelf calls the game: an arcade set's game, else the file's name.
    pub fn shelf_name(&self) -> &str {
        match self.platform.spec().header {
            Header::Arcade if !self.title.is_empty() => &self.title,
            _ => &self.stem,
        }
    }

    pub fn read_header(&mut self) {
        match self.platform.spec().header {
            Header::Gba => {
                if let Some((title, code)) = title_code(&self.rom) {
                    self.title = title;
                    self.code = code;
                }
            }
            Header::Gb => {
                if let Some(title) = crate::gb::title(&self.rom) {
                    self.title = title;
                }
            }
            Header::Arcade => {
                if let Some(name) = crate::arcade::name(&self.stem) {
                    self.title = name.to_string();
                }
            }
            Header::None => {}
        }
    }
}

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

pub fn scan(root: &Path) -> Result<Vec<Cart>, StoreError> {
    let read = |file: &str| std::fs::read_to_string(root.join(file)).unwrap_or_default();
    let shells = crate::cart_shell::layered(
        &read(crate::CART_SHELL_FILE),
        &read(crate::LABELS_SHELL_FILE),
    );
    let mut carts = Vec::new();
    let recursive = read("Config/recursive_rom_scan.txt")
        .trim()
        .eq_ignore_ascii_case("true");
    for platform in Platform::ALL {
        let dir = root.join("Games").join(platform.dir_name());
        let entries = match discover_roms(&dir, platform, recursive) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                eprintln!("slot: scan: {}: {e}", dir.display());
                continue;
            }
        };
        let labels_dir = root.join("Labels").join(platform.dir_name());
        let labels = listing(&labels_dir);
        let first = carts.len();
        for rom in entries {
            let arcade = platform.spec().header == Header::Arcade;
            let Some(stem) = rom.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if arcade && crate::arcade::is_bios(stem) {
                continue;
            }
            let label = labels
                .get(&format!("{stem}.png").to_lowercase())
                .map(|name| labels_dir.join(name));
            carts.push(Cart {
                platform,
                stem: stem.to_string(),
                title: String::new(),
                code: String::new(),
                shell: shells.get(&crate::cart_shell::key(stem)).copied(),
                label,
                rom,
            });
            if arcade {
                // A table lookup, cheap enough for every game at boot, unlike reading a ROM.
                carts.last_mut().map(Cart::read_header);
            }
        }
        let listed = in_playlists(&carts[first..]);
        let mut at = first;
        carts.retain(|c| {
            at += 1;
            at <= first || !listed.contains(&c.rom)
        });
    }
    carts.sort_by(|a, b| {
        (a.platform as u8, sort_key(a.shelf_name()))
            .cmp(&(b.platform as u8, sort_key(b.shelf_name())))
    });
    Ok(carts)
}

/// Discover the normal top-level files plus, when opted in, ROMs in category folders.
/// Disc directories stay atomic; arcade CHD directories are never treated as categories.
/// Do not follow symlinks; cap depth to avoid pathological directory trees.
fn discover_roms(dir: &Path, platform: Platform, recursive: bool) -> std::io::Result<Vec<PathBuf>> {
    fn visit(dir: &Path, platform: Platform, recursive: bool, depth: usize,
             out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if is_hidden(&path) { continue; }
            let ty = entry.file_type()?;
            if ty.is_symlink() { continue; }
            if ty.is_file() {
                if platform.accepts(&path) { out.push(path); }
                continue;
            }
            if !ty.is_dir() { continue; }
            let arcade = platform.spec().header == Header::Arcade;
            if !arcade {
                if let Some(disc) = disc_in(&path, platform) {
                    out.push(disc);
                    continue;
                }
            } else {
                // A directory containing CHD images belongs to an arcade romset.
                // It is not a category folder; leave its CHDs alongside the parent zip.
                let chd_dir = std::fs::read_dir(&path)?
                    .filter_map(Result::ok)
                    .any(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("chd")));
                if chd_dir { continue; }
            }
            if recursive && depth < 8 {
                visit(&path, platform, recursive, depth + 1, out)?;
            }
        }
        Ok(())
    }
    let mut found = Vec::new();
    visit(dir, platform, recursive, 0, &mut found)?;
    Ok(found)
}

/// The discs the shelf's `.m3u` playlists name. A multi-disc game is shown once, as its playlist,
/// not once a disc.
fn in_playlists(carts: &[Cart]) -> Vec<PathBuf> {
    carts
        .iter()
        .filter(|c| {
            c.rom
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("m3u"))
        })
        .flat_map(|c| {
            let dir = c.rom.parent().map(Path::to_path_buf).unwrap_or_default();
            std::fs::read_to_string(&c.rom)
                .unwrap_or_default()
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|l| dir.join(l))
                .collect::<Vec<_>>()
        })
        .collect()
}

pub fn listing(dir: &Path) -> HashMap<String, String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return HashMap::new();
    };
    entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|name| !name.starts_with('.'))
        .map(|name| (name.to_lowercase(), name))
        .collect()
}

/// The disc in `dir` named after it, in the order the platform lists its extensions.
fn disc_in(dir: &Path, platform: Platform) -> Option<PathBuf> {
    let name = dir.file_name()?.to_str()?;
    platform
        .extensions()
        .iter()
        .map(|ext| dir.join(format!("{name}.{ext}")))
        .find(|p| p.is_file())
}

pub fn sort_key(stem: &str) -> (u8, String) {
    (group_of(stem), stem.to_uppercase())
}

fn group_of(stem: &str) -> u8 {
    match stem.chars().find(|c| !c.is_whitespace()) {
        Some(c) if c.is_ascii_digit() => 0,
        Some(c) if c.is_alphabetic() => 1,
        _ => 2,
    }
}

pub fn initial(stem: &str) -> char {
    match group_of(stem) {
        1 => stem
            .chars()
            .find(|c| !c.is_whitespace())
            .and_then(|c| c.to_uppercase().next())
            .unwrap_or('#'),
        _ => '#',
    }
}

pub fn is_hidden(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
}

#[cfg(test)]
mod custom_scan_tests {
    use super::*;
    #[test]
    fn nested_roms_require_opt_in() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("GBA");
        std::fs::create_dir_all(base.join("RPG")).unwrap();
        std::fs::write(base.join("RPG/Pokemon.gba"), b"rom").unwrap();
        std::fs::write(base.join("RPG/thumbnail.png"), b"png").unwrap();
        assert!(discover_roms(&base, Platform::Gba, false).unwrap().is_empty());
        let entries = discover_roms(&base, Platform::Gba, true).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].ends_with("RPG/Pokemon.gba"));
    }
    #[test]
    fn missing_directory_returns_error() {
        let temp = tempfile::tempdir().unwrap();
        assert!(discover_roms(&temp.path().join("not-found"), Platform::Gba, true).is_err());
    }
}
