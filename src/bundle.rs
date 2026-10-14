//! The embedded game (the `bundle` feature, see `build.rs`): OpenCE's
//! halo.exe and SDL3.dll, the Rust simulation's halo_sim.dll and, with
//! `dlss`, the DLSS runtime libraries, unpacked at start-up into a folder of
//! their own under `%LOCALAPPDATA%\halo_rt`, named by their hash, so a new
//! build gets a new folder and the old one is left alone. The viewer starts
//! the game from there, hidden, with every simulation system in Rust.

#[cfg(feature = "bundle")]
mod files {
    include!(concat!(env!("OUT_DIR"), "/bundle.rs"));
}

use std::path::PathBuf;

/// The unpacked game: its folder, its halo.exe and its halo_sim.dll.
pub struct Runtime {
    pub dir: PathBuf,
    pub game: PathBuf,
    pub simulation: PathBuf,
}

/// Whether this build carries the game.
pub const EMBEDDED: bool = cfg!(feature = "bundle");

/// Unpacks the embedded files (those not already there, or of another size)
/// and points DLSS at its libraries; None in a build without them.
#[cfg(feature = "bundle")]
pub fn unpack() -> std::io::Result<Option<Runtime>> {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let dir = base.join("halo_rt").join(format!("runtime-{}", files::ID));
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
