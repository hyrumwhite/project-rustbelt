//! `src/realtime/nrc.wesl` is generated: a header declaring the NRC bind
//! group, then `crates/nrc_records/shaders/nrc_records.wgsl` and
//! `nrc_buffers.wgsl` verbatim. The record formats are tested against the
//! library's layout in that crate (a CPU reference and a GPU round trip), so
//! the copy here must not drift from them.
//!
//! After changing either source (or the header below), regenerate with
//! `NRC_MODULE_BLESS=1 cargo test --test nrc_module`.

use std::path::{Path, PathBuf};

/// The bind group of the passes that use NRC (group 2), as
/// docs/nrc-integration.md's table and `realtime/nrc.rs` lay it out.
const HEADER: &str = "\
// GENERATED, do not edit: this header, then crates/nrc_records/shaders/
// nrc_records.wgsl and nrc_buffers.wgsl verbatim. tests/nrc_module.rs checks
// that the file is exactly that; change the sources (or the header in the
// test) and regenerate with `NRC_MODULE_BLESS=1 cargo test --test nrc_module`.
//
// NVIDIA's Neural Radiance Cache, the shader side: the records the library
// reads and writes, their packing, and the NRC bind group. The module is only
// loaded with the `nrc` feature, so nothing in it is conditional. The buffers
// are word arrays because the library reads them with DX scalar layout (a
// WGSL struct would pad every vec3 to 16 bytes); nrc_buffers.wgsl addresses
// them by hand.
//
// Binding 5 is not the library's Counter buffer but Solarik's own
// device-local pair of counters, copied into the library's (host-visible)
// buffer before QueryAndTrain: an atomic on host memory is a PCIe round trip.

@group(2) @binding(0) var<uniform> nrc_constants: NrcConstants;
@group(2) @binding(1) var<storage, read_write> nrc_query_path_info: array<u32>;
@group(2) @binding(2) var<storage, read_write> nrc_training_path_info: array<u32>;
@group(2) @binding(3) var<storage, read_write> nrc_training_path_vertices: array<u32>;
@group(2) @binding(4) var<storage, read_write> nrc_query_radiance_params: array<u32>;
@group(2) @binding(5) var<storage, read_write> nrc_counters: array<atomic<u32>>;
@group(2) @binding(6) var<storage, read> nrc_query_radiance: array<f32>;

";

fn read(path: &Path) -> String {
    // Line endings follow the checkout (`core.autocrlf`), not the content.
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
}

#[test]
fn nrc_module_is_the_header_and_the_record_sources() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let shaders = manifest.join("../crates/nrc_records/shaders");
    let expected = format!(
        "{HEADER}{}{}",
        read(&shaders.join("nrc_records.wgsl")),
        read(&shaders.join("nrc_buffers.wgsl")),
    );
    let module = manifest.join("src/realtime/nrc.wesl");
    if std::env::var_os("NRC_MODULE_BLESS").is_some() {
        std::fs::write(&module, &expected).unwrap();
    }
    let actual = read(&module);
    assert!(
        actual == expected,
        "src/realtime/nrc.wesl differs from the header + nrc_records.wgsl + nrc_buffers.wgsl; \
         regenerate with NRC_MODULE_BLESS=1 cargo test --test nrc_module"
    );
}
