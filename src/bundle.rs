//! The embedded game (the `bundle` feature, see `build.rs`): OpenCE's
//! halo.exe and SDL3.dll, the Rust simulation's halo_sim.dll and, with
//! `dlss`, the DLSS runtime libraries, unpacked at start-up into a folder of
//! their own under `%LOCALAPPDATA%\halo_rt`, named by their hash, so a new
//! build gets a new folder, and the other builds' folders are removed once it
//! is there (`prune_runtimes_in_background`; one a game runs from is left
//! alone). The viewer starts the game from there, hidden, with every
//! simulation system in Rust and its settings in one file every build shares
//! (`share_game_settings`).

#[cfg(feature = "bundle")]
mod files {
    include!(concat!(env!("OUT_DIR"), "/bundle.rs"));
}

use std::path::{Path, PathBuf};

/// The unpacked game: its folder, its halo.exe and its halo_sim.dll.
pub struct Runtime {
    pub dir: PathBuf,
    pub game: PathBuf,
    pub simulation: PathBuf,
}

/// Whether this build carries the game.
pub const EMBEDDED: bool = cfg!(feature = "bundle");

/// The folder of every build's unpacked game: `%LOCALAPPDATA%\halo_rt` (the
/// temporary folder's without it).
#[cfg(feature = "bundle")]
fn runtimes_base() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("halo_rt")
}

/// Whether `name` is a build's folder's (`unpack`'s): `runtime-` and 16
/// lowercase hex digits, nothing more.
fn is_runtime_name(name: &str) -> bool {
    name.strip_prefix("runtime-")
        .is_some_and(|id| id.len() == 16 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

/// The builds' folders directly under `base` (real folders: not a link, a
/// junction or a file of such a name).
fn runtime_folders(base: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(base) else { return Vec::new() };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_str().is_some_and(is_runtime_name))
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir() && !kind.is_symlink()))
        .map(|entry| entry.path())
        .collect()
}

/// The other builds' folders under `base`: all but `current` (a folder's
/// name).
fn stale_runtimes(base: &Path, current: &str) -> Vec<PathBuf> {
    runtime_folders(base).into_iter().filter(|dir| dir.file_name().is_none_or(|name| name != current)).collect()
}

/// The bytes of the files under `dir` (links not followed).
fn folder_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() && !kind.is_symlink() => folder_size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |metadata| metadata.len()),
            _ => 0,
        })
        .sum()
}

/// Removes `folders`, but not one whose halo.exe cannot be removed first (a
/// game running from it: Windows keeps a running executable); the folders
/// and the bytes removed. Failures are passed over.
fn remove_runtimes(folders: &[PathBuf]) -> (usize, u64) {
    let (mut removed, mut bytes) = (0, 0);
    for dir in folders {
        match std::fs::remove_file(dir.join("halo.exe")) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => continue,
        }
        let size = folder_size(dir);
        if std::fs::remove_dir_all(dir).is_ok() {
            removed += 1;
            bytes += size;
        }
    }
    (removed, bytes)
}

/// Removes the other builds' folders beside `current` (this build's), on a
/// thread of its own so that start-up does not wait for it: started only
/// once the environment is set. `HALO_RT_KEEP_RUNTIMES=1` keeps them, the
/// old.
pub fn prune_runtimes_in_background(current: &Path) {
    if std::env::var("HALO_RT_KEEP_RUNTIMES").is_ok_and(|v| v == "1") {
        return;
    }
    let (Some(base), Some(name)) = (current.parent(), current.file_name().and_then(|name| name.to_str())) else { return };
    let (base, name) = (base.to_path_buf(), name.to_string());
    std::thread::spawn(move || {
        let (removed, bytes) = remove_runtimes(&stale_runtimes(&base, &name));
        if removed > 0 {
            eprintln!("removed {removed} other builds' game folders ({} MB) from {}", bytes / 1_000_000, base.display());
        }
    });
}

/// The config.toml changed last among the builds' folders under `base`.
fn newest_runtime_config(base: &Path) -> Option<PathBuf> {
    runtime_folders(base)
        .into_iter()
        .map(|dir| dir.join("config.toml"))
        .filter_map(|path| {
            let metadata = std::fs::metadata(&path).ok().filter(|metadata| metadata.is_file())?;
            Some((metadata.modified().ok()?, path))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

/// Gives `target` the game's settings of the build under `base` that changed
/// them last, when it has none (never over one); the folder they came from,
/// None when nothing was copied.
fn migrate_config(base: &Path, target: &Path) -> std::io::Result<Option<PathBuf>> {
    if std::fs::symlink_metadata(target).is_ok() {
        return Ok(None);
    }
    let Some(source) = newest_runtime_config(base) else { return Ok(None) };
    // (whole or not at all, and not over one another viewer wrote meanwhile)
    let partial = target.with_extension(format!("partial-{}", std::process::id()));
    std::fs::copy(&source, &partial)?;
    let placed = if std::fs::symlink_metadata(target).is_ok() { Ok(false) } else { std::fs::rename(&partial, target).map(|()| true) };
    let _ = std::fs::remove_file(&partial);
    Ok(if placed? { source.parent().map(Path::to_path_buf) } else { None })
}

/// Points the game unpacked in `current` at one settings file for every
/// build, config.toml beside the viewer's options (the game's own is beside
/// its executable, a new one each build), first copying there the one of
/// the build that changed it last (the game writes its defaults to a file
/// it does not find). An explicit `HALO_CONFIG_FILE` wins (the scripted
/// runs'); `HALO_RT_CONFIG_PER_BUILD=1` leaves each build its own, the old.
pub fn share_game_settings(current: &Path) {
    if std::env::var_os("HALO_CONFIG_FILE").is_some_and(|file| !file.is_empty())
        || std::env::var("HALO_RT_CONFIG_PER_BUILD").is_ok_and(|v| v == "1")
    {
        return;
    }
    let (Some(base), Some(target)) = (current.parent(), crate::options::settings_path().map(|path| path.with_file_name("config.toml"))) else {
        return;
    };
    if let Some(folder) = target.parent()
        && let Err(e) = std::fs::create_dir_all(folder)
    {
        eprintln!("could not make {} for the game's settings: {e}", folder.display());
        return;
    }
    let from = match migrate_config(base, &target) {
        Ok(Some(dir)) => format!(" (copied from {})", dir.display()),
        Ok(None) => String::new(),
        Err(e) => format!(" (not copied from the last build's: {e})"),
    };
    // SAFETY: at start-up, before any other thread runs (the game inherits it)
    unsafe {
        std::env::set_var("HALO_CONFIG_FILE", &target);
    }
    eprintln!("the game's settings: {}{from}", target.display());
}

/// Unpacks the embedded files (those not already there, or of another size)
/// and points DLSS at its libraries; None in a build without them.
#[cfg(feature = "bundle")]
pub fn unpack() -> std::io::Result<Option<Runtime>> {
    let dir = runtimes_base().join(format!("runtime-{}", files::ID));
    for (name, bytes) in files::FILES {
        let path = dir.join(name);
        if std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() == bytes.len() as u64) {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // (whole or not at all: another viewer may be starting too)
        let partial = path.with_extension(format!("partial-{}", std::process::id()));
        std::fs::write(&partial, bytes)?;
        if std::fs::rename(&partial, &path).is_err() {
            let _ = std::fs::remove_file(&partial);
            if !std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() == bytes.len() as u64) {
                return Err(std::io::Error::other(format!("could not write {}", path.display())));
            }
        }
    }
    #[cfg(feature = "dlss")]
    // SAFETY: at start-up, before any other thread runs (dlss_wgpu reads it
    // when the renderer starts)
    unsafe {
        std::env::set_var("DLSS_SDK", dir.join("dlss"));
    }
    Ok(Some(Runtime { game: dir.join("halo.exe"), simulation: dir.join("halo_sim.dll"), dir }))
}

#[cfg(not(feature = "bundle"))]
pub fn unpack() -> std::io::Result<Option<Runtime>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{Duration, SystemTime};

    const OTHER: &str = "runtime-0123456789abcdef";
    const CURRENT: &str = "runtime-fedcba9876543210";

    /// A new empty folder under the temporary one, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static COUNT: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir()
                .join(format!("halo_rt_bundle_test-{}-{}", std::process::id(), COUNT.fetch_add(1, Ordering::Relaxed)));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        /// A folder `name` with a halo.exe of 3 bytes.
        fn folder(&self, name: &str) -> PathBuf {
            let dir = self.0.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("halo.exe"), b"exe").unwrap();
            dir
        }

        /// A config.toml in the folder `name`, changed `age` seconds ago.
        fn config(&self, name: &str, text: &str, age: u64) {
            let path = self.folder(name).join("config.toml");
            std::fs::write(&path, text).unwrap();
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(SystemTime::now() - Duration::from_secs(age)).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn runtime_names() {
        assert!(is_runtime_name(OTHER));
        assert!(is_runtime_name(CURRENT));
        for name in [
            "runtime-XYZ",
            "runtime-0123456789ABCDEF",
            "runtime-0123456789abcdef-old",
            "runtime-0123456789abcde",
            "runtime-0123456789abcdefa",
            "runtime-0123456789abcdeg",
            "runtime-",
            "Runtime-0123456789abcdef",
            "xruntime-0123456789abcdef",
            "settings",
            "settings.txt",
            "config.toml",
        ] {
            assert!(!is_runtime_name(name), "{name}");
        }
    }

    #[test]
    fn stale_runtimes_are_the_other_builds_folders() {
        let scratch = Scratch::new();
        scratch.folder(OTHER);
        scratch.folder(CURRENT);
        scratch.folder("runtime-XYZ");
        scratch.folder("runtime-0123456789ABCDEF");
        scratch.folder("runtime-0123456789abcdef-old");
        scratch.folder("settings");
        // (a file named as a build's folder is not one)
        std::fs::write(scratch.0.join("runtime-1111111111111111"), b"file").unwrap();
        assert_eq!(stale_runtimes(&scratch.0, CURRENT), vec![scratch.0.join(OTHER)]);
    }

    #[test]
    fn pruning_leaves_the_current_and_the_rest() {
        let scratch = Scratch::new();
        let other = scratch.folder(OTHER);
        std::fs::create_dir_all(other.join("dlss")).unwrap();
        std::fs::write(other.join("dlss").join("lib.dll"), [0u8; 1000]).unwrap();
        let second = scratch.folder("runtime-00000000000000ff");
        scratch.folder(CURRENT);
        scratch.folder("runtime-0123456789abcdef-old");
        std::fs::write(scratch.0.join("settings.txt"), b"x").unwrap();
        std::fs::write(scratch.0.join("runtime-1111111111111111"), b"file").unwrap();
        // (halo.exe is removed before the folder's size is taken)
        assert_eq!(remove_runtimes(&stale_runtimes(&scratch.0, CURRENT)), (2, 1000));
        assert!(!other.exists() && !second.exists());
        assert!(scratch.0.join(CURRENT).join("halo.exe").exists());
        assert!(scratch.0.join("runtime-0123456789abcdef-old").join("halo.exe").exists());
        assert!(scratch.0.join("settings.txt").exists());
        assert!(scratch.0.join("runtime-1111111111111111").is_file());
    }

    #[test]
    fn a_running_game_keeps_its_folder() {
        let scratch = Scratch::new();
        let other = scratch.folder(OTHER);
        // (Windows does not remove a running executable; a file held open
        // without delete sharing stands in for one)
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let held = std::fs::File::options().read(true).share_mode(1).open(other.join("halo.exe")).unwrap();
            assert_eq!(remove_runtimes(std::slice::from_ref(&other)), (0, 0));
            assert!(other.join("halo.exe").exists());
            drop(held);
        }
        assert_eq!(remove_runtimes(std::slice::from_ref(&other)).0, 1);
        assert!(!other.exists());
    }

    #[test]
    fn newest_config_by_modification_time() {
        let scratch = Scratch::new();
        assert_eq!(newest_runtime_config(&scratch.0), None);
        scratch.config(OTHER, "old", 300);
        scratch.config("runtime-00000000000000ff", "newest", 10);
        scratch.config(CURRENT, "older", 200);
        // (not a build's folder)
        scratch.config("runtime-0123456789abcdef-old", "newer still", 1);
        assert_eq!(newest_runtime_config(&scratch.0), Some(scratch.0.join("runtime-00000000000000ff").join("config.toml")));
    }

    #[test]
    fn migration_copies_once_and_never_over() {
        let scratch = Scratch::new();
        let target = scratch.0.join("stable").join("config.toml");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        // (nothing to copy: nothing written)
        assert_eq!(migrate_config(&scratch.0, &target).unwrap(), None);
        assert!(!target.exists());
        scratch.config(OTHER, "old", 300);
        scratch.config(CURRENT, "newest", 10);
        assert_eq!(migrate_config(&scratch.0, &target).unwrap(), Some(scratch.0.join(CURRENT)));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "newest");
        scratch.config(OTHER, "newer", 1);
        assert_eq!(migrate_config(&scratch.0, &target).unwrap(), None);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "newest");
        // (no partial file left)
        assert_eq!(std::fs::read_dir(target.parent().unwrap()).unwrap().count(), 1);
    }
}
