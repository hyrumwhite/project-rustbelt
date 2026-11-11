import bevy_pbr::render::rgb9e5::vec3_to_rgb9e5_;
import bevy_pbr::render::rgb9e5::rgb9e5_to_vec3_;
import bevy_render::utils::octahedral_encode;
import bevy_render::utils::octahedral_decode;
import bevy_render::view::View;
import bevy_solarik::scene::sampling::generate_random_light_sample;
import bevy_solarik::scene::sampling::LightSample;
import bevy_solarik::scene::sampling::ResolvedLightSample;
import bevy_solarik::scene::sampling::LIGHT_SAMPLE_DIRECTIONAL;
import bevy_solarik::scene::sampling::LIGHT_SAMPLE_EMISSIVE_MESH;
import bevy_solarik::scene::sampling::LIGHT_SAMPLE_LOCAL;
import bevy_solarik::realtime::realtime_bindings::light_tile_samples;
import bevy_solarik::realtime::realtime_bindings::light_tile_resolved_samples;
import bevy_solarik::realtime::realtime_bindings::view;
import bevy_solarik::realtime::realtime_bindings::constants;
import bevy_solarik::realtime::realtime_bindings::ResolvedLightSamplePacked;
import bevy_solarik::scene::raytracing_scene_bindings::light_sources;
import bevy_solarik::scene::raytracing_scene_bindings::directional_lights;
import bevy_solarik::scene::raytracing_scene_bindings::has_sun;

// https://cwyman.org/papers/hpg21_rearchitectingReSTIR.pdf

enable wgpu_ray_query;



@compute @workgroup_size(1024, 1, 1)
fn presample_light_tiles(@builtin(workgroup_id) workgroup_id: vec3<u32>, @builtin(local_invocation_index) sample_index: u32) {
    let tile_id = workgroup_id.x;
    var rng = light_tile_seed((tile_id * 1024u) + sample_index, constants.frame_index);

    var sample = generate_random_light_sample(&rng);
    // The tiles hold the lights other than the sun, which ReSTIR DI and the
    // world cache sample by themselves: a sun sample is drawn again, and the
    // others' density is the table's over what isn't the sun's.
    if has_sun() {
        let sun_probability = light_sources[directional_lights[0].light_source_index].selection_probability;
        if sun_probability < 0.999 {
            for (var tries = 0u; tries < 16u && sample.resolved_light_sample.world_position.w == LIGHT_SAMPLE_DIRECTIONAL; tries++) {
                sample = generate_random_light_sample(&rng);
            }
            sample.resolved_light_sample.inverse_pdf *= 1.0 - sun_probability;
        }
    }

    let i = (tile_id * 1024u) + sample_index;
    light_tile_samples[i] = sample.light_sample;
    light_tile_resolved_samples[i] = pack_resolved_light_sample(sample.resolved_light_sample);
}

// A seed for `index` that is new every frame. Bevy's generators use their
// seed as their state, so `index + frame` seeds slide: frame f + 1's slot i
// drew frame f's slot i + 1's light, and the tiles' 131072 samples turned
// over in a thousand frames; a rare light's share of a tile then drifted for
// hundreds of frames (interiors brightening and darkening fivefold).
fn light_tile_seed(index: u32, frame_index: u32) -> u32 {
    return seed_hash(seed_hash(index) ^ (frame_index * 0x9E3779B9u));
}

fn seed_hash(input: u32) -> u32 {
    let state = input * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// The kind rides in two signs: a negative inverse_pdf is a directional light
// (upstream), a positive range is a point/spot light (whose range is always
// positive). An emissive mesh stores the distance from its point to the
// triangle's farthest corner negated in the range, and its near distance (signed: the unseen mark) in spot_cos.y as an
// f16 rounded away from 0, so that sampling.wesl's near pre-test stays on
// the safe side; a directional light -1. An emissive mesh's emission cone is
// its spot_cos.x above 0.
fn pack_resolved_light_sample(sample: ResolvedLightSample) -> ResolvedLightSamplePacked {
    let local = sample.world_position.w == LIGHT_SAMPLE_LOCAL;
    let emissive = sample.world_position.w == LIGHT_SAMPLE_EMISSIVE_MESH;
    return ResolvedLightSamplePacked(
        sample.world_position.x,
        sample.world_position.y,
        sample.world_position.z,
        pack2x16unorm(octahedral_encode(sample.world_normal)),
        vec3_to_rgb9e5_(log2(sample.radiance * view.exposure + 1.0)),
        sample.inverse_pdf * select(1.0, -1.0, sample.world_position.w == LIGHT_SAMPLE_DIRECTIONAL),
        pack2x16unorm(octahedral_encode(select(vec3(0.0, 0.0, 1.0), sample.spot_direction, local || (emissive && sample.spot_cos.x > 0.0)))),
        pack2x16float(select(sample.spot_cos, vec2(sample.spot_cos.x, clamp(sample.spot_cos.y * 1.001, -65000.0, 65000.0)), emissive)),
        select(select(-1.0, -max(sample.range, 0.0), emissive), max(sample.range, 1e-4), local),
    );
}

fn unpack_resolved_light_sample(packed: ResolvedLightSamplePacked, exposure: f32) -> ResolvedLightSample {
    let local = packed.range > 0.0;
    let kind = select(select(LIGHT_SAMPLE_EMISSIVE_MESH, LIGHT_SAMPLE_LOCAL, local), LIGHT_SAMPLE_DIRECTIONAL, packed.inverse_pdf < 0.0);
    return ResolvedLightSample(
        vec4(packed.world_position_x, packed.world_position_y, packed.world_position_z, kind),
        octahedral_decode(unpack2x16unorm(packed.world_normal)),
        (exp2(rgb9e5_to_vec3_(packed.radiance)) - 1.0) / exposure,
        abs(packed.inverse_pdf),
        octahedral_decode(unpack2x16unorm(packed.spot_direction)),
        unpack2x16float(packed.spot_cos),
        select(select(0.0, -packed.range, kind == LIGHT_SAMPLE_EMISSIVE_MESH), packed.range, local),
    );
}
