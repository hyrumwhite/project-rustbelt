// Fog over what the specular paths see (SolarikRayFog): the application's
// fog, which it draws over the picture by each pixel's depth, then covers
// only the way from the camera to the surface a path leaves. Here the path's
// first hit is fogged so that, with that pass over it, it is fogged for the
// whole way (camera to surface to hit).
//
// The fog is Halo's environment fog (two layers, display-encoded colours):
//   atmospheric = density * A((distance - minimum) / range)
//   planar      = density * mix(L(distance) L(depth), L(distance), the
//                 camera's planar density)
//   colour      = atmospheric colour * atmospheric * (1 - (1 - eye) planar)
//               + planar colour * planar * (1 - atmospheric * eye)
//   coverage    = 1 - (1 - atmospheric) (1 - planar)
// over a picture p as colour + p (1 - coverage); A a ramp and L a smoothstep
// over 16 texels, the depth under the fog's plane over its maximum depth, eye
// the camera's atmospheric density.

import bevy_solarik::scene::raytracing_scene_bindings::sky_light;

// The fog's premultiplied colour (display-encoded) and its coverage, at a
// view depth (metres) and a height over the fog's plane.
fn ray_fog_layers(view_depth: f32, height: f32) -> vec4<f32> {
    let fog = sky_light.fog;
    let range = max(fog.atmospheric.y - fog.atmospheric.x, 1e-4);
    let atmospheric = fog.atmospheric_color.a * fog_texel((view_depth - fog.atmospheric.x) / range);
    let distance = smoothstep(0.0, 1.0, fog_texel(view_depth / max(fog.planar.x, 1e-4)));
    let under = smoothstep(0.0, 1.0, fog_texel(-height / max(fog.planar.y, 1e-4)));
    let planar = fog.planar_color.a * mix(distance * under, distance, fog.planar.z);
    let eye = fog.atmospheric.z;
    let colour = fog.atmospheric_color.rgb * atmospheric * (1.0 - (1.0 - eye) * planar)
        + fog.planar_color.rgb * planar * (1.0 - atmospheric * eye);
    return vec4(colour, 1.0 - (1.0 - atmospheric) * (1.0 - planar));
}

// A 16-texel density texture's coordinate as the texel it falls on.
fn fog_texel(u: f32) -> f32 {
    return saturate((u * 16.0 - 0.5) / 15.0);
}

fn ray_fog_active() -> bool {
    return sky_light.fog.atmospheric_color.a > 0.0 || sky_light.fog.planar_color.a > 0.0;
}

// The radiance a path brings back from a hit `segment` metres beyond the
// surface it leaves (at view depth `surface_depth` and `surface_height`),
// the hit `hit_height` over the plane: such that the picture's fog by the
// surface's depth, over it, leaves the hit fogged for the whole way. The fog's
// colour is taken as radiance at the camera's exposure (the scene's units
// over `exposure`), unmapped by the picture's tonemapping.
fn fog_ray_segment(radiance: vec3<f32>, surface_depth: f32, surface_height: f32, segment: f32, hit_height: f32, exposure: f32) -> vec3<f32> {
    if !ray_fog_active() { return radiance; }
    let near = ray_fog_layers(surface_depth, surface_height);
    let far = ray_fog_layers(surface_depth + segment, hit_height);
    let scale = 1.0 / max(exposure, 1e-12);
    let near_colour = fog_decode(near.rgb) * scale;
    let far_colour = fog_decode(far.rgb) * scale;
    let fogged = radiance * (1.0 - far.a) + max(far_colour - near_colour, vec3(0.0));
    return fogged / max(1.0 - near.a, 1e-4);
}

fn fog_decode(encoded: vec3<f32>) -> vec3<f32> {
    let low = encoded / 12.92;
    let high = pow((max(encoded, vec3(0.0)) + 0.055) / 1.055, vec3(2.4));
    return select(high, low, encoded <= vec3(0.04045));
}
