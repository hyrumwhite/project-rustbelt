//! The NRC shaders (feature `nrc`) through the actual Bevy WESL compiler and
//! naga's validator: the generated record module on its own, the training
//! pass, restir_gi with `NRC` next to each def it is combined with in the
//! game, and specular_gi and the resolve with `NRC_SPECULAR`. (restir_gi
//! without `NRC` is covered by scene_shader_variants.rs.)
#[path = "support/shader_graph.rs"]
mod shader_graph;
use bevy_shader::ShaderDefVal;

fn base_defs() -> Vec<ShaderDefVal> {
    vec![
        ShaderDefVal::UInt("WORLD_CACHE_SIZE".into(), 1 << 20),
        ShaderDefVal::UInt("RASTER_SUBPIXEL_BITS".into(), 8),
        ShaderDefVal::UInt("DI_TILE_CANDIDATES".into(), 16),
        ShaderDefVal::UInt("DI_CELL_CANDIDATES".into(), 16),
        ShaderDefVal::UInt("DI_CONFIDENCE_CAP".into(), 8),
        ShaderDefVal::UInt("GI_CONFIDENCE_CAP".into(), 8),
        ShaderDefVal::UInt("CACHE_TILE_CANDIDATES".into(), 128),
        ShaderDefVal::UInt("GI_ADAPTIVE_CONFIDENCE".into(), 4),
        ShaderDefVal::UInt("SPECULAR_REUSE_ROUGHNESS".into(), 30),
        "BINDLESS_SURFACE_DETAIL".into(),
        "GI_ADAPTIVE_SPATIAL".into(),
        "CELL_LOOKUP_NEIGHBOURS".into(),
        "CACHE_SHADE_SECOND_RAY".into(),
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
        &["NRC_QUERY_AT_SPREAD"],
        &["NRC_QUERY_AT_SPREAD", "VISUALIZE_NRC", "VISUALIZE_NRC_GAIN"],
        &["NRC_QUERY_AT_SPREAD", "VISUALIZE_NRC", "VISUALIZE_NRC_BOUNCED"],
        &[
            "NRC_QUERY_AT_SPREAD",
            "REGIONAL_HISTORY",
            "FOLIAGE_TRANSMISSION",
            "RAY_MATERIAL_FOOTPRINTS",
            "DLSS_RR_GUIDE_BUFFERS",
        ],
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
    // NRC_SPECULAR: the resolve reads the specular slot (and its view).
    for extra in [
        &[][..],
        &["VISUALIZE_NRC_SPECULAR"],
        &["VISUALIZE_NRC"],
        &["REGIONAL_HISTORY", "FOLIAGE_TRANSMISSION", "RAY_MATERIAL_FOOTPRINTS"],
        &["WORLD_CACHE_FIRST_BOUNCE_LIGHT_LEAK_PREVENTION", "FOLIAGE_TRANSMISSION"],
    ] {
        let mut names = vec!["NRC", "NRC_SPECULAR"];
        names.extend_from_slice(extra);
        graph.validate("bevy_solarik::realtime::restir_gi", &with(&names));
    }
    // The specular pass writing its query: plain and with the PSR guides
    // (their textures at group 3 then, NRC's buffers at group 2).
    for extra in [
        &["FOLIAGE_TRANSMISSION"][..],
        &["FOLIAGE_TRANSMISSION", "DLSS_RR_GUIDE_BUFFERS"],
        &["FOLIAGE_TRANSMISSION", "RAY_MATERIAL_FOOTPRINTS"],
        &["FOLIAGE_TRANSMISSION", "RAY_MATERIAL_FOOTPRINTS", "DLSS_RR_GUIDE_BUFFERS", "REGIONAL_HISTORY"],
    ] {
        let mut names = vec!["NRC", "NRC_SPECULAR"];
        names.extend_from_slice(extra);
        graph.validate("bevy_solarik::realtime::specular_gi", &with(&names));
    }
    // Its second stage, the surfaces reusing the GI reservoir: no NRC
    // bindings, the guide textures at group 2.
    for extra in [
        &["FOLIAGE_TRANSMISSION"][..],
        &["FOLIAGE_TRANSMISSION", "DLSS_RR_GUIDE_BUFFERS"],
        &["FOLIAGE_TRANSMISSION", "RAY_MATERIAL_FOOTPRINTS", "DLSS_RR_GUIDE_BUFFERS"],
    ] {
        let mut names = vec!["NRC_SPECULAR_REUSE"];
        names.extend_from_slice(extra);
        graph.validate("bevy_solarik::realtime::specular_gi", &with(&names));
    }
    // primary_glass imports specular_gi's path; it never gets NRC_SPECULAR,
    // with or without NRC among its defs.
    for extra in [&[][..], &["NRC"], &["NRC", "DLSS_RR_GUIDE_BUFFERS"]] {
        let mut names = vec!["FOLIAGE_TRANSMISSION"];
        names.extend_from_slice(extra);
        graph.validate("bevy_solarik::realtime::primary_glass", &with(&names));
    }
}
