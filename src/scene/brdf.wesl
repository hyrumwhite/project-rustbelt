import bevy_solarik::lommel_math::lommel_scattering;
import bevy_solarik::gaussian_math::gaussian_specular;
import bevy_solarik::gaussian_math::dielectric_ior;
import bevy_solarik::gaussian_math::dielectric_fresnel;
import bevy_core_pipeline::tonemapping::tonemapping_luminance as luminance;
import bevy_pbr::render::pbr_lighting::D_GGX;
import bevy_pbr::render::pbr_lighting::V_SmithGGXCorrelated;
import bevy_pbr::render::pbr_lighting::specular_multiscatter;
import bevy_pbr::render::pbr_functions::calculate_diffuse_color;
import bevy_pbr::render::pbr_functions::calculate_F0;
import bevy_pbr::render::utils::rand_f;
import bevy_pbr::render::utils::sample_cosine_hemisphere;
import bevy_render::maths::PI;
import bevy_render::maths::orthonormalize;
import bevy_solarik::scene::sampling::sample_ggx_vndf;
import bevy_solarik::scene::sampling::ggx_vndf_pdf;
import bevy_solarik::scene::sampling::ggx_vndf_sample_invalid;
import bevy_solarik::scene::raytracing_scene_bindings::ResolvedMaterial;
import bevy_solarik::scene::raytracing_scene_bindings::MIRROR_ROUGHNESS_THRESHOLD;
import bevy_solarik::scene::raytracing_scene_bindings::brdf_dfg_lut;
import bevy_solarik::scene::raytracing_scene_bindings::brdf_dfg_lut_sampler;

enable wgpu_ray_query;



struct EvaluateAndSampleBrdfResult {
    wi: vec3<f32>,
    throughput: vec3<f32>,
    pdf: f32,
    sampled_roughness: f32,
}

// A surface point seen from `wo` faces `wo`. Interpolated and mapped shading
// normals can point past the viewer's horizon at grazing angles, where every
// lobe is zero: the silhouette of a normal-mapped surface is then outlined in
// black pixels. The facets such a normal stands for are hidden behind others
// that tilt towards the viewer about as far, so the normal is mirrored into
// the visible hemisphere. A normal that already faces `wo` is unchanged.
fn view_facing_normal(shading_normal: vec3<f32>, wo: vec3<f32>) -> vec3<f32> {
    let cosine = dot(shading_normal, wo);
    return shading_normal - 2.0 * min(cosine, 0.0) * wo;
}

fn evaluate_and_sample_brdf(
    wo: vec3<f32>,
    shading_normal: vec3<f32>,
    material: ResolvedMaterial,
    rng: ptr<function, u32>,
) -> EvaluateAndSampleBrdfResult {
    let world_normal = view_facing_normal(shading_normal, wo);
    let NdotV = dot(world_normal, wo);
    if NdotV < 0.0001 { return EvaluateAndSampleBrdfResult(vec3(0.0), vec3(0.0), 0.0, 0.0); }
    let F0 = calculate_F0(material.base_color, material.metallic, vec3(material.reflectance));
    let df = select(1.0 - luminance(specular_albedo(F0, NdotV, material.perceptual_roughness)), 1.0, material.lommel);

    let diffuse_weight = select(mix(df, 0.0, material.metallic), 1.0, material.lommel);
    let specular_weight = 1.0 - diffuse_weight;

    let TBN = orthonormalize(world_normal);
    let T = TBN[0];
    let B = TBN[1];
    let N = TBN[2];

    let wo_tangent = vec3(dot(wo, T), dot(wo, B), dot(wo, N));

    var wi: vec3<f32>;
    var wi_tangent: vec3<f32>;
    // The coat is picked by its share of the reflected light; without one the
    // random number (and so the sequence) is the same as before.
    let coat_weight = clearcoat_albedo(material, NdotV);
    var lobe = rand_f(rng);
    let coat_selected = lobe < coat_weight;
    lobe = (lobe - coat_weight) / (1.0 - coat_weight);
    let diffuse_selected = !coat_selected && lobe < diffuse_weight;
    if coat_selected {
        wi_tangent = sample_ggx_vndf(wo_tangent, clearcoat_roughness(material), rng);
        if ggx_vndf_sample_invalid(wi_tangent) {
            return EvaluateAndSampleBrdfResult(vec3(0.0), vec3(0.0), 0.0, 0.0);
        }
        wi = wi_tangent.x * T + wi_tangent.y * B + wi_tangent.z * N;
    } else if diffuse_selected {
        // Preserve the old random sequence when transmission is disabled.
        var normal = world_normal;
        if material.diffuse_transmission > 0.0 {
            if rand_f(rng) < material.diffuse_transmission { normal = -normal; }
        }
        wi = sample_cosine_hemisphere(normal, rng);
        wi_tangent = vec3(dot(wi, T), dot(wi, B), dot(wi, N));
    } else {
        wi_tangent = sample_ggx_vndf(wo_tangent, material.roughness, rng);
        if ggx_vndf_sample_invalid(wi_tangent) {
            return EvaluateAndSampleBrdfResult(vec3(0.0), vec3(0.0), 0.0, 0.0);
        }
        wi = wi_tangent.x * T + wi_tangent.y * B + wi_tangent.z * N;
    }

    let pdf = evaluate_brdf_pdf(wo, wi, world_normal, material);
    if pdf <= 0.0 { return EvaluateAndSampleBrdfResult(wi, vec3(0.0), 0.0, 0.0); }

    var throughput = evaluate_brdf(wo, wi, world_normal, material);
    // (A mirror surface has no coat: clearcoat_albedo.)
    if coat_selected || diffuse_selected || material.roughness > MIRROR_ROUGHNESS_THRESHOLD {
        throughput /= pdf;
    } else {
        throughput /= specular_weight;
    }

    var sampled_roughness = select(material.roughness, 1.0, diffuse_selected);
    if coat_selected { sampled_roughness = clearcoat_roughness(material); }
    return EvaluateAndSampleBrdfResult(wi, throughput, pdf, sampled_roughness);
}

fn evaluate_brdf(
    wo: vec3<f32>,
    wi: vec3<f32>,
    world_normal: vec3<f32>,
    material: ResolvedMaterial,
) -> vec3<f32> {
    return evaluate_diffuse_brdf(wo, wi, world_normal, material) + evaluate_specular_brdf(wo, wi, world_normal, material);
}

// A clear coat (Filament's model): a smooth dielectric layer (f0 4%) of its
// own roughness over the surface, which sees the light the coat lets through
// on the way in and out. Mirror surfaces take none (their delta lobe has no
// share to give up).
const CLEARCOAT_F0: f32 = 0.04;
// Kept off the mirror threshold, where the lobe would turn into a delta.
const CLEARCOAT_MIN_PERCEPTUAL_ROUGHNESS: f32 = 0.045;

fn clearcoat_roughness(material: ResolvedMaterial) -> f32 {
    let perceptual = max(material.clearcoat_perceptual_roughness, CLEARCOAT_MIN_PERCEPTUAL_ROUGHNESS);
    return perceptual * perceptual;
}

// Share of the light arriving at `cosine` to the normal that the coat reflects.
fn clearcoat_albedo(material: ResolvedMaterial, cosine: f32) -> f32 {
    if material.clearcoat <= 0.0 || material.lommel || material.roughness <= MIRROR_ROUGHNESS_THRESHOLD { return 0.0; }
    let perceptual = max(material.clearcoat_perceptual_roughness, CLEARCOAT_MIN_PERCEPTUAL_ROUGHNESS);
    return material.clearcoat * specular_albedo(vec3(CLEARCOAT_F0), saturate(cosine), perceptual).g;
}

// What the coat leaves of the surface beneath it, for light in and out.
fn clearcoat_attenuation(material: ResolvedMaterial, NdotV: f32, NdotL: f32) -> f32 {
    return (1.0 - clearcoat_albedo(material, NdotV)) * (1.0 - clearcoat_albedo(material, abs(NdotL)));
}

fn evaluate_clearcoat_brdf(wo: vec3<f32>, wi: vec3<f32>, shading_normal: vec3<f32>, material: ResolvedMaterial) -> vec3<f32> {
    if clearcoat_albedo(material, 1.0) <= 0.0 { return vec3(0.0); }
    let world_normal = view_facing_normal(shading_normal, wo);
    let H = normalize(wi + wo);
    let NdotL = dot(world_normal, wi);
    let NdotH = dot(world_normal, H);
    let LdotH = dot(wi, H);
    let NdotV = dot(world_normal, wo);
    if NdotL < 0.0001 || NdotH < 0.0001 || LdotH < 0.0001 || NdotV < 0.0001 { return vec3(0.0); }
    let roughness = clearcoat_roughness(material);
    let F = clearcoat_fresnel(LdotH);
    return material.clearcoat * D_GGX(roughness, NdotH) * V_SmithGGXCorrelated(roughness, NdotV, NdotL) * F * NdotL;
}

// The coat's reflectance at `LdotH`: a 4% dielectric's, or with
// THIN_FILM_COAT a thin film's, whose interference colours it by the angle.
fn clearcoat_fresnel(LdotH: f32) -> vec3<f32> {
    var F = vec3(fresnel(vec3(CLEARCOAT_F0), LdotH).g);
@if(THIN_FILM_COAT) {
    F = thin_film_reflectance(LdotH);
}
    return F;
}

// A thin dielectric film over the surface (THIN_FILM_COAT: its thickness in
// nanometres, its and the surface's indices of refraction in thousandths):
// the two-beam interference of the light reflected by the film's top and its
// bottom (Airy's sum), for s and p polarisations, averaged over three
// wavelengths in each of red, green and blue. An oil film's or a beetle's
// colours, shifting with the angle.
@if(THIN_FILM_COAT)
fn thin_film_reflectance(cos1: f32) -> vec3<f32> {
    let n2 = f32(constants::THIN_FILM_IOR) / 1000.0;
    let n3 = f32(constants::THIN_FILM_BASE_IOR) / 1000.0;
    let thickness = f32(constants::THIN_FILM_NM);
    let sin1_squared = 1.0 - cos1 * cos1;
    let cos2 = sqrt(max(1.0 - sin1_squared / (n2 * n2), 0.0));
    let cos3 = sqrt(max(1.0 - sin1_squared / (n3 * n3), 0.0));
    let r12s = (cos1 - n2 * cos2) / (cos1 + n2 * cos2);
    let r12p = (n2 * cos1 - cos2) / (n2 * cos1 + cos2);
    let r23s = (n2 * cos2 - n3 * cos3) / (n2 * cos2 + n3 * cos3);
    let r23p = (n3 * cos2 - n2 * cos3) / (n3 * cos2 + n2 * cos3);
    // the phase difference of the two reflections per nanometre of wavelength
    let path = 4.0 * PI * n2 * thickness * cos2;
    let red = path / vec3(610.0, 650.0, 690.0);
    let green = path / vec3(500.0, 532.0, 565.0);
    let blue = path / vec3(420.0, 450.0, 480.0);
    let s = vec3(dot(airy(r12s, r23s, red), vec3(1.0)), dot(airy(r12s, r23s, green), vec3(1.0)), dot(airy(r12s, r23s, blue), vec3(1.0)));
    let p = vec3(dot(airy(r12p, r23p, red), vec3(1.0)), dot(airy(r12p, r23p, green), vec3(1.0)), dot(airy(r12p, r23p, blue), vec3(1.0)));
    return (s + p) / 6.0;
}

// |r12 + r23 e^(i phase)|^2 / |1 + r12 r23 e^(i phase)|^2
@if(THIN_FILM_COAT)
fn airy(r12: f32, r23: f32, phase: vec3<f32>) -> vec3<f32> {
    let c = cos(phase);
    return (r12 * r12 + r23 * r23 + 2.0 * r12 * r23 * c) / (1.0 + r12 * r12 * r23 * r23 + 2.0 * r12 * r23 * c);
}

// Picks a glossy lobe (the coat or the surface's own specular) by their
// shares of the reflected light; the tangent-space direction and the pair's
// combined density, for paths that follow only glossy reflection.
fn sample_glossy_lobes(wo_tangent: vec3<f32>, material: ResolvedMaterial, rng: ptr<function, u32>) -> vec4<f32> {
    let coat = glossy_coat_probability(wo_tangent.z, material);
    var roughness = material.roughness;
    if coat > 0.0 && rand_f(rng) < coat { roughness = clearcoat_roughness(material); }
    let wi_tangent = sample_ggx_vndf(wo_tangent, roughness, rng);
    if ggx_vndf_sample_invalid(wi_tangent) { return vec4(wi_tangent, 0.0); }
    return vec4(wi_tangent, glossy_lobes_pdf(wo_tangent, wi_tangent, material));
}

fn glossy_lobes_pdf(wo_tangent: vec3<f32>, wi_tangent: vec3<f32>, material: ResolvedMaterial) -> f32 {
    let base = ggx_vndf_pdf(wo_tangent, wi_tangent, material.roughness);
    let coat = glossy_coat_probability(wo_tangent.z, material);
    if coat <= 0.0 { return base; }
    return mix(base, ggx_vndf_pdf(wo_tangent, wi_tangent, clearcoat_roughness(material)), coat);
}

fn glossy_coat_probability(NdotV: f32, material: ResolvedMaterial) -> f32 {
    let coat = clearcoat_albedo(material, NdotV);
    if coat <= 0.0 { return 0.0; }
    let F0 = calculate_F0(material.base_color, material.metallic, vec3(material.reflectance));
    let base = (1.0 - coat) * luminance(specular_albedo(F0, saturate(NdotV), material.perceptual_roughness));
    return coat / max(coat + base, 1e-6);
}

fn evaluate_diffuse_brdf(wo: vec3<f32>, wi: vec3<f32>, shading_normal: vec3<f32>, material: ResolvedMaterial) -> vec3<f32> {
    let world_normal = view_facing_normal(shading_normal, wo);
    let diffuse_color = calculate_diffuse_color(material.base_color, material.metallic, 0.0, 0.0) / PI;

    let NdotL = dot(world_normal, wi);
    let NdotV = dot(world_normal, wo);
    if material.lommel { return lommel_scattering(material.base_color, NdotL, NdotV); }
    if abs(NdotL) < 0.0001 || NdotV < 0.0001 { return vec3(0.0); }
    let F0 = calculate_F0(material.base_color, material.metallic, vec3(material.reflectance));
    // Light the specular layer reflects on the way in or out never reaches
    // the diffuse base.
    var layering = (1.0 - specular_albedo(F0, abs(NdotL), material.perceptual_roughness))
        * (1.0 - specular_albedo(F0, NdotV, material.perceptual_roughness));

    if material.gaussian_weight > 0.0 {
        let ior = dielectric_ior(material.reflectance);
        let transmission = (1.0-dielectric_fresnel(abs(NdotL),ior))*(1.0-dielectric_fresnel(NdotV,ior));
        layering = mix(layering, vec3(transmission), material.gaussian_weight);
    }
    let side_weight = select(1.0 - material.diffuse_transmission, material.diffuse_transmission, NdotL < 0.0);
    return diffuse_color * layering * abs(NdotL) * side_weight * clearcoat_attenuation(material, NdotV, NdotL);
}

// Both glossy lobes: the surface's own beneath the coat, and the coat.
fn evaluate_specular_brdf(wo: vec3<f32>, wi: vec3<f32>, shading_normal: vec3<f32>, material: ResolvedMaterial) -> vec3<f32> {
    return evaluate_base_specular_brdf(wo, wi, shading_normal, material) + evaluate_clearcoat_brdf(wo, wi, shading_normal, material);
}

// The surface's own specular lobe, beneath any coat.
fn evaluate_base_specular_brdf(wo: vec3<f32>, wi: vec3<f32>, shading_normal: vec3<f32>, material: ResolvedMaterial) -> vec3<f32> {
    let world_normal = view_facing_normal(shading_normal, wo);
    let coat = clearcoat_attenuation(material, dot(world_normal, wo), dot(world_normal, wi));
    return evaluate_uncoated_specular_brdf(wo, wi, shading_normal, material) * coat;
}

fn evaluate_uncoated_specular_brdf(wo: vec3<f32>, wi: vec3<f32>, shading_normal: vec3<f32>, material: ResolvedMaterial) -> vec3<f32> {
    if material.lommel { return vec3(0.0); }
    let world_normal = view_facing_normal(shading_normal, wo);
    if dot(world_normal, wi) < 0.0001 { return vec3(0.0); }
    let H = normalize(wi + wo);
    let NdotL = dot(world_normal, wi);
    let NdotH = dot(world_normal, H);
    let LdotH = dot(wi, H);
    let NdotV = dot(world_normal, wo);
    if NdotL < 0.0001 || NdotH < 0.0001 || LdotH < 0.0001 || NdotV < 0.0001 { return vec3(0.0); }

    let F0 = calculate_F0(material.base_color, material.metallic, vec3(material.reflectance));
    let F = fresnel(F0, LdotH);

    if material.roughness <= MIRROR_ROUGHNESS_THRESHOLD {
        if abs(NdotH - 1.0) < 0.0001 {
            return F;
        } else {
            return vec3(0.0);
        }
    }

    let D = D_GGX(material.roughness, NdotH);
    let Vs = V_SmithGGXCorrelated(material.roughness, NdotV, NdotL);
    let F_ab = F_AB(material.perceptual_roughness, NdotV);
    let ggx = specular_multiscatter(D, Vs, F, F0, F_ab, 1.0) * NdotL;
    if material.gaussian_weight <= 0.0 { return ggx; }
    let gaussian = gaussian_specular(material.roughness, dielectric_ior(material.reflectance), NdotV, NdotL, NdotH, LdotH);
    return mix(ggx, vec3(gaussian), material.gaussian_weight);
}

fn fresnel(f0: vec3<f32>, LdotH: f32) -> vec3<f32> {
    return f0 + (fresnel_f90(f0) - f0) * pow(1.0 - LdotH, 5.0);
}

// Reflectance at grazing angles: 1 for any real interface (f0 of 2% and up),
// less below, as in Bevy's rasterizer and the guide buffers' albedo
// (Filament's specular occlusion): a surface given less reflectance than any
// real material, standing for one too rough and cavitied to reflect, otherwise
// still turned into a mirror edge-on (grass looking toward a low sun).
fn fresnel_f90(f0: vec3<f32>) -> f32 {
    return saturate(f0.g * 50.0);
}

// Share of the light arriving at `cosine` to the normal that the specular
// layer reflects: the directional albedo of the GGX lobe from the split-sum
// table (Karis 2013, "Real Shading in Unreal Engine 4"). A smooth interface
// reflects its Fresnel reflectance, which the table reproduces at zero
// roughness. A rough one reflects far less at grazing angles; using the smooth
// value there removed nine tenths of the diffuse light from every rough
// surface seen edge-on.
fn specular_albedo(F0: vec3<f32>, cosine: f32, perceptual_roughness: f32) -> vec3<f32> {
    let F_ab = F_AB(perceptual_roughness, cosine);
    return saturate(F0 * F_ab.x + fresnel_f90(F0) * F_ab.y);
}

// Scale/bias approximation
fn F_AB(perceptual_roughness: f32, NdotV: f32) -> vec2<f32> {
    return textureSampleLevel(brdf_dfg_lut, brdf_dfg_lut_sampler, vec2<f32>(NdotV, perceptual_roughness), 0.0).rg;
}

// Solid-angle PDF shared by continuation sampling and light-sample MIS.
fn evaluate_brdf_pdf(wo: vec3<f32>, wi: vec3<f32>, shading_normal: vec3<f32>, material: ResolvedMaterial) -> f32 {
    let world_normal = view_facing_normal(shading_normal, wo);
    let NdotV = dot(world_normal, wo);
    if NdotV < 0.0001 { return 0.0; }
    let NdotL = dot(world_normal, wi);
    let F0 = calculate_F0(material.base_color, material.metallic, vec3(material.reflectance));
    let diffuse_weight = select(mix(1.0 - luminance(specular_albedo(F0, NdotV, material.perceptual_roughness)), 0.0, material.metallic), 1.0, material.lommel);
    let side_weight = select(1.0 - material.diffuse_transmission, material.diffuse_transmission, NdotL < 0.0);
    let coat_weight = clearcoat_albedo(material, NdotV);
    let diffuse_pdf = diffuse_weight * side_weight * abs(NdotL) / PI;
    // GGX has support only on the reflection hemisphere.
    if NdotL <= 0.0 { return (1.0 - coat_weight) * diffuse_pdf; }
    let TBN = orthonormalize(world_normal);
    let wo_tangent = transpose(TBN) * wo;
    let wi_tangent = transpose(TBN) * wi;
    let base_pdf = diffuse_pdf + (1.0 - diffuse_weight) * ggx_vndf_pdf(wo_tangent, wi_tangent, material.roughness);
    if coat_weight <= 0.0 { return base_pdf; }
    return mix(base_pdf, ggx_vndf_pdf(wo_tangent, wi_tangent, clearcoat_roughness(material)), coat_weight);
}
