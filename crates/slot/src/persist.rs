use std::path::{Path, PathBuf};

use slot_store::{atomic_write, read_slot_state, write_slot_state, Core, Platform, StateRing};

pub trait Snapshot {
    fn state(&self) -> Option<Vec<u8>>;
    fn save_ram(&self) -> Option<Vec<u8>>;
    fn thumb(&self) -> Option<Vec<u8>>;
    fn load(&self, state: Vec<u8>);

    fn resume_trusted(&self) -> bool {
        true
    }

    fn save_ram_trusted(&self) -> bool {
        true
    }

    /// Before play pauses: the frames hardcore wants played first, or 0 if it may pause now.
    fn pause_wait(&self) -> u32 {
        0
    }
}

pub fn flush(
    root: &Path,
    platform: Platform,
    core: Core,
    stem: &str,
    state: Option<&[u8]>,
    sav: Option<&[u8]>,
) -> std::io::Result<()> {
    if let Some(state) = state {
        StateRing::new(root, platform, core, stem).write_resume(state)?;
    }
    if let Some(sav) = sav {
        write_sav(root, platform, stem, sav)?;
    }
    Ok(())
}

pub fn eject(
    root: &Path,
    platform: Platform,
    core: Core,
    stem: &str,
    state: Option<&[u8]>,
    sav: Option<&[u8]>,
) -> std::io::Result<()> {
    flush(root, platform, core, stem, state, sav)?;
    let mut slot = read_slot_state(root);
    slot.cart = None;
    slot.cart_platform = None;
    write_slot_state(root, &slot)
}

pub fn write_sav(root: &Path, platform: Platform, stem: &str, sav: &[u8]) -> std::io::Result<bool> {
    let path = sav_path(root, platform, stem);
    // Only skip writing if the preferred output file itself is already current.
    // A matching fallback extension is not enough: the preferred file must be created.
    if std::fs::read(&path).is_ok_and(|old| old == sav) {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    atomic_write(&path, sav)?;
    Ok(true)
}

pub fn read_sav(root: &Path, platform: Platform, stem: &str) -> Option<Vec<u8>> {
    let preferred = sav_path(root, platform, stem);
    let other_ext = if preferred.extension().is_some_and(|s| s == "srm") { "sav" } else { "srm" };
    std::fs::read(&preferred)
        .or_else(|_| std::fs::read(preferred.with_extension(other_ext)))
        .ok()
}

pub fn read_resume(root: &Path, platform: Platform, core: Core, stem: &str) -> Option<Vec<u8>> {
    StateRing::new(root, platform, core, stem)
        .read_resume()
        .ok()
        .flatten()
}

fn sav_path(root: &Path, platform: Platform, stem: &str) -> PathBuf {
    let extension = match std::fs::read_to_string(root.join("Config/save_format.txt")) {
        Ok(value) if value.trim().eq_ignore_ascii_case("srm") => "srm",
        _ => "sav", // upstream default
    };
    crate::root::saves_dir(root)
        .join(platform.dir_name())
        .join(format!("{stem}.{extension}"))
}

#[cfg(test)]
mod custom_tests {
    use super::*;
    #[test]
    fn preferred_srm_is_written_and_sav_is_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("Config")).unwrap();
        std::fs::write(root.join("Config/save_format.txt"), "srm\n").unwrap();
        let save_dir = root.join("Saves/GBA");
        std::fs::create_dir_all(&save_dir).unwrap();
        std::fs::write(save_dir.join("Example.sav"), b"old").unwrap();
        assert_eq!(read_sav(root, Platform::Gba, "Example"), Some(b"old".to_vec()));
        assert!(write_sav(root, Platform::Gba, "Example", b"new").unwrap());
        assert_eq!(std::fs::read(save_dir.join("Example.srm")).unwrap(), b"new");
        assert_eq!(read_sav(root, Platform::Gba, "Example"), Some(b"new".to_vec()));
        assert!(!write_sav(root, Platform::Gba, "Example", b"new").unwrap());
    }
    #[test]
    fn default_still_writes_sav() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_sav(root, Platform::Gba, "Example", b"data").unwrap();
        assert_eq!(std::fs::read(root.join("Saves/GBA/Example.sav")).unwrap(), b"data");
    }
}
