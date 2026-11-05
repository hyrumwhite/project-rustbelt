//! The NRC shaders (feature `nrc`) through the actual Bevy WESL compiler and
//! naga's validator: the generated record module on its own, the training
//! pass, and restir_gi with `NRC` next to each def it is combined with in the
//! game. (restir_gi without `NRC` is covered by scene_shader_variants.rs.)
#[path = "support/shader_graph.rs"]
mod shader_graph;
use bevy_shader::ShaderDefVal;

fn base_defs() -> Vec<ShaderDefVal> {
    vec![
        ShaderDefVal::UInt("WORLD_CACHE_SIZE".into(), 1 << 20),
        ShaderDefVal::UInt("RASTER_SUBPIXEL_BITS".into(), 8),
        "BINDLESS_SURFACE_DETAIL".into(),
    ]
}

fn with(extra: &[&'static str]) -> Vec<ShaderDefVal> {
    let mut defs = base_defs();
    defs.extend(extra.iter().map(|name| ShaderDefVal::from(*name)));
    defs
}

#[test]
fn nrc_shaders_compile() {
    let mut graph = shader_graph::Graph::load();
    graph.validate("bevy_solarik::realtime::nrc", &base_defs());
    graph.validate(
        "bevy_solarik::realtime::nrc_training",
        &with(&["NRC", "FOLIAGE_TRANSMISSION"]),
    );
    // In case the renderer hands the training pipeline its global defs too.
    graph.validate(
        "bevy_solarik::realtime::nrc_training",
        &with(&[
            "NRC",
            "FOLIAGE_TRANSMISSION",
            "RAY_MATERIAL_FOOTPRINTS",
            "REGIONAL_HISTORY",
        ]),
    );
    for extra in [
        &[][..],
        &["REGIONAL_HISTORY"],
        &["FOLIAGE_TRANSMISSION"],
        &["WORLD_CACHE_FIRST_BOUNCE_LIGHT_LEAK_PREVENTION"],
        &["RAY_MATERIAL_FOOTPRINTS"],
        &["VISUALIZE_NRC"],
        &[
            "REGIONAL_HISTORY",
            "FOLIAGE_TRANSMISSION",
            "RAY_MATERIAL_FOOTPRINTS",
            "DLSS_RR_GUIDE_BUFFERS",
        ],
    ] {
        let mut names = vec!["NRC"];
        names.extend_from_slice(extra);
        graph.validate("bevy_solarik::realtime::restir_gi", &with(&names));
    }
}
