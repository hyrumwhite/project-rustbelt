//! A Halo level as Bevy entities: its structure BSPs and the objects its
//! scenario places.

use crate::{
    material::{
        HaloCombinerParams, HaloEnvironment, HaloEnvironmentMaterial, HaloEnvironmentParams, HaloTransparentMaterial,
    },
    textures::{LuminanceGrid, TextureCache},
};
use bevy_solarik::prelude::RaytracingMesh3d;
use bevy::{
    asset::RenderAssetUsages,
    ecs::system::EntityCommands,
    mesh::{
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
        Indices, PrimitiveTopology, VertexAttributeValues,
    },
    math::Affine3A,
    platform::collections::{HashMap, HashSet},
    prelude::*,
};
use halo_map::{
    bsp,
    math::{halo_to_bevy, METERS_PER_WORLD_UNIT},
    model::{self, Model},
    objects::{self, Placement},
    shader::{self, ShaderInfo, ShaderType},
    CacheFile, TagRef,
};
use crate::clusters::{ClusterVisibility, LightPlace};
use std::sync::Arc;

/// An object's model shaders whose self-illumination animates
/// (`shader::ModelSelfIlluminationAnimation`): each its own copy of the
/// shader's material, whose emissive [`animate_self_illumination`] sets
/// every frame, the object's phase, and the object's change colours (as the
/// game gives them, unstepped: a30's crashed lifepod's lights flicker by
/// them).
#[derive(Component, Clone)]
pub struct AnimatedSelfIllumination {
    pub materials: Vec<(Handle<StandardMaterial>, Option<Handle<HaloEnvironmentMaterial>>, shader::ModelSelfIlluminationAnimation)>,
    /// Each material's shader, which [`LevelBuilder::recolour`] colours in
    /// place.
    shaders: Vec<ShaderUse>,
    pub phase: f32,
    pub change_colors: [[f32; 3]; 4],
}

/// A moving object's mesh whose material its change colours can make anew
/// in place ([`LevelBuilder::recolour`]): the object's root, the piece's
/// uncoloured surface and shader, and whether this entity is the rays' (or
/// shares the rays' material) or the camera's alone.
#[derive(Component, Clone)]
pub struct Recolourable {
    pub root: Entity,
    base: Surface,
    shader: Option<ShaderUse>,
    traced: bool,
}

/// A model's shader and its permutation (which of its bitmaps' images it
/// draws with, see [`TextureCache::permuted`]).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShaderUse {
    datum: u32,
    permutation: u16,
}

/// The game's periodic functions, for the animations evaluated here.
#[derive(Resource)]
pub struct PeriodicTables(pub [[u8; 1024]; 12]);

impl Default for PeriodicTables {
    fn default() -> Self {
        PeriodicTables(halo_map::object_functions::periodic_function_tables())
    }
}

/// `periodic_function_evaluate`, with the game's tables (made once).
pub fn periodic_function(function: i16, time: f32) -> f32 {
    static TABLES: std::sync::OnceLock<[[u8; 1024]; 12]> = std::sync::OnceLock::new();
    let tables = TABLES.get_or_init(halo_map::object_functions::periodic_function_tables);
    let functions = halo_map::object_functions::FunctionTables {
        periodic: std::array::from_fn(|index| Some(&tables[index])),
        // (the transition functions aren't used here; any table will do)
        transition: std::array::from_fn(|_| Some(&tables[0])),
    };
    functions.periodic(function, time)
}

/// An object's function values (A-D: what its shaders' animations take as
/// sources) and change colours, as the game last shared them.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct ObjectFunctions {
    pub values: [f32; 4],
    pub colors: [[f32; 3]; 4],
    /// how camouflaged it is (0..1), which fades its transparent surfaces
    pub camouflage: f32,
}

/// Each animated transparent surface's maps and colours at this time
/// (`shader_texture_animation_evaluate`, the generic stages' colour
/// animation), with its object's functions where it has one. Written into
/// the combiner table's slot (one buffer rewritten a frame), not the
/// material (whose every change cost the asset pipeline a frame).
pub fn animate_transparent(
    time: Res<Time>,
    objects: Query<&ObjectFunctions>,
    mut materials: ResMut<Assets<HaloTransparentMaterial>>,
    mut table: ResMut<crate::combiners::CombinerTable>,
) {
    let seconds = if std::env::var_os("HALO_RT_FREEZE_ANIMATION").is_some() { 0.0 } else { time.elapsed_secs() };
    let animated: Vec<_> = materials
        .iter()
        .filter(|(_, m)| m.animation.is_some() || m.meter.is_some() || m.plasma.is_some())
        .map(|(id, _)| id)
        .collect();
    for id in animated {
        let Some(material) = materials.get(id) else { continue };
        let mut functions = material.object.and_then(|object| objects.get(object).ok());
        // (HALO_RT_PLASMA_DEBUG: shields' plasma at full strength, to see it)
        if material.plasma.is_some() && std::env::var_os("HALO_RT_PLASMA_DEBUG").is_some() {
            functions = None;
        }
        let Some(mut params) =
            material.animate(functions.map(|f| &f.values), functions.map(|f| &f.colors), seconds, &periodic_function)
        else {
            continue;
        };
        if let Some(functions) = functions {
            material.camouflage_fade(&mut params, functions.camouflage);
        }
        // (a shield's plasma unseen, as it is but when hit, and already so:
        // its moving noise left be, rather than rewriting the material every
        // frame for each shielded unit)
        let unseen = |p: &crate::material::HaloCombinerParams| p.constant0[0].w <= 0.0 && p.constant0[0].w + p.constant1[0].w <= 0.0;
        let slot = material.slot;
        if material.plasma.is_some() && unseen(&params) && table.params(slot).is_some_and(unseen) {
            continue;
        }
        // a numeric counter's digit, when it changes
        let digit = material
            .numeric
            .as_ref()
            .zip(functions)
            .and_then(|(numeric, functions)| numeric.frame(&functions.values))
            .filter(|frame| **frame != material.map0)
            .cloned();
        if let Some(frame) = digit
            && let Some(mut material) = materials.get_mut(id)
        {
            material.map0 = frame;
        }
        table.set(slot, &params);
    }
}

/// Each animated model shader's emissive: its colour between the bounds by
/// its periodic function (rasterizer_xbox_models.c), times the object's
/// change colour when it says, at the lighting's emissive scale.
pub fn animate_self_illumination(
    time: Res<Time>,
    tables: Res<PeriodicTables>,
    settings: Res<crate::LightingSettings>,
    objects: Query<&AnimatedSelfIllumination>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut raster_materials: ResMut<Assets<HaloEnvironmentMaterial>>,
) {
    let functions = halo_map::object_functions::FunctionTables {
        periodic: std::array::from_fn(|index| Some(&tables.0[index])),
        // (the transition functions aren't used here; any table will do)
        transition: std::array::from_fn(|_| Some(&tables.0[0])),
    };
    // (HALO_RT_FREEZE_ANIMATION, diagnostics: the glow as at the start)
    let seconds = if std::env::var_os("HALO_RT_FREEZE_ANIMATION").is_some() { 0.0 } else { time.elapsed_secs() };
    for object in &objects {
        for (material, raster, animation) in &object.materials {
            let phase = if animation.random_phase { object.phase } else { 0.0 };
            let mut fraction = functions.periodic(animation.function, seconds / animation.period + phase);
            // (in steps: each change is two materials through the asset
            // pipeline and the binder's rows, and 70 flickering objects
            // changed every frame in the library; a 32nd of the glow's
            // range is below what the eye picks from a pulsing light)
            let steps = self_illumination_steps();
            if steps > 0.0 {
                fraction = (fraction * steps).round() / steps;
            }
            let mut color = Vec3::from(animation.lower) + (Vec3::from(animation.upper) - Vec3::from(animation.lower)) * fraction;
            if let Some(source) = animation.color_source {
                color *= Vec3::from(object.change_colors[source]).clamp(Vec3::ZERO, Vec3::ONE);
            }
            let radiance = color * SELF_ILLUMINATION_RADIANCE * settings.emissive_scale;
            let emissive = LinearRgba::rgb(radiance.x, radiance.y, radiance.z);
            // (only when it changes: each change prepares the material
            // again, and a material changed every frame flickers)
            if materials.get(material).is_some_and(|m| m.emissive != emissive)
                && let Some(mut material) = materials.get_mut(material)
            {
                material.emissive = emissive;
            }
            if let Some(raster) = raster
                && raster_materials.get(raster).is_some_and(|m| m.base.emissive != emissive)
                && let Some(mut raster) = raster_materials.get_mut(raster)
            {
                raster.base.emissive = emissive;
            }
        }
    }
}

/// rasterizer_xbox_models.c's self-illumination phase: the first of the
/// object's random numbers, seeded by its object index.
/// The steps a self-illumination's glow moves in (`HALO_RT_SELF_ILLUMINATION_STEPS`,
/// default 32; 0 for every change of the game's 255).
fn self_illumination_steps() -> f32 {
    static STEPS: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *STEPS.get_or_init(|| {
        std::env::var("HALO_RT_SELF_ILLUMINATION_STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(32.0)
    })
}

pub fn self_illumination_phase(object_index: u32) -> f32 {
    let seed = object_index.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (seed >> 16) as f32 / 65535.0
}

/// A level material that emits light; its emissive strength follows
/// [`crate::LightingSettings::emissive_scale`].
#[derive(Component, Clone)]
pub struct EmissiveSurface {
    /// Radiosity colour × power from the Halo shader.
    pub radiance: Vec3,
}

/// An object placed from the scenario.
#[derive(Component)]
pub struct HaloObject;

/// The sky model; it keeps its centre on the camera (see [`follow_camera`]).
#[derive(Component)]
pub struct SkyDome;

/// A sky model's index in the scenario's skies: a level's BSPs' clusters
/// name the one drawn over them (a30's second BSP has its own).
#[derive(Component)]
pub struct SkyIndex(pub usize);

/// Metres per sky model unit. A sky is drawn around the camera at no
/// particular size, behind the level (the game draws it first); this puts it
/// far beyond any level. Its domes are ~10⁵ units across, but some of its
/// parts are much smaller (a50's fog band: a ring ~3200 units across), and
/// at a metre per unit, the level beyond them was hidden behind them.
const SKY_SCALE: f32 = 100.0;

fn sky_scale() -> f32 {
    std::env::var("HALO_RT_SKY_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(SKY_SCALE)
}
/// Transparent meshes sort back to front by their bounds' centres plus a
/// bias. Sky layers are drawn first (behind every transparent surface of the
/// level), in Halo's part order: each step is larger than the sky is (at
/// `SKY_SCALE`, its layers' centres up to ~2·10⁶ m from its origin: b30's
/// ring's is 1.6·10⁶ m up, and with steps of 10⁶ it sorted after the clouds
/// once the view pitched down, drawn over them). The bias is also the GPU's
/// constant depth bias, an i32: room for 20 layers. The panes' sky masks
/// (`material::GLASS_SKY_MASK_BIAS`) must sort below it.
const SKY_BIAS: f32 = -2.0e9;
const SKY_LAYER_BIAS: f32 = 1.0e8;

/// Light fixtures become point lights, a sphere this size (metres).
const LIGHT_PROXY_RADIUS: f32 = 0.12;
/// How far in front of its fixture (metres) a light sits, clear of the wall.
const LIGHT_PROXY_OFFSET: f32 = 0.3;
/// Radiance per unit of fixture intensity, relative to shader radiosity
/// power (a fixture's sphere is far smaller than a glowing panel).
const LIGHT_PROXY_RADIANCE: f32 = 4000.0;
/// How far a fixture's light reaches (metres): Halo's fixtures give none.
const FIXTURE_LIGHT_RANGE: f32 = 30.0;
/// A fixture's attached spot light (c10's floodlights, `ground_bright`):
/// a sphere this size (metres), as the game's spot lights (`crate::lights`).
const FLOODLIGHT_RADIUS: f32 = 0.15;
/// Its radiance per unit of the light's colour and radius (world units),
/// as the game's spot lights': `RADIANCE_PER_REACH2` × `SPOT_LIGHT_SCALE` ×
/// `SPOT_REFERENCE_REACH`.
const FLOODLIGHT_RADIANCE: f32 = 18000.0;
/// How far it reaches (metres): the game's lights reach their radius, which
/// lights only a fixture's own surroundings; a floodlight throws farther.
const FLOODLIGHT_RANGE: f32 = 15.0;
/// Fixtures whose light shines every way (in the game, the lightmaps' and a
/// lens flare) that get a big spot light at their light's marker: the
/// fixture, the cone's falloff and cutoff (degrees), brightness (times a
/// floodlight's), reach (metres), and the way it shines in the fixture's
/// own Halo space (its tag says none). The foglights (c10's two by the
/// facility's entrance, facing out over the hill and trees; b40's, on its
/// garage's walls, turned on their sides): along the model's -Z, its lens's
/// facing and its volumetric cone's (their own -X, aimed by eye on c10,
/// lit the wall beside b40's; play feedback: light cast to their left).
const LAMP_SPOTLIGHTS: [(&str, f32, f32, f32, f32, [f32; 3]); 1] =
    [("levels\\b40\\devices\\b40_foglight\\b40_foglight", 30.0, 55.0, 6.0, 35.0, [0.0, 0.0, -1.0])];
/// Fixtures whose spot lights are restyled (see [`FixtureSpotLook`]).
/// Truth and Reconciliation's ceiling lamps (a50's ship, 90 of them,
/// flanking every door) lit the wall a hand's breadth below them in a hard
/// bright oval beside each door (aimed straight down, still: the wall is
/// that near); dimmed, with their cones widened to fade out, it is a glow.
/// Their near-white is a soft pink, the ship's own (play feedback: teal
/// first, then pink asked for). Its four big ceiling lamps (three bulbs in
/// an oval, a volumetric cone under each) high over its large rooms lit
/// nothing to see: the floor lay at the floodlights' reach, 15 metres; a
/// strong spot light reaching it (play feedback).
const FIXTURE_SPOT_LOOKS: [(&str, FixtureSpotLook); 2] = [
    (
        "levels\\a50\\devices\\light_ceiling\\light_ceiling",
        FixtureSpotLook { brightness: 0.35, falloff: 0.0, cutoff: 65.0, color: Some([1.10, 0.57, 0.92]), range: None },
    ),
    (
        "levels\\a50\\devices\\light_ceiling_large\\light_ceiling_large",
        FixtureSpotLook { brightness: 6.0, falloff: 6.0, cutoff: 40.0, color: None, range: Some(40.0) },
    ),
];

/// A fixture's spot light restyled: its brightness (times its own), its
/// cone's falloff and cutoff (degrees), colour (`None` its own) and reach
/// (metres, `None` a floodlight's).
#[derive(Clone, Copy)]
struct FixtureSpotLook {
    brightness: f32,
    falloff: f32,
    cutoff: f32,
    color: Option<[f32; 3]>,
    range: Option<f32>,
}

/// A fixture's restyled spot light (see [`FIXTURE_SPOT_LOOKS`]).
/// (tuning: `HALO_RT_FIXTURE_LOOK=brightness,falloff,cutoff[,r,g,b]` for
/// the fixtures listed, keeping each one's reach)
fn fixture_spot_look(fixture: &str) -> Option<FixtureSpotLook> {
    let listed = FIXTURE_SPOT_LOOKS.iter().find(|(name, _)| fixture.eq_ignore_ascii_case(name))?.1;
    let tuned = std::env::var("HALO_RT_FIXTURE_LOOK").ok().and_then(|text| {
        let values: Vec<f32> = text.split(',').map(|v| v.trim().parse().ok()).collect::<Option<_>>()?;
        let look = |color| FixtureSpotLook { brightness: values[0], falloff: values[1], cutoff: values[2], color, range: listed.range };
        match values[..] {
            [_, _, _] => Some(look(None)),
            [_, _, _, r, g, b] => Some(look(Some([r, g, b]))),
            _ => None,
        }
    });
    Some(tuned.unwrap_or(listed))
}
/// Radiance of fully self-illuminated texels, relative to radiosity power
/// (with the default emissive scale and exposure, about display white).
const SELF_ILLUMINATION_RADIANCE: f32 = 2.5;

/// Glowing see-through surfaces light the scene (play feedback: a50's dark
/// ship; its holograms, energy doors and grav lift lit nothing): a
/// transparent shader added to the picture (additively, not a decal) is an
/// emitter the rays alone see, as a level's light cards are, its picture
/// drawn over it as before. Its radiance is its first map's mean colour (by
/// alpha, alpha-multiplied) as self-illumination's, times this: their maps
/// are mostly dark round their bright lines (a hologram's mean is 2%), and
/// at self-illumination's own they lit nothing to see. (Tuning:
/// `HALO_RT_GLOW_LIGHT=<share>`, 0 none.)
const GLOW_LIGHT: f32 = 8.0;
/// The share of a whole-surface emitter's light that one patterned by its
/// card casts (see [`LevelBuilder::weigh_emitter_maps`]).
const EMITTER_MAP_SHARE: f32 = 0.85;
/// Transparent radiosity emitters patterned by their cards (see
/// [`LevelBuilder::emitter_map`]), by shader tag: b40's direction markers,
/// pentagonal plates whose card is a lit outline, lit the plate and the
/// floor round it to white at their animation's peak (play feedback, 9
/// November). Every card patterned (`HALO_RT_EMITTER_MAPS=1`) is the look
/// liked in play, and in the harness still a little less steady than the
/// whole surfaces and 0.1 to 0.26 ms dearer (10 November, refined, the
/// invisible lights whole: c20's strip hall's stills 0.010 to 0.013, its
/// index hall's noise 0.024 to 0.039, the shaft's stills 0.010 to 0.012),
/// so the rule stays with the listed shaders.
const EMITTER_MAPPED: [&str; 1] = [r"levels\b40\shaders\b40_light_yellow"];
/// A light card whose brightest texel is under this (linear luminance)
/// shows nothing: an invisible light (Bungie's `_inv_` shaders, c10's
/// `1blue`), its card drawing nothing and its radiosity power lighting the
/// room, which emits whole as its shader says (see
/// [`LevelBuilder::emitter_map`]).
const EMITTER_MAP_BLACK: f32 = 0.02;
/// Glowing surfaces that light nothing (a shader path's start, or a word
/// in its name): what a weapon or effect holds is its own (its lights are
/// the game's), and the volumetric cones picture a fixture's light, which
/// the fixture already casts; b40's dome is a sky.
const GLOW_UNLIT: [&str; 5] = ["weapons\\", "effects\\", "sky\\", "vol", "levels\\b40\\shaders\\b40_domesky"];
/// The most a glowing surface's light reaches (radiance, before its tint):
/// an energy door's white map, whole, would light like the ship's own lamps
/// (theirs are near 100; a hologram's, mostly dark, under 1).
const GLOW_MOST: f32 = 8.0;

/// Light strips added to a level where it has none (play feedback): an
/// emitting rectangle, level with the world's floor, facing up, in a BSP
/// of a level (its scenario's name), its corners (world units), and its
/// radiance. a50's prison block, its cells all but black: a teal strip in
/// the trench under its corridor's grate, lighting the corridor through it.
/// (Tuning: `HALO_RT_ADDED_STRIPS=<gain>`, 0 none.)
const ADDED_LIGHT_STRIPS: [AddedLightStrip; 1] = [AddedLightStrip {
    map: "a50",
    bsp: 3,
    from: [-14.75, -116.85, -21.27],
    to: [-8.15, -116.31, -21.27],
    radiance: [24.0, 80.0, 72.0],
}];

/// See [`ADDED_LIGHT_STRIPS`].
struct AddedLightStrip {
    map: &'static str,
    bsp: usize,
    from: [f32; 3],
    to: [f32; 3],
    radiance: [f32; 3],
}

/// Emitters restyled (by shader tag): their light's gain, and its colour
/// (`None` its own; else this, at its own luminance). Truth and
/// Reconciliation's white light strips (a50's ship, d20's the same) a soft
/// pink, as its ceiling lamps (`FIXTURE_SPOT_LOOKS`; play feedback: the
/// ship's light all white, then teal, then pink); its grav lift's column, a
/// dim purple that lit nothing at night, brighter; a10's bridge console's
/// cyan strip under its holographic panel brighter, for the command
/// display's light left out (`LIGHT_DIMMING`; play feedback: glowing).
const EMITTER_LOOKS: [(&str, f32, Option<[f32; 3]>); 5] = [
    ("levels\\a50\\shaders\\covenant light bright white", 1.0, Some(SHIP_PINK)),
    ("levels\\a50\\shaders\\covenant light dim white", 1.0, Some(SHIP_PINK)),
    ("levels\\d20\\shaders\\covenant light 40 white", 1.0, Some(SHIP_PINK)),
    ("levels\\a50\\shaders\\grav lift energy", 12.0, None),
    ("levels\\a10\\shaders\\monitor glow", 8.0, None),
];
/// Light cards that light both ways, by shader tag: an emitter lights the
/// side its face is turned to (Solarik's, as Halo's lightmapper), and the
/// Library's door lights, a white card each side of its big doors and a
/// warm invisible one behind it, lit the far side alone, the near one
/// barely (play feedback, 4 November: lights flanking a door that cast no
/// light). Their backs are emitters too, for the rays alone. And the
/// volumetric cone under Assault on the Control Room's ceiling spotlights
/// (b40's, 92 of them, the fixture's model), a lamp's light pictured, whose
/// radiosity power lit the floor round its foot and left the foot itself,
/// which sees only its inside, a dark disc (play feedback, 8 November:
/// lights occluded by their base). (Tuning: `HALO_RT_EMITTERS_BOTH_WAYS=0`
/// leaves every back out.)
const EMITTERS_BOTH_WAYS: [&str; 2] = ["levels\\c20\\shaders\\c20 light bright white", "levels\\b40\\shaders\\b40_volumelight"];

/// Whether an unseen emitter (a transparent shader's radiosity emitter,
/// which the rays never see: `SolidSurface::unseen`) is a light-only
/// instance (`RaytracingMask::LIGHT_ONLY`, met by no ray) rather than an
/// alpha-masked surface every ray passes through, paying its any-hit test:
/// b40's 92 spotlight cones, each a room's width of masked triangles in the
/// middle of its corridor, cost the lighting passes 0.8 ms at the user's
/// lamp spot once their backs emitted too. The light is the same either
/// way: rays never met them. (Tuning: `HALO_RT_LIGHT_ONLY_EMITTERS=0`, the
/// masked surfaces.)
fn light_only_emitters() -> bool {
    std::env::var("HALO_RT_LIGHT_ONLY_EMITTERS").map_or(true, |v| v.trim() != "0")
}

/// Whether a shader's emitter lights both ways (see [`EMITTERS_BOTH_WAYS`]).
fn emits_both_ways(shader: &str) -> bool {
    EMITTERS_BOTH_WAYS.iter().any(|n| shader.eq_ignore_ascii_case(n))
        && std::env::var("HALO_RT_EMITTERS_BOTH_WAYS").map_or(true, |v| v.trim() != "0")
}
/// Whether a light card's emitter is refined to the texels that emit (see
/// [`refined_emitter`]). (Tuning: `HALO_RT_EMITTER_REFINE=0`, the BSP's own
/// triangles as before.)
fn emitter_refine() -> bool {
    std::env::var("HALO_RT_EMITTER_REFINE").map_or(true, |v| v.trim() != "0")
}

/// Whether a light card's emitter, by its shader's tag name, is refined:
/// all, unless `HALO_RT_EMITTER_REFINE_ONLY=<word>` (diagnostics) names a
/// part of the names to refine (any case), the others as with
/// `HALO_RT_EMITTER_REFINE=0`. Read once.
fn emitter_refine_only(shader: &str) -> bool {
    static ONLY: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let only = ONLY.get_or_init(|| std::env::var("HALO_RT_EMITTER_REFINE_ONLY").ok().map(|v| v.trim().to_lowercase()).filter(|v| !v.is_empty()));
    only.as_ref().is_none_or(|word| shader.to_lowercase().contains(word.as_str()))
}

/// [`EMITTER_DEEPEST`], or `HALO_RT_EMITTER_DEEPEST=<n>` (diagnostics: 0
/// splits nothing, leaving the drop alone to look at). Read once.
fn emitter_deepest() -> u32 {
    static DEEPEST: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *DEEPEST.get_or_init(|| std::env::var("HALO_RT_EMITTER_DEEPEST").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(EMITTER_DEEPEST))
}

/// [`EMITTER_DROP_SHARE`], or `HALO_RT_EMITTER_DROP_SHARE=<share>`
/// (diagnostics: 0 drops nothing). Read once.
fn emitter_drop_share() -> f64 {
    static SHARE: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *SHARE.get_or_init(|| std::env::var("HALO_RT_EMITTER_DROP_SHARE").ok().and_then(|v| v.trim().parse::<f32>().ok()).map_or(EMITTER_DROP_SHARE, f64::from))
}

/// A refined emitter's pieces per source triangle, aimed at (see
/// [`refined_emitter`]): its light shared out in this many even parts...
const EMITTER_PIECES_PER_TRIANGLE: usize = 16;
/// ... but at least this many a light...
const EMITTER_FEWEST_TARGET: usize = 32;
/// ... and at most this many.
const EMITTER_MOST_TARGET: usize = 1024;
/// The most times a piece is split in four (a source triangle's finest
/// pieces, a 1024th of it). (Diagnostics: `HALO_RT_EMITTER_DEEPEST=<n>`,
/// see [`emitter_deepest`].)
const EMITTER_DEEPEST: u32 = 5;
/// The share of a refined emitter's light its dropped dark pieces may hold.
/// (Diagnostics: `HALO_RT_EMITTER_DROP_SHARE=<share>`, see
/// [`emitter_drop_share`].)
const EMITTER_DROP_SHARE: f64 = 0.01;
/// A piece is dark where nothing under it reaches this share of its card's
/// brightest texel: one a lit line crosses is kept, whatever its mean.
const EMITTER_DARK: f32 = 0.02;
/// The most pieces one light keeps (its pieces coarsened until it fits;
/// Solarik's limit is 65535).
const EMITTER_MOST_PIECES: usize = 2048;
/// The most pieces a level's refined emitters keep in all: a quarter of
/// Solarik's emissive triangle table (`LIGHT_TRIANGLE_TABLE_CAPACITY`, 1 <<
/// 18), the rest for the other emitters; past it, an emitter keeps its own
/// triangles.
const EMITTER_PIECES_BUDGET: usize = 1 << 16;

/// Truth and Reconciliation's light, a soft pink of luminance one.
const SHIP_PINK: [f32; 3] = [1.55, 0.81, 1.29];

/// Emitters whose light (the rays' emission alone; the camera sees them as
/// before) is raised, by shader tag: Truth and Reconciliation's small blue
/// and green circuit lights, mostly dark texture round their dots, and the
/// long blue strips of its big rooms, self-illuminated at display white,
/// lit nothing to see (play feedback; the circuits down to 3 on 2 November,
/// the ship's rooms having their lamps now: many bright dots made the
/// first-person rifle's lighting jump between states); a10's monitors, its bridge's
/// navigation screens and the rest, for its bridge scenes (play feedback).
/// The Library's holographic panels (c20's own, amber, and b30's tall blue
/// ones stacked under them), mostly dark round their lines: they lit
/// nothing in its dark halls (play feedback, 4 November). (b30's, flush on
/// the wall, some turned to face it, lit it as bright as themselves at 12.)
/// (Tuning: `HALO_RT_SHIP_LIGHT=<gain>` for all of them, 1 as Halo's.)
const EMITTER_LIGHT_GAINS: [(&str, f32); 13] = [
    ("levels\\a50\\shaders\\covenant circuits", 3.0),
    ("levels\\a50\\shaders\\covenant circuits 0percent", 3.0),
    ("levels\\a50\\shaders\\covenant circuits 25percent", 3.0),
    ("levels\\a50\\shaders\\covenant circuits 50percent", 3.0),
    ("levels\\a50\\shaders\\cov circuits orng 25per", 3.0),
    ("levels\\a50\\shaders\\cov circuits purp 25per", 3.0),
    ("levels\\a50\\shaders\\covenant metal strips wide", 12.0),
    ("levels\\a10\\shaders\\monitors", 16.0),
    ("levels\\c20\\devices\\holo control\\shaders\\c20 holo control", 6.0),
    ("levels\\c20\\devices\\holo control\\shaders\\c20 holo control blink", 6.0),
    ("levels\\c20\\devices\\holo control\\shaders\\c20 holo control blink slow", 6.0),
    ("levels\\b30\\devices\\interior tech objects\\holo control\\shaders\\holo control", 3.0),
    ("levels\\c20\\shaders\\c20 light bright white", 4.0),
];

/// An emitter's light gain (see [`EMITTER_LIGHT_GAINS`]).
fn emitter_light_gain(shader: &str) -> f32 {
    let Some(&(_, gain)) = EMITTER_LIGHT_GAINS.iter().find(|(name, _)| shader.eq_ignore_ascii_case(name)) else { return 1.0 };
    std::env::var("HALO_RT_SHIP_LIGHT").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(gain)
}

/// An emitter's restyling (see [`EMITTER_LOOKS`]).
fn emitter_look(shader: &str) -> Option<(f32, Option<[f32; 3]>)> {
    EMITTER_LOOKS.iter().find(|(name, ..)| shader.eq_ignore_ascii_case(name)).map(|&(_, gain, tint)| (gain, tint))
}

/// What a loaded level contains, for the overlay.
#[derive(Resource, Default, Clone)]
pub struct LevelStats {
    pub map_name: String,
    pub bsps: usize,
    pub meshes: usize,
    pub triangles: usize,
    pub emissive_meshes: usize,
    pub objects: usize,
    pub object_triangles: usize,
    pub object_failures: usize,
    /// Light fixtures turned into emissive proxies.
    pub lights: usize,
    /// Transparent-shader geometry not drawn (glass, water, ...: the
    /// shader types the combiner programs do not cover yet).
    pub skipped_transparent: usize,
    /// Transparent geometry drawn with its combiner program.
    pub transparent_meshes: usize,
    pub texture_failures: usize,
}

/// The asset collections a level is built into.
pub struct LevelAssets<'a> {
    pub meshes: &'a mut Assets<Mesh>,
    pub materials: &'a mut Assets<StandardMaterial>,
    pub environment_materials: &'a mut Assets<HaloEnvironmentMaterial>,
    pub transparent_materials: &'a mut Assets<HaloTransparentMaterial>,
    /// A sky's water (where a level's sky is spawned).
    pub sky_water: Option<&'a mut Assets<crate::material::SkyWaterMaterial>>,
    /// The ray-traced water panes' ripples (where a level's BSPs are).
    pub water_ripples: Option<&'a mut bevy_solarik::water_ripples::WaterRippleMaterials>,
    /// The materials whose coat is Solarik's pearl film, for its rays.
    pub pearl_coats: Option<&'a mut bevy_solarik::lommel::PearlCoatMaterials>,
    /// The materials whose coat is plain (no thin film), for Solarik's rays.
    pub plain_coats: Option<&'a mut bevy_solarik::lommel::PlainCoatMaterials>,
    /// The glass panes whose texture is their grime layer, for Solarik's
    /// rays.
    pub glass_grime: Option<&'a mut bevy_solarik::glass_grime::GlassGrimeMaterials>,
    pub images: &'a mut Assets<Image>,
    pub inverse_bindposes: &'a mut Assets<SkinnedMeshInverseBindposes>,
    /// The game's default detail texture (see [`crate::material::NeutralDetail`]).
    pub neutral_detail: Handle<Image>,
    /// A flat normal map (see [`crate::material::flat_normal_image`]).
    pub flat_normal: Handle<Image>,
}

/// How a shader's surfaces are drawn.
#[derive(Clone)]
enum Surface {
    /// Ray traced and rasterized.
    Solid(SolidSurface),
    /// Rasterized only, blended over the scene (Solari traces opaque
    /// surfaces only).
    Transparent(Handle<HaloTransparentMaterial>),
    /// Glass and water: rasterized only, a blended `StandardMaterial`.
    Blended(Handle<StandardMaterial>),
}

#[derive(Clone)]
struct SolidSurface {
    /// What the ray tracer sees, and the camera too without `raster`.
    standard: Handle<StandardMaterial>,
    /// A Halo material for the camera where `StandardMaterial` falls short
    /// (see [`crate::material`]).
    raster: Option<Handle<HaloEnvironmentMaterial>>,
    /// Emitted light (see [`EmissiveSurface`]).
    radiance: Option<Vec3>,
    /// The rays' emission times this, where the camera has its own
    /// material (`raster` or `seen_standard`): light cast, not looks (see
    /// [`EMITTER_LIGHT_GAINS`]).
    light_gain: f32,
    /// The camera's copy of `standard`, its emission not raised, for an
    /// emitter with a light gain and no `raster`.
    seen_standard: Option<Handle<StandardMaterial>>,
    /// For the rays only: a transparent shader's radiosity emitter (c10's
    /// light cards, an invisible bitmap added), which lit the lightmaps but
    /// which the camera doesn't see (the game's overlay draws whatever it
    /// shows).
    unseen: bool,
    /// What the camera sees of such an emitter, now that the viewer draws
    /// the transparent surfaces (b30's control room hologram): its
    /// transparent material, drawn over the unseen emitter.
    seen_as: Option<Handle<HaloTransparentMaterial>>,
    /// The light card's bitmap that patterns this emitter's emission (see
    /// [`LevelBuilder::emitter_map`]), for the level to weigh its light by
    /// what the card shows over the meshes that carry it.
    emitter_map: Option<u32>,
}

/// A shader's material and the texture coordinate scale it applies.
#[derive(Clone)]
struct ShaderSurface {
    /// `None`: not drawn (a transparent shader the combiners cannot draw).
    surface: Option<Surface>,
    uv_scale: [f32; 2],
}

/// A model's meshes for one permutation choice, made once and instanced.
type ModelMeshes = Vec<(Handle<Mesh>, Surface, usize)>;

/// A model split by node for an animated object (see
/// [`LevelBuilder::animated_model`]).
pub struct AnimatedModel {
    pub name: String,
    /// The parts whose every triangle follows one node wholly, split by node
    /// and rigid, for the rays (Solari traces no skinned meshes); with
    /// `skinned`, the camera sees those instead.
    pieces: Vec<ModelPiece>,
    /// The parts that bend between nodes, for the rays: skinned here each
    /// time the nodes move, so that they keep to the surface the camera sees.
    pub traced_skins: Vec<TracedSkin>,
    skinned: Vec<SkinnedPart>,
    /// Identity, one per node: the nodes' transforms already include their
    /// default inverse.
    inverse_bindposes: Handle<SkinnedMeshInverseBindposes>,
    /// Per node, its default inverse matrix (Halo space).
    pub default_inverses: Vec<Affine3A>,
}

struct ModelPiece {
    node: usize,
    mesh: Handle<Mesh>,
    surface: Surface,
    /// Its shader, for change colours.
    shader: Option<ShaderUse>,
}

/// A whole model part as one mesh skinned to the model's nodes (each vertex
/// to its two, weighted), for the camera.
struct SkinnedPart {
    mesh: Handle<Mesh>,
    /// The same, carrying its rest position (the game's units and axes) as
    /// its colour, for a modifier shader's noise (a shield's plasma): built
    /// with the part, since a mesh's data leaves for the GPU once drawn.
    modifier_mesh: Handle<Mesh>,
    surface: Surface,
    shader: Option<ShaderUse>,
}

/// How far inside a skinned model its rigid ray-traced copy sits (world
/// units: 2 cm).
const TRACED_INSET: f32 = 0.02 / METERS_PER_WORLD_UNIT;

/// StandardMaterial's reflectance for a surface Halo gives no reflection (1%
/// head-on, not the usual 4%, and half at grazing angles, not all: Solarik
/// lowers grazing reflectance below 2%): Halo shades these diffuse only, and
/// the usual sheen of the sky on them washed grass out grey looking toward the
/// sun (a30's fields).
const MATTE_REFLECTANCE: f32 = 0.25;
/// A glass pane's reflectance at normal incidence at the least: the 0.6
/// reflectance every pane had before Halo's own reflection strength scaled it.
const PLAIN_GLASS_F0: f32 = 0.16 * 0.6 * 0.6;

/// How deep an actively camouflaged body's glass is (metres): how far it
/// bends the picture behind it.
const CAMOUFLAGE_THICKNESS: f32 = 0.6;
/// The first-person arms' and weapon's: none, a thin pane's glass that
/// reflects without bending what lies behind. At a limb's depth, a hand's
/// breadth from the camera, each flat face of the weapon bent the picture
/// behind it its own way, and the weapon broke into shards (play feedback:
/// as if its colliders showed); at 0.05 metres still slabs of the doorway
/// behind, out of place in its outline.
const FIRST_PERSON_CAMOUFLAGE_THICKNESS: f32 = 0.0;

/// Roughness where a reflective model shader's specular mask is zero: a
/// plastic's sheen, not the matte of surfaces Halo gives no reflection. That
/// is for the brightest reflections (armour, Covenant hulls); dimmer ones fade
/// toward matte by the square of their brightness (the crewmen's cloth,
/// 0.5: 0.79; the lifepod's vinyl, 0.025: matte), as does their reflectance.
const MODEL_UNMASKED_ROUGHNESS: f32 = 0.45;

/// Models duller than their shaders' reflections make them (by the shader
/// tag's path): the share of their gloss and metal they keep, the rest
/// toward [`DULL_ROUGHNESS`] (lit by the level's own lights, Chief's
/// armour and the vehicles shone like chrome).
const MODEL_SHINE: [(&str, f32); 9] = [
    ("characters\\cyborg\\", 0.3),
    // (the Hunters' armour, a dull metal: play feedback, near a mirror)
    ("characters\\hunter\\", 0.3),
    ("characters\\marine", 0.5),
    ("characters\\grunt\\", 0.8),
    ("vehicles\\scorpion\\", 0.15),
    ("vehicles\\warthog\\", 0.25),
    ("vehicles\\pelican\\", 0.35),
    ("scenery\\rocks\\", 0.6),
    ("scenery\\cutscene_small_rock\\", 0.6),
];

/// Models whose reflection is paint's sheen, not bare metal (by the shader
/// tag's path): the share of their metalness they keep. Halo adds a model's
/// reflection over its full colour; as metal it greyed the Warthog's olive.
/// More than one keeps a dulled model metal: the Hunters' armour, which
/// [`MODEL_SHINE`] dulls, metalness and all, to 0.3 of Halo's (its metalness
/// 0.9 of what its mask says, as before; rough, 0.44).
const MODEL_METAL: [(&str, f32); 3] = [("vehicles\\warthog\\", 0.1), ("vehicles\\pelican\\", 0.2), ("characters\\hunter\\", 3.3)];

/// The roughness a model's gloss fades toward as [`MODEL_SHINE`] dulls it.
const DULL_ROUGHNESS: f32 = 0.6;

/// Chief's visor's shader, drawn as a smooth metal of this colour (Halo's
/// visor reflects gold) instead of by its maps.
const VISOR_SHADER: &str = "characters\\cyborg\\shaders\\visor";
const VISOR_COLOR: Color = Color::srgb(0.8, 0.55, 0.2);
const VISOR_ROUGHNESS: f32 = 0.12;

/// How far inside the camera's skinned surface the rays' skinned copy sits
/// (meters): the two are skinned alike, but on different processors.
const TRACED_SKIN_INSET: f32 = 0.005;

/// A model part that bends between nodes, as the rays see it: its vertices
/// at rest (Bevy space) with their two nodes and weights, skinned by
/// [`TracedSkin::mesh`] to the nodes' current transforms.
pub struct TracedSkin {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    tangents: Vec<Vec4>,
    uvs: Vec<[f32; 2]>,
    joints: Vec<[u16; 2]>,
    weights: Vec<[f32; 2]>,
    /// Counter-clockwise (Bevy's front faces).
    indices: Vec<u32>,
    surface: Surface,
    shader: Option<ShaderUse>,
    /// The camera's skinned mesh of the same part (its rest pose, joints and
    /// weights), which the GPU skinning reads (`crate::rt_skinning`).
    raster_mesh: Option<Handle<Mesh>>,
}

impl TracedSkin {
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// The camera's skinned mesh this is the rays' copy of.
    pub fn raster_mesh(&self) -> Option<&Handle<Mesh>> {
        self.raster_mesh.as_ref()
    }

    /// The part skinned to the nodes' transforms (Bevy space, each node's
    /// default inverse included), as the camera's skinned mesh is: each
    /// vertex blended between its two nodes by their weights.
    pub fn mesh(&self, nodes: &[Mat4]) -> Mesh {
        let node = |nodes: &[Mat4], index: u16| nodes.get(index as usize).copied().unwrap_or(Mat4::IDENTITY);
        let count = self.positions.len();
        let (mut positions, mut normals, mut tangents) =
            (Vec::with_capacity(count), Vec::with_capacity(count), Vec::with_capacity(count));
        for i in 0..count {
            let ([a, b], [wa, wb]) = (self.joints[i], self.weights[i]);
            let (ma, mb) = (node(nodes, a), node(nodes, b));
            let (p, n, t) = (self.positions[i], self.normals[i], self.tangents[i]);
            let normal = (wa * ma.transform_vector3(n) + wb * mb.transform_vector3(n)).normalize_or_zero();
            let skinned = wa * ma.transform_point3(p) + wb * mb.transform_point3(p);
            let position = skinned - normal * TRACED_SKIN_INSET;
            let tangent =
                (wa * ma.transform_vector3(t.truncate()) + wb * mb.transform_vector3(t.truncate())).normalize_or_zero();
            positions.push(position.to_array());
            normals.push(normal.to_array());
            tangents.push(tangent.extend(t.w).to_array());
        }
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tangents)
            .with_inserted_indices(Indices::U32(self.indices.clone()))
    }
}

/// What a mesh entity is for.
enum MeshUse {
    /// The camera and the rays.
    Both,
    /// The rays only.
    Traced,
    /// The camera only, skinned to these joints if given.
    Raster(Option<SkinnedMesh>),
}

/// A Halo `real_matrix4x3` as an affine transform (Halo space).
pub fn halo_affine(m: &halo_map::model::Matrix4x3) -> Affine3A {
    Affine3A::from_cols(
        Vec3A::from(m.forward) * m.scale,
        Vec3A::from(m.left) * m.scale,
        Vec3A::from(m.up) * m.scale,
        Vec3A::from(m.position),
    )
}

/// Halo space (Z up, world units) to Bevy's (Y up, metres) as an affine map.
pub fn halo_to_bevy_affine() -> Affine3A {
    Affine3A::from_mat3(Mat3::from_cols(Vec3::X, Vec3::NEG_Z, Vec3::Y) * METERS_PER_WORLD_UNIT)
}

pub struct LevelBuilder {
    pub cache: Arc<CacheFile>,
    /// While a moving object's piece is spawned: its root, uncoloured
    /// surface and shader, put on its entities as [`Recolourable`].
    recolour: Option<(Entity, Surface, Option<ShaderUse>)>,
    pub textures: TextureCache,
    /// By shader and permutation.
    surfaces: HashMap<(Option<u32>, u16), ShaderSurface>,
    /// While a surface is made: its shader's permutation.
    permutation: u16,
    models: HashMap<(u32, [u8; 8]), Option<ModelMeshes>>,
    animated_models: HashMap<(u32, [u8; 8]), Option<Arc<AnimatedModel>>>,
    /// Its liquid glass, in the rays' scene.
    camouflage_glass: [Option<Handle<StandardMaterial>>; 2],
    /// Surfaces tinted by a change colour, by shader and colour.
    coloured_surfaces: HashMap<(ShaderUse, [u8; 3]), Surface>,
    loaded_bsps: Vec<usize>,
    /// Each loaded BSP's root entity, by BSP index.
    pub bsp_roots: Vec<(usize, Entity)>,
    /// The surfaces of each BSP's breakable surfaces, under an entity each
    /// (BSP index, breakable surface index): hidden once the game breaks it
    /// (crate::bridge).
    pub breakable_roots: Vec<(usize, i16, Entity)>,
    /// Each loaded BSP's water surfaces' heights (world units, their
    /// vertices' mean): the fog pass's water is only a fog plane on one.
    pub water_heights: Vec<f32>,
    /// Each loaded BSP's clusters and visibility, by BSP index (None where
    /// unread): the emitters' places (crate::clusters).
    pub clusters: Vec<Option<Arc<ClusterVisibility>>>,
    light_proxy: Option<Handle<Mesh>>,
    /// Whether light fixtures get emissive proxies.
    pub fixture_lights: bool,
    /// Rasterize the light proxies too (to see where they are).
    pub show_light_proxies: bool,
    /// Ignore Halo's reflection settings: every surface a rough dielectric.
    pub matte: bool,
    /// Leave out transparent surfaces (the game's overlay draws them)...
    pub skip_transparent: bool,
    /// ... but glass and water (thin panes Solarik ray traces), when the
    /// game leaves those out of its overlay.
    pub draw_blended: bool,
    /// The parent of everything this level spawns, so that despawning it
    /// unloads the level (when the bridged game changes level).
    pub root: Option<Entity>,
    /// Each glass or water pane's sky mask (see
    /// [`crate::material::GlassSkyMask`]).
    pub glass_sky_mask: Option<crate::material::GlassSkyMaskMaterial>,
    /// Whether the models spawned are the first-person weapon and hands
    /// (their panes' masks at their own depth).
    pub first_person: bool,
    /// The next model spawned's object's modifier shader (a shield's
    /// plasma), drawn over it.
    pub modifier_shader: Option<TagRef>,
    /// Whether the next model spawned's object's transparent parts occlude
    /// one another (`halo_map::objects::transparency_self_occludes`): each
    /// gets a depth-only copy drawn just before it.
    pub self_occluding: bool,
    /// Transparent materials left out of an object's effect (their shaders'
    /// IGNORE_EFFECT): no depth for a self-occluding object.
    ignores_effect: HashSet<AssetId<HaloTransparentMaterial>>,
    pub stats: LevelStats,
}

/// One vertex in Halo's convention, as BSPs and models both provide.
#[derive(Clone, Copy)]
struct SourceVertex {
    position: [f32; 3],
    normal: [f32; 3],
    binormal: [f32; 3],
    tangent: [f32; 3],
    texcoord: [f32; 2],
}

impl LevelBuilder {
    pub fn new(cache: Arc<CacheFile>, bc_supported: bool) -> Self {
        LevelBuilder {
            recolour: None,
            glass_sky_mask: None,
            first_person: false,
            modifier_shader: None,
            self_occluding: false,
            ignores_effect: HashSet::default(),
            textures: TextureCache::new(bc_supported),
            surfaces: HashMap::default(),
            permutation: 0,
            models: HashMap::default(),
            animated_models: HashMap::default(),
            coloured_surfaces: HashMap::default(),
            camouflage_glass: [None, None],
            loaded_bsps: Vec::new(),
            bsp_roots: Vec::new(),
            breakable_roots: Vec::new(),
            water_heights: Vec::new(),
            clusters: Vec::new(),
            light_proxy: None,
            fixture_lights: true,
            show_light_proxies: false,
            matte: false,
            skip_transparent: false,
            draw_blended: true,
            root: None,
            stats: LevelStats { map_name: cache.header.name.clone(), ..default() },
            cache,
        }
    }

    /// Puts a top-level entity of this level under [`Self::root`].
    pub fn adopt(&self, commands: &mut Commands, entity: Entity) {
        if let Some(root) = self.root {
            commands.entity(entity).insert(ChildOf(root));
        }
    }

    /// Spawns the meshes of the given BSPs (all when `only` is `None`).
    pub fn spawn_bsps(
        &mut self,
        only: Option<usize>,
        raster: bool,
        commands: &mut Commands,
        assets: &mut LevelAssets,
    ) -> halo_map::Result<()> {
        // (HALO_RT_NO_CLUSTER_LIGHTS=1 for A/B: each emissive material one light)
        let no_cluster_lights = std::env::var_os("HALO_RT_NO_CLUSTER_LIGHTS").is_some();
        // (the emitters patterned by their cards, weighed by what the cards
        // show over the level's meshes)
        self.weigh_emitter_maps(assets)?;
        // (the pieces the level's refined emitters keep: see refined_emitter)
        let mut refine_budget = EMITTER_PIECES_BUDGET;
        // (the box NRC's position encoding spans: the loaded BSPs' bounds)
        #[cfg(feature = "nrc")]
        let mut nrc_bounds: Option<(Vec3, Vec3)> = None;
        for reference in bsp::references(&self.cache)? {
            if only.is_some_and(|i| i != reference.index) {
                continue;
            }
            let loaded = bsp::Bsp::load(&self.cache, &reference)?;
            #[cfg(feature = "nrc")]
            if let Ok([x, y, z]) = loaded.world_bounds() {
                // (the axes change: min and max per component after the conversion)
                let a = Vec3::from(halo_to_bevy([x[0], y[0], z[0]], METERS_PER_WORLD_UNIT));
                let b = Vec3::from(halo_to_bevy([x[1], y[1], z[1]], METERS_PER_WORLD_UNIT));
                let (min, max) = (a.min(b), a.max(b));
                nrc_bounds = Some(nrc_bounds.map_or((min, max), |(lo, hi)| (lo.min(min), hi.max(max))));
            }
            if self.clusters.len() <= reference.index {
                self.clusters.resize(reference.index + 1, None);
            }
            if self.clusters[reference.index].is_none() {
                self.clusters[reference.index] = ClusterVisibility::load(&self.cache, reference.index as i32).map(Arc::new);
            }
            let level = commands
                .spawn((Name::new(reference.name.clone()), Transform::default(), Visibility::default()))
                .id();
            self.adopt(commands, level);
            for (material_index, material) in loaded.materials()?.into_iter().enumerate() {
                if material.indices.is_empty() {
                    continue;
                }
                let shader = self.surface(material.shader, 0, assets);
                let Some(surface) = shader.surface else {
                    self.stats.skipped_transparent += 1;
                    continue;
                };
                if material.shader.group == halo_map::fourcc(b"swat") && !material.vertices.is_empty() {
                    let height = material.vertices.iter().map(|v| v.position[2]).sum::<f32>() / material.vertices.len() as f32;
                    self.water_heights.push(height);
                }
                let vertices = material.vertices.iter().map(|v| SourceVertex {
                    position: v.position,
                    normal: v.normal,
                    binormal: v.binormal,
                    tangent: v.tangent,
                    texcoord: v.texcoord,
                });
                let mesh = assets.meshes.add(build_mesh(vertices, &material.indices, shader.uv_scale));
                let name = format!("{} material {material_index}", reference.name);
                // (a breakable surface's, its own parent, to hide when broken)
                let parent = match material.breakable_surface_index {
                    index if index >= 0 => match self.breakable_roots.iter().find(|r| r.0 == reference.index && r.1 == index) {
                        Some(&(_, _, root)) => root,
                        None => {
                            let root = commands
                                .spawn((Name::new(format!("breakable surface {index}")), Transform::default(), Visibility::default(), ChildOf(level)))
                                .id();
                            self.breakable_roots.push((reference.index, index, root));
                            root
                        }
                    },
                    _ => level,
                };
                // (an emitter in chunks, one a cluster, each a light of its
                // own place for the rays: a lightmap page's material was one
                // light, its triangles drawn uniformly wherever they were,
                // so a cell's slot that found a strip's material still drew
                // the strips on other floors; the camera's copy stays whole)
                let chunks = match &surface {
                    Surface::Solid(solid) if solid.radiance.is_some() && !no_cluster_lights => self.emissive_chunks(reference.index, &material),
                    _ => Vec::new(),
                };
                // (a light card's emitter, which no ray meets, refined to
                // the texels that emit: see refined_emitter)
                let card = match &surface {
                    Surface::Solid(solid)
                        if solid.unseen
                            && solid.radiance.is_some()
                            && emitter_refine()
                            && emitter_refine_only(&material.shader.datum.and_then(|d| self.cache.tag(d).ok()).map_or_else(String::new, |t| t.name.clone())) =>
                    {
                        solid.emitter_map.and_then(|datum| self.textures.luminance(&self.cache, datum, 0))
                    }
                    _ => None,
                };
                let sources: Vec<SourceVertex> = if card.is_some() {
                    material
                        .vertices
                        .iter()
                        .map(|v| SourceVertex { position: v.position, normal: v.normal, binormal: v.binormal, tangent: v.tangent, texcoord: v.texcoord })
                        .collect()
                } else {
                    Vec::new()
                };
                let mut tally = RefineTally::default();
                let mut refine = |indices: &[u32], sources: &[SourceVertex], tally: &mut RefineTally| -> Option<Mesh> {
                    let card = card.as_deref()?;
                    let (vertices, indices) = refined_emitter(sources, indices, card, shader.uv_scale, &mut refine_budget, tally)?;
                    Some(build_mesh(vertices.into_iter(), &indices, shader.uv_scale))
                };
                if chunks.len() > 1 {
                    if raster {
                        self.spawn_mesh_as(commands, parent, name.clone(), mesh, &surface, MeshUse::Raster(None));
                    }
                    for (cluster, indices) in chunks {
                        let chunk = match refine(&indices, &sources, &mut tally) {
                            Some(refined) => assets.meshes.add(refined),
                            None => {
                                let vertices = material.vertices.iter().map(|v| SourceVertex {
                                    position: v.position,
                                    normal: v.normal,
                                    binormal: v.binormal,
                                    tangent: v.tangent,
                                    texcoord: v.texcoord,
                                });
                                assets.meshes.add(build_mesh(vertices, &indices, shader.uv_scale))
                            }
                        };
                        if let Some(traced) = self.spawn_mesh_as(commands, parent, format!("{name} cluster {cluster}"), chunk, &surface, MeshUse::Traced) {
                            commands.entity(traced).insert(LightPlace { bsp: reference.index, cluster });
                        }
                    }
                } else if let Some(refined) = refine(&material.indices, &sources, &mut tally) {
                    // (the camera's card whole, the rays' emitter refined)
                    if raster {
                        self.spawn_mesh_as(commands, parent, name.clone(), mesh, &surface, MeshUse::Raster(None));
                    }
                    let traced = self.spawn_mesh_as(commands, parent, name.clone(), assets.meshes.add(refined), &surface, MeshUse::Traced);
                    if let (Some(traced), [(cluster, _)]) = (traced, chunks.as_slice()) {
                        commands.entity(traced).insert(LightPlace { bsp: reference.index, cluster: *cluster });
                    }
                } else {
                    let mode = if raster { MeshUse::Both } else { MeshUse::Traced };
                    let traced = self.spawn_mesh_as(commands, parent, name.clone(), mesh, &surface, mode);
                    if let (Some(traced), [(cluster, _)]) = (traced, chunks.as_slice()) {
                        commands.entity(traced).insert(LightPlace { bsp: reference.index, cluster: *cluster });
                    }
                }
                self.stats.triangles += material.indices.len() / 3;
                // (and its back, for an emitter that lights both ways: see
                // EMITTERS_BOTH_WAYS)
                if let Surface::Solid(solid) = &surface
                    && solid.radiance.is_some()
                    && let Some(datum) = material.shader.datum
                    && self.cache.tag(datum).is_ok_and(|tag| emits_both_ways(&tag.name))
                {
                    let back = material.vertices.iter().map(|v| SourceVertex {
                        position: v.position,
                        normal: v.normal.map(|n| -n),
                        binormal: v.binormal,
                        tangent: v.tangent,
                        texcoord: v.texcoord,
                    });
                    let indices: Vec<_> = material.indices.chunks_exact(3).flat_map(|t| [t[0], t[2], t[1]]).collect();
                    let backs: Vec<SourceVertex> = sources.iter().map(|v| SourceVertex { normal: v.normal.map(|n| -n), ..*v }).collect();
                    let mesh = match refine(&indices, &backs, &mut tally) {
                        Some(refined) => assets.meshes.add(refined),
                        None => assets.meshes.add(build_mesh(back, &indices, shader.uv_scale)),
                    };
                    self.spawn_mesh(commands, parent, format!("{name} back"), mesh, &surface, false);
                }
                // (HALO_RT_GLOW_LOG: each refined emitter, in the log)
                if card.is_some() && std::env::var_os("HALO_RT_GLOW_LOG").is_some() {
                    let shader_name = material.shader.datum.and_then(|d| self.cache.tag(d).ok()).map_or_else(String::new, |t| t.name.clone());
                    let share = if tally.energy > 0.0 { tally.dropped_energy / tally.energy * 100.0 } else { 0.0 };
                    info!(
                        "emitter refined {shader_name} ({name}): {} triangles to {} kept (aimed at {} in {} lights), {} dropped ({share:.3}% of its light); unrefined: {} no light, {} over the cap, {} budget spent, {} nothing to refine",
                        tally.source, tally.kept, tally.target, tally.refined, tally.dropped, tally.no_light, tally.over_cap, tally.over_budget, tally.nothing
                    );
                    if let Some((peak, mean)) = tally.dark_card {
                        info!("emitter refined {shader_name} ({name}): no light under its card, its peak {peak:.4}, its mean over the mesh {mean:.6}");
                    }
                    for note in &tally.notes {
                        info!("emitter refined {shader_name} ({name}): {note}");
                    }
                }
            }
            // (and the light strips added to it: see ADDED_LIGHT_STRIPS)
            let gain = std::env::var("HALO_RT_ADDED_STRIPS").ok().and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(1.0);
            let map_name = self.stats.map_name.clone();
            for strip in ADDED_LIGHT_STRIPS.iter().filter(|s| s.bsp == reference.index && map_name.eq_ignore_ascii_case(s.map)) {
                if gain <= 0.0 {
                    break;
                }
                let [x0, y0, z] = strip.from;
                let [x1, y1, _] = strip.to;
                let corner = |x: f32, y: f32| SourceVertex {
                    position: [x, y, z],
                    normal: [0.0, 0.0, 1.0],
                    binormal: [0.0, 1.0, 0.0],
                    tangent: [1.0, 0.0, 0.0],
                    texcoord: [0.0, 0.0],
                };
                let vertices = [corner(x0, y0), corner(x1, y0), corner(x1, y1), corner(x0, y1)];
                let mesh = assets.meshes.add(build_model_mesh(vertices.into_iter(), &[0, 1, 2, 0, 2, 3], [1.0, 1.0]));
                let standard = assets.materials.add(StandardMaterial { base_color: Color::BLACK, ..default() });
                let radiance = Some(Vec3::from(strip.radiance) * gain);
                let surface = Surface::Solid(SolidSurface {
                    standard,
                    raster: None,
                    radiance,
                    light_gain: 1.0,
                    seen_standard: None,
                    unseen: false,
                    seen_as: None,
                    emitter_map: None,
                });
                self.spawn_mesh(commands, level, format!("{} added light strip", reference.name), mesh, &surface, raster);
            }
            self.loaded_bsps.push(reference.index);
            self.bsp_roots.push((reference.index, level));
            self.stats.bsps += 1;
        }
        if refine_budget < EMITTER_PIECES_BUDGET && std::env::var_os("HALO_RT_GLOW_LOG").is_some() {
            info!("emitters refined: {} triangles of their budget of {EMITTER_PIECES_BUDGET}", EMITTER_PIECES_BUDGET - refine_budget);
        }
        // Plus a margin for the objects and the sky's water outside the
        // BSPs; a new level's bounds reset the cache.
        #[cfg(feature = "nrc")]
        if let Some((min, max)) = nrc_bounds {
            const MARGIN: f32 = 50.0;
            commands.insert_resource(bevy_solarik::SolarikNrcSceneBounds {
                min: min - Vec3::splat(MARGIN),
                max: max + Vec3::splat(MARGIN),
            });
        }
        self.stats.texture_failures = self.textures.failures;
        Ok(())
    }

    /// Spawns the objects the game would create in the loaded BSPs; with
    /// `lights_only`, only the light fixtures' lights (when a running game
    /// places the objects, see [`crate::bridge`]).
    pub fn spawn_objects(
        &mut self,
        raster: bool,
        lights_only: bool,
        commands: &mut Commands,
        assets: &mut LevelAssets,
    ) -> halo_map::Result<()> {
        let mut placements = objects::placements(&self.cache)?;
        // `HALO_RT_PLACE=<biped, vehicle or weapon tag>@x,y,z,yaw[;...]`: more
        // there (Halo world units, yaw in degrees), to look at their materials
        // where the light suits
        let specs = std::env::var("HALO_RT_PLACE").unwrap_or_default();
        for spec in specs.split(';').filter(|s| !s.is_empty()) {
            let parsed = spec.split_once('@').and_then(|(name, at)| {
                let v: Vec<f32> = at.split(',').filter_map(|v| v.trim().parse().ok()).collect();
                let weapon = halo_map::fourcc(b"weap");
                let vehicle = halo_map::fourcc(b"vehi");
                let (group, tag) = [halo_map::groups::BIPED, vehicle, weapon]
                    .into_iter()
                    .find_map(|group| self.cache.find_tag(group, name).map(|tag| (group, tag)))?;
                let kind = match group {
                    g if g == vehicle => objects::ObjectKind::Vehicle,
                    g if g == weapon => objects::ObjectKind::Weapon,
                    _ => objects::ObjectKind::Biped,
                };
                let like = placements.iter().find(|p| p.kind == objects::ObjectKind::Biped).or(placements.first())?;
                (v.len() >= 3).then(|| objects::Placement {
                    kind,
                    definition: halo_map::TagRef { group, datum: Some(tag.datum) },
                    position: [v[0], v[1], v[2]],
                    rotation: [v.get(3).copied().unwrap_or(0.0).to_radians(), 0.0, 0.0],
                    on_bsp_flags: 0,
                    not_automatic: false,
                    region_permutations: [0; 8],
                    ..like.clone()
                })
            });
            match parsed {
                Some(placement) => placements.push(placement),
                None => warn!("HALO_RT_PLACE {spec}: no such biped or weapon (or nothing to copy) in this map"),
            }
        }
        let objects_root = commands.spawn((Name::new("objects"), Transform::default(), Visibility::default())).id();
        self.adopt(commands, objects_root);
        // `HALO_RT_LIST_OBJECTS=<text>`: log the placements whose tag names
        // hold it (finding objects to look at)
        if let Ok(text) = std::env::var("HALO_RT_LIST_OBJECTS") {
            for placement in &placements {
                let name = self.cache.tag(placement.definition.datum.unwrap())?.name.clone();
                if name.contains(text.as_str()) {
                    info!("object {name} at {:?} bsps {:#x} scripted {}", placement.position, placement.on_bsp_flags, placement.not_automatic);
                }
            }
        }
        for placement in &placements {
            // (multiplayer vehicles have no BSP flags: the game type spawns them)
            let in_loaded_bsp =
                placement.on_bsp_flags == 0 || self.loaded_bsps.iter().any(|&b| placement.in_bsp(b));
            if placement.not_automatic || !in_loaded_bsp {
                continue;
            }
            if lights_only && placement.light.is_none() {
                continue;
            }
            // (light fixtures often have no model, only their light)
            let model = if lights_only {
                None
            } else {
                objects::object_model(&self.cache, placement.definition)?.and_then(|m| m.datum)
            };
            let parts = match model {
                Some(model) => self.model_meshes(model, placement.region_permutations, assets),
                None => None,
            };
            if parts.is_none() && placement.light.is_none() {
                continue;
            }
            let name = self.cache.tag(placement.definition.datum.unwrap())?.name.clone();
            let object = commands
                .spawn((
                    Name::new(name.clone()),
                    HaloObject,
                    placement_transform(placement),
                    Visibility::default(),
                    ChildOf(objects_root),
                ))
                .id();
            if placement.on_bsp_flags != 0 {
                commands.entity(object).insert(OnBsps(placement.on_bsp_flags as u32));
            }
            for (mesh, surface, triangles) in parts.into_iter().flatten() {
                self.spawn_mesh(commands, object, name.clone(), mesh, &surface, raster);
                self.stats.object_triangles += triangles;
            }
            if let Some(light) = placement.light
                && light.intensity > 0.0
                && self.fixture_lights
            {
                self.spawn_fixture_light(commands, object, Vec3::from(light.color) * light.intensity, assets);
            } else if placement.kind == objects::ObjectKind::LightFixture && self.fixture_lights {
                self.spawn_fixture_spotlights(commands, object, placement);
            }
            self.stats.objects += 1;
        }
        self.stats.texture_failures = self.textures.failures;
        Ok(())
    }

    /// Spawns a sky model as raster-only, unlit, alpha-blended layers in the
    /// model's part order. Solari never sees it: the sky's light comes from
    /// the environment map and the sun.
    ///
    /// The layers are drawn in Bevy's transmissive phase (their materials
    /// say they read the transmission texture; the camera's
    /// `ScreenSpaceTransmission` has no steps, so nothing is copied),
    /// between the opaque and transparent passes, before Solarik's lighting:
    /// Solarik composites the camera's glass and water panes over what is
    /// behind them then. Drawn in the transparent pass, the layers came too
    /// late, and the panes had only the skybox behind them (a30's opening,
    /// from the dropship: the sky's horizon land black through the windows,
    /// where the skybox, seen from the sky's origin, has nothing).
    pub fn spawn_sky(
        &mut self,
        model_datum: u32,
        sky_index: usize,
        visibility: Visibility,
        commands: &mut Commands,
        assets: &mut LevelAssets,
    ) -> halo_map::Result<()> {
        use bevy::light::NotShadowCaster;
        let cache = self.cache.clone();
        let model = Model::load(&cache, model_datum)?;
        let dome = commands
            .spawn((Name::new(model.name.clone()), SkyDome, SkyIndex(sky_index), Transform::default(), visibility))
            .id();
        self.adopt(commands, dome);
        let mut layer = 0;
        for geometry in model.geometries_for(&[], model::HIGHEST_DETAIL) {
            for part in model.geometry(geometry)? {
                let Some(shader_ref) = model.shaders.get(part.shader_index.max(0) as usize).copied() else { continue };
                let Some(info) = shader::shader_info(&self.cache, shader_ref)? else { continue };
                let depth_bias = SKY_BIAS + layer as f32 * SKY_LAYER_BIAS;
                // (a water shader: its reflection cube map, see SkyWaterMaterial)
                if let Some(sky_water) = assets.sky_water.as_deref_mut()
                    && let Some(reflection) = info
                        .water
                        .as_ref()
                        .and_then(|w| w.reflection_map)
                        .and_then(|r| r.datum)
                        .and_then(|datum| crate::material::cube_map_image(&self.cache, datum))
                {
                    let water = info.water.as_ref().expect("a water shader");
                    // (each ripple's bump map, from the ripple maps by its index)
                    let mut ripple = |index: usize| {
                        let map = water.ripples[index].map_index.max(0) as usize;
                        water
                            .ripple_maps
                            .and_then(|r| r.datum)
                            .filter(|_| index < water.ripple_count)
                            .and_then(|datum| self.textures.get(&self.cache, datum, map, false, assets.images))
                            .unwrap_or_else(|| assets.flat_normal.clone())
                    };
                    let ripples = [ripple(0), ripple(1), ripple(2), ripple(3)];
                    let [ripple0, ripple1, ripple2, ripple3] = ripples;
                    let material = sky_water.add(crate::material::SkyWaterMaterial {
                        reflection: assets.images.add(reflection),
                        params: crate::material::SkyWaterParams::new(water),
                        ripple0,
                        ripple1,
                        ripple2,
                        ripple3,
                        depth_bias,
                    });
                    let vertices = part.vertices.iter().map(|v| SourceVertex {
                        position: v.position.map(|c| c * sky_scale() / METERS_PER_WORLD_UNIT),
                        normal: v.normal,
                        binormal: v.binormal,
                        tangent: v.tangent,
                        texcoord: v.texcoord,
                    });
                    let mesh = assets.meshes.add(build_mesh(vertices, &part.indices, [1.0, 1.0]));
                    commands.spawn((
                        Name::new(format!("{} layer {layer}", info.name)),
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        NotShadowCaster,
                        Transform::default(),
                        ChildOf(dome),
                    ));
                    layer += 1;
                    continue;
                }
                if let Some(material) = self.transparent_material(&info, depth_bias, true, assets) {
                    let vertices = part.vertices.iter().map(|v| SourceVertex {
                        position: v.position.map(|c| c * sky_scale() / METERS_PER_WORLD_UNIT),
                        normal: v.normal,
                        binormal: v.binormal,
                        tangent: v.tangent,
                        texcoord: v.texcoord,
                    });
                    let mesh = assets.meshes.add(build_mesh(vertices, &part.indices, [1.0, 1.0]));
                    commands.spawn((
                        Name::new(format!("{} layer {layer}", info.name)),
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        NotShadowCaster,
                        Transform::default(),
                        ChildOf(dome),
                    ));
                    layer += 1;
                    continue;
                }
                let (texture, uv_scale, uv_offset) = match sky_layer_map(&info) {
                    Some(map) => (
                        map.bitmap.datum.and_then(|d| self.textures.get(&self.cache, d, 0, true, assets.images)),
                        map.uv_scale,
                        map.uv_offset,
                    ),
                    None => (None, info.uv_scale, [0.0, 0.0]),
                };
                let additive = info
                    .transparent
                    .as_ref()
                    .is_some_and(|t| matches!(t.blend, shader::BlendFunction::Add | shader::BlendFunction::AlphaMultiplyAdd));
                let material = assets.materials.add(StandardMaterial {
                    base_color_texture: texture,
                    unlit: true,
                    alpha_mode: if additive { AlphaMode::Add } else { AlphaMode::Blend },
                    cull_mode: None,
                    fog_enabled: false,
                    depth_bias: SKY_BIAS + layer as f32 * SKY_LAYER_BIAS,
                    // (in the transmissive phase, as above; unlit, it
                    // refracts nothing)
                    specular_transmission: f32::MIN_POSITIVE,
                    ..default()
                });
                let vertices = part.vertices.iter().map(|v| SourceVertex {
                    position: v.position.map(|c| c * sky_scale() / METERS_PER_WORLD_UNIT),
                    normal: v.normal,
                    binormal: v.binormal,
                    tangent: v.tangent,
                    texcoord: [v.texcoord[0] + uv_offset[0] / uv_scale[0].max(1e-6), v.texcoord[1] + uv_offset[1] / uv_scale[1].max(1e-6)],
                });
                let mesh = assets.meshes.add(build_model_mesh(vertices, &part.indices, uv_scale));
                commands.spawn((
                    Name::new(format!("{} layer {layer}", info.name)),
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    NotShadowCaster,
                    Transform::default(),
                    ChildOf(dome),
                ));
                layer += 1;
            }
        }
        Ok(())
    }

    /// A transparent radiosity emitter's light card, for its emission's
    /// pattern: the first stage map of an added card, where the map is not
    /// scaled or moved (one set of texture coordinates). Halo's radiosity
    /// took the shader's emitted light over the whole surface; here the
    /// card's own picture emits, and the surface round its shape goes dark:
    /// b40's direction markers, pentagonal plates whose card is a lit
    /// triangle, lit the plate and the floor round it to white at their
    /// animation's peak (play feedback, 9 November: the light should fill
    /// the triangle). The emission in the bright texels is the shader's
    /// own, so the light cast falls by the map's mean. A black card
    /// ([`EMITTER_MAP_BLACK`]) patterns nothing: patterned, every invisible
    /// light went out (every card patterned, 10 November). (Tuning:
    /// `HALO_RT_EMITTER_MAPS=0`, the whole surface as before;
    /// `HALO_RT_EMITTER_MAPS_BLACK=1`, black cards patterned too.)
    fn emitter_map(&mut self, info: &ShaderInfo, assets: &mut LevelAssets) -> Option<(u32, Handle<Image>)> {
        use shader::BlendFunction;
        // (the listed shaders; HALO_RT_EMITTER_MAPS=1 every card, 0 none)
        let listed = EMITTER_MAPPED.iter().any(|name| info.name.eq_ignore_ascii_case(name));
        match std::env::var("HALO_RT_EMITTER_MAPS").ok().as_deref().map(str::trim) {
            Some("0") => return None,
            Some("1") => {}
            _ if !listed => return None,
            _ => {}
        }
        let transparent = info.transparent.as_ref()?;
        if transparent.first_map_type != 0 || !matches!(transparent.blend, BlendFunction::Add | BlendFunction::AlphaMultiplyAdd) {
            return None;
        }
        let map = transparent.maps.first()?.as_ref()?;
        let still = |channel: &shader::AnimationChannel| channel.function == 0 || channel.scale == 0.0;
        if map.uv_scale != [1.0, 1.0] || map.uv_offset != [0.0, 0.0] || map.rotation != 0.0 || !still(&map.animation.u) || !still(&map.animation.v) {
            return None;
        }
        let datum = map.bitmap.datum?;
        // (a black card, an invisible light: whole, unless asked; the
        // image the handle below is made of, permutation % count)
        if std::env::var("HALO_RT_EMITTER_MAPS_BLACK").map_or(true, |v| v.trim() != "1") {
            let peak = self.textures.luminance(&self.cache, datum, self.permutation)?.peak;
            if peak < EMITTER_MAP_BLACK {
                if std::env::var_os("HALO_RT_GLOW_LOG").is_some() {
                    info!("emitter map {}: its card is black (peak {peak:.4}), the whole surface emits", info.name);
                }
                return None;
            }
        }
        Some((datum, self.textures.permuted(&self.cache, datum, self.permutation, true, assets.images)?))
    }

    /// Weighs the light of the emitters patterned by their cards (see
    /// [`LevelBuilder::emitter_map`]) by what the cards show over the BSPs'
    /// meshes that carry them: the card emits in its bright texels at the
    /// shader's own radiance, so the light cast falls by the card's mean
    /// over the mesh (a lit triangle on a dark plate, a third); raised
    /// again so that the light cast is `EMITTER_MAP_SHARE` of the whole
    /// surface's (play feedback, 9 November: the marker's light to fill its
    /// triangle, or failing that 15% less of it). Once per level, before
    /// the meshes are spawned: the surfaces are shared by shader. (Every
    /// card patterned, `HALO_RT_EMITTER_MAPS=1`: b40's corridor strips were
    /// 8 to 30% lit over their meshes, gains 3 to 10.)
    fn weigh_emitter_maps(&mut self, assets: &mut LevelAssets) -> halo_map::Result<()> {
        let share: f32 = std::env::var("HALO_RT_EMITTER_MAP_SHARE").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(EMITTER_MAP_SHARE);
        let cache = self.cache.clone();
        let mut sums: HashMap<Option<u32>, (f64, usize)> = HashMap::default();
        for reference in bsp::references(&cache)? {
            let loaded = bsp::Bsp::load(&cache, &reference)?;
            for material in loaded.materials()? {
                if material.indices.is_empty() {
                    continue;
                }
                let Some(Surface::Solid(SolidSurface { emitter_map: Some(datum), .. })) = self.surface(material.shader, 0, assets).surface else {
                    continue;
                };
                // (each triangle's corners, edge midpoints and centre)
                let uvs = material.indices.chunks_exact(3).flat_map(|tri| {
                    let [a, b, c] = [0, 1, 2].map(|i| material.vertices[tri[i] as usize].texcoord);
                    let mid = |p: [f32; 2], q: [f32; 2]| [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
                    [a, b, c, mid(a, b), mid(b, c), mid(c, a), [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0]]
                });
                let count = material.indices.len() / 3 * 7;
                if let Some(mean) = self.textures.footprint_mean(&cache, datum, 0, uvs) {
                    let entry = sums.entry(material.shader.datum).or_default();
                    entry.0 += mean as f64 * count as f64;
                    entry.1 += count;
                }
            }
        }
        for (shader, (sum, count)) in sums {
            let mean = (sum / count.max(1) as f64) as f32;
            // (a card showing almost nothing over its mesh is left as it is:
            // raising its light a hundredfold would be a mistake of ours)
            if mean < 0.02 {
                if std::env::var_os("HALO_RT_GLOW_LOG").is_some() {
                    let name = shader.and_then(|d| self.cache.tag(d).ok()).map_or_else(String::new, |t| t.name.clone());
                    info!("emitter map {name}: mean {mean:.4} over its meshes, left as it is (nearly black there)");
                }
                continue;
            }
            let gain = (share / mean).min(1.0 / mean);
            if let Some(ShaderSurface { surface: Some(Surface::Solid(solid)), .. }) = self.surfaces.get_mut(&(shader, 0)) {
                if let Some(radiance) = solid.radiance.as_mut() {
                    *radiance *= gain;
                }
                if std::env::var_os("HALO_RT_GLOW_LOG").is_some() {
                    info!("emitter map {shader:?}: mean {mean:.3} over its meshes, light gain {gain:.2}");
                }
            }
        }
        Ok(())
    }

    /// A glowing see-through surface's light, if its shader is one (see
    /// [`GLOW_LIGHT`]).
    fn glow_radiance(&mut self, info: &ShaderInfo) -> Option<Vec3> {
        use shader::{BlendFunction, TransparentInfo};
        let scale = std::env::var("HALO_RT_GLOW_LIGHT").ok().and_then(|v| v.parse().ok()).unwrap_or(GLOW_LIGHT);
        let transparent = info.transparent.as_ref()?;
        let alpha = match transparent.blend {
            BlendFunction::Add => false,
            BlendFunction::AlphaMultiplyAdd => true,
            _ => return None,
        };
        if scale <= 0.0
            || info.meter.is_some()
            || transparent.flags & TransparentInfo::DECAL != 0
            || transparent.first_map_type != 0
            || GLOW_UNLIT.iter().any(|unlit| if unlit.contains('\\') { info.name.starts_with(unlit) } else { info.name.contains(unlit) })
        {
            return None;
        }
        let map = transparent.maps.first()?.as_ref()?.bitmap.datum?;
        let mean = Vec3::from(self.textures.mean_linear(&self.cache, map, self.permutation, alpha)?);
        // a generic shader's colour: its first stage colour that has one (an
        // energy door's map is white, its blue the stage's), white
        // otherwise
        let tint = transparent
            .stage_colors
            .iter()
            .map(|stage| Vec3::new(stage.color1[0], stage.color1[1], stage.color1[2]).max(Vec3::new(stage.color0[0], stage.color0[1], stage.color0[2])))
            .find(|color| color.max_element() > 0.0)
            .map_or(Vec3::ONE, |color| color / color.max_element());
        let radiance = (mean * SELF_ILLUMINATION_RADIANCE * scale).min(Vec3::splat(GLOW_MOST)) * tint;
        // (HALO_RT_GLOW_LOG: each, in the log)
        if std::env::var_os("HALO_RT_GLOW_LOG").is_some() {
            info!("glowing surface {}: mean {mean:.4}, radiance {radiance:.3}", info.name);
        }
        (radiance.max_element() > 1e-3).then_some(radiance)
    }

    /// A light fixture's light: a point light just in front of it, as
    /// bright as the emissive sphere that stood in for it under Solari (a
    /// sphere of radiance L and radius r is L π r² candela, 4π times that in
    /// lumens). With `show_light_proxies` the camera sees that sphere too.
    fn spawn_fixture_light(
        &mut self,
        commands: &mut Commands,
        object: Entity,
        light: Vec3,
        assets: &mut LevelAssets,
    ) {
        // (HALO_RT_NO_FIXTURE_LIGHTS, diagnostics: none)
        if std::env::var_os("HALO_RT_NO_FIXTURE_LIGHTS").is_some() {
            return;
        }
        let power = light * LIGHT_PROXY_RADIANCE * sphere_lumens_per_radiance(LIGHT_PROXY_RADIUS);
        let mut source = commands.spawn((
            Name::new("light fixture light"),
            PointLight { radius: LIGHT_PROXY_RADIUS, range: FIXTURE_LIGHT_RANGE, shadow_maps_enabled: false, ..default() },
            LightSource { power },
            // (lighting the rays' scene from off screen too: Bevy's frustum
            // culling would leave it out with its range off screen)
            bevy::camera::visibility::NoFrustumCulling,
            // object space is Bevy-converted Halo space: Halo forward (+x) is +x
            Transform::from_xyz(LIGHT_PROXY_OFFSET, 0.0, 0.0),
            ChildOf(object),
        ));
        if self.show_light_proxies {
            let mesh = self
                .light_proxy
                .get_or_insert_with(|| assets.meshes.add(Sphere::new(LIGHT_PROXY_RADIUS).mesh().build()))
                .clone();
            let material = assets.materials.add(StandardMaterial {
                base_color: Color::BLACK,
                emissive: LinearRgba::rgb(light.x, light.y, light.z) * LIGHT_PROXY_RADIANCE,
                unlit: true,
                ..default()
            });
            source.insert((Mesh3d(mesh), MeshMaterial3d(material)));
        }
        self.stats.lights += 1;
    }

    /// A light fixture's own spot lights, where its placement gives it no
    /// light (c10's floodlights, `ground_bright`: in the game their light is
    /// the lightmaps' and their tag's only a lens flare): each spot light its
    /// tag attaches, at the attachment's marker, along it, in the light's
    /// colour and cone. (Tuning: `HALO_RT_FLOODLIGHTS=brightness[,range in
    /// metres]`, 0 none.)
    fn spawn_fixture_spotlights(&mut self, commands: &mut Commands, object: Entity, placement: &objects::Placement) {
        use halo_map::{collision::matrix_inverse, markers, pose::matrix_multiply};
        let (scale, range) = std::env::var("HALO_RT_FLOODLIGHTS").ok().map_or((1.0, FLOODLIGHT_RANGE), |text| {
            let mut values = text.split(',').map(|v| v.trim().parse::<f32>().ok());
            (values.next().flatten().unwrap_or(1.0), values.next().flatten().unwrap_or(FLOODLIGHT_RANGE))
        });
        let Some(datum) = placement.definition.datum else { return };
        if scale <= 0.0 {
            return;
        }
        let Ok(root) = self.cache.tag_root(datum, halo_map::groups::OBJECT) else { return };
        // (struct object_attachment: its type, then its marker's name)
        let Ok(attachments) = root.block(halo_map::layout::_object_definition::ATTACHMENTS, 0x48) else { return };
        let model = objects::object_model(&self.cache, placement.definition).ok().flatten().and_then(|m| m.datum);
        let marker_set = model.and_then(|model| markers::ModelMarkers::load(&self.cache, model).ok());
        let nodes = model.and_then(|model| halo_map::model::Model::load(&self.cache, model).ok()).map(|m| m.nodes);
        for attachment in attachments.iter() {
            let Ok(light) = attachment.reference(0) else { continue };
            let Some(light_datum) = light.datum.filter(|_| light.group == halo_map::groups::LIGHT) else { continue };
            let Ok(definition) = self.cache.tag_root(light_datum, halo_map::groups::LIGHT) else { continue };
            let f = |offset: usize| definition.f32(offset).unwrap_or(0.0);
            // (struct light_definition: the radius, the falloff and cutoff
            // angles, the upper colour)
            let (radius, falloff, cutoff) = (f(0x4), f(0x10), f(0x14));
            // (one shining every way: a lamp's big spot light, if it is one)
            let lamp = self.cache.tag(datum).ok().and_then(|tag| LAMP_SPOTLIGHTS.iter().find(|(name, ..)| tag.name.eq_ignore_ascii_case(name)));
            let (falloff, cutoff, brightness, range, aim) = match lamp {
                Some(&(_, falloff, cutoff, brightness, reach, aim)) => {
                    (falloff.to_radians(), cutoff.to_radians(), brightness, reach.max(range), Some(aim))
                }
                None if cutoff > 0.0 && cutoff < std::f32::consts::FRAC_PI_2 => (falloff, cutoff, 1.0, range, None),
                None => continue,
            };
            if radius <= 0.0 {
                continue;
            }
            let mut color = Vec3::new(f(0x4c), f(0x50), f(0x54));
            let (mut falloff, mut cutoff, mut brightness, mut range) = (falloff, cutoff, brightness, range);
            let look = self.cache.tag(datum).ok().and_then(|tag| fixture_spot_look(&tag.name));
            if let Some(look) = look {
                (falloff, cutoff, brightness) = (look.falloff.to_radians(), look.cutoff.to_radians(), brightness * look.brightness);
                color = look.color.map_or(color, Vec3::from);
                range = look.range.unwrap_or(range);
            }
            // where its marker is on the model, in the object's space (its
            // node at rest)
            let name = attachment.bytes(0x10, 32).ok().map(|bytes| String::from_utf8_lossy(bytes.split(|&b| b == 0).next().unwrap_or(&[])).into_owned());
            let matrix = name.as_deref().and_then(|name| {
                let marker = marker_set.as_ref()?.markers.iter().find(|m| m.name.eq_ignore_ascii_case(name.as_bytes()))?;
                let instance = marker.instances.first()?;
                let node = nodes.as_ref()?.get(instance.node as usize)?;
                let local = markers::matrix_from_point_and_quaternion(instance.translation, instance.rotation);
                Some(matrix_multiply(&matrix_inverse(&node.default_inverse), &local))
            });
            let (position, forward) = matrix.map_or(([0.0; 3], [1.0, 0.0, 0.0]), |m| (m.position, m.forward));
            let forward = aim.unwrap_or(forward);
            let position = Vec3::from(halo_to_bevy(position, METERS_PER_WORLD_UNIT));
            let forward = Vec3::from(halo_to_bevy(forward, 1.0)).normalize_or(Vec3::X);
            // (HALO_RT_LIGHT_LOG: each, in the log)
            if std::env::var_os("HALO_RT_LIGHT_LOG").is_some() {
                info!(
                    "fixture spot light at {:?} (marker {name:?} {}): {position} along {forward}, colour {color}, radius {radius}, cone {:.0}..{:.0} deg",
                    placement.position, if matrix.is_some() { "found" } else { "not found" }, falloff.to_degrees(), cutoff.to_degrees()
                );
            }
            let power = color * FLOODLIGHT_RADIANCE * radius * scale * brightness * sphere_lumens_per_radiance(FLOODLIGHT_RADIUS);
            let outer = cutoff.min(std::f32::consts::FRAC_PI_2 - 0.01);
            commands.spawn((
                Name::new("light fixture spot light"),
                SpotLight {
                    radius: FLOODLIGHT_RADIUS,
                    range,
                    outer_angle: outer,
                    inner_angle: falloff.clamp(0.0, outer * 0.99),
                    shadow_maps_enabled: false,
                    ..default()
                },
                LightSource { power },
                bevy::camera::visibility::NoFrustumCulling,
                Transform::from_translation(position).looking_to(forward, if forward.y.abs() > 0.99 { Vec3::X } else { Vec3::Y }),
                ChildOf(object),
            ));
            self.stats.lights += 1;
        }
    }

    /// An emissive BSP material's triangles by the cluster each is in (by
    /// its centre; -1 for none): the cluster and the triangles' indices. One
    /// chunk, or none, when the BSP's clusters can't be read.
    fn emissive_chunks(&self, bsp: usize, material: &bsp::Material) -> Vec<(i16, Vec<u32>)> {
        let Some(visibility) = self.clusters.get(bsp).and_then(|v| v.as_ref()) else { return Vec::new() };
        let mut chunks: Vec<(i16, Vec<u32>)> = Vec::new();
        for triangle in material.indices.chunks_exact(3) {
            let mut centre = [0.0f32; 3];
            for &index in triangle {
                let position = material.vertices[index as usize].position;
                for axis in 0..3 {
                    centre[axis] += position[axis] / 3.0;
                }
            }
            let cluster = visibility.cluster(centre);
            match chunks.iter_mut().find(|(c, _)| *c == cluster) {
                Some((_, indices)) => indices.extend_from_slice(triangle),
                None => chunks.push((cluster, triangle.to_vec())),
            }
        }
        chunks
    }

    fn spawn_mesh(
        &mut self,
        commands: &mut Commands,
        parent: Entity,
        name: String,
        mesh: Handle<Mesh>,
        surface: &Surface,
        raster: bool,
    ) {
        let mode = if raster { MeshUse::Both } else { MeshUse::Traced };
        self.spawn_mesh_as(commands, parent, name, mesh, surface, mode);
    }

    /// Spawns a mesh for the camera, for the rays, or both (see [`MeshUse`]);
    /// the rays' entity, if any.
    fn spawn_mesh_as(
        &mut self,
        commands: &mut Commands,
        parent: Entity,
        name: String,
        mesh: Handle<Mesh>,
        surface: &Surface,
        mode: MeshUse,
    ) -> Option<Entity> {
        let (traced_use, raster_use) = match &mode {
            MeshUse::Both => (true, true),
            MeshUse::Traced => (true, false),
            MeshUse::Raster(_) => (false, true),
        };
        let skin = match mode {
            MeshUse::Raster(skin) => skin,
            _ => None,
        };
        // (a skinned mesh is placed by its joints: its rest-pose bounds
        // would cull it wrongly)
        let skinned = |entity: &mut EntityCommands, skin: Option<SkinnedMesh>| {
            if let Some(skin) = skin {
                entity.insert((skin, bevy::camera::visibility::NoFrustumCulling));
            }
        };
        let surface = match surface {
            Surface::Solid(solid) => solid,
            Surface::Transparent(_) | Surface::Blended(_) => {
                let skipped = match surface {
                    Surface::Blended(_) => !self.draw_blended,
                    _ => self.skip_transparent,
                };
                if raster_use && !skipped {
                    let mut drawn = commands.spawn((
                        Name::new(name),
                        Mesh3d(mesh.clone()),
                        bevy::light::NotShadowCaster,
                        Transform::default(),
                        ChildOf(parent),
                    ));
                    match surface {
                        Surface::Transparent(material) => drawn.insert(MeshMaterial3d(material.clone())),
                        // glass and water are thin panes in the ray-traced
                        // scene too: Solarik reflects, tints and shadows
                        // through them, and composites the camera's panes in
                        // place of the raster draw once it can
                        Surface::Blended(material) if traced_use => {
                            drawn.insert((MeshMaterial3d(material.clone()), RaytracingMesh3d(mesh.clone()), TracedMesh(mesh.clone())))
                        }
                        Surface::Blended(material) => drawn.insert(MeshMaterial3d(material.clone())),
                        Surface::Solid(_) => unreachable!(),
                    };
                    skinned(&mut drawn, skin.clone());
                    // (a self-occluding object's part's depth, before it)
                    if let (Surface::Transparent(material), true, Some(masks)) = (surface, self.self_occluding, &self.glass_sky_mask)
                        && !self.ignores_effect.contains(&material.id())
                    {
                        let mut depth = commands.spawn((
                            Name::new("self-occluding depth"),
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(masks.self_occlusion.clone()),
                            bevy::light::NotShadowCaster,
                            Transform::default(),
                            ChildOf(parent),
                        ));
                        skinned(&mut depth, skin.clone());
                    }
                    // (a pane's sky mask: the sky's layers not over it)
                    if let (Surface::Blended(_), Some(masks)) = (surface, &self.glass_sky_mask) {
                        let mask = if self.first_person { &masks.near } else { &masks.far };
                        let mut masked = commands.spawn((
                            Name::new("sky mask"),
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(mask.clone()),
                            bevy::light::NotShadowCaster,
                            Transform::default(),
                            ChildOf(parent),
                        ));
                        skinned(&mut masked, skin);
                    }
                    self.stats.meshes += 1;
                    self.stats.transparent_meshes += 1;
                }
                return None;
            }
        };
        let mut traced = None;
        if traced_use {
            let mut entity = commands.spawn((
                Name::new(name.clone()),
                RaytracingMesh3d(mesh.clone()),
                TracedMesh(mesh.clone()),
                Visibility::Inherited,
                MeshMaterial3d(surface.standard.clone()),
                Transform::default(),
                ChildOf(parent),
            ));
            if let Some(radiance) = surface.radiance {
                // (raised for light alone where the camera draws its own)
                let gain = if surface.raster.is_some() || surface.seen_standard.is_some() { surface.light_gain } else { 1.0 };
                entity.insert(EmissiveSurface { radiance: radiance * gain });
                self.stats.emissive_meshes += 1;
                // (an unseen emitter: met by no ray, instead of passed
                // through by every one; see light_only_emitters)
                if surface.unseen && light_only_emitters() {
                    entity.insert(bevy_solarik::scene::RaytracingMask::LIGHT_ONLY);
                }
            }
            if let Some((root, base, shader)) = &self.recolour {
                entity.insert(Recolourable { root: *root, base: base.clone(), shader: *shader, traced: true });
            }
            traced = Some(entity.id());
        }
        // (an unseen emitter's transparent surface, which the camera sees)
        if raster_use
            && !self.skip_transparent
            && let Some(material) = surface.seen_as.as_ref().filter(|_| surface.unseen)
        {
            let mut drawn = commands.spawn((
                Name::new(name.clone()),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                bevy::light::NotShadowCaster,
                Transform::default(),
                ChildOf(parent),
            ));
            skinned(&mut drawn, skin.clone());
            // (a self-occluding object's part's depth, before it, as for any
            // transparent part: Cortana's glow, her eyes behind their lids)
            if self.self_occluding
                && let Some(masks) = &self.glass_sky_mask
                && !self.ignores_effect.contains(&material.id())
            {
                let mut depth = commands.spawn((
                    Name::new("self-occluding depth"),
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(masks.self_occlusion.clone()),
                    bevy::light::NotShadowCaster,
                    Transform::default(),
                    ChildOf(parent),
                ));
                skinned(&mut depth, skin.clone());
            }
            self.stats.transparent_meshes += 1;
        }
        if raster_use && !surface.unseen {
            match (&surface.raster, traced) {
                // the same entity, when the camera sees what the rays do and
                // nothing moves it differently
                (None, Some(traced)) if skin.is_none() && surface.seen_standard.is_none() => {
                    commands.entity(traced).insert(Mesh3d(mesh));
                }
                (raster, _) => {
                    let mut drawn = commands.spawn((Name::new(name), Mesh3d(mesh), Transform::default(), ChildOf(parent)));
                    match raster {
                        // the camera sees the Halo material; rays see the standard one
                        Some(material) => drawn.insert(MeshMaterial3d(material.clone())),
                        None => drawn.insert(MeshMaterial3d(surface.seen_standard.clone().unwrap_or_else(|| surface.standard.clone()))),
                    };
                    if let Some(radiance) = surface.radiance {
                        drawn.insert(EmissiveSurface { radiance });
                    }
                    if let Some((root, base, shader)) = &self.recolour {
                        drawn.insert(Recolourable { root: *root, base: base.clone(), shader: *shader, traced: false });
                    }
                    skinned(&mut drawn, skin);
                }
            }
        }
        self.stats.meshes += 1;
        traced
    }

    /// A model's meshes split by node, for an object the game moves (see
    /// [`crate::bridge`]): each triangle goes with the node that moves its
    /// first vertex most (a rigid stand-in for Halo's two-node skinning), and
    /// a node's pieces are drawn at `node matrix * default inverse`, as the
    /// game draws its parts. Made once per (model, permutations).
    pub fn animated_model(
        &mut self,
        datum: u32,
        permutations: [u8; 8],
        assets: &mut LevelAssets,
    ) -> Option<Arc<AnimatedModel>> {
        if let Some(cached) = self.animated_models.get(&(datum, permutations)) {
            return cached.clone();
        }
        let _profile = bevy::render::diagnostic::profile_scope("main.animated_model");
        let built = match self.build_animated_model(datum, &permutations, assets) {
            Ok(model) => Some(Arc::new(model)),
            Err(e) => {
                let name = self.cache.tag(datum).map(|t| t.name.clone()).unwrap_or_default();
                warn!("model {name}: {e}");
                self.stats.object_failures += 1;
                None
            }
        };
        self.animated_models.insert((datum, permutations), built.clone());
        built
    }

    fn build_animated_model(
        &mut self,
        datum: u32,
        permutations: &[u8],
        assets: &mut LevelAssets,
    ) -> halo_map::Result<AnimatedModel> {
        let cache = self.cache.clone();
        let model = { let _profile = bevy::render::diagnostic::profile_scope("main.model_load"); Model::load(&cache, datum)? };
        let mut pieces = Vec::new();
        let mut traced_skins = Vec::new();
        let mut skinned = Vec::new();
        for geometry in model.geometries_for(permutations, model.finest_played_detail()) {
            for part in model.geometry(geometry)? {
                if part.indices.is_empty() {
                    continue;
                }
                let shader_ref = model
                    .shaders
                    .get(part.shader_index.max(0) as usize)
                    .copied()
                    .unwrap_or(TagRef { group: 0, datum: None });
                let permutation = model.shader_permutations.get(part.shader_index.max(0) as usize).copied().unwrap_or(0);
                let shader_use = shader_ref.datum.map(|datum| ShaderUse { datum, permutation });
                let shader = { let _profile = bevy::render::diagnostic::profile_scope("main.model_surface"); self.surface(shader_ref, permutation, assets) };
                if let Some(text) = std::env::var_os("HALO_RT_OBJECT_LOG")
                    && model.name.contains(text.to_string_lossy().as_ref())
                {
                    let kind = match &shader.surface {
                        None => "none",
                        Some(Surface::Solid(_)) => "solid",
                        Some(Surface::Transparent(_)) => "transparent",
                        Some(Surface::Blended(_)) => "blended",
                    };
                    info!("model {} part: shader {:?} {kind}, {} vertices", model.name, shader_ref.datum, part.vertices.len());
                }
                let Some(surface) = shader.surface else {
                    self.stats.skipped_transparent += 1;
                    continue;
                };
                let last_node = model.nodes.len().saturating_sub(1) as u16;
                let joints: Vec<[u16; 4]> = part
                    .vertices
                    .iter()
                    .map(|v| [(v.nodes[0] as u16).min(last_node), (v.nodes[1] as u16).min(last_node), 0, 0])
                    .collect();
                let weights: Vec<[f32; 4]> = part
                    .vertices
                    .iter()
                    .map(|v| {
                        let (a, b) = (v.node_weights[0].max(0.0), v.node_weights[1].max(0.0));
                        let total = a + b;
                        if total > 0.0 { [a / total, b / total, 0.0, 0.0] } else { [1.0, 0.0, 0.0, 0.0] }
                    })
                    .collect();
                // (a vertex's one node, where it follows only that)
                let sole_node = |index: u32| {
                    let (j, w) = (joints[index as usize], weights[index as usize]);
                    if w[1] == 0.0 || j[0] == j[1] {
                        Some(j[0])
                    } else if w[0] == 0.0 {
                        Some(j[1])
                    } else {
                        None
                    }
                };
                let rigid = part.indices.chunks_exact(3).all(|t| {
                    let node = sole_node(t[0]);
                    node.is_some() && sole_node(t[1]) == node && sole_node(t[2]) == node
                });
                if !rigid {
                    let source: Vec<SourceVertex> = part
                        .vertices
                        .iter()
                        .map(|v| SourceVertex {
                            position: v.position,
                            normal: v.normal,
                            binormal: v.binormal,
                            tangent: v.tangent,
                            texcoord: v.texcoord,
                        })
                        .collect();
                    let rest = build_model_mesh(source.into_iter(), &part.indices, shader.uv_scale);
                    let vec3s = |attribute| match rest.attribute(attribute) {
                        Some(VertexAttributeValues::Float32x3(values)) => values.iter().map(|&v| Vec3::from(v)).collect(),
                        _ => Vec::new(),
                    };
                    let tangents = match rest.attribute(Mesh::ATTRIBUTE_TANGENT) {
                        Some(VertexAttributeValues::Float32x4(values)) => values.iter().map(|&v| Vec4::from(v)).collect(),
                        _ => Vec::new(),
                    };
                    let uvs = match rest.attribute(Mesh::ATTRIBUTE_UV_0) {
                        Some(VertexAttributeValues::Float32x2(values)) => values.clone(),
                        _ => Vec::new(),
                    };
                    let indices = match rest.indices() {
                        Some(Indices::U32(indices)) => indices.clone(),
                        _ => Vec::new(),
                    };
                    traced_skins.push(TracedSkin {
                        positions: vec3s(Mesh::ATTRIBUTE_POSITION),
                        normals: vec3s(Mesh::ATTRIBUTE_NORMAL),
                        tangents,
                        uvs,
                        joints: joints.iter().map(|j| [j[0], j[1]]).collect(),
                        weights: weights.iter().map(|w| [w[0], w[1]]).collect(),
                        indices,
                        surface: surface.clone(),
                        shader: shader_use,
                        raster_mesh: None,
                    });
                }
                let mut by_node: HashMap<usize, Vec<u32>> = HashMap::default();
                for triangle in part.indices.chunks_exact(3).filter(|_| rigid) {
                    let v = &part.vertices[triangle[0] as usize];
                    let node = if v.node_weights[0] >= v.node_weights[1] { v.nodes[0] } else { v.nodes[1] } as usize;
                    by_node.entry(node.min(model.nodes.len().saturating_sub(1))).or_default().extend_from_slice(triangle);
                }
                let source = || {
                    part.vertices.iter().map(|v| SourceVertex {
                        position: v.position,
                        normal: v.normal,
                        binormal: v.binormal,
                        tangent: v.tangent,
                        texcoord: v.texcoord,
                    })
                };
                // (the rays' rigid copy sits a little inside the skinned
                // surface the camera sees: where it bends differently, rays
                // leaving that surface would otherwise hit it, and the model
                // would shadow itself in blotches)
                let inset = |v: SourceVertex| SourceVertex {
                    position: std::array::from_fn(|i| v.position[i] - v.normal[i] * TRACED_INSET),
                    ..v
                };
                for (node, indices) in by_node {
                    let mesh = assets.meshes.add(build_model_mesh(source().map(inset), &indices, shader.uv_scale));
                    pieces.push(ModelPiece { node, mesh, surface: surface.clone(), shader: shader_use });
                }
                // the whole part, skinned: each vertex follows its nodes by
                // their weights, so joints bend without seams
                let mesh = build_model_mesh(source(), &part.indices, shader.uv_scale)
                    .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(joints))
                    .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, weights);
                let rest: Vec<[f32; 4]> = source()
                    .map(|v| [v.position[0], v.position[1], v.position[2], 1.0])
                    .collect();
                let modifier_mesh = assets.meshes.add(mesh.clone().with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, rest));
                let mesh = assets.meshes.add(mesh);
                if !rigid && let Some(traced) = traced_skins.last_mut() {
                    traced.raster_mesh = Some(mesh.clone());
                }
                skinned.push(SkinnedPart {
                    mesh,
                    modifier_mesh,
                    surface: surface.clone(),
                    shader: shader_use,
                });
            }
        }
        let default_inverses: Vec<Affine3A> = model.nodes.iter().map(|n| halo_affine(&n.default_inverse)).collect();
        let inverse_bindposes =
            assets.inverse_bindposes.add(SkinnedMeshInverseBindposes::from(vec![Mat4::IDENTITY; default_inverses.len().max(1)]));
        Ok(AnimatedModel { name: model.name, pieces, traced_skins, skinned, inverse_bindposes, default_inverses })
    }

    /// Spawns an animated model: a root, an entity per node it draws (whose
    /// transforms the caller sets each frame) and its pieces under them.
    pub fn spawn_animated_model(
        &mut self,
        commands: &mut Commands,
        model: &AnimatedModel,
        raster: bool,
        change_colors: &[[f32; 3]; 4],
        camouflaged: bool,
        object_index: u32,
        assets: &mut LevelAssets,
    ) -> (Entity, Vec<Option<Entity>>, Vec<Entity>) {
        let root = commands.spawn((Name::new(model.name.clone()), Transform::default(), Visibility::default())).id();
        // the shaders whose self-illumination animates: a copy of each one's
        // material for this object (its phase is its own)
        let mut animated: Vec<(Handle<StandardMaterial>, Option<Handle<HaloEnvironmentMaterial>>, shader::ModelSelfIlluminationAnimation)> =
            Vec::new();
        let mut animated_shaders: Vec<ShaderUse> = Vec::new();
        let mut animated_by_shader: HashMap<ShaderUse, Surface> = HashMap::new();
        let mut animate = |builder: &mut Self, surface: Surface, shader: Option<ShaderUse>, assets: &mut LevelAssets| -> Surface {
            // a glowing surface's picture, as a transparent surface's below
            // (its light, the rays', keeps still)
            if let (Surface::Solid(solid @ SolidSurface { unseen: true, seen_as: Some(material), .. }), Some(shader)) = (&surface, shader) {
                if let Some(copy) = animated_by_shader.get(&shader) {
                    return copy.clone();
                }
                let Some(mut copy) = assets.transparent_materials.get(material).cloned() else { return surface };
                if !copy.animation.as_ref().is_some_and(|a| a.uses_object()) && copy.meter.is_none() && copy.plasma.is_none() {
                    return surface;
                }
                copy.object = Some(root);
                let copy = Surface::Solid(SolidSurface { seen_as: Some(assets.transparent_materials.add(copy)), ..solid.clone() });
                animated_by_shader.insert(shader, copy.clone());
                return copy;
            }
            // a transparent surface its object's functions or colours drive:
            // a copy of its material for this object (`ObjectFunctions` on
            // the root, `animate_transparent`)
            if let (Surface::Transparent(material), Some(shader)) = (&surface, shader) {
                if let Some(copy) = animated_by_shader.get(&shader) {
                    return copy.clone();
                }
                let Some(mut copy) = assets.transparent_materials.get(material).cloned() else { return surface };
                // (shared by every object where nothing of its object's
                // changes it)
                if !copy.animation.as_ref().is_some_and(|a| a.uses_object()) && copy.meter.is_none() && copy.plasma.is_none() {
                    return surface;
                }
                copy.object = Some(root);
                let copy = Surface::Transparent(assets.transparent_materials.add(copy));
                animated_by_shader.insert(shader, copy.clone());
                return copy;
            }
            let (Surface::Solid(solid), Some(shader)) = (&surface, shader) else { return surface };
            if let Some(copy) = animated_by_shader.get(&shader) {
                return copy.clone();
            }
            let info = shader::shader_info(&builder.cache, TagRef { group: 0, datum: Some(shader.datum) }).ok().flatten();
            let Some(animation) = info.and_then(|i| i.model_self_illumination_animation) else { return surface };
            // (one that holds its upper colour is the shader's own material:
            // function 0 is "one")
            if animation.color_source.is_none() && (animation.function == 0 || animation.lower == animation.upper) {
                return surface;
            }
            let Some(material) = assets.materials.get(&solid.standard).cloned() else { return surface };
            let handle = assets.materials.add(material);
            let raster = solid.raster.as_ref().and_then(|raster| assets.environment_materials.get(raster).cloned()).map(|raster| assets.environment_materials.add(raster));
            animated.push((handle.clone(), raster.clone(), animation));
            animated_shaders.push(shader);
            let copy = Surface::Solid(SolidSurface { standard: handle, raster: raster.or(solid.raster.clone()), ..solid.clone() });
            animated_by_shader.insert(shader, copy.clone());
            copy
        };
        self.adopt(commands, root);
        // every node, as the skinned parts' joints
        let nodes: Vec<Entity> = (0..model.default_inverses.len().max(1))
            .map(|_| commands.spawn((Transform::default(), Visibility::default(), ChildOf(root))).id())
            .collect();
        // camouflaged: a body of liquid glass in the rays' scene, which
        // Solarik composites for the camera, bending the picture behind it
        // and reflecting what is round it (and, as glass, it casts no
        // shadow); the game's own shimmer refracts the picture behind, which
        // its overlay doesn't have. The first-person arms and weapon too,
        // their bending parts skinned for the rays as they move
        // (crate::bridge): a faint shell, they looked painted on beside the
        // camouflaged enemies' glass.
        if camouflaged {
            let material = self.camouflage_glass(assets);
            let glass = |commands: &mut Commands, parent: Entity, mesh: Handle<Mesh>| {
                commands
                    .spawn((
                        Name::new(format!("{} (camouflaged)", model.name)),
                        RaytracingMesh3d(mesh.clone()),
                        TracedMesh(mesh),
                        Visibility::Inherited,
                        MeshMaterial3d(material.clone()),
                        Transform::default(),
                        ChildOf(parent),
                    ))
                    .id()
            };
            // (its solid surfaces alone: a transparent one, a weapon's
            // muzzle flash or a glowing card the game draws only now and
            // then, as glass was a slab out from the first-person weapon,
            // the "shards" of play feedback)
            let solid = |surface: &Surface| matches!(surface, Surface::Solid(SolidSurface { unseen: false, .. }));
            for piece in model.pieces.iter().filter(|piece| solid(&piece.surface)) {
                glass(commands, nodes[piece.node], piece.mesh.clone());
            }
            // (its bending parts skinned anew as its nodes move, as below)
            let rest = vec![Mat4::IDENTITY; model.default_inverses.len()];
            let skin_parent = nodes.first().copied().unwrap_or(root);
            // (each skin its entity, as crate::bridge pairs them; hidden
            // for a transparent one)
            let skins = model
                .traced_skins
                .iter()
                .map(|skin| {
                    let entity = glass(commands, skin_parent, assets.meshes.add(skin.mesh(&rest)));
                    if !solid(&skin.surface) {
                        commands.entity(entity).insert(Visibility::Hidden);
                    }
                    entity
                })
                .collect();
            return (root, nodes.into_iter().map(Some).collect(), skins);
        }
        // the rays see the rigid pieces; the camera sees the skinned parts,
        // but glass and water: a pane both see, which Solarik ray traces
        // and composites for the camera as the map's own panes
        for piece in &model.pieces {
            let surface = self.coloured_surface(&piece.surface, piece.shader, change_colors, assets);
            let surface = animate(self, surface, piece.shader, assets);
            let pane = matches!(surface, Surface::Blended(_));
            let mode = if raster && (model.skinned.is_empty() || pane) { MeshUse::Both } else { MeshUse::Traced };
            // (the first-person arms and weapon are the camera's alone, but
            // their panes: posed for its view, in the rays' scene they'd be
            // reflected and shadow where nothing is, and the panes traced
            // from the camera, the sea's, showed their outline around them)
            let mode = match mode {
                _ if !self.first_person || pane => mode,
                MeshUse::Both => MeshUse::Raster(None),
                _ => continue,
            };
            self.recolour = Some((root, piece.surface.clone(), piece.shader));
            let _ = self.spawn_mesh_as(commands, nodes[piece.node], model.name.clone(), piece.mesh.clone(), &surface, mode);
            self.recolour = None;
        }
        // the bending parts, under the model's root node, given a mesh
        // skinned anew (relative to that node) as its nodes move
        // (crate::bridge)
        let rest = vec![Mat4::IDENTITY; model.default_inverses.len()];
        let skin_parent = nodes.first().copied().unwrap_or(root);
        let mut skins = Vec::new();
        for skin in &model.traced_skins {
            let surface = self.coloured_surface(&skin.surface, skin.shader, change_colors, assets);
            let surface = animate(self, surface, skin.shader, assets);
            let mesh = assets.meshes.add(skin.mesh(&rest));
            let mode = if raster && matches!(surface, Surface::Blended(_)) { MeshUse::Both } else { MeshUse::Traced };
            // (none for the first-person arms and weapon, as above)
            if self.first_person && !matches!(mode, MeshUse::Both) {
                continue;
            }
            self.recolour = Some((root, skin.surface.clone(), skin.shader));
            // (a pane is the camera's too, unbent, under the root as before;
            // the first-person one stays unbent for the rays too)
            if matches!(mode, MeshUse::Both) {
                let pane = if self.first_person { MeshUse::Both } else { MeshUse::Raster(None) };
                let _ = self.spawn_mesh_as(commands, root, model.name.clone(), mesh.clone(), &surface, pane);
            }
            if !self.first_person
                && let Some(traced) =
                    self.spawn_mesh_as(commands, skin_parent, model.name.clone(), mesh, &surface, MeshUse::Traced)
            {
                skins.push(traced);
            }
            self.recolour = None;
        }
        if raster {
            for part in &model.skinned {
                // (its panes are drawn above)
                if matches!(part.surface, Surface::Blended(_)) {
                    continue;
                }
                let surface = self.coloured_surface(&part.surface, part.shader, change_colors, assets);
                let surface = animate(self, surface, part.shader, assets);
                let skin = SkinnedMesh { inverse_bindposes: model.inverse_bindposes.clone(), joints: nodes.clone() };
                self.recolour = Some((root, part.surface.clone(), part.shader));
                let _ =
                    self.spawn_mesh_as(commands, root, model.name.clone(), part.mesh.clone(), &surface, MeshUse::Raster(Some(skin)));
                self.recolour = None;
            }
        }
        // its modifier shader (a shield's plasma) over its skinned parts: a
        // copy of each part's mesh carrying its rest position (the game's
        // units and axes) as its colour, where the noise is sampled
        if raster
            && let Some(shader) = self.modifier_shader.take()
            && let Some(plasma) = self.plasma_material(shader, assets)
            && let Some(mut material) = assets.transparent_materials.get(&plasma).cloned()
        {
            material.object = Some(root);
            let material = assets.transparent_materials.add(material);
            for part in &model.skinned {
                let skin = SkinnedMesh { inverse_bindposes: model.inverse_bindposes.clone(), joints: nodes.clone() };
                commands.spawn((
                    Name::new(format!("{} (modifier)", model.name)),
                    Mesh3d(part.modifier_mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    bevy::light::NotShadowCaster,
                    skin,
                    bevy::camera::visibility::NoFrustumCulling,
                    Transform::default(),
                    ChildOf(root),
                ));
            }
        }
        self.modifier_shader = None;
        if !animated.is_empty() {
            commands.entity(root).insert(AnimatedSelfIllumination {
                materials: animated,
                shaders: animated_shaders,
                phase: self_illumination_phase(object_index),
                change_colors: *change_colors,
            });
        }
        (root, nodes.into_iter().map(Some).collect(), skins)
    }

    /// A model's meshes at the highest level of detail, made once per
    /// (model, permutations).
    fn model_meshes(
        &mut self,
        datum: u32,
        permutations: [u8; 8],
        assets: &mut LevelAssets,
    ) -> Option<ModelMeshes> {
        if let Some(cached) = self.models.get(&(datum, permutations)) {
            return cached.clone();
        }
        let built = match self.build_model(datum, &permutations, assets) {
            Ok(parts) => Some(parts),
            Err(e) => {
                let name = self.cache.tag(datum).map(|t| t.name.clone()).unwrap_or_default();
                warn!("model {name}: {e}");
                self.stats.object_failures += 1;
                None
            }
        };
        self.models.insert((datum, permutations), built.clone());
        built
    }

    fn build_model(
        &mut self,
        datum: u32,
        permutations: &[u8],
        assets: &mut LevelAssets,
    ) -> halo_map::Result<ModelMeshes> {
        let cache = self.cache.clone();
        let model = Model::load(&cache, datum)?;
        let mut out = Vec::new();
        for geometry in model.geometries_for(permutations, model.finest_played_detail()) {
            for part in model.geometry(geometry)? {
                if part.indices.is_empty() {
                    continue;
                }
                let shader_ref = model
                    .shaders
                    .get(part.shader_index.max(0) as usize)
                    .copied()
                    .unwrap_or(TagRef { group: 0, datum: None });
                let permutation = model.shader_permutations.get(part.shader_index.max(0) as usize).copied().unwrap_or(0);
                let shader = self.surface(shader_ref, permutation, assets);
                let Some(surface) = shader.surface else {
                    self.stats.skipped_transparent += 1;
                    continue;
                };
                let vertices = part.vertices.iter().map(|v| SourceVertex {
                    position: v.position,
                    normal: v.normal,
                    binormal: v.binormal,
                    tangent: v.tangent,
                    texcoord: v.texcoord,
                });
                let mesh = assets.meshes.add(build_model_mesh(vertices, &part.indices, shader.uv_scale));
                // (and its back, for an emitter that lights both ways: see
                // EMITTERS_BOTH_WAYS)
                let both_ways = matches!(&surface, Surface::Solid(solid) if solid.radiance.is_some())
                    && shader_ref.datum.and_then(|d| cache.tag(d).ok()).is_some_and(|tag| emits_both_ways(&tag.name));
                if both_ways {
                    let back = part.vertices.iter().map(|v| SourceVertex {
                        position: v.position,
                        normal: v.normal.map(|n| -n),
                        binormal: v.binormal,
                        tangent: v.tangent,
                        texcoord: v.texcoord,
                    });
                    let indices: Vec<_> = part.indices.chunks_exact(3).flat_map(|t| [t[0], t[2], t[1]]).collect();
                    out.push((assets.meshes.add(build_model_mesh(back, &indices, shader.uv_scale)), surface.clone(), indices.len() / 3));
                }
                out.push((mesh, surface, part.indices.len() / 3));
            }
        }
        Ok(out)
    }

    /// An actively camouflaged object's liquid glass: clear but for a cool
    /// tinge, smooth, as thick as a limb (Solarik bends the picture behind
    /// it through that depth); the first person's thin.
    fn camouflage_glass(&mut self, assets: &mut LevelAssets) -> Handle<StandardMaterial> {
        let thickness = if self.first_person { FIRST_PERSON_CAMOUFLAGE_THICKNESS } else { CAMOUFLAGE_THICKNESS };
        self.camouflage_glass[self.first_person as usize]
            .get_or_insert_with(|| {
                assets.materials.add(StandardMaterial {
                    base_color: Color::srgb(0.93, 0.97, 1.0),
                    perceptual_roughness: 0.05,
                    reflectance: 0.5,
                    specular_transmission: 1.0,
                    thickness,
                    alpha_mode: AlphaMode::Blend,
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                })
            })
            .clone()
    }

    /// A piece's surface with the object's change colour, where its shader
    /// takes one (made once per shader and colour).
    /// An object's meshes in new change colours, their materials swapped in
    /// place: remaking the model left it missing a frame and lost its
    /// motion's history, and the plasma rifle's colours follow its heat.
    pub fn recolour<'a>(
        &mut self,
        commands: &mut Commands,
        entities: impl Iterator<Item = (Entity, &'a Recolourable)>,
        change_colors: &[[f32; 3]; 4],
        animation: Option<&AnimatedSelfIllumination>,
        assets: &mut LevelAssets,
    ) {
        for (entity, recolourable) in entities {
            let Surface::Solid(solid) = self.coloured_surface(&recolourable.base, recolourable.shader, change_colors, assets) else {
                continue;
            };
            // (a piece whose self-illumination animates keeps the object's
            // own copy of its material, given the coloured base map)
            if let Some(animation) = animation
                && let Some(index) = recolourable.shader.and_then(|shader| animation.shaders.iter().position(|s| *s == shader))
            {
                let (standard, raster, _) = &animation.materials[index];
                let texture = assets.materials.get(&solid.standard).and_then(|m| m.base_color_texture.clone());
                if let Some(texture) = texture {
                    if assets.materials.get(standard).is_some_and(|m| m.base_color_texture.as_ref() != Some(&texture))
                        && let Some(mut material) = assets.materials.get_mut(standard)
                    {
                        material.base_color_texture = Some(texture.clone());
                    }
                    if let Some(raster) = raster
                        && assets.environment_materials.get(raster).is_some_and(|m| m.extension.base_map != texture)
                        && let Some(mut material) = assets.environment_materials.get_mut(raster)
                    {
                        material.base.base_color_texture = Some(texture.clone());
                        material.extension.base_map = texture;
                    }
                }
                continue;
            }
            match (&solid.raster, recolourable.traced) {
                (Some(raster), false) => commands.entity(entity).insert(MeshMaterial3d(raster.clone())),
                _ => commands.entity(entity).insert(MeshMaterial3d(solid.standard.clone())),
            };
        }
    }

    fn coloured_surface(
        &mut self,
        base: &Surface,
        shader: Option<ShaderUse>,
        change_colors: &[[f32; 3]; 4],
        assets: &mut LevelAssets,
    ) -> Surface {
        let Surface::Solid(solid) = base else { return base.clone() };
        let Some(shader) = shader else { return base.clone() };
        let info = shader::shader_info(&self.cache, TagRef { group: 0, datum: Some(shader.datum) }).ok().flatten();
        let Some((source, multipurpose)) =
            info.as_ref().and_then(|i| Some((i.change_color?, i.multipurpose_map.and_then(|m| m.datum)?)))
        else {
            return base.clone();
        };
        let color = change_colors[source].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        // (Chief's visor is its own colour, not its base map's; a tuning
        // colour, `painted`, is already the base's)
        if color == [255; 3] || info.as_ref().is_some_and(|i| painted(&i.name).is_some()) || info.as_ref().is_some_and(|i| i.name.eq_ignore_ascii_case(VISOR_SHADER)) {
            return base.clone();
        }
        if let Some(surface) = self.coloured_surfaces.get(&(shader, color)) {
            return surface.clone();
        }
        let bake = crate::textures::Bake::ChangeColor {
            base: info.as_ref().and_then(|i| i.base_map).and_then(|b| b.datum),
            multipurpose,
            color,
        };
        let surface = match (self.textures.baked(&self.cache, bake, shader.permutation, assets.images), assets.materials.get(&solid.standard)) {
            (Some(texture), Some(material)) => {
                let material = StandardMaterial { base_color_texture: Some(texture.clone()), ..material.clone() };
                // (and the camera's, a model's detail, over the coloured base)
                let raster = solid.raster.as_ref().and_then(|raster| assets.environment_materials.get(raster).cloned()).map(|mut raster| {
                    raster.base.base_color_texture = Some(texture.clone());
                    raster.extension.base_map = texture;
                    assets.environment_materials.add(raster)
                });
                let standard = assets.materials.add(material);
                if info.as_ref().is_some_and(|i| pearl_coat(&i.name)) && let Some(pearl) = assets.pearl_coats.as_deref_mut() {
                    pearl.0.insert(standard.id());
                }
                if info.as_ref().is_some_and(|i| plain_coat(&i.name)) && let Some(plain) = assets.plain_coats.as_deref_mut() {
                    plain.0.insert(standard.id());
                }
                Surface::Solid(SolidSurface { standard, raster: raster.or(solid.raster.clone()), ..solid.clone() })
            }
            _ => base.clone(),
        };
        debug!("change colour {color:?} on {}", info.as_ref().map_or("", |i| i.name.as_str()));
        self.coloured_surfaces.insert((shader, color), surface.clone());
        surface
    }

    /// The material for a shader, made once per shader and permutation.
    fn surface(
        &mut self,
        shader: TagRef,
        permutation: u16,
        assets: &mut LevelAssets,
    ) -> ShaderSurface {
        if let Some(surface) = self.surfaces.get(&(shader.datum, permutation)) {
            return surface.clone();
        }
        self.permutation = permutation;
        let info = match shader::shader_info(&self.cache, shader) {
            Ok(info) => info,
            Err(e) => {
                warn!("shader {:?}: {e}", shader.datum);
                None
            }
        };
        let surface = ShaderSurface {
            surface: self.make_surface(info.as_ref(), assets),
            uv_scale: info.as_ref().map_or([1.0, 1.0], |i| i.uv_scale),
        };
        if permutation != 0 {
            debug!("shader {} at permutation {permutation}", info.as_ref().map_or("-", |i| i.name.as_str()));
        }
        self.permutation = 0;
        self.surfaces.insert((shader.datum, permutation), surface.clone());
        surface
    }

    fn make_surface(&mut self, info: Option<&ShaderInfo>, assets: &mut LevelAssets) -> Option<Surface> {
        // (HALO_RT_SKIP_SHADERS, debugging: shaders whose names hold any of
        // these comma-separated words, not drawn)
        if let (Some(info), Ok(skip)) = (info, std::env::var("HALO_RT_SKIP_SHADERS"))
            && skip.split(',').any(|word| !word.is_empty() && info.name.contains(word))
        {
            return None;
        }
        let Some(info) = info else {
            let standard = assets.materials.add(StandardMaterial {
                base_color: Color::srgb(0.5, 0.5, 0.5),
                perceptual_roughness: 0.9,
                ..default()
            });
            return Some(Surface::Solid(SolidSurface { standard, raster: None, radiance: None, light_gain: 1.0, seen_standard: None, unseen: false, seen_as: None, emitter_map: None }));
        };
        let base_color_texture = info
            .base_map
            .and_then(|r| r.datum)
            .and_then(|datum| self.textures.permuted(&self.cache, datum, self.permutation, true, assets.images));
        let radiance = Vec3::from(info.emitted_light) * info.emitted_power;
        let emits = info.emitted_power > 0.0 && radiance != Vec3::ZERO;
        // (or a glowing see-through surface's light: see GLOW_LIGHT)
        let glow = if emits { None } else { self.glow_radiance(info) };
        let (radiance, emits) = glow.map_or((radiance, emits), |glow| (glow, true));
        // (some restyled: see EMITTER_LOOKS)
        let radiance = emitter_look(&info.name).map_or(radiance, |(gain, tint)| {
            let tinted = tint.map_or(radiance, |tint| Vec3::from(tint) * radiance.dot(Vec3::new(0.2126, 0.7152, 0.0722)));
            tinted * gain
        });

        if info.shader_type.is_transparent() && !emits {
            if let Some(material) = self.glass_or_water_material(info, assets) {
                return Some(Surface::Blended(material));
            }
            return self.transparent_material(info, 0.0, false, assets).map(Surface::Transparent);
        }
        // (a transparent radiosity emitter's light in its card's own pattern:
        // see emitter_map)
        let emitter_map = if emits && glow.is_none() && info.shader_type.is_transparent() { self.emitter_map(info, assets) } else { None };
        let self_illumination = info.self_illumination.and_then(|si| match si {
            shader::SelfIllumination::Model { multipurpose_map, color_upper, .. } => {
                let bake = crate::textures::Bake::ModelSelfIllumination {
                    base: info.base_map.and_then(|b| b.datum),
                    multipurpose: multipurpose_map.datum?,
                };
                Some((self.textures.baked(&self.cache, bake, self.permutation, assets.images)?, Vec3::from(color_upper)))
            }
            shader::SelfIllumination::Environment { map, primary, secondary, plasma } => {
                let bake = crate::textures::Bake::EnvironmentSelfIllumination {
                    base: info.base_map.and_then(|b| b.datum),
                    map: map.datum?,
                    colors: [primary, secondary, plasma].map(|c| c.map(f32::to_bits)),
                };
                Some((self.textures.baked(&self.cache, bake, self.permutation, assets.images)?, Vec3::ONE))
            }
        });
        let shine = self.shininess(info, assets);
        let material = StandardMaterial {
            base_color: if base_color_texture.is_some() || !emits {
                Color::linear_rgb(shine.paint, shine.paint, shine.paint)
            } else {
                Color::srgb(0.2, 0.2, 0.2)
            },
            base_color_texture: shine.base_color_texture.clone().or_else(|| base_color_texture.clone()),
            perceptual_roughness: shine.roughness,
            metallic: shine.metallic,
            metallic_roughness_texture: shine.texture,
            reflectance: shine.reflectance,
            // A surface that lit the lightmaps (radiosity emission) keeps that
            // as its brightness, the real light output; self-illumination
            // otherwise.
            emissive_texture: match &self_illumination {
                Some((texture, _)) => Some(texture.clone()),
                None if emits && info.shader_type == ShaderType::Environment => base_color_texture.clone(),
                None if emitter_map.is_some() => emitter_map.as_ref().map(|(_, image)| image.clone()),
                None => None,
            },
            ..default()
        };
        let radiance = match &self_illumination {
            _ if emits => Some(radiance),
            Some((_, color)) => Some(*color * SELF_ILLUMINATION_RADIANCE),
            None => None,
        };
        // the ray-traced surface gets the bump map too, where it tiles as the
        // base map does (StandardMaterial has one set of texture coordinates)
        let mut material = material;
        // Chief's visor: a gold mirror of what is round it (its base map is
        // black, which as a metal reflects nothing, with a reflection
        // painted on in grey)
        // the Elites' armour: a clear coat over it, where its reflection mask
        // says (the baked mask's red channel; skin and joints have none)
        // (tuning: `HALO_RT_ELITE_COLOR=r,g,b` paints the Elites' armour
        // its change colour, as the game does a live Elite, here where the
        // level's placements have none; `HALO_RT_CHIEF_COLOR` Chief's)
        if let Some(color) = painted(&info.name) && let Some(multipurpose) = info.multipurpose_map.and_then(|m| m.datum)
            && info.change_color.is_some()
        {
            let bake = crate::textures::Bake::ChangeColor { base: info.base_map.and_then(|b| b.datum), multipurpose, color };
            if let Some(texture) = self.textures.baked(&self.cache, bake, self.permutation, assets.images) {
                material.base_color_texture = Some(texture);
            }
        }
        if let Some((strength, roughness)) = clear_coat(&info.name) {
            material.clearcoat = strength;
            material.clearcoat_perceptual_roughness = roughness;
            material.clearcoat_texture = material.metallic_roughness_texture.clone();
            // (the eyes' all over)
            if human_eyes(&info.name) {
                material.clearcoat_texture = None;
            }
            // Chief's only on his armour plates: the multipurpose map's blue,
            // the change colour's mask (its red, the reflection mask, takes
            // in his black suit too), baked into the red the coat reads; the
            // Hunters' only on their worms, the same blue
            if (CHIEF_ARMOUR.iter().any(|name| info.name.eq_ignore_ascii_case(name)) || hunter_worms(&info.name))
                && let Some(multipurpose) = info.multipurpose_map.and_then(|m| m.datum)
            {
                let bake = crate::textures::Bake::MetallicRoughness { mask: multipurpose, channel: 2, unmasked: 0, roughness: 0, metallic: 0, cloth: None, curve: None };
                material.clearcoat_texture = self.textures.baked(&self.cache, bake, self.permutation, assets.images);
            }
        }
        let base_color_texture = if info.name.eq_ignore_ascii_case(VISOR_SHADER) {
            material.base_color = VISOR_COLOR;
            material.base_color_texture = None;
            material.metallic = 1.0;
            material.perceptual_roughness = VISOR_ROUGHNESS;
            material.metallic_roughness_texture = None;
            None
        } else {
            // (painted, when a tuning colour is)
            material.base_color_texture.clone()
        };
        // a model shader's cut-out (the game's alpha test, reference 0x7f, on
        // the base map's alpha: leaves, grilles) and its two sides; an
        // alpha-blended decal cut out the same way, its shape kept (a50's
        // doors' symbols, drawn opaque, were blue squares)
        if info.environment.is_none() && (info.alpha_tested || info.alpha_blended_decal) && base_color_texture.is_some() {
            material.alpha_mode = AlphaMode::Mask(0.5);
        }
        if info.two_sided {
            material.double_sided = true;
            material.cull_mode = None;
            // leaves let light through: Solarik's thin foliage, for a
            // two-sided cut-out of a plant (not a grille); deferred, which
            // Bevy's automatic choice isn't for transmission
            if material.alpha_mode == AlphaMode::Mask(0.5) && is_foliage(&info.name) {
                material.diffuse_transmission = FOLIAGE_TRANSMISSION;
                material.opaque_render_method = bevy::material::OpaqueRendererMethod::Deferred;
            }
        }
        if let Some(environment) = &info.environment {
            let base_size = info.base_map.and_then(|b| b.datum).and_then(|d| bitmap_size(&self.cache, d));
            if let Some((normal_map, scale)) = self.bump_map(environment, base_size, assets.images)
                && scale.abs_diff_eq(Vec2::ONE, 1e-3)
            {
                material.normal_map_texture = Some(normal_map);
            }
        }
        let raster = match (&info.environment, &base_color_texture) {
            (Some(environment), Some(base_map)) => {
                Some(self.environment_material(environment, base_map.clone(), &material, info, assets))
            }
            // (a model's detail map: the camera's; the rays keep the base)
            // (and the plain coats', with no detail, for its G-buffer flag:
            // the human eyes', the Hunters' worms')
            (None, Some(base_map)) if info.model_detail.is_some() || plain_coat(&info.name) => {
                self.model_detail_material(base_map.clone(), &material, info, assets)
            }
            _ => None,
        };
        // an alpha-tested shader_environment's cut-out (the game's alpha
        // test on its bump map's alpha: grates, grilles) for the rays too,
        // their base colour's alpha at the bump map's tiling: they were
        // solid sheets, and no light came through them (play feedback)
        if let (Some(environment), true, Some(base)) = (&info.environment, info.alpha_tested, info.base_map.and_then(|b| b.datum))
            && let Some(map) = environment.bump_map
            && let Some(bump) = map.bitmap.datum
        {
            let rescale = match (environment.rescale_bump_map, bitmap_size(&self.cache, base), bitmap_size(&self.cache, bump)) {
                (true, Some(base), Some(bump)) => base / bump,
                _ => Vec2::ONE,
            };
            let scale = (rescale * map.scale).to_array().map(f32::to_bits);
            if let Some(texture) = self.textures.baked(&self.cache, crate::textures::Bake::CutOut { base, bump, scale }, self.permutation, assets.images) {
                material.base_color_texture = Some(texture);
                material.alpha_mode = AlphaMode::Mask(0.5);
            }
        }
        let unseen = info.shader_type.is_transparent();
        // (and the rays don't see it either: masked out everywhere, every
        // ray passes through it, so nothing reflects it and it shadows
        // nothing, while Solarik's light sampling still takes its emission,
        // which a mask doesn't scale)
        if unseen {
            material.base_color = material.base_color.with_alpha(0.0);
            material.alpha_mode = AlphaMode::Mask(0.5);
        }
        let seen_as = if unseen { self.transparent_material(info, 0.0, false, assets) } else { None };
        let light_gain = emitter_light_gain(&info.name);
        let seen_standard =
            (light_gain != 1.0 && raster.is_none() && radiance.is_some()).then(|| assets.materials.add(material.clone()));
        let standard = assets.materials.add(material);
        if pearl_coat(&info.name) && let Some(pearl) = assets.pearl_coats.as_deref_mut() {
            pearl.0.insert(standard.id());
        }
        if plain_coat(&info.name) && let Some(plain) = assets.plain_coats.as_deref_mut() {
            plain.0.insert(standard.id());
        }
        Some(Surface::Solid(SolidSurface { standard, raster, radiance, light_gain, seen_standard, unseen, seen_as, emitter_map: emitter_map.map(|(datum, _)| datum) }))
    }

    /// A `shader_model`'s camera material with its detail map (see
    /// `shader::ModelDetail` and `halo_environment.wesl`'s model mode), over
    /// `standard`; None if its detail map can't be had. A shader without one
    /// gets the neutral detail at no gain, which leaves its base map as it is
    /// (the plain coats', for their G-buffer flag).
    fn model_detail_material(
        &mut self,
        base_map: Handle<Image>,
        standard: &StandardMaterial,
        info: &ShaderInfo,
        assets: &mut LevelAssets,
    ) -> Option<Handle<HaloEnvironmentMaterial>> {
        let (detail, detail_map, gain) = match info.model_detail {
            Some(detail) => {
                let map = self.textures.permuted(&self.cache, detail.map.datum?, self.permutation, true, assets.images)?;
                (detail, map, model_detail_gain(&info.name))
            }
            None => {
                let none = halo_map::shader::ModelDetail { map: TagRef { group: 0, datum: None }, function: 0, mask: 0, scale: [1.0, 1.0] };
                (none, assets.neutral_detail.clone(), 0.0)
            }
        };
        // (the mask, read as it is stored)
        let multipurpose = info
            .multipurpose_map
            .and_then(|m| m.datum)
            .and_then(|datum| self.textures.permuted(&self.cache, datum, self.permutation, false, assets.images))
            .unwrap_or_else(|| assets.neutral_detail.clone());
        // (the mesh's texture coordinates are the base map's, already scaled by
        // the shader's map scale; the detail's scale is against the model's own)
        let scale = Vec2::from(detail.scale) / Vec2::from(info.uv_scale).max(Vec2::splat(1e-4));
        Some(assets.environment_materials.add(HaloEnvironmentMaterial {
            base: standard.clone(),
            extension: HaloEnvironment {
                params: HaloEnvironmentParams {
                    detail_scales: scale.extend(1.0).extend(1.0),
                    // (x: the detail's gain, see MODEL_DETAIL_GAINS)
                    micro_scale: Vec4::new(gain, 1.0, 1.0, 1.0),
                    modes: UVec4::new(
                        detail.mask as u32,
                        detail.function as u32,
                        0,
                        crate::material::MODEL_DETAIL
                            | if pearl_coat(&info.name) { crate::material::PEARL_COAT } else { 0 }
                            | if plain_coat(&info.name) { crate::material::PLAIN_COAT } else { 0 },
                    ),
                    ..default()
                },
                base_map,
                primary_detail_map: detail_map,
                secondary_detail_map: multipurpose,
                micro_detail_map: assets.neutral_detail.clone(),
                bump_map: assets.flat_normal.clone(),
                self_illumination_map: assets.flat_normal.clone(),
                periodic_tables: crate::material::PERIODIC_TABLES.get().cloned().unwrap_or_default(),
            },
        }))
    }

    /// Glass and water as smooth, thin panes: Halo draws them in several
    /// passes (tint, reflection, ripples); this keeps their colour as the
    /// tint of what is seen through them, and a reflection. Solarik's thin
    /// glass is a blended material with full specular transmission; its
    /// alpha is coverage (zero a hole), so a pane is fully covered and
    /// tinted, and reflects by Fresnel. A glass's diffuse map (grime and
    /// smudges, which Halo blends over it, lit) is Solarik's grime layer
    /// over the pane, its texture with the tint its colour. Its
    /// head-on reflection's strength (colour times alpha, added over the
    /// pane in Halo) raises its reflectance: half of it as the reflectance
    /// at normal incidence, from plain glass's up to 0.25. (b30's glass
    /// floor, a30's too: 0.75 of a pale blue, read 0.18.)
    fn glass_or_water_material(&mut self, info: &ShaderInfo, assets: &mut LevelAssets) -> Option<Handle<StandardMaterial>> {
        let texture = |map: Option<TagRef>, this: &mut Self, assets: &mut LevelAssets| {
            map.and_then(|m| m.datum).and_then(|d| this.textures.permuted(&this.cache, d, this.permutation, true, assets.images))
        };
        let material = if let Some(glass) = info.glass {
            let [r, g, b] = glass.tint;
            let layered = glass.tint_map.is_none() && glass.diffuse_map.and_then(|m| m.datum).is_some();
            let base_color_texture = texture(if layered { glass.diffuse_map } else { glass.tint_map }, self, assets);
            let [pr, pg, pb, pa] = glass.reflection_perpendicular;
            let head_on = (0.5 * pa * (0.2126 * pr + 0.7152 * pg + 0.0722 * pb)).clamp(PLAIN_GLASS_F0, 0.25);
            let material = StandardMaterial {
                base_color: Color::srgba(r, g, b, 1.0),
                base_color_texture,
                perceptual_roughness: 0.05,
                // (Solarik's thin glass: F0 = 0.16 reflectance squared)
                reflectance: (head_on / 0.16).sqrt(),
                specular_transmission: 1.0,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                double_sided: true,
                ..default()
            };
            (material, layered)
        } else {
            let water = info.water?;
            let [r, g, b, _] = water.perpendicular_tint;
            let material = assets.materials.add(StandardMaterial {
                base_color: Color::srgba(r, g, b, 1.0),
                perceptual_roughness: 0.08,
                reflectance: 0.5,
                specular_transmission: 1.0,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                double_sided: true,
                ..default()
            });
            // (its ripples, which Solarik bends the pane's normal by;
            // HALO_RT_NO_WATER_RIPPLES=1 leaves it flat)
            let (ripples, drifts, present) = crate::material::water_ripple_terms(&water);
            let present = present && !std::env::var("HALO_RT_NO_WATER_RIPPLES").is_ok_and(|v| v == "1");
            if present && let Some(registry) = assets.water_ripples.as_deref_mut() {
                let mut map = |index: usize| {
                    water
                        .ripple_maps
                        .and_then(|r| r.datum)
                        .filter(|_| index < water.ripple_count)
                        .and_then(|datum| self.textures.get(&self.cache, datum, water.ripples[index].map_index.max(0) as usize, false, assets.images))
                        .unwrap_or_else(|| assets.flat_normal.clone())
                };
                let maps = [map(0), map(1), map(2), map(3)];
                registry.0.insert(
                    material.id(),
                    bevy_solarik::water_ripples::WaterRipples {
                        maps,
                        scale: water.ripple_scale,
                        drift: crate::material::water_ripple_drift(&water),
                        ripples,
                        drifts,
                        levels: water.ripple_mipmap_levels.clamp(1, 4) as f32,
                        fade: water.ripple_mipmap_fade_factor,
                        lod_bias: water.ripple_mipmap_lod_bias,
                        size: crate::material::WATER_BUMPMAP_SIZE,
                    },
                );
            }
            return Some(material);
        };
        let (material, layered) = material;
        let material = assets.materials.add(material);
        if layered && let Some(registry) = assets.glass_grime.as_deref_mut() {
            registry.0.insert(material.id());
        }
        Some(material)
    }

    /// A transparent chicago or generic shader's combiner material: its maps
    /// in stage order, their texture transforms, and Bevy's nearest blend
    /// mode to the shader's framebuffer blend function.
    /// (`sky`: a sky's layer, seen from inside, from both sides)
    fn transparent_material(
        &mut self,
        info: &ShaderInfo,
        depth_bias: f32,
        sky: bool,
        assets: &mut LevelAssets,
    ) -> Option<Handle<HaloTransparentMaterial>> {
        use shader::BlendFunction;
        if let Some(meter) = info.meter {
            return self.meter_material(info, meter, assets);
        }
        let transparent = info.transparent.as_ref()?;
        let shader_ref = TagRef { group: info.group, datum: Some(info.datum) };
        let program = match halo_map::combiner::program(&self.cache, shader_ref) {
            Ok(program) => program?,
            Err(e) => {
                warn!("shader {}: {e}", info.name);
                return None;
            }
        };
        let mut params = HaloCombinerParams::from_program(&program);
        let mut maps: [Handle<Image>; 4] = std::array::from_fn(|_| assets.neutral_detail.clone());
        let mut present = 0;
        // a cube map first stage (reflection, object or viewer centred): its
        // cube map, sampled by the direction its type takes (a10's command
        // displays: the screens, which its cube map holds)
        let mut cube = None;
        for (stage, slot) in transparent.maps.iter().enumerate().take(4) {
            if stage == 0 && transparent.first_map_type != 0 {
                if let Some(image) = slot.as_ref().and_then(|map| map.bitmap.datum).and_then(|datum| crate::material::cube_map_image(&self.cache, datum)) {
                    cube = Some(assets.images.add(image));
                    present |= 1;
                }
                continue;
            }
            let Some(map) = slot else { continue };
            let Some(image) = map.bitmap.datum.and_then(|d| self.textures.permuted(&self.cache, d, self.permutation, true, assets.images))
            else {
                continue;
            };
            maps[stage] = image;
            present |= 1 << stage;
        }
        let blend = match transparent.blend {
            BlendFunction::AlphaBlend => 0,
            BlendFunction::Multiply => 1,
            BlendFunction::DoubleMultiply => 2,
            BlendFunction::Add => 3,
            BlendFunction::Subtract => 4,
            BlendFunction::ComponentMin => 5,
            BlendFunction::ComponentMax => 6,
            BlendFunction::AlphaMultiplyAdd => 7,
        };
        params.modes = UVec4::new(blend, present, transparent.flags as u32, transparent.fade_mode.clamp(0, 2) as u32);
        // (HALO_RT_TRANSPARENT_DEBUG=1: each in magenta, to see where they are)
        if std::env::var("HALO_RT_TRANSPARENT_DEBUG").is_ok_and(|v| v == "1") {
            params.fade.y = 1.0;
        }
        let [map0, map1, map2, map3] = maps;
        let mut material = HaloTransparentMaterial {
            params,
            map0,
            map1,
            map2,
            map3,
            alpha_mode: AlphaMode::Blend,
            depth_bias,
            blend: blend as u16,
            one_sided: !sky && transparent.flags & shader::TransparentInfo::TWO_SIDED == 0,
            decal: !sky && transparent.flags & shader::TransparentInfo::DECAL != 0,
            sky,
            animation: Some(std::sync::Arc::new(transparent.clone())),
            object: None,
            numeric: None,
            meter: None,
            plasma: None,
            noise0: crate::material::neutral_volume(),
            noise1: crate::material::neutral_volume(),
            cube0: crate::material::neutral_cube(),
            fog: if sky { crate::fog::no_fog_layers() } else { crate::fog::fog_layers() },
            slot: crate::combiners::NO_SLOT,
            combiners: crate::combiners::combiners(),
        };
        if let Some(cube) = cube {
            material.cube0 = cube;
            material.params.fade.z = f32::from(transparent.first_map_type);
        }
        // a numeric counter's digit: its first map's every frame
        if transparent.flags & shader::TransparentInfo::NUMERIC != 0
            && let Some(Some(map)) = transparent.maps.first()
            && let Some(datum) = map.bitmap.datum
        {
            let count = halo_map::bitmap::bitmaps(&self.cache, datum).map_or(0, |b| b.len());
            let frames: Vec<_> =
                (0..count).filter_map(|index| self.textures.get(&self.cache, datum, index, true, assets.images)).collect();
            if frames.len() == count && count > 0 {
                material.numeric = Some(std::sync::Arc::new(crate::material::NumericCounter {
                    frames,
                    digit: self.permutation,
                    limit: transparent.numeric_counter_limit,
                }));
            }
        }
        // its maps' transforms (and stages' colours) as at the start; kept
        // for the animation only where they change
        let periodic = periodic_function;
        if let Some(params) = material.animate(None, None, 0.0, &periodic) {
            material.params = params;
        }
        if !transparent.animated() && !transparent.uses_object() {
            material.animation = None;
        }
        let handle = assets.transparent_materials.add(material);
        if transparent.flags & shader::TransparentInfo::IGNORE_EFFECT != 0 {
            self.ignores_effect.insert(handle.id());
        }
        Some(handle)
    }

    /// A plasma shader's material (a shield's energy, its object's modifier
    /// shader): its combiner program over its two noise maps; drawn from both
    /// sides, added by its alpha.
    pub fn plasma_material(&mut self, shader: TagRef, assets: &mut LevelAssets) -> Option<Handle<HaloTransparentMaterial>> {
        let info = shader::shader_info(&self.cache, shader).ok().flatten()?;
        let plasma = info.plasma?;
        let program = halo_map::combiner::plasma();
        let mut params = HaloCombinerParams::from_program(&program);
        let noise0 = plasma.noise[0].map.datum.and_then(|d| self.textures.volume(&self.cache, d, assets.images))?;
        let noise1 = plasma.noise[1].map.datum.and_then(|d| self.textures.volume(&self.cache, d, assets.images))?;
        params.modes = UVec4::new(10, 0, 1 << 9, 0);
        if std::env::var("HALO_RT_TRANSPARENT_DEBUG").is_ok_and(|v| v == "1") {
            params.fade.y = 1.0;
        }
        let mut material = HaloTransparentMaterial {
            params,
            map0: assets.neutral_detail.clone(),
            map1: assets.neutral_detail.clone(),
            map2: assets.neutral_detail.clone(),
            map3: assets.neutral_detail.clone(),
            noise0,
            noise1,
            cube0: crate::material::neutral_cube(),
            fog: crate::fog::fog_layers(),
            alpha_mode: AlphaMode::Blend,
            depth_bias: 0.0,
            blend: 10,
            one_sided: false,
            decal: false,
            sky: false,
            animation: None,
            object: None,
            numeric: None,
            meter: None,
            plasma: Some(std::sync::Arc::new(plasma)),
            slot: crate::combiners::NO_SLOT,
            combiners: crate::combiners::combiners(),
        };
        if let Some(params) = material.animate(None, None, 0.0, &periodic_function) {
            material.params = params;
        }
        Some(assets.transparent_materials.add(material))
    }

    /// A meter's material: its combiner program, its constants (as for a
    /// full meter until its object's functions say), its blend by its tint
    /// mode.
    fn meter_material(
        &mut self,
        info: &ShaderInfo,
        meter: shader::MeterInfo,
        assets: &mut LevelAssets,
    ) -> Option<Handle<HaloTransparentMaterial>> {
        let program = halo_map::combiner::meter(meter.flags & shader::MeterInfo::FLASH_COLOR_IS_NEGATIVE != 0);
        let mut params = HaloCombinerParams::from_program(&program);
        let map = meter.map.datum.and_then(|d| self.textures.permuted(&self.cache, d, self.permutation, true, assets.images))?;
        let blend: u16 = if meter.flags & shader::MeterInfo::TINT_MODE_2 != 0 { 9 } else { 8 };
        // (alpha kill: a map texel of zero alpha isn't drawn)
        params.modes = UVec4::new(blend as u32, 1, 1 << 8, 0);
        if std::env::var("HALO_RT_TRANSPARENT_DEBUG").is_ok_and(|v| v == "1") {
            params.fade.y = 1.0;
        }
        let mut material = HaloTransparentMaterial {
            params,
            map0: map,
            map1: assets.neutral_detail.clone(),
            map2: assets.neutral_detail.clone(),
            map3: assets.neutral_detail.clone(),
            alpha_mode: AlphaMode::Blend,
            depth_bias: 0.0,
            blend,
            one_sided: meter.flags & shader::MeterInfo::TWO_SIDED == 0,
            decal: false,
            sky: false,
            animation: None,
            object: None,
            numeric: None,
            meter: Some(std::sync::Arc::new(meter)),
            plasma: None,
            noise0: crate::material::neutral_volume(),
            noise1: crate::material::neutral_volume(),
            cube0: crate::material::neutral_cube(),
            fog: crate::fog::fog_layers(),
            slot: crate::combiners::NO_SLOT,
            combiners: crate::combiners::combiners(),
        };
        let periodic = periodic_function;
        if let Some(params) = material.animate(None, None, 0.0, &periodic) {
            material.params = params;
        }
        let _ = info;
        Some(assets.transparent_materials.add(material))
    }

    /// Roughness and metalness from Halo's reflection settings. Halo has no
    /// roughness: a surface reflects its cube map, scaled by a brightness and
    /// a specular mask. Brighter reflections become smoother and more metallic
    /// surfaces; the mask (when the shader has one) becomes a texture.
    fn shininess(&mut self, info: &ShaderInfo, assets: &mut LevelAssets) -> Shininess {
        use crate::textures::{Bake, MATTE_ROUGHNESS};
        use shader::SpecularMask;
        let matte = Shininess { roughness: MATTE_ROUGHNESS, metallic: 0.0, reflectance: MATTE_REFLECTANCE, texture: None, base_color_texture: None, paint: 1.0 };
        let Some(reflection) = info.reflection.filter(|_| !self.matte) else { return matte };
        let r = reflection.perpendicular_brightness.max(reflection.parallel_brightness).clamp(0.0, 1.0);
        if r < 0.02 || reflection.cube_map.is_none() {
            return matte;
        }
        // Models (armour, weapons, vehicles) gloss the most: Halo gives them
        // a bright cube map whatever lies round them; here they reflect the
        // level itself, near mirror-like, tinted by their colour where their
        // mask says (metallic, which the mask scales; reflectance is the
        // whole material's, and raised it hazed the matte parts like dust).
        let model = info.shader_type == ShaderType::Model;
        let roughness = if model { 0.4 - 0.34 * r.sqrt() } else { 0.8 - 0.65 * r.sqrt() };
        let metallic = if model { 0.9 * r } else { 0.5 * r };
        // (some duller still)
        let mut shine = MODEL_SHINE.iter().find(|(path, _)| model && info.name.starts_with(path)).map_or(1.0, |&(_, shine)| shine);
        // (some painted, not bare metal: their gloss is a dielectric's sheen)
        let mut metal = MODEL_METAL.iter().find(|(path, _)| model && info.name.starts_with(path)).map_or(1.0, |&(_, metal)| metal);
        // (and their sheen and paint: see MODEL_SHEEN)
        let (mut sheen, mut paint) = MODEL_SHEEN.iter().find(|(path, _)| model && info.name.starts_with(path)).map_or((1.0, 1.0), |&(_, v)| (v[0], v[1]));
        // (tuning: `HALO_RT_MODEL_TUNE=<shader path prefix>=shine,metal[,sheen,paint];...`)
        if let Ok(text) = std::env::var("HALO_RT_MODEL_TUNE") {
            for entry in text.split(';') {
                let Some((prefix, values)) = entry.split_once('=') else { continue };
                if !model || prefix.is_empty() || !info.name.starts_with(prefix) {
                    continue;
                }
                let mut values = values.split(',').map(|v| v.trim().parse::<f32>().ok());
                if let Some(Some(value)) = values.next() {
                    shine = value;
                }
                if let Some(Some(value)) = values.next() {
                    metal = value;
                }
                if let Some(Some(value)) = values.next() {
                    sheen = value;
                }
                if let Some(Some(value)) = values.next() {
                    paint = value;
                }
            }
        }
        // (tuning: `HALO_RT_ELITE_SHINE` for the coated Elite armour beneath its coat)
        if elite_armour(&info.name) {
            if let Some(value) = std::env::var("HALO_RT_ELITE_SHINE").ok().and_then(|v| v.parse().ok()) {
                shine = value;
            }
        }
        let dull = |roughness: f32| roughness + (DULL_ROUGHNESS.max(roughness) - roughness) * (1.0 - shine);
        let (roughness, metallic) = (dull(roughness), metallic * shine * metal);
        // a dim reflection's sheen, between matte and a plastic's (see
        // MODEL_UNMASKED_ROUGHNESS)
        let gloss = if model { r * r } else { 1.0 };
        let reflectance = MATTE_REFLECTANCE + (0.5 - MATTE_REFLECTANCE) * gloss * sheen;
        // a human weapon's metal finish, where Halo gives it a bright
        // reflection (not the AR's rubber grip)
        if let Some(finish) = gunmetal(&info.name).filter(|_| model && r >= 0.5)
            && let (Some(base), Some(mask)) = (info.base_map.and_then(|b| b.datum), info.multipurpose_map.and_then(|m| m.datum))
            && reflection.mask == SpecularMask::MultipurposeRed
        {
            let mut bake = |color| self.textures.baked(&self.cache, Bake::Gunmetal { base, mask, color, finish: finish.bits() }, self.permutation, assets.images);
            if let (Some(base_color_texture), Some(texture)) = (bake(true), bake(false)) {
                return Shininess { roughness: 1.0, metallic: 1.0, reflectance: 0.5, texture: Some(texture), base_color_texture: Some(base_color_texture), paint: 1.0 };
            }
        }
        let mask = match reflection.mask {
            // an alpha-tested base map's alpha is its cut-out, not a mask
            SpecularMask::BaseAlpha if !info.alpha_tested => info.base_map.and_then(|b| b.datum).map(|d| (d, 3)),
            SpecularMask::MultipurposeRed => info.multipurpose_map.and_then(|m| m.datum).map(|d| (d, 0)),
            _ => None,
        };
        // Halo's mask dims a reflection, never blurs it: a model's unmasked
        // parts stay semi-glossy (fading them to matte while partly metallic
        // left Covenant crates looking dusty).
        let unmasked = if model { dull(MATTE_ROUGHNESS + (MODEL_UNMASKED_ROUGHNESS - MATTE_ROUGHNESS) * gloss) } else { MATTE_ROUGHNESS };
        let texture = mask.and_then(|(mask, channel)| {
            // the mask reshaped to bring out its pattern (see MASK_CURVES)
            // (tuning: `HALO_RT_MASK_CURVE=<shader path prefix>=power,gain;...`)
            let tuned = std::env::var("HALO_RT_MASK_CURVE").ok().and_then(|text| {
                text.split(';').find_map(|entry| {
                    let (prefix, values) = entry.split_once('=')?;
                    let (power, gain) = values.split_once(',')?;
                    (!prefix.is_empty() && info.name.starts_with(prefix))
                        .then(|| Some([power.trim().parse::<f32>().ok()?, gain.trim().parse::<f32>().ok()?]))?
                })
            });
            let curve = tuned
                .or_else(|| MASK_CURVES.iter().find(|(path, _)| model && info.name.starts_with(path)).map(|&(_, curve)| curve))
                .map(|curve| curve.map(f32::to_bits));
            let bake = Bake::MetallicRoughness {
                curve,
                mask,
                channel,
                unmasked: unmasked.to_bits(),
                roughness: roughness.to_bits(),
                metallic: metallic.to_bits(),
                // Chief's suit, outside his plates (the multipurpose map's
                // blue, the change colour's mask): cloth, not his armour's
                // gloss (his belt, joints and gloves)
                cloth: (channel == 0 && CHIEF_ARMOUR.iter().any(|name| info.name.eq_ignore_ascii_case(name))).then_some((2, CHIEF_SUIT_ROUGHNESS.to_bits())),
            };
            self.textures.baked(&self.cache, bake, self.permutation, assets.images)
        });
        match texture {
            // the texture holds the values; the factors multiply them
            Some(texture) => Shininess { roughness: 1.0, metallic: 1.0, reflectance, texture: Some(texture), base_color_texture: None, paint },
            None => Shininess { roughness, metallic, reflectance, texture: None, base_color_texture: None, paint },
        }
    }

    /// A `shader_environment`'s camera material: its detail layers over
    /// `standard` (see `halo_environment.wesl`).
    fn environment_material(
        &mut self,
        environment: &shader::EnvironmentDiffuse,
        base_map: Handle<Image>,
        standard: &StandardMaterial,
        info: &ShaderInfo,
        assets: &mut LevelAssets,
    ) -> Handle<HaloEnvironmentMaterial> {
        use shader::{DetailFunction, EnvironmentType};
        let base_size = info.base_map.and_then(|b| b.datum).and_then(|d| bitmap_size(&self.cache, d));
        let mut layer = |map: Option<shader::DetailMap>| -> (Handle<Image>, Vec2) {
            let Some(map) = map else { return (assets.neutral_detail.clone(), Vec2::ONE) };
            let Some(datum) = map.bitmap.datum else { return (assets.neutral_detail.clone(), Vec2::ONE) };
            let image = self.textures.permuted(&self.cache, datum, self.permutation, true, assets.images);
            // rescaled detail maps repeat relative to the base map's size
            let rescale = match (environment.rescale_detail_maps, base_size, bitmap_size(&self.cache, datum)) {
                (true, Some(base), Some(detail)) => base / detail,
                _ => Vec2::ONE,
            };
            (image.unwrap_or_else(|| assets.neutral_detail.clone()), rescale * map.scale)
        };
        let (primary, primary_scale) = layer(environment.primary_detail);
        let (secondary, secondary_scale) = layer(environment.secondary_detail);
        let (micro, micro_scale) = layer(environment.micro_detail);
        let bump = self.bump_map(environment, base_size, assets.images);
        let mut flags = 0;
        if bump.is_some() {
            flags |= crate::material::ENVIRONMENT_HAS_BUMP_MAP;
        }
        if info.alpha_tested && bump.is_some() {
            flags |= crate::material::ENVIRONMENT_ALPHA_TESTED;
        }
        let (bump_map, bump_scale) = bump.unwrap_or((assets.flat_normal.clone(), Vec2::ONE));
        let function = |f: DetailFunction| match f {
            DetailFunction::DoubleBiasedMultiply => 0,
            DetailFunction::Multiply => 1,
            DetailFunction::DoubleBiasedAdd => 2,
        };
        // the self-illumination and textures as they animate
        let mut params = HaloEnvironmentParams::default();
        let mut self_illumination_map = assets.flat_normal.clone();
        if let (Some(animation), Some(shader::SelfIllumination::Environment { map, .. })) =
            (info.self_illumination_animation, info.self_illumination)
            && let Some(image) = map.datum.and_then(|datum| self.textures.permuted(&self.cache, datum, self.permutation, false, assets.images))
        {
            self_illumination_map = image;
            flags |= crate::material::ENVIRONMENT_ANIMATED_ILLUMINATION;
            for channel in 0..3 {
                params.illumination_on[channel] = Vec3::from(animation.on_colors[channel]).extend(0.0);
                params.illumination_off[channel] = Vec3::from(animation.off_colors[channel]).extend(0.0);
            }
            let [primary, secondary, plasma] = animation.functions.map(|f| f.max(0) as u32);
            params.illumination_functions = UVec4::new(primary, secondary, plasma, 0);
            params.illumination_periods = Vec3::from(animation.periods).extend(1.0);
            params.illumination_phases = Vec3::from(animation.phases).extend(0.0);
            let band = halo_map::combiner::environment_plasma_band(crate::material::PLASMA_BAND_SAMPLES);
            for (index, samples) in band.chunks(4).enumerate() {
                params.plasma_band[index] = Vec4::from_slice(samples);
            }
        }
        if let Some(animation) = info.texture_animation {
            flags |= crate::material::ENVIRONMENT_TEXTURE_ANIMATION;
            params.texture_functions = UVec4::new(animation.functions[0].max(0) as u32, animation.functions[1].max(0) as u32, 0, 0);
            params.texture_animation = Vec4::new(animation.periods[0], animation.periods[1], animation.scales[0], animation.scales[1]);
        }
        let mut base = standard.clone();
        if flags & crate::material::ENVIRONMENT_ALPHA_TESTED != 0 {
            base.alpha_mode = AlphaMode::Mask(0.5);
        }
        assets.environment_materials.add(HaloEnvironmentMaterial {
            base,
            extension: HaloEnvironment {
                params: HaloEnvironmentParams {
                    detail_scales: primary_scale.extend(secondary_scale.x).extend(secondary_scale.y),
                    micro_scale: micro_scale.extend(bump_scale.x).extend(bump_scale.y),
                    modes: UVec4::new(
                        u32::from(environment.kind != EnvironmentType::Normal),
                        function(environment.detail_function),
                        function(environment.micro_detail_function),
                        flags,
                    ),
                    ..params
                },
                base_map,
                primary_detail_map: primary,
                secondary_detail_map: secondary,
                micro_detail_map: micro,
                bump_map,
                self_illumination_map,
                periodic_tables: crate::material::PERIODIC_TABLES.get().cloned().unwrap_or_default(),
            },
        })
    }

    /// An environment shader's bump map as a linear normal map, and how many
    /// times it repeats across the base map.
    fn bump_map(
        &mut self,
        environment: &shader::EnvironmentDiffuse,
        base_size: Option<Vec2>,
        images: &mut Assets<Image>,
    ) -> Option<(Handle<Image>, Vec2)> {
        let map = environment.bump_map?;
        let datum = map.bitmap.datum?;
        let image = self.textures.permuted(&self.cache, datum, self.permutation, false, images)?;
        let rescale = match (environment.rescale_bump_map, base_size, bitmap_size(&self.cache, datum)) {
            (true, Some(base), Some(bump)) => base / bump,
            _ => Vec2::ONE,
        };
        Some((image, rescale * map.scale))
    }
}

/// StandardMaterial roughness and metalness for a Halo shader.
/// The Elites' armour shaders, which get a clear coat.
const ELITE_ARMOUR: [&str; 6] = [
    "characters\\elite\\shaders\\elite arms",
    "characters\\elite\\shaders\\elite head",
    "characters\\elite\\shaders\\elite heada",
    "characters\\elite\\shaders\\elite legs",
    "characters\\elite\\shaders\\elite legsa",
    "characters\\elite\\shaders\\elite torso",
];
const ELITE_SPECIAL_ARMOUR: &str = "characters\\elite\\elite_special\\shaders\\elite special body";
/// The coat's strength and perceptual roughness (the G-buffer keeps the
/// roughness in thirtieths up to 0.5). A plain coat added little over the
/// Elites' near-mirror metal; as Solarik's thin film (main.rs's `THIN_FILM`)
/// it plays teal and violet over it, as their painted cube maps' swirls do in
/// Halo. (A duller base shows it more: `HALO_RT_ELITE_SHINE=0.4`.)
const ELITE_COAT: (f32, f32) = (1.0, 0.1);

fn elite_armour(shader: &str) -> bool {
    ELITE_ARMOUR.iter().chain([&ELITE_SPECIAL_ARMOUR]).any(|name| shader.eq_ignore_ascii_case(name))
}

/// Chief's armour shaders (his own and his first-person arms').
const CHIEF_ARMOUR: [&str; 2] = ["characters\\cyborg\\shaders\\armor", "characters\\cyborg\\fp\\shaders\\armor hands"];
/// Chief's black suit's roughness (his belt, joints and gloves: the parts of
/// his armour shader outside the change colour's mask), a fabric's.
const CHIEF_SUIT_ROUGHNESS: f32 = 0.85;
/// Chief's armour's coat (strength, perceptual roughness), on his armour
/// plates alone, as strong as the Elites'.
const CHIEF_COAT: (f32, f32) = (1.0, 0.1);

/// The Covenant vehicles' and turrets' shaders (their paths' starts), which
/// get the Elites' coat on their reflection mask's painted panels; not their
/// glowing parts or burned wrecks. `HALO_RT_COVENANT_COAT=strength,roughness`
/// overrides it (0 none).
const COVENANT_VEHICLES: [&str; 5] = [
    "vehicles\\banshee\\",
    "vehicles\\ghost\\",
    "vehicles\\wraith\\",
    "vehicles\\c_dropship\\",
    "vehicles\\c gun turret\\",
];

fn covenant_vehicle(shader: &str) -> bool {
    let shader = shader.to_ascii_lowercase();
    COVENANT_VEHICLES.iter().any(|path| shader.starts_with(path))
        && !["self illuminated", "burned", "destroyed"].iter().any(|part| shader.contains(part))
}

/// Keyes' eye shader (`captain_eyes`: his eyes are meshes of their own),
/// which gets a plain clear coat, a wet eye's gloss
/// (`HALO_RT_EYE_COAT=strength,roughness`, 0 none). The marines' and
/// Johnson's (`eyes_<name>`) have none: their eyes are flat planes, and the
/// coat's reflection lay across them strangely (play feedback).
fn human_eyes(shader: &str) -> bool {
    let shader = shader.to_ascii_lowercase();
    shader.starts_with("characters\\captain") && shader.rsplit('\\').next().is_some_and(|name| name.contains("eyes"))
}
/// The eyes' coat: full strength, near a mirror (the G-buffer's thirtieths:
/// 2/30).
const EYE_COAT: (f32, f32) = (1.0, 0.067);

/// The Hunters' torso shader, whose orange (the multipurpose map's blue: the
/// worm colony showing between its plates, and the spines on its back) gets
/// a plain clear coat, wet (play feedback; `HALO_RT_HUNTER_COAT=strength,
/// roughness`, 0 none).
fn hunter_worms(shader: &str) -> bool {
    shader.eq_ignore_ascii_case("characters\\hunter\\shaders\\hunter torso")
}
/// The worms' coat: full strength, near a mirror (4/30).
const HUNTER_COAT: (f32, f32) = (1.0, 0.133);

/// Whether a shader's coat is plain (no thin film): the human eyes', the
/// Hunters' worms'.
fn plain_coat(shader: &str) -> bool {
    human_eyes(shader) || hunter_worms(shader)
}

/// Whether a shader's coat is Solarik's pearl film (its second thin film,
/// main.rs's `PEARL_FILM`): Chief's armour.
fn pearl_coat(shader: &str) -> bool {
    CHIEF_ARMOUR.iter().any(|name| shader.eq_ignore_ascii_case(name))
}

/// A tuning colour for the Elites' or Chief's armour, in place of an
/// object's change colour (`HALO_RT_ELITE_COLOR`, `HALO_RT_CHIEF_COLOR=r,g,b`).
fn painted(shader: &str) -> Option<[u8; 3]> {
    let variable = if elite_armour(shader) {
        "HALO_RT_ELITE_COLOR"
    } else if CHIEF_ARMOUR.iter().any(|name| shader.eq_ignore_ascii_case(name)) {
        "HALO_RT_CHIEF_COLOR"
    } else {
        return None;
    };
    let c: Vec<f32> = std::env::var(variable).ok()?.split(',').filter_map(|v| v.trim().parse().ok()).collect();
    (c.len() == 3).then(|| [c[0], c[1], c[2]].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
}

/// A shader's clear coat, if it has one: the Elites' and Chief's armour, the
/// Covenant vehicles' panels, the human eyes and the Hunters' worms.
/// `HALO_RT_ELITE_COAT` and `HALO_RT_CHIEF_COAT=strength,roughness` override
/// them (0 none).
fn clear_coat(shader: &str) -> Option<(f32, f32)> {
    let (default, variable) = if elite_armour(shader) {
        (ELITE_COAT, "HALO_RT_ELITE_COAT")
    } else if CHIEF_ARMOUR.iter().any(|name| shader.eq_ignore_ascii_case(name)) {
        (CHIEF_COAT, "HALO_RT_CHIEF_COAT")
    } else if covenant_vehicle(shader) {
        (ELITE_COAT, "HALO_RT_COVENANT_COAT")
    } else if human_eyes(shader) {
        (EYE_COAT, "HALO_RT_EYE_COAT")
    } else if hunter_worms(shader) {
        (HUNTER_COAT, "HALO_RT_HUNTER_COAT")
    } else {
        return None;
    };
    let coat = std::env::var(variable).ok().and_then(|text| {
        let mut values = text.split(',').map(|v| v.trim().parse::<f32>().ok());
        Some((values.next()??, values.next().flatten().unwrap_or(default.1)))
    });
    Some(coat.unwrap_or(default)).filter(|&(strength, _)| strength > 0.0)
}

struct Shininess {
    roughness: f32,
    metallic: f32,
    /// StandardMaterial's reflectance (0.5: the usual 4% head-on).
    reflectance: f32,
    texture: Option<Handle<Image>>,
    /// In place of the base map (a human weapon's finish's colour).
    base_color_texture: Option<Handle<Image>>,
    /// The base colour's factor (see MODEL_SHEEN).
    paint: f32,
}

/// Models whose paint is duller than their maps (by the shader tag's path):
/// the share of their sheen (reflectance where unmasked) and of their base
/// colour they keep. The Warthog's flat light-grey panels caught the sun.
const MODEL_SHEEN: [(&str, [f32; 2]); 1] = [("vehicles\\warthog\\", [0.3, 0.8])];

/// Models whose detail maps are strengthened (by the shader tag's path):
/// their departure from neutral times this. Chief's corroded metal gives
/// his plates much of their depth. (First match wins.) His first-person
/// hands keep theirs: their detail has no mask, covering them wholly, and
/// doubled its dark pits went black whatever lit them.
const MODEL_DETAIL_GAINS: [(&str, f32); 2] = [("characters\\cyborg\\fp\\", 1.0), ("characters\\cyborg\\", 2.0)];

/// A model shader's detail gain (see [`MODEL_DETAIL_GAINS`]).
/// (tuning: `HALO_RT_DETAIL_GAIN=<shader path prefix>=gain;...`)
fn model_detail_gain(shader: &str) -> f32 {
    let tuned = std::env::var("HALO_RT_DETAIL_GAIN").ok().and_then(|text| {
        text.split(';').find_map(|entry| {
            let (prefix, gain) = entry.split_once('=')?;
            (!prefix.is_empty() && shader.starts_with(prefix)).then(|| gain.trim().parse().ok())?
        })
    });
    tuned.or_else(|| MODEL_DETAIL_GAINS.iter().find(|(path, _)| shader.starts_with(path)).map(|&(_, gain)| gain)).unwrap_or(1.0)
}

/// Models whose masks' pattern is brought out (by the shader tag's path): a
/// power and a gain on the mask (`min(mask^power * gain, 1)`). The Covenant
/// weapons' masks cover their hulls, bright with a faint mottling, which
/// as it was left them an even gloss; deepened, it shows them worked alloy.
/// The Covenant vehicles' are alike. The Warthog's marks its glossy paint
/// (not wear: as wear its hood was bare steel); a milder curve brings out
/// the grime in it.
const MASK_CURVES: [(&str, [f32; 2]); 10] = [
    ("weapons\\plasma", COVENANT_MASK_CURVE),
    ("weapons\\needler", COVENANT_MASK_CURVE),
    ("vehicles\\ghost\\", COVENANT_MASK_CURVE),
    ("vehicles\\banshee\\", COVENANT_MASK_CURVE),
    ("vehicles\\wraith\\", COVENANT_MASK_CURVE),
    ("vehicles\\c_dropship\\", COVENANT_MASK_CURVE),
    ("vehicles\\c gun turret\\", COVENANT_MASK_CURVE),
    ("vehicles\\warthog\\", [2.0, 2.0]),
    // (Chief's: doubled, his shine's variation with his detail's)
    ("characters\\cyborg\\", [1.0, 2.0]),
    // (the Covenant crates': dim, a quarter on the whole, it left them a
    // flat rubber; raised, undulled, a worked metal with sharp highlights,
    // play feedback)
    ("scenery\\c_storage\\", [0.8, 1.6]),
];
const COVENANT_MASK_CURVE: [f32; 2] = [3.0, 2.5];

/// The human weapons drawn with a metal finish (by the shader tag's path).
/// Halo reflects its cube map only where their masks say and leaves the rest
/// matte; with a model's unmasked sheen (a smooth dielectric) the dark AR
/// and sniper rifle looked like plastic, the sniper's black flat in the sun.
/// The AR is dark metal all over. The sniper rifle's dark greys (its scope)
/// are black paint, which as metal or lifted turned grey in the sun; its
/// light greys are bare metal. The others are painted like the sniper rifle,
/// their masks' wear (dim: the pistol's and shotgun's polished parts, the
/// rocket launcher's, the grenade's scuffs) brought out as bare steel.
const GUNMETAL_WEAPONS: [(&str, GunmetalFinish); 6] = [
    ("weapons\\assault rifle\\", finish(0.5, 0.3, 0.03, 0.0, 0.001, 0.3, 2.0)),
    ("weapons\\sniper rifle\\", finish(0.4, 0.3, 0.0, 0.06, 0.12, 0.0, 1.0)),
    ("weapons\\pistol\\", PAINTED_WEAPON),
    ("weapons\\shotgun\\", PAINTED_WEAPON),
    ("weapons\\rocket launcher\\", PAINTED_WEAPON),
    ("weapons\\frag grenade\\", PAINTED_WEAPON),
];
const PAINTED_WEAPON: GunmetalFinish = finish(0.45, 0.3, 0.0, 0.06, 0.12, 0.3, 2.0);

use crate::textures::GunmetalFinish;

/// A weapon's finish: roughness, worn roughness, the paint's floor, the
/// luminance from and to which it is bare metal, worn steel's reflectance and
/// the mask's gain (metal reflectance twice the base map's, at most 0.6).
const fn finish(roughness: f32, worn_roughness: f32, paint_floor: f32, metal_from: f32, metal_to: f32, worn_metal: f32, worn_gain: f32) -> GunmetalFinish {
    GunmetalFinish { roughness, worn_roughness, paint_floor, metal_from, metal_to, metal_scale: 2.0, metal_cap: 0.6, worn_metal, worn_gain }
}

/// A human weapon's finish (see [`GUNMETAL_WEAPONS`]).
/// (tuning: `HALO_RT_GUNMETAL=[<shader path prefix>=]roughness,worn roughness,paint floor,metal from,metal to,metal scale,metal cap,worn metal,worn gain;...`,
/// or `off`)
fn gunmetal(shader: &str) -> Option<GunmetalFinish> {
    let finish = GUNMETAL_WEAPONS.iter().find(|(path, _)| shader.starts_with(path)).map(|&(_, finish)| finish);
    let tuning = std::env::var("HALO_RT_GUNMETAL").unwrap_or_default();
    if tuning == "off" {
        return None;
    }
    // (a prefix's entry gives any model the finish; a bare one, the table's)
    let tuned = tuning.split(';').find_map(|entry| {
        let values = match entry.split_once('=') {
            Some((prefix, values)) if !prefix.is_empty() && shader.starts_with(prefix) => values,
            Some(_) => return None,
            None if finish.is_some() => entry,
            None => return None,
        };
        let v: Vec<f32> = values.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        let bits: [f32; 9] = v.try_into().ok()?;
        Some(GunmetalFinish::from_bits(bits.map(f32::to_bits)))
    });
    tuned.or(finish)
}

/// A bitmap group's first bitmap's size.
fn bitmap_size(cache: &CacheFile, datum: u32) -> Option<Vec2> {
    let bitmaps = halo_map::bitmap::bitmaps(cache, datum).ok()?;
    bitmaps.first().map(|b| Vec2::new(b.width as f32, b.height as f32))
}

/// The map that carries a sky layer's look: transparent shaders composite
/// several, so until their combiners are reproduced (ROADMAP phase 3) take the
/// least-tiled 2D map, the last one on ties (later stages draw over earlier).
fn sky_layer_map(info: &ShaderInfo) -> Option<shader::TransparentMap> {
    let transparent = info.transparent.as_ref()?;
    // a first map of a cube type is a reflection, not the layer's colour
    let skip = usize::from(transparent.first_map_type != 0);
    transparent
        .maps
        .iter()
        .skip(skip)
        .flatten()
        .rev()
        .min_by(|a, b| (a.uv_scale[0] * a.uv_scale[1]).total_cmp(&(b.uv_scale[0] * b.uv_scale[1])))
        .copied()
}

/// The share of the light a leaf lets through to its other side.
const FOLIAGE_TRANSMISSION: f32 = 0.4;

/// Whether a shader (by its tag name) is a plant's: trees, plants and
/// vines, which Halo keeps under `scenery\trees` and `scenery\plants`.
fn is_foliage(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["\\trees\\", "\\plants\\", "leaves", "leaf", "vine", "bush", "fern", "palm"].iter().any(|word| name.contains(word))
}

/// A point or spot light the emissive scale applies to: its power (lumens
/// per channel) at a scale of one.
#[derive(Component)]
pub struct LightSource {
    pub power: Vec3,
}

impl LightSource {
    /// The light's colour and intensity (lumens) at the emissive scale.
    pub fn color_and_intensity(&self, scale: f32) -> (Color, f32) {
        let peak = self.power.max_element();
        if peak <= 0.0 {
            return (Color::BLACK, 0.0);
        }
        let color = self.power / peak;
        (Color::linear_rgb(color.x, color.y, color.z), peak * scale)
    }
}

/// A light the size of a sphere of radius `radius` (metres): its lumens
/// per unit of the sphere's surface radiance (π r² candela, 4π lumens per
/// candela, as bevy_pbr counts for point and spot lights alike).
pub fn sphere_lumens_per_radiance(radius: f32) -> f32 {
    std::f32::consts::PI * radius * radius * 4.0 * std::f32::consts::PI
}

/// Keeps the sky centred on the camera.
/// A ray-traced mesh, kept so that it can leave the ray-traced scene while
/// hidden and come back: Solari traces every `RaytracingMesh3d`, visible or
/// not.
#[derive(Component)]
pub struct TracedMesh(pub Handle<Mesh>);

/// A placed object's BSPs (a bit per BSP index): shown only while one of
/// them is the active BSP (bridged).
#[derive(Component)]
pub struct OnBsps(pub u32);

/// Puts hidden ray-traced meshes out of the ray-traced scene, and shown ones
/// back; not a masked one (`RaytracingMask`), hidden for the camera alone:
/// the first-person player's body, which shadows the world. (It went out
/// once the body's skinned meshes were changed in place: a new mesh every
/// frame had put it back each time, and the player lost their shadow.) A
/// light-only emitter (`RaytracingMask::LIGHT_ONLY`) is masked for another
/// reason and goes out with its level's BSP like any other.
pub fn sync_traced_visibility(
    mut commands: Commands,
    traced: Query<
        (Entity, &TracedMesh, &InheritedVisibility, Has<RaytracingMesh3d>, Option<&bevy_solarik::scene::RaytracingMask>),
        Or<(Changed<InheritedVisibility>, Changed<bevy_solarik::scene::RaytracingMask>)>,
    >,
    all: Query<(Entity, &TracedMesh, &InheritedVisibility, Has<RaytracingMesh3d>, Option<&bevy_solarik::scene::RaytracingMask>)>,
    mut unmasked: RemovedComponents<bevy_solarik::scene::RaytracingMask>,
) {
    let unmasked: Vec<Entity> = unmasked.read().collect();
    for (entity, mesh, visibility, in_scene, mask) in traced.iter().chain(unmasked.iter().filter_map(|&e| all.get(e).ok())) {
        let masked = mask.is_some_and(|mask| *mask != bevy_solarik::scene::RaytracingMask::LIGHT_ONLY);
        let wanted = visibility.get() || masked;
        if wanted && !in_scene {
            commands.entity(entity).insert(RaytracingMesh3d(mesh.0.clone()));
        } else if !wanted && in_scene {
            commands.entity(entity).remove::<RaytracingMesh3d>();
        }
    }
}

/// The game draws the sky model at 1/1024 scale round the camera's position
/// times 0.999 (render_sky.c): seen from the point in the model at the
/// camera's world position, in world units. Its parts are laid out about the
/// level (a30's sea and far land, finite sheets below the horizon), and seen
/// from the model's origin instead, their edges came into view.
pub fn follow_camera(
    camera: Query<&Transform, (With<Camera3d>, Without<SkyDome>)>,
    mut domes: Query<&mut Transform, With<SkyDome>>,
) {
    let Ok(camera) = camera.single() else { return };
    // (the camera in world units, at sky_scale() metres a sky unit)
    let offset = camera.translation / METERS_PER_WORLD_UNIT * sky_scale();
    for mut dome in &mut domes {
        dome.translation = camera.translation - offset;
    }
}

/// A placement's position and orientation in Bevy's convention. Halo's
/// object axes (forward, left, up) form the rotation's columns; the change to
/// Bevy coordinates C makes it C·R·Cᵀ.
fn placement_transform(placement: &Placement) -> Transform {
    let [forward, left, up] = placement.axes();
    let halo_rotation = Mat3::from_cols(Vec3::from(forward), Vec3::from(left), Vec3::from(up));
    let to_bevy = Mat3::from_cols(Vec3::X, Vec3::NEG_Z, Vec3::Y);
    let rotation = Quat::from_mat3(&(to_bevy * halo_rotation * to_bevy.transpose()));
    Transform::from_translation(Vec3::from(placement.bevy_position())).with_rotation(rotation.normalize())
}

/// A material's refined emitters' counts, for the log (see
/// [`refined_emitter`]).
#[derive(Default)]
struct RefineTally {
    source: usize,
    kept: usize,
    dropped: usize,
    /// The pieces aimed at by the lights refined, and how many they are.
    target: usize,
    refined: usize,
    /// Each piece's area (world units squared) times its card's mean.
    energy: f64,
    dropped_energy: f64,
    /// Lights that kept their own triangles, by why: their card shows
    /// nothing over them (it emits nothing: the card's peak and the mean
    /// measured, the last such), they kept over [`EMITTER_MOST_PIECES`]
    /// unsplit, the level's budget was spent, or no piece was split or
    /// dropped.
    no_light: usize,
    dark_card: Option<(f32, f64)>,
    over_cap: usize,
    over_budget: usize,
    nothing: usize,
    /// Under `HALO_RT_GLOW_LOG`: each light's dropped pieces (the first
    /// few) and where the light is, for the log.
    notes: Vec<String>,
}

/// A piece of a refined emitter: its corners, area, card mean and most,
/// and how many times it was split.
type EmitterPiece = ([SourceVertex; 3], f64, f32, f32, u32);

/// A light card's emitter (a transparent shader's radiosity emitter, its
/// emission patterned by its card: see [`LevelBuilder::emitter_map`]) as
/// pieces of near equal light, for the rays alone (it is unseen: no ray
/// meets it, the camera draws its card). Solarik's light sampling picks a
/// light's triangle uniformly by index and a uniform point on it, so on a
/// card lit in a thin strip or a glow's centre most candidates landed on
/// dim texels, and the patterned cards boiled more than the whole surfaces
/// had. Pieces of equal area with the dark ones dropped did nothing: the
/// cards are soft glows, nearly nothing under them dark enough to drop
/// (c20's strips kept 20061 of 20064 pieces), and equal pieces left the
/// pick as blind as before. So the light is shared out instead: a light's
/// energy (area times the card's mean) over a target count
/// ([`EMITTER_PIECES_PER_TRIANGLE`] a source triangle, between
/// [`EMITTER_FEWEST_TARGET`] and [`EMITTER_MOST_TARGET`]), and a piece
/// holding more is split in four at its edges' midpoints, down to
/// [`EMITTER_DEEPEST`] times or about a texel: the uniform pick then goes
/// by power, the bright texels in many small pieces, the dim glow in a few
/// large ones. (Measured, 10 November: c20's index hall's still wander
/// 0.010 to 0.003 with every card patterned, its strip hall's 0.018 to
/// 0.014 with the invisible lights patterned out as first played; b40's
/// shaft the same; about 0.1 ms.) The card is measured under each piece as
/// the rays sample it (filtered, repeating); the darkest pieces go while
/// what they held stays within [`EMITTER_DROP_SHARE`] of the light and none
/// has a texel brighter than [`EMITTER_DARK`] of the card's peak; what
/// they held isn't made up.
/// The share doubled while a light keeps over [`EMITTER_MOST_PIECES`];
/// `None` (its own triangles) when it is still over unsplit, past the
/// level's `budget`, or where nothing was split or dropped. The pieces'
/// vertices, three each, and their indices, wound as the source's.
fn refined_emitter(
    sources: &[SourceVertex],
    indices: &[u32],
    card: &LuminanceGrid,
    uv_scale: [f32; 2],
    budget: &mut usize,
    tally: &mut RefineTally,
) -> Option<(Vec<SourceVertex>, Vec<u32>)> {
    tally.source += indices.len() / 3;
    let (w, h) = (card.width as f32, card.height as f32);
    let texels = |v: &SourceVertex| Vec2::new(v.texcoord[0] * uv_scale[0] * w, v.texcoord[1] * uv_scale[1] * h);
    let measure = |corners: [SourceVertex; 3], depth: u32| -> EmitterPiece {
        let [p, q, r] = corners.map(|v| Vec3::from(v.position));
        let area = (q - p).cross(r - p).length() as f64 / 2.0;
        let (mean, most) = card_under(card, corners.map(|v| texels(&v)));
        (corners, area, mean, most, depth)
    };
    let energy_of = |piece: &EmitterPiece| piece.1 * piece.2 as f64;
    let roots: Vec<EmitterPiece> = indices
        .chunks_exact(3)
        .filter_map(|t| Some([*sources.get(t[0] as usize)?, *sources.get(t[1] as usize)?, *sources.get(t[2] as usize)?]))
        .map(|corners| measure(corners, 0))
        .collect();
    let energy: f64 = roots.iter().map(energy_of).sum();
    if !(energy > 0.0) {
        tally.no_light += 1;
        let area: f64 = roots.iter().map(|r| r.1).sum();
        tally.dark_card = Some((card.peak, if area > 0.0 { energy / area } else { 0.0 }));
        return None;
    }
    let target = (EMITTER_PIECES_PER_TRIANGLE * roots.len()).clamp(EMITTER_FEWEST_TARGET, EMITTER_MOST_TARGET);
    let mut share = energy / target as f64;
    let (deepest, drop_share) = (emitter_deepest(), emitter_drop_share());
    loop {
        // (split by need: a piece over the share in four, its corners'
        // three and the middle one, each wound as the source)
        let mut pieces: Vec<EmitterPiece> = Vec::new();
        let mut work: Vec<EmitterPiece> = roots.clone();
        let mut split = false;
        while let Some(piece) = work.pop() {
            let [a, b, c] = piece.0;
            let longest = [(a, b), (b, c), (c, a)].iter().map(|(p, q)| texels(p).distance(texels(q))).fold(0.0f32, f32::max);
            if energy_of(&piece) <= share || piece.4 >= deepest || !(longest > 1.0) {
                pieces.push(piece);
                continue;
            }
            split = true;
            let half = |p: &SourceVertex, q: &SourceVertex| lerp_vertex(p, q, q, [0.5, 0.5, 0.0]);
            let (ab, bc, ca) = (half(&a, &b), half(&b, &c), half(&c, &a));
            for corners in [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]] {
                work.push(measure(corners, piece.4 + 1));
            }
        }
        // (the darkest first, dropped while within the share)
        let mut order: Vec<usize> = (0..pieces.len()).collect();
        order.sort_by(|&x, &y| pieces[x].2.total_cmp(&pieces[y].2));
        let mut drop = vec![false; pieces.len()];
        let mut dropped_energy = 0.0f64;
        for &i in &order {
            let held = energy_of(&pieces[i]);
            if pieces[i].3 <= EMITTER_DARK * card.peak && dropped_energy + held <= drop_share * energy {
                drop[i] = true;
                dropped_energy += held;
            }
        }
        let kept = drop.iter().filter(|d| !**d).count();
        if kept > EMITTER_MOST_PIECES {
            // (coarser, unless nothing is split already)
            if split {
                share *= 2.0;
                continue;
            }
            tally.over_cap += 1;
            return None;
        }
        // (HALO_RT_GLOW_LOG: what was dropped, and where)
        if kept < pieces.len() && std::env::var_os("HALO_RT_GLOW_LOG").is_some() {
            let bounds = |corners: &[SourceVertex; 3]| {
                let t = corners.map(|v| texels(&v));
                t[0].max(t[1]).max(t[2]) - t[0].min(t[1]).min(t[2])
            };
            let centre = |corners: &[SourceVertex; 3]| corners.iter().map(|v| Vec3::from(v.position)).sum::<Vec3>() / 3.0;
            let dropped: Vec<&EmitterPiece> = pieces.iter().zip(&drop).filter(|(_, d)| **d).map(|(p, _)| p).collect();
            for piece in dropped.iter().take(6) {
                let (extent, at) = (bounds(&piece.0), centre(&piece.0));
                tally.notes.push(format!(
                    "dropped a piece of depth {}, area {:.4}, {:.1} x {:.1} texels, mean {:.5}, most {:.5}, at {:.2}, {:.2}, {:.2}",
                    piece.4, piece.1, extent.x, extent.y, piece.2, piece.3, at.x, at.y, at.z
                ));
            }
            if dropped.len() > 6 {
                tally.notes.push(format!("and {} more dropped", dropped.len() - 6));
            }
            let largest = roots.iter().map(|r| bounds(&r.0)).max_by(|a, b| a.max_element().total_cmp(&b.max_element())).unwrap_or_default();
            let at = roots.iter().map(|r| centre(&r.0)).sum::<Vec3>() / roots.len().max(1) as f32;
            tally.notes.push(format!(
                "a light of {} triangles, {} pieces, {} dropped: its largest triangle {:.1} x {:.1} texels, its centre at {:.2}, {:.2}, {:.2} (card {} x {}, peak {:.4})",
                roots.len(), pieces.len(), dropped.len(), largest.x, largest.y, at.x, at.y, at.z, card.width, card.height, card.peak
            ));
        }
        if kept == 0 {
            tally.no_light += 1;
            return None;
        }
        if !split && kept == pieces.len() {
            tally.nothing += 1;
            return None;
        }
        if kept > *budget {
            tally.over_budget += 1;
            return None;
        }
        *budget -= kept;
        tally.kept += kept;
        tally.dropped += pieces.len() - kept;
        tally.target += target;
        tally.refined += 1;
        tally.energy += energy;
        tally.dropped_energy += dropped_energy;
        let vertices: Vec<SourceVertex> = pieces.iter().zip(&drop).filter(|(_, d)| !**d).flat_map(|(p, _)| p.0).collect();
        let indices = (0..vertices.len() as u32).collect();
        return Some((vertices, indices));
    }
}

/// A vertex between three by barycentric weights, its directions
/// renormalized.
fn lerp_vertex(a: &SourceVertex, b: &SourceVertex, c: &SourceVertex, weights: [f32; 3]) -> SourceVertex {
    let mix = |f: fn(&SourceVertex) -> [f32; 3]| Vec3::from(f(a)) * weights[0] + Vec3::from(f(b)) * weights[1] + Vec3::from(f(c)) * weights[2];
    let direction = |f: fn(&SourceVertex) -> [f32; 3]| {
        let v = mix(f);
        if v.length_squared() > 1e-12 { v.normalize().to_array() } else { f(a) }
    };
    let uv = Vec2::from(a.texcoord) * weights[0] + Vec2::from(b.texcoord) * weights[1] + Vec2::from(c.texcoord) * weights[2];
    SourceVertex {
        position: mix(|v| v.position).to_array(),
        normal: direction(|v| v.normal),
        binormal: direction(|v| v.binormal),
        tangent: direction(|v| v.tangent),
        texcoord: uv.to_array(),
    }
}

/// A card's mean and most luminance under a triangle (its corners in
/// texels): filtered lookups at its corners, edge midpoints and centre, and
/// on a half-texel grid over its bounds inside it, so a lit line one texel
/// wide falls on some (a broad triangle's grid coarser, at most 192 a side).
fn card_under(card: &LuminanceGrid, corners: [Vec2; 3]) -> (f32, f32) {
    let [p, q, r] = corners;
    if !(p.is_finite() && q.is_finite() && r.is_finite()) {
        return (0.0, 0.0);
    }
    let (w, h) = (card.width as f32, card.height as f32);
    let (mut sum, mut count, mut most) = (0.0f32, 0usize, 0.0f32);
    let mut sample = |x: Vec2| {
        let l = card.bilinear(x.x / w, x.y / h);
        sum += l;
        count += 1;
        most = most.max(l);
    };
    for x in [p, q, r, (p + q) / 2.0, (q + r) / 2.0, (r + p) / 2.0, (p + q + r) / 3.0] {
        sample(x);
    }
    let (lo, hi) = (p.min(q).min(r), p.max(q).max(r));
    let step = ((hi - lo).max_element() / 192.0).max(0.5);
    // (inside by the edge functions, either winding, a little loose)
    let doubled = (q - p).perp_dot(r - p);
    if doubled.abs() > 1e-6 {
        let slack = -1e-4 * doubled.abs();
        let mut y = lo.y + step / 2.0;
        while y < hi.y {
            let mut x = lo.x + step / 2.0;
            while x < hi.x {
                let at = Vec2::new(x, y);
                let e = [(q - p).perp_dot(at - p), (r - q).perp_dot(at - q), (p - r).perp_dot(at - r)];
                if e.iter().all(|&e| e * doubled.signum() >= slack) {
                    sample(at);
                }
                x += step;
            }
            y += step;
        }
    }
    (sum / count.max(1) as f32, most)
}

/// Halo triangles as a Solari-compatible mesh: positions, normals, UV0 and
/// tangents, u32 indices, no UV1.
fn build_mesh(vertices: impl Iterator<Item = SourceVertex>, indices: &[u32], uv_scale: [f32; 2]) -> Mesh {
    build_mesh_oriented(vertices, indices, uv_scale, false)
}

/// [`build_mesh`] for a model's part, its triangles turned to face their
/// vertices' normals (see below).
fn build_model_mesh(vertices: impl Iterator<Item = SourceVertex>, indices: &[u32], uv_scale: [f32; 2]) -> Mesh {
    let _profile = bevy::render::diagnostic::profile_scope("main.model_mesh");
    build_mesh_oriented(vertices, indices, uv_scale, true)
}

fn build_mesh_oriented(vertices: impl Iterator<Item = SourceVertex>, indices: &[u32], uv_scale: [f32; 2], orient: bool) -> Mesh {
    let (mut positions, mut normals, mut uvs, mut tangents) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut sources = Vec::new();
    for v in vertices {
        sources.push((Vec3::from(v.position), Vec3::from(v.normal)));
        positions.push(halo_to_bevy(v.position, METERS_PER_WORLD_UNIT));
        normals.push(halo_to_bevy(v.normal, 1.0));
        uvs.push([v.texcoord[0] * uv_scale[0], v.texcoord[1] * uv_scale[1]]);
        let (n, t, b) = (Vec3::from(v.normal), Vec3::from(v.tangent), Vec3::from(v.binormal));
        let handedness = if n.cross(t).dot(b) < 0.0 { -1.0 } else { 1.0 };
        let [x, y, z] = halo_to_bevy(v.tangent, 1.0);
        tangents.push([x, y, z, handedness]);
    }
    // Halo's triangles are clockwise seen from the front (Direct3D);
    // Bevy's front faces are counter-clockwise. A few are wound against
    // their vertices' normals (a model's handful): the game culls them where
    // they face the camera and lights them by their normals where they face
    // away, where the rays' lighting finds them black. Turned to face their
    // normals (models only) where all three disagree, they fill their holes
    // and stay unseen from behind. Where only some do, they're seams whose
    // normals bend round an edge (the root of Johnson's cap bill, which the
    // game draws facing down): drawn from both sides, as turned they left a
    // notch seen from below.
    let mut wound = Vec::with_capacity(indices.len());
    for t in indices.chunks_exact(3) {
        let corner = |i: u32| sources.get(i as usize).copied().unwrap_or_default();
        let ((a, na), (b, nb), (c, nc)) = (corner(t[0]), corner(t[1]), corner(t[2]));
        // (clockwise from the front: the cross product points back)
        let back = (b - a).cross(c - a);
        let game = [t[0], t[2], t[1]];
        let turned = [t[0], t[1], t[2]];
        if !orient || back.dot(na + nb + nc) <= 0.0 {
            wound.extend(game);
        } else if [na, nb, nc].iter().all(|n| back.dot(*n) > 0.0) {
            wound.extend(turned);
        } else {
            wound.extend(game);
            wound.extend(turned);
        }
    }
    let indices = wound;

    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tangents)
        .with_inserted_indices(Indices::U32(indices))
}

#[cfg(test)]
mod tests {
    use super::*;
    use halo_map::objects::ObjectKind;

    fn placement(rotation: [f32; 3]) -> Placement {
        Placement {
            kind: ObjectKind::Scenery,
            definition: TagRef { group: 0, datum: None },
            position: [0.0; 3],
            rotation,
            on_bsp_flags: 1,
            not_automatic: false,
            region_permutations: [0; 8],
            light: None,
        }
    }

    /// An object's local forward (Halo +x) must end up along its yawed
    /// Halo forward, converted to Bevy.
    #[test]
    fn yaw_turns_forward_counter_clockwise() {
        let t = placement_transform(&placement([std::f32::consts::FRAC_PI_2, 0.0, 0.0]));
        // Halo +x in Bevy is +x; after a quarter turn it should face Halo +y,
        // which is Bevy -z.
        let forward = t.rotation * Vec3::X;
        assert!(forward.abs_diff_eq(Vec3::NEG_Z, 1e-5), "{forward}");
        // up stays up
        assert!((t.rotation * Vec3::Y).abs_diff_eq(Vec3::Y, 1e-5));
    }
}
