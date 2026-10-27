//! With the `bundle` feature, embeds the game the viewer starts: OpenCE's
//! halo.exe and SDL3.dll (its Windows build), the Rust simulation's 32-bit
//! halo_sim.dll and, with `dlss`, the DLSS runtime libraries. `src/bundle.rs`
//! unpacks them at start-up. The game's data (the maps) is never embedded.
//!
//! Where they come from: `HALO_RT_OPENCE_BUILD` (default
//! `opence/build/windows`), `HALO_RT_SIM_DLL` (default
//! `target/i686-pc-windows-msvc/release/halo_sim.dll`) and `DLSS_SDK`
//! (`.cargo/config.toml` sets `vendor/DLSS`). `tools/package.py` builds all
//! of them first.

use std::{
    env,
    fmt::Write as _,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    for name in ["HALO_RT_OPENCE_BUILD", "HALO_RT_SIM_DLL", "DLSS_SDK"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    if env::var_os("CARGO_FEATURE_BUNDLE").is_none() {
        return;
    }
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let from_env = |name: &str, default: &str| {
        env::var_os(name).map(PathBuf::from).unwrap_or_else(|| manifest.join(default))
    };
    let opence = from_env("HALO_RT_OPENCE_BUILD", "opence/build/windows");
    let sim = from_env("HALO_RT_SIM_DLL", "target/i686-pc-windows-msvc/release/halo_sim.dll");
    // (the published name, and where it comes from)
    let mut files: Vec<(String, PathBuf)> = vec![
        ("halo.exe".into(), opence.join("halo.exe")),
        ("SDL3.dll".into(), opence.join("SDL3.dll")),
        ("halo_sim.dll".into(), sim),
    ];
    if env::var_os("CARGO_FEATURE_DLSS").is_some() {
        let sdk = from_env("DLSS_SDK", "vendor/DLSS");
        for name in ["nvngx_dlss.dll", "nvngx_dlssd.dll"] {
            let relative = format!("dlss/lib/Windows_x86_64/rel/{name}");
            files.push((relative, sdk.join("lib/Windows_x86_64/rel").join(name)));
        }
    }
    let mut source = String::from("/// The embedded files: each one's path in the runtime folder, and its bytes.\npub const FILES: &[(&str, &[u8])] = &[\n");
    // (FNV-1a over the files: a new runtime folder when any changes)
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (name, path) in &files {
        let bytes = std::fs::read(path).unwrap_or_else(|e| {
            panic!("the bundle needs {} ({e}); build it first (tools/package.py builds everything)", path.display())
        });
        for byte in name.bytes().chain(bytes) {
            hash = (hash ^ byte as u64).wrapping_mul(0x100_0000_01b3);
        }
        println!("cargo:rerun-if-changed={}", path.display());
        writeln!(source, "    ({name:?}, include_bytes!({:?})),", absolute(path)).unwrap();
    }
    source.push_str("];\n");
    writeln!(source, "/// Names the runtime folder: the files' hash.\npub const ID: &str = \"{hash:016x}\";").unwrap();
    std::fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("bundle.rs"), source).unwrap();
}

fn absolute(path: &Path) -> String {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()).display().to_string()
}
