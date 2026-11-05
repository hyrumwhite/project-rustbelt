//! Builds the C shim over NRC's C++ API and links `NRC_Vulkan.lib`.
//!
//! The SDK comes from `NRC_SDK`, defaulting to `<repo>/vendor/NRC`. Its
//! `Bin/x64` directory is passed to the crate as `NRC_SDK_BIN_DIR`
//! ([`nrc_wgpu::sdk_bin_dir`]).
//!
//! `NRC_Vulkan.dll` is delay-loaded (`/DELAYLOAD`), so a program starts
//! without it on the search path and `nrc_wgpu::load_library` loads it from a
//! directory chosen at run time. `rustc-link-arg` only reaches this package's
//! own binaries and tests: a binary in another package that links this crate
//! must pass `/DELAYLOAD:NRC_Vulkan.dll` itself (see README.md), or the DLL
//! becomes a load-time dependency.

use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=shim/nrc_shim.cpp");
    println!("cargo:rerun-if-changed=shim/nrc_shim.h");
    println!("cargo:rerun-if-changed=shim/vk_stub/vulkan/vulkan.h");
    println!("cargo:rerun-if-env-changed=NRC_SDK");

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let sdk = env::var_os("NRC_SDK")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("../../vendor/NRC"));
    let sdk = std::fs::canonicalize(&sdk).unwrap_or_else(|e| {
        panic!(
            "the NRC SDK is not at {} ({e}); set NRC_SDK to the directory holding Include/, Lib/x64 and Bin/x64",
            sdk.display()
        )
    });
    // (canonicalize gives a \\?\ path on Windows; the compiler and linker
    // take it, but the run-time path is nicer without)
    let sdk = PathBuf::from(sdk.display().to_string().trim_start_matches(r"\\?\"));

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" || target_arch != "x86_64" || target_env != "msvc" {
        panic!("nrc_wgpu supports x86_64-pc-windows-msvc only (the SDK ships Windows x64 binaries)");
    }

    let include = sdk.join("Include");
    // (joined part by part: a "Bin/x64" join leaves a forward slash in the run-time path)
    let lib_dir = sdk.join("Lib").join("x64");
    let bin_dir = sdk.join("Bin").join("x64");
    for (path, what) in [
        (include.join("NrcVk.h"), "Include/NrcVk.h"),
        (lib_dir.join("NRC_Vulkan.lib"), "Lib/x64/NRC_Vulkan.lib"),
        (bin_dir.join("NRC_Vulkan.dll"), "Bin/x64/NRC_Vulkan.dll"),
    ] {
        if !path.is_file() {
            panic!("the NRC SDK at {} has no {what}", sdk.display());
        }
        println!("cargo:rerun-if-changed={}", path.display());
    }

    cc::Build::new()
        .cpp(true)
        .file(manifest.join("shim/nrc_shim.cpp"))
        .include(manifest.join("shim"))
        .include(manifest.join("shim/vk_stub"))
        .include(&include)
        .flag_if_supported("/std:c++17")
        .flag_if_supported("/EHsc")
        .compile("nrc_shim");

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=dylib=NRC_Vulkan");
    // The delay-load helper (__delayLoadHelper2), and the flag for this
    // package's own test and example binaries.
    println!("cargo:rustc-link-lib=delayimp");
    println!("cargo:rustc-link-arg=/DELAYLOAD:NRC_Vulkan.dll");

    println!("cargo:rustc-env=NRC_SDK_BIN_DIR={}", bin_dir.display());
}
