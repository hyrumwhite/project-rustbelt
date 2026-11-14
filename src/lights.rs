//! The game's dynamic lights (muzzle flashes, plasma, explosions, the
//! flashlight, headlights) in the ray-traced scene: each light the bridge
//! shares is a point light, or a spot light for one with a cone of up to a
//! hemisphere, which Solarik samples as a small sphere, full inside the
//! light's falloff angle and fading to none at its cutoff. Muzzle flashes
//! (their tag's, protocol 38), which shine every way from the weapon, are
//! spot lights too: the player's out ahead along the camera's aim (round the
//! camera, in first person, they lit a circle round the player), others'
//! along their weapon's. Explosions, grenades and bolts stay round. A light
//! shining every way through a gel of a few narrow discs (a10's spinning
//! alarm lights: two) is a spot light for each, turned with the light. They
//! keep their entities from frame to frame (by their tag and object), and
//! the unused are hidden, then reused. Smooth surfaces don't show their
//! spheres (`SolarikUnreflectedLight`): the game's picture of a light is its
//! effect's sprites, and in a pane a sphere was a flat disc (b40's glass:
//! play feedback, circles).
//! Projectiles glow too: the plasma bolts by the lights their tags attach,
//! which in the game are only lens flares and light nothing (the game
//! shares them anyway, at their tags' colour and reach); rockets, needles,
//! and the Wraith's mortar, whose tags carry no light, by their own here.
//! The Sentinels' beams glow along their contrails (the beam's projectile
//! never outlives the tick that fired it), which the game shares with the
//! lights.

use crate::{
    bridge::{BridgeLevel, BridgeLink},
    level::{sphere_lumens_per_radiance, LightSource},
    LightingSettings,
};
use bevy::{camera::visibility::NoFrustumCulling, platform::collections::HashMap, prelude::*};
use bevy_solarik::scene::{SolarikLightCone, SolarikUnreflectedLight};
use halo_bridge::{Light, LIGHT_CONTRAIL, LIGHT_MUZZLE_FLASH, OBJECT_OUTSIDE_OF_MAP};
use halo_map::{
    light_gel::{gel_beams, GelBeam},
    math::{halo_to_bevy, METERS_PER_WORLD_UNIT},
};

/// The lights' size (metres): larger, softer round the light (a muzzle
/// flash's bright spot on the weapon and the wall beside it).
const LIGHT_RADIUS: f32 = 0.25;
/// A spot light's size: the disc that stood in for one, emitting from its
/// front.
const SPOT_RADIUS: f32 = 0.15;
/// Radiance per unit of light colour and squared reach (world units), before
/// the emissive scale: a muzzle flash (reaching 2 world units) visibly lights
/// the weapon and the ground nearby in daylight, and a dark room.
const RADIANCE_PER_REACH2: f32 = 30000.0;
/// Point lights' share of that (muzzle flashes, plasma, explosions).
const POINT_LIGHT_SCALE: f32 = 0.25;
/// Spot lights' (the flashlight's): its cone holds its light, and at full
/// strength it washed the view white.
const SPOT_LIGHT_SCALE: f32 = 0.1;
/// A spot light's brightness grows with its reach only linearly, from the
/// flashlight's (6 world units): the Warthog's headlights reach 12, and by
/// its square they glared.
const SPOT_REFERENCE_REACH: f32 = 6.0;
/// How far ahead of the game's position a spot light shines from (metres):
/// the flashlight sits at the eye, inside the first-person weapon, which
/// it lit glaring white.
const SPOT_FORWARD: f32 = 0.6;
/// Others' spot lights (the Warthog's headlights, on a marker atop its
/// windscreen, one light 90 degrees wide that lit its hood): this far
/// further ahead and this far down (metres, the world's down: about its
/// bumper), their cone this share of the tag's, and this share of the
/// brightness.
const VEHICLE_SPOT_FORWARD: f32 = 1.6;
const VEHICLE_SPOT_DROP: f32 = 0.6;
const VEHICLE_SPOT_CONE: f32 = 0.6;
const VEHICLE_SPOT_SCALE: f32 = 0.5;
/// The player's spot light (their flashlight; the game marks it first
/// person, or failing that it is this near the camera, in world units) is
/// aimed where the camera looks: the game's light keeps the unit's level
/// facing.
const FLASHLIGHT_REACH: f32 = 1.0;
/// The player's flashlight's cone, as a share of its tag's (by the tag's it
/// lit most of the view), and its share of a spot light's brightness.
const FLASHLIGHT_CONE: f32 = 0.7;
const FLASHLIGHT_SCALE: f32 = 0.8;
/// A muzzle flash is a spot light: the player's this far out along the aim
/// (metres, about the muzzle), others' this far out along their weapon's,
/// with this cone (radians: full to the first, none at the second), and this
/// share of a point light's brightness.
const MUZZLE_FORWARD: f32 = 1.0;
const OTHERS_MUZZLE_FORWARD: f32 = 0.3;
const MUZZLE_FALLOFF: f32 = 0.45;
const MUZZLE_CUTOFF: f32 = 1.2;
const MUZZLE_LIGHT_SCALE: f32 = 0.15;
/// Up to this cutoff angle (radians, from the light's axis) a light is a
/// spot light (the Warthog's headlights: a hemisphere, 90 degrees).
const SPOT_CUTOFF: f32 = std::f32::consts::FRAC_PI_2 + 0.001;
/// The widest spot cone: Bevy's cones stop short of a hemisphere.
const SPOT_WIDEST: f32 = std::f32::consts::FRAC_PI_2 - 0.01;
/// How far a light reaches, over its radius: Bevy's range window fades it
/// to none at the range, and at the radius the flashlight stopped short.
const RANGE_PER_RADIUS: f32 = 1.75;
/// A gel is drawn as spot lights when each of its discs is dark beyond this
/// angle (radians; a10's alarm beams, 40 degrees): the holograms' and the
/// Monitor's spill everywhere, and stay a point light.
const GEL_WIDEST: f32 = 1.05;
/// A gel's beams' share of the light's brightness (by the gel's own, its
/// discs lit the rooms as brightly as the light unmasked).
const GEL_LIGHT_SCALE: f32 = 0.5;

/// The projectiles that glow in flight with no light in their tags (the
/// fuel rods, charged bolts and grenades have their own, the other bolts
/// their lens flares'): their tag's name, their light's colour and reach
/// (world units, as a light tag's radius).
const PROJECTILE_GLOWS: [(&str, [f32; 3], f32); 3] = [
    // (its exhaust's flame)
    ("weapons\\rocket launcher\\rocket", [1.0, 0.55, 0.2], 2.0),
    // (pink crystal, in flight and stuck in what it hit)
    ("weapons\\needler\\needle", [1.0, 0.3, 0.8], 0.75),
    // (a ball of blue plasma)
    ("vehicles\\wraith\\wraith mortar", [0.5, 0.45, 1.0], 2.0),
];

/// Whether a projectile's glow at exactly the world's origin is a light, as
/// before (`HALO_RT_ORIGIN_GLOWS=1`): by default it is left out, an object
/// the game holds there, not in the world yet or already gone, whose glow lit
/// whatever was near the map's origin. Read once.
fn origin_glows() -> bool {
    static ORIGIN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ORIGIN.get_or_init(|| std::env::var("HALO_RT_ORIGIN_GLOWS").is_ok_and(|v| v.trim() == "1"))
}

/// The contrails that glow (by their contrail tag's name), with the colour
/// and reach (world units) of the lights along them: one each this far
/// apart (world units), up to this many.
const CONTRAIL_GLOWS: [(&str, [f32; 3], f32); 1] = [
    // (the Sentinels' beam: an orange laser)
    ("characters\\sentinel\\beam", [1.0, 0.45, 0.15], 1.0),
];
const CONTRAIL_GLOW_SPACING: f32 = 3.0;
const CONTRAIL_GLOW_MOST: usize = 4;

/// Lights dimmed from their tags' brightness (by the light tag's name, or
/// a folder's, ending in a backslash): a wide reach is a bright light here
/// (brightness grows with its square). Dimmed to 0, left out.
const LIGHT_DIMMING: [(&str, f32); 13] = [
    // (Guilty Spark's, reaching 5.5 world units: it lit rooms like
    // floodlights)
    ("characters\\monitor\\monitor dynamic", 0.15),
    // (the grenades' glow in flight, and their explosions', reaching 10)
    ("weapons\\frag grenade\\frag grenade", 0.7),
    ("weapons\\plasma grenade\\plasma grenade", 0.7),
    ("weapons\\frag grenade\\explosion", 0.6),
    ("weapons\\plasma grenade\\explosion", 0.6),
    // (b30's control room light, reaching 3.5 around its hologram until the
    // terminal turns it off: it washed out the hologram, which the game
    // draws brighter than the walls; play feedback: to 15%)
    ("levels\\b30\\devices\\interior tech objects\\holo control room display\\light", 0.15),
    // (c10's coloured point lights, flanking its doors, some blinking: they
    // flooded the rooms green and red; play feedback: dramatically down,
    // then half that again)
    ("levels\\c10\\scenery\\", 0.05),
    // (the batons, glow sticks: c10's landing zone is strewn with them;
    // play feedback: too bright)
    ("scenery\\baton\\baton light dynamic", 0.5),
    // (a50's doors' lock lights, red, green or white, reaching 2.5 from
    // the middle of each door: they flooded the ship's rooms; play
    // feedback: much too bright. With its small sphere's twenty-fifth
    // (`SMALL_LIGHTS`), a twelfth of the tag's.)
    ("levels\\a50\\devices\\small door\\small door light", 2.0),
    // (a10's command displays', at each object's origin, inside the
    // bridge's console under its holographic panel: its sphere cut a dark
    // disc in the console in a bright ring, play feedback's "donut", and
    // wherever it was moved clear, the panel's glass reflected it a white
    // ball, however small or dim; the bridge's lamps light the console)
    ("levels\\a10\\devices\\command display\\command display", 0.0),
    // (a10's alarm klaxons' red, flashing through the bridge scenes; play
    // feedback: 30% down)
    ("levels\\a10\\devices\\alert_light\\alert_light", 0.7),
    // (the broken machines' sparks, flickering, on b40 and others: they
    // lit the walls round them brightly; play feedback: much too bright)
    ("scenery\\emitters\\sparks constant\\sparks flicker", 0.25),
    // (the Ghost's bolts', at each as it leaves the guns, two in turn: the
    // ground before a firing Ghost glowed bright cyan; play feedback:
    // dimmer)
    ("vehicles\\ghost\\ghost bolt", 0.35),
];
/// b40's door blinkers' light tag (see [`UPWARD_LIGHTS`]).
const DOOR_BLINKER_LIGHT: &str = "levels\\b40\\scenery\\b40_doorblinker\\doorblinker";

/// Lights that shine every way but down onto their own object, by tag
/// ([`Upward`]): b40's door blinkers, its direction markers, sit a hand's
/// breadth over the block before each door; as a sphere the light lit the
/// block's top to white and flooded the floor, hiding the glyph (the user's
/// captures; `HALO_RT_MARKER_POINT_LIGHT=1`, the old look); the glyph card
/// alone (`crate::level`'s GLOWS_FOLLOWING_OBJECT) lit the walls hardly at
/// all at any radiance short of thousands, where the original lights them
/// brightly at the pulse's peak. So it shines in a cone about straight up,
/// a little wider than the hemisphere: the walls and ceiling as the
/// original's, the block dark, the floor faintly and by what the walls
/// return. A plain hemisphere left the walls' base dark below the light's
/// horizon, 15 to 20 cm, where the original lights them to the floor
/// (`HALO_RT_MARKER_LIGHT_INNER=89 HALO_RT_MARKER_LIGHT_OUTER=89.4` is
/// that). Lowered 5 cm to sit just clear of the block's top; its share 0.2
/// by MCC's capture of the wall's pulse. (Tuning:
/// `HALO_RT_MARKER_LIGHT=<share>`, 0 none, the card alone;
/// `HALO_RT_MARKER_LIGHT_INNER=<degrees>`;
/// `HALO_RT_MARKER_LIGHT_OUTER=<degrees>`;
/// `HALO_RT_MARKER_LIGHT_RAISE=<metres>`; each read once.)
const UPWARD_LIGHTS: [(&str, Upward); 1] = [(
    DOOR_BLINKER_LIGHT,
    Upward { inner: MARKER_LIGHT_INNER, outer: MARKER_LIGHT_OUTER, share: MARKER_LIGHT, raise: MARKER_LIGHT_RAISE },
)];
/// b40's door blinkers' soft inner angle (radians from straight up, 95
/// degrees; see [`UPWARD_LIGHTS`]).
const MARKER_LIGHT_INNER: f32 = 1.658;
/// b40's door blinkers' outer angle (radians from straight up, 97 degrees,
/// a little below the horizon: the walls' base; see [`UPWARD_LIGHTS`]).
const MARKER_LIGHT_OUTER: f32 = 1.693;
/// b40's door blinkers' share of a point light's brightness, by MCC's
/// capture of the wall's pulse (see [`UPWARD_LIGHTS`]).
const MARKER_LIGHT: f32 = 0.2;
/// b40's door blinkers' raise (metres; see [`UPWARD_LIGHTS`]): the 5 cm
/// sphere about 2 cm clear of the block's top, its horizon 12 cm over the
/// object.
const MARKER_LIGHT_RAISE: f32 = -0.05;

/// A light shining every way but down: a cone about the world's up (a
/// point light with a `SolarikLightCone`: Bevy's spot cones stop short of
/// a hemisphere), soft from the inner angle to the outer, at a share of a
/// point light's brightness, raised (or lowered) from the game's position
/// in place of [`SMALL_LIGHTS`]' raise.
#[derive(Clone, Copy, Debug)]
struct Upward {
    /// Where its softening starts from the up (radians).
    inner: f32,
    /// Where it ends from the up (radians; past the horizon, short of
    /// straight down).
    outer: f32,
    /// Its share of a point light's brightness.
    share: f32,
    /// How far it is raised (metres, the world's up; negative lower).
    raise: f32,
}

/// A light's upward cone, if it shines so (see [`UPWARD_LIGHTS`]): none in
/// the old look (`crate::level::marker_point_light`). The outer angle at
/// most straight down, the inner at most the outer.
fn upward_light(name: &str) -> Option<Upward> {
    type Tuned = (Option<f32>, Option<f32>, Option<f32>, Option<f32>);
    static TUNED: std::sync::OnceLock<Tuned> = std::sync::OnceLock::new();
    if crate::level::marker_point_light() {
        return None;
    }
    let &(_, upward) = UPWARD_LIGHTS.iter().find(|(tag, _)| name.eq_ignore_ascii_case(tag))?;
    let (inner, outer, share, raise) = *TUNED.get_or_init(|| {
        let read = |name: &str| std::env::var(name).ok().and_then(|v| v.trim().parse::<f32>().ok());
        let angle = |name: &str| read(name).map(f32::to_radians);
        (angle("HALO_RT_MARKER_LIGHT_INNER"), angle("HALO_RT_MARKER_LIGHT_OUTER"), read("HALO_RT_MARKER_LIGHT"), read("HALO_RT_MARKER_LIGHT_RAISE"))
    });
    let outer = outer.unwrap_or(upward.outer).clamp(0.0, std::f32::consts::PI);
    let inner = inner.unwrap_or(upward.inner).clamp(0.0, outer);
    Some(Upward { inner, outer, share: share.unwrap_or(upward.share), raise: raise.unwrap_or(upward.raise) })
}

/// Lights smaller than [`LIGHT_RADIUS`] (by the light tag's name), raised
/// this much (metres, the world's up) to clear the ground they lie on: a
/// baton's light is at its middle, on the ground, and the ground within its
/// sphere, under the light's surface, went unlit, a dark disc round the
/// stick. a50's doors' lock lights are in each door's middle, and the door
/// cut their spheres in half: a dark disc in a bright ring on the door.
/// b40's door blinkers, white, pulsing, reaching 3 world units, sit a
/// hand's breadth over the block before each of its doors: the block cut
/// a dark disc round each, and the pulse flooded the white halls (play
/// feedback: its "direction indicators" much too bright, with a "donut
/// hole"); a twenty-fifth of the light, as the small sphere's. Since 12
/// November a cone shining up from there, a little wider than the
/// hemisphere (`UPWARD_LIGHTS`), the sphere with
/// `HALO_RT_MARKER_POINT_LIGHT=1`.
const SMALL_LIGHTS: [(&str, f32); 3] = [
    ("scenery\\baton\\baton light dynamic", 0.05),
    ("levels\\a50\\devices\\small door\\small door light", 0.05),
    (DOOR_BLINKER_LIGHT, 0.05),
];

/// Whether a light tag's name is the dimmed one, or in the dimmed folder.
fn dimmed_by(name: &str, dimmed: &str) -> bool {
    if dimmed.ends_with('\\') {
        name.len() > dimmed.len() && name.is_char_boundary(dimmed.len()) && name[..dimmed.len()].eq_ignore_ascii_case(dimmed)
    } else {
        name.eq_ignore_ascii_case(dimmed)
    }
}

/// What a light's tag says of it here.
#[derive(Clone, Copy)]
struct LightTag {
    /// A muzzle flash, by its name.
    muzzle: bool,
    /// Its share of its brightness ([`LIGHT_DIMMING`]).
    dimming: f32,
    /// Its size, if small ([`SMALL_LIGHTS`]).
    small: Option<f32>,
    /// Its upward cone, if it shines every way but down ([`UPWARD_LIGHTS`]).
    upward: Option<Upward>,
    /// Its sphere and brightness as tuned, a point light's ([`light_tune`]).
    tune: Option<Tune>,
}

/// What a light's tag says of it here, by the tag's name (none: a plain
/// light).
fn light_tag(name: Option<&str>) -> LightTag {
    LightTag {
        muzzle: name.is_some_and(|name| name.ends_with("muzzle flash")),
        dimming: name.and_then(|name| LIGHT_DIMMING.iter().find(|(dimmed, _)| dimmed_by(name, dimmed))).map_or(1.0, |&(_, dimming)| dimming),
        small: name.and_then(|name| SMALL_LIGHTS.iter().find(|(small, _)| name.eq_ignore_ascii_case(small))).map(|&(_, size)| size),
        upward: name.and_then(upward_light),
        tune: name.and_then(light_tune),
    }
}

/// A point light's sphere and brightness as tuned for measuring
/// (`HALO_RT_LIGHT_TUNE`, see [`light_tune`]).
#[derive(Clone, Copy, Debug)]
struct Tune {
    /// The source sphere's radius (metres).
    radius: f32,
    /// How far it is raised from the game's position (metres, the world's
    /// up; negative lower), in place of a small light's raise or an upward
    /// light's.
    raise: f32,
    /// Its share of the power it would have had untuned.
    share: f32,
}

/// The tuning of the point lights whose light tag's name holds a part,
/// for measuring a light's look before its table entry is made:
/// `HALO_RT_LIGHT_TUNE="<tag name part>=<source radius m>,<raise m>,<share>[;...]"`,
/// the part matched without regard to case, the first that matches winning
/// (a test light, [`TestLights`], by its tag's name, or by `test` without
/// one). A light's power is its sphere's radiance times its sphere's size
/// (`sphere_lumens_per_radiance`), so a smaller sphere at the same
/// radiance is a dimmer light; [`SMALL_LIGHTS`]' are so on purpose (their
/// twenty-fifth of the light is part of their tuning, and `LIGHT_DIMMING`'s
/// shares were set over it). Tuned, the power is kept at what the light
/// would have had at its untuned size (a small light's small sphere's, any
/// other's [`LIGHT_RADIUS`]'s) and the sphere's radiance follows the size
/// asked: the radius alone moves where the light comes from and how
/// concentrated it is, not how much there is of it; the share then
/// multiplies that power (1 the untuned light's); the raise replaces the
/// untuned one (0 for most lights, `size × 1.2` for a small one, the upward
/// lights' own). Spot lights, and a gelled light's beams (which take the
/// raise alone), are left as they are. Read once.
fn light_tune(name: &str) -> Option<Tune> {
    static TUNES: std::sync::LazyLock<Vec<(String, Tune)>> = std::sync::LazyLock::new(|| {
        let text = std::env::var("HALO_RT_LIGHT_TUNE").unwrap_or_default();
        text.split(';')
            .filter(|entry| !entry.trim().is_empty())
            .filter_map(|entry| {
                let parsed = entry.split_once('=').and_then(|(part, numbers)| {
                    let numbers: Vec<f32> = numbers.split(',').map(|n| n.trim().parse().ok()).collect::<Option<_>>()?;
                    let &[radius, raise, share] = numbers.as_slice() else { return None };
                    Some((part.trim().to_ascii_lowercase(), Tune { radius: radius.max(0.001), raise, share: share.max(0.0) }))
                });
                if parsed.is_none() {
                    warn!("HALO_RT_LIGHT_TUNE: {entry:?} is not <tag name part>=<radius>,<raise>,<share>");
                }
                parsed
            })
            .collect()
    });
    let name = name.to_ascii_lowercase();
    TUNES.iter().find(|(part, _)| name.contains(part.as_str())).map(|&(_, tune)| tune)
}

/// The light tag (none in the maps) of a test light given no tag's name, or
/// one the map hasn't, and the name it is tuned by ([`light_tune`]).
const TEST_LIGHT_DEFINITION: u32 = u32::MAX - 1;
const TEST_LIGHT_NAME: &str = "test";
/// The test lights' objects (none in the game), from this one up. (The
/// game's first-person models, `halo_bridge::OBJECT_MODEL`, take the same
/// range, `0xFFFF0000 | slot`: no game light is attached to one, so the
/// lights' keys don't meet, but nothing should look a test light's object
/// up among the snapshot's.)
const TEST_LIGHT_OBJECT: u32 = 0xFFFF_0000;

/// Lights put in the level as if the game had sent them, for diagnosing a
/// light's look where the game can't be made to show it (a plasma pistol
/// dropped against a wall): in the level viewer, with no game, and bridged
/// beside the game's. `HALO_RT_TEST_LIGHT="x,y,z,r,g,b,radius[,tag name]"`,
/// several separated by `;`: the position in the game's world units and
/// axes (as the bridge's lights'), the colour 0..1 a channel, the reach in
/// world units, and a light tag's name from the loaded map, whose
/// treatment here it takes (`LIGHT_DIMMING`, `SMALL_LIGHTS`,
/// `UPWARD_LIGHTS`, a muzzle flash's by its name); without one, or with one
/// the map hasn't, a plain point light. Each is built by `sync_lights` as a
/// bridged light, shining every way. Read at each level's load.
#[derive(Resource, Default)]
pub struct TestLights {
    lights: Vec<Light>,
    /// The tags' names of those given one the map has, by its datum: the
    /// level viewer keeps no map to look them up in.
    names: HashMap<u32, String>,
}

impl TestLights {
    pub fn from_env(cache: &halo_map::CacheFile) -> Self {
        let mut test = Self::default();
        let Ok(text) = std::env::var("HALO_RT_TEST_LIGHT") else { return test };
        for (index, entry) in text.split(';').map(str::trim).filter(|entry| !entry.is_empty()).enumerate() {
            let fields: Vec<&str> = entry.splitn(8, ',').map(str::trim).collect();
            let numbers: Option<Vec<f32>> = fields.iter().take(7).map(|field| field.parse().ok()).collect();
            let Some(n) = numbers.filter(|n| n.len() == 7) else {
                warn!("HALO_RT_TEST_LIGHT: {entry:?} is not x,y,z,r,g,b,radius[,tag name]");
                continue;
            };
            let name = fields.get(7).copied().filter(|name| !name.is_empty());
            let tag = name.and_then(|name| cache.find_tag(halo_map::groups::LIGHT, name));
            if let (Some(name), None) = (name, tag) {
                warn!("HALO_RT_TEST_LIGHT: no light tag {name:?} in {}: a plain point light", cache.header.name);
            }
            let definition_index = tag.map_or(TEST_LIGHT_DEFINITION, |tag| {
                test.names.insert(tag.datum, tag.name.clone());
                tag.datum
            });
            test.lights.push(Light {
                position: [n[0], n[1], n[2]],
                forward: [1.0, 0.0, 0.0],
                up: [0.0, 0.0, 1.0],
                color: [n[3], n[4], n[5]],
                radius: n[6],
                cutoff_angle: std::f32::consts::PI,
                falloff_angle: std::f32::consts::PI,
                definition_index,
                object_index: TEST_LIGHT_OBJECT + index as u32,
                ..default()
            });
        }
        test
    }

    /// Whether any were asked for (`HALO_RT_TEST_LIGHT`).
    pub fn asked() -> bool {
        std::env::var_os("HALO_RT_TEST_LIGHT").is_some()
    }

    /// A test light's tag's name, by its datum.
    pub(crate) fn name(&self, definition_index: u32) -> Option<&str> {
        if definition_index == TEST_LIGHT_DEFINITION {
            return Some(TEST_LIGHT_NAME);
        }
        self.names.get(&definition_index).map(String::as_str)
    }
}

pub struct DynamicLightsPlugin;

impl Plugin for DynamicLightsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DynamicLights>().init_resource::<ShownLights>().add_systems(Update, sync_lights);
    }
}

/// A light's entity from frame to frame, by its tag, its object and its
/// place among that pair's (gel beams, contrail glows). Handed out by the
/// lights' order in the game's list, entities swapped lights whenever
/// another came or went (a muzzle flash, a bolt), and the lighting's
/// history went with them: the Warthog's headlights flickered for a frame.
type LightKey = (u32, u32, u32);

#[derive(Resource, Default)]
struct DynamicLights {
    points: HashMap<LightKey, Entity>,
    spots: HashMap<LightKey, Entity>,
    /// Hidden, for the lights to come.
    free_points: Vec<Entity>,
    free_spots: Vec<Entity>,
}

/// A spot light's entity: the one it had, a free one, or a new one (made as
/// it is now, to shine this frame).
#[allow(clippy::too_many_arguments)]
fn place_spot(
    commands: &mut Commands,
    spots: &mut Query<(&mut SpotLight, &mut Transform, &mut Visibility), Without<PointLight>>,
    last: &mut HashMap<LightKey, Entity>,
    free: &mut Vec<Entity>,
    now: &mut HashMap<LightKey, Entity>,
    key: LightKey,
    light: SpotLight,
    transform: Transform,
) {
    let reused = last.remove(&key).or_else(|| free.pop());
    let entity = match reused.map(|entity| (entity, spots.get_mut(entity))) {
        Some((entity, Ok((mut light_component, mut light_transform, mut visibility)))) => {
            *light_component = light;
            *light_transform = transform;
            visibility.set_if_neq(Visibility::Inherited);
            entity
        }
        _ => commands.spawn((Name::new("spotlight"), light, transform, Visibility::Inherited, NoFrustumCulling, SolarikUnreflectedLight)).id(),
    };
    now.insert(key, entity);
}

/// The point lights' query: each one's cone, if it has one.
type PointLights<'w, 's> =
    Query<'w, 's, (&'static mut PointLight, &'static mut Transform, &'static mut Visibility, Option<&'static SolarikLightCone>), Without<SpotLight>>;

/// A point light's entity (as [`place_spot`]), with its cone or none (a
/// reused entity's given or taken away when it differs).
#[allow(clippy::too_many_arguments)]
fn place_point(
    commands: &mut Commands,
    points: &mut PointLights,
    last: &mut HashMap<LightKey, Entity>,
    free: &mut Vec<Entity>,
    now: &mut HashMap<LightKey, Entity>,
    key: LightKey,
    light: PointLight,
    transform: Transform,
    cone: Option<SolarikLightCone>,
) {
    let reused = last.remove(&key).or_else(|| free.pop());
    let entity = match reused.map(|entity| (entity, points.get_mut(entity))) {
        Some((entity, Ok((mut light_component, mut light_transform, mut visibility, had)))) => {
            *light_component = light;
            *light_transform = transform;
            visibility.set_if_neq(Visibility::Inherited);
            if had.copied() != cone {
                match cone {
                    Some(cone) => commands.entity(entity).insert(cone),
                    None => commands.entity(entity).remove::<SolarikLightCone>(),
                };
            }
            entity
        }
        _ => {
            let mut spawned = commands.spawn((Name::new("light"), light, transform, Visibility::Inherited, NoFrustumCulling, SolarikUnreflectedLight));
            if let Some(cone) = cone {
                spawned.insert(cone);
            }
            spawned.id()
        }
    };
    now.insert(key, entity);
}

/// The lights `sync_lights` took this frame, each with what it made of it,
/// for the F9 capture's note (`crate::capture`): a capture of a lighting
/// problem names the light behind it. The numbers alone, written out only
/// when a capture is taken.
#[derive(Resource, Default)]
pub struct ShownLights(pub Vec<ShownLight>);

/// A light as the game sent it (or a projectile's, a contrail's, a test
/// light) and as it was built.
#[derive(Clone, Copy)]
pub struct ShownLight {
    pub light: Light,
    pub test: bool,
    pub spot: bool,
    pub upward: bool,
    /// Drawn as its gel's beams (spot lights).
    pub gelled: bool,
    /// Dimmed to none, shining upward at no share, or a projectile's glow at
    /// the world's origin: not in the scene.
    pub left_out: bool,
    /// The source sphere's radius and raise (metres).
    pub radius: f32,
    pub raise: f32,
    /// Its share of its brightness ([`LIGHT_DIMMING`]).
    pub dimming: f32,
    /// Its power (lumens a channel, at an emissive scale of one).
    pub power: Vec3,
    pub tuned: bool,
}

impl ShownLight {
    /// The capture note's line for it, `camera` the camera's place
    /// (metres), `tag` and `object` the names of its light tag and object.
    pub fn note(&self, camera: Vec3, tag: Option<&str>, object: Option<&str>) -> String {
        let light = &self.light;
        let [x, y, z] = light.position;
        let [r, g, b] = light.color;
        let distance = Vec3::from(halo_to_bevy(light.position, METERS_PER_WORLD_UNIT)).distance(camera);
        let tag = tag.map_or_else(|| format!("{:#010x}", light.definition_index), str::to_string);
        // (a test light's object index is made up: it has no object)
        let object = if self.test || light.object_index == u32::MAX {
            "none".to_string()
        } else {
            format!("{:#010x} {}", light.object_index, object.unwrap_or("-"))
        };
        let kind = if self.left_out {
            "left out"
        } else if self.gelled {
            "gelled spots"
        } else if self.spot {
            "spot"
        } else if self.upward {
            "upward"
        } else {
            "point"
        };
        let [pr, pg, pb] = self.power.to_array();
        format!(
            "{distance:.2} m | at {x:.3},{y:.3},{z:.3} | colour {r:.3},{g:.3},{b:.3} | radius {:.3} | flags {:#x}{}{} | tag {tag} | object {object} | {kind}, source {:.3} m, raise {:.3} m, dimming {:.2}, power {pr:.0},{pg:.0},{pb:.0} lm, {}",
            light.radius,
            light.definition_flags,
            if light.first_person != 0 { " first person" } else { "" },
            if self.test { " test light" } else { "" },
            self.radius,
            self.raise,
            self.dimming,
            if self.tuned { "tuned" } else { "untuned" },
        )
    }
}

/// The game's lights (and the test lights, [`TestLights`]) as the scene's:
/// bridged, or in the level viewer the test lights alone.
#[allow(clippy::too_many_arguments)]
fn sync_lights(
    mut commands: Commands,
    link: Option<Res<BridgeLink>>,
    test: Option<Res<TestLights>>,
    settings: Res<LightingSettings>,
    mut pools: ResMut<DynamicLights>,
    mut points: PointLights,
    mut spots: Query<(&mut SpotLight, &mut Transform, &mut Visibility), Without<PointLight>>,
    level: Option<Res<BridgeLevel>>,
    in_solid: Option<Res<crate::bridge::LightsInSolid>>,
    mut light_tags: Local<HashMap<u32, LightTag>>,
    mut gels: Local<HashMap<u32, Vec<GelBeam>>>,
    mut glows: Local<HashMap<u32, Option<([f32; 3], f32)>>>,
    mut contrail_glows: Local<HashMap<u32, Option<([f32; 3], f32)>>>,
    mut scenario: Local<String>,
    mut test_logged: Local<bool>,
    mut shown: ResMut<ShownLights>,
) {
    shown.0.clear();
    static LIGHT_LOG: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var_os("HALO_RT_LIGHT_LOG").is_some());
    let snapshot = link.as_ref().and_then(|link| link.latest.as_ref());
    let test_lights = test.as_deref().map_or(&[][..], |test| test.lights.as_slice());
    // (a level's test lights, anew: logged as built, their tags looked up)
    if test.as_ref().is_some_and(|test| test.is_changed()) {
        *test_logged = false;
        light_tags.clear();
    }
    if snapshot.is_none() && test_lights.is_empty() {
        return;
    }
    // (a new level: its own tags)
    if let Some(snapshot) = snapshot
        && snapshot.scenario_name != *scenario
    {
        light_tags.clear();
        gels.clear();
        glows.clear();
        contrail_glows.clear();
        *scenario = snapshot.scenario_name.clone();
    }
    let camera = snapshot.and_then(|snapshot| snapshot.camera.as_ref());
    // the glowing projectiles' lights, round from their middles (by no tag:
    // neither muzzle flashes nor gelled; hidden ones too: a projectile
    // without a model, a Sentinel's beam, the game hides; not those outside
    // the map: b40's two Wraith mortars, kept at the world's origin)
    let projectile_lights: Vec<Light> = snapshot
        .into_iter()
        .flat_map(|snapshot| snapshot.objects.iter())
        .filter(|object| object.object_flags & OBJECT_OUTSIDE_OF_MAP == 0)
        .filter_map(|object| {
            let (color, radius) = (*glows.entry(object.definition_index).or_insert_with(|| {
                let tag = level.as_ref()?.builder.cache.tag(object.definition_index).ok()?;
                PROJECTILE_GLOWS.iter().find(|(name, ..)| tag.name.eq_ignore_ascii_case(name)).map(|&(_, color, radius)| (color, radius))
            }))?;
            Some(Light {
                position: object.bounding_center,
                forward: [1.0, 0.0, 0.0],
                up: [0.0, 0.0, 1.0],
                color,
                radius,
                cutoff_angle: std::f32::consts::PI,
                falloff_angle: std::f32::consts::PI,
                definition_index: u32::MAX,
                object_index: object.object_index,
                ..default()
            })
        })
        .collect();
    // the glowing contrails' lights, evenly along each (the rest of the
    // contrails light nothing)
    let mut contrail_lights: Vec<Light> = Vec::new();
    for contrail in snapshot.into_iter().flat_map(|snapshot| snapshot.lights.iter()).filter(|light| light.definition_flags & LIGHT_CONTRAIL != 0) {
        let Some((color, radius)) = *contrail_glows.entry(contrail.definition_index).or_insert_with(|| {
            let tag = level.as_ref()?.builder.cache.tag(contrail.definition_index).ok()?;
            CONTRAIL_GLOWS.iter().find(|(name, ..)| tag.name.eq_ignore_ascii_case(name)).map(|&(_, color, radius)| (color, radius))
        }) else {
            continue;
        };
        let (middle, along, length) = (Vec3::from(contrail.position), Vec3::from(contrail.forward), 2.0 * contrail.radius);
        let count = ((length / CONTRAIL_GLOW_SPACING).ceil() as usize).clamp(1, CONTRAIL_GLOW_MOST);
        for index in 0..count {
            let offset = ((index as f32 + 0.5) / count as f32 - 0.5) * length;
            contrail_lights.push(Light {
                position: (middle + along * offset).to_array(),
                forward: [1.0, 0.0, 0.0],
                up: [0.0, 0.0, 1.0],
                color,
                radius,
                cutoff_angle: std::f32::consts::PI,
                falloff_angle: std::f32::consts::PI,
                definition_index: u32::MAX,
                object_index: contrail.object_index,
                ..default()
            });
        }
    }
    // (but those in the drawn BSP's solid, another BSP's: crate::bridge)
    let lights = snapshot.into_iter().flat_map(|snapshot| snapshot.lights.iter()).filter(|light| {
        light.definition_flags & LIGHT_CONTRAIL == 0
            && !in_solid.as_ref().is_some_and(|in_solid| in_solid.0.contains(&(light.definition_index, light.object_index)))
    });
    let pools = &mut *pools;
    let (mut last_points, mut last_spots) = (std::mem::take(&mut pools.points), std::mem::take(&mut pools.spots));
    // (each tag and object's lights so far, numbering them)
    let mut placed: HashMap<(u32, u32), u32> = HashMap::new();
    let mut key = |light: &Light| {
        let count = placed.entry((light.definition_index, light.object_index)).or_insert(0);
        *count += 1;
        (light.definition_index, light.object_index, *count - 1)
    };
    // (the test lights after the game's; each with whether it is a test
    // light and a projectile's glow)
    let all = lights
        .map(|light| (light, false, false))
        .chain(projectile_lights.iter().map(|light| (light, false, true)))
        .chain(contrail_lights.iter().map(|light| (light, false, false)))
        .chain(test_lights.iter().map(|light| (light, true, false)));
    for (light, test_light, projectile) in all {
        let near = camera.filter(|camera| Vec3::from(camera.position).distance(Vec3::from(light.position)) < FLASHLIGHT_REACH);
        let own = light.first_person != 0;
        // (a muzzle flash: its tag's flag, or its name; a test light's tag
        // by the name it was given, the level viewer having no map)
        let tag = *light_tags.entry(light.definition_index).or_insert_with(|| {
            let name = level
                .as_ref()
                .and_then(|level| level.builder.cache.tag(light.definition_index).ok())
                .map(|tag| tag.name.as_str())
                .or_else(|| test.as_deref().and_then(|test| test.name(light.definition_index)));
            light_tag(name)
        });
        let muzzle_tag = light.definition_flags & LIGHT_MUZZLE_FLASH != 0 || tag.muzzle;
        let muzzle = light.cutoff_angle > SPOT_CUTOFF && muzzle_tag;
        let spot = light.cutoff_angle <= SPOT_CUTOFF || muzzle;
        // (the player's spot lights follow the camera's aim)
        let near_camera = camera.filter(|_| spot && (own || (!muzzle && near.is_some())));
        // (one shining upward: a point light's but for its cone)
        let upward = tag.upward.filter(|_| !spot);
        // (a point light's tuning: HALO_RT_LIGHT_TUNE)
        let tune = tag.tune.filter(|_| !spot);
        let forward = Vec3::from(halo_to_bevy(near_camera.map_or(light.forward, |camera| camera.forward), 1.0)).normalize_or(Vec3::NEG_Z);
        let vehicle = spot && !muzzle && near_camera.is_none();
        let flashlight = spot && !muzzle && near_camera.is_some();
        let ahead = match (muzzle, spot) {
            (true, _) if near_camera.is_some() => MUZZLE_FORWARD,
            (true, _) => OTHERS_MUZZLE_FORWARD,
            (false, true) if vehicle => SPOT_FORWARD + VEHICLE_SPOT_FORWARD,
            (false, true) => SPOT_FORWARD,
            (false, false) => 0.0,
        };
        let drop = if vehicle { Vec3::NEG_Y * VEHICLE_SPOT_DROP } else { Vec3::ZERO };
        let position = Vec3::from(halo_to_bevy(light.position, METERS_PER_WORLD_UNIT)) + forward * ahead + drop;
        let scale = if muzzle {
            MUZZLE_LIGHT_SCALE
        } else if vehicle {
            SPOT_LIGHT_SCALE * VEHICLE_SPOT_SCALE
        } else if flashlight {
            SPOT_LIGHT_SCALE * FLASHLIGHT_SCALE
        } else if spot {
            SPOT_LIGHT_SCALE
        } else {
            POINT_LIGHT_SCALE * upward.map_or(1.0, |upward| upward.share)
        } * tag.dimming;
        let reach2 = if spot && !muzzle { light.radius * SPOT_REFERENCE_REACH } else { light.radius * light.radius };
        let radiance = Vec3::from(light.color) * RADIANCE_PER_REACH2 * scale * reach2;
        let range = light.radius * METERS_PER_WORLD_UNIT * RANGE_PER_RADIUS;
        let radius = if spot { SPOT_RADIUS } else { tag.small.unwrap_or(LIGHT_RADIUS) };
        // (an upward light at its own height, the rest clear of the ground)
        let raise = upward.map_or_else(|| tag.small.map_or(0.0, |size| size * 1.2), |upward| upward.raise);
        let power = radiance * sphere_lumens_per_radiance(radius);
        // (tuned: its sphere's size and height, its power its untuned
        // size's times the share; see light_tune)
        let (radius, raise, power) = match tune {
            Some(tune) => (tune.radius, tune.raise, power * tune.share),
            None => (radius, raise, power),
        };
        let position = position + Vec3::Y * raise;
        let source = LightSource { power };
        let (color, intensity) = source.color_and_intensity(settings.emissive_scale);
        // (HALO_RT_LIGHT_LOG: each light, in the log)
        if *LIGHT_LOG {
            let distance = camera.map(|c| Vec3::from(c.position).distance(Vec3::from(light.position)));
            info!(
                "light at tick {}: position {:?} forward {:?} colour {:?} radius {} cutoff {:.1} falloff {:.1} deg, {distance:?} from the camera (forward {:?}), own {own} muzzle {muzzle} aimed by the camera {} upward {:?} tag {:#x} flags {:#x} object {:#x}; source radius {radius} m raise {raise} m power {power:.1?} lm ({intensity:.1} lm at the emissive scale) tuned {tune:?} test {test_light}",
                snapshot.map_or(0, |snapshot| snapshot.game_tick), light.position, light.forward, light.color, light.radius, light.cutoff_angle.to_degrees(), light.falloff_angle.to_degrees(),
                camera.map(|c| c.forward), near_camera.is_some(), tag.upward, light.definition_index, light.definition_flags, light.object_index
            );
        }
        // (a test light, once as built)
        if test_light && !*test_logged {
            let name = test.as_deref().and_then(|test| test.name(light.definition_index)).unwrap_or(TEST_LIGHT_NAME);
            info!(
                "test light {name:?} at {position} m: radiance {radiance:.1?}, power {power:.1?} lm ({intensity:.1} lm at the emissive scale), range {range:.2} m, source radius {radius} m, raised {raise} m{}{}{}",
                if spot { ", a spot light (a muzzle flash's name)" } else { "" },
                if upward.is_some() { ", shining upward" } else { "" },
                if tag.dimming <= 0.0 { ", dimmed to none: left out" } else { "" },
            );
        }
        // (dimmed to none, or shining upward at no share: left out; and a
        // projectile's glow at exactly the world's origin, where the game
        // holds objects not in the world: three wraith mortar glows at 0,0,0
        // in b40's notes of 13 November; HALO_RT_ORIGIN_GLOWS=1 the old)
        let at_origin = projectile && light.position == [0.0; 3] && !origin_glows();
        let left_out = tag.dimming <= 0.0 || upward.is_some_and(|upward| upward.share <= 0.0) || at_origin;
        shown.0.push(ShownLight {
            light: *light,
            test: test_light,
            spot,
            upward: upward.is_some(),
            gelled: false,
            left_out,
            radius,
            raise,
            dimming: tag.dimming,
            power,
            tuned: tune.is_some(),
        });
        if left_out {
            continue;
        }
        // a light shining every way through a gel of narrow discs: a spot
        // light along each, by the light's forward, left and up (the game
        // turns the gel with them), each as bright in its cone as the light
        // was there, by the gel
        let beams = if spot || upward.is_some() {
            &[][..]
        } else {
            gels.entry(light.definition_index)
                .or_insert_with(|| {
                    let beams = level.as_ref().and_then(|level| gel_beams(&level.builder.cache, light.definition_index).ok()).unwrap_or_default();
                    if beams.iter().all(|beam| beam.cutoff_angle <= GEL_WIDEST) { beams } else { Vec::new() }
                })
                .as_slice()
        };
        let (light_forward, light_up) = (Vec3::from(light.forward), Vec3::from(light.up));
        let light_left = light_forward.cross(light_up);
        if !beams.is_empty() && light_left.length_squared() > 1e-6 {
            if let Some(shown) = shown.0.last_mut() {
                shown.gelled = true;
            }
            let light_left = light_left.normalize();
            let light_up = light_left.cross(light_forward).normalize();
            let source = LightSource { power: radiance * GEL_LIGHT_SCALE * sphere_lumens_per_radiance(SPOT_RADIUS) };
            for beam in beams {
                let [f, l, u] = beam.direction;
                let along = light_forward * f + light_left * l + light_up * u;
                let forward = Vec3::from(halo_to_bevy(along.to_array(), 1.0)).normalize_or(Vec3::NEG_Z);
                let (color, intensity) = LightSource { power: source.power * beam.strength }.color_and_intensity(settings.emissive_scale);
                // (never frustum culled, here and below: Bevy would leave a
                // light out of the scene with its range off screen, and its
                // bounces with it)
                let outer = beam.cutoff_angle.clamp(0.01, SPOT_WIDEST);
                let spot_light = SpotLight {
                    color,
                    intensity,
                    range,
                    radius: SPOT_RADIUS,
                    outer_angle: outer,
                    inner_angle: beam.falloff_angle.clamp(0.0, outer * 0.99),
                    shadow_maps_enabled: false,
                    ..default()
                };
                let transform = Transform::from_translation(position).looking_to(forward, if forward.y.abs() > 0.99 { Vec3::X } else { Vec3::Y });
                place_spot(&mut commands, &mut spots, &mut last_spots, &mut pools.free_spots, &mut pools.spots, key(light), spot_light, transform);
            }
            continue;
        }
        if spot {
            let (cutoff, falloff) = if muzzle {
                (MUZZLE_CUTOFF, MUZZLE_FALLOFF)
            } else if vehicle {
                (light.cutoff_angle * VEHICLE_SPOT_CONE, light.falloff_angle * VEHICLE_SPOT_CONE)
            } else if flashlight {
                (light.cutoff_angle * FLASHLIGHT_CONE, light.falloff_angle * FLASHLIGHT_CONE)
            } else {
                (light.cutoff_angle, light.falloff_angle)
            };
            let outer = cutoff.clamp(0.01, SPOT_WIDEST);
            let spot_light = SpotLight {
                color,
                intensity,
                range,
                radius,
                outer_angle: outer,
                inner_angle: falloff.clamp(0.0, outer * 0.99),
                shadow_maps_enabled: false,
                ..default()
            };
            // (a spot light shines along its -Z)
            let transform = Transform::from_translation(position).looking_to(forward, if forward.y.abs() > 0.99 { Vec3::X } else { Vec3::Y });
            place_spot(&mut commands, &mut spots, &mut last_spots, &mut pools.free_spots, &mut pools.spots, key(light), spot_light, transform);
        } else if let Some(Upward { inner, outer, .. }) = upward {
            // (a point light in a cone wider than a spot light's: Solarik's
            // cone, along the transform's forward, its -Z, the world's up)
            let point_light = PointLight { color, intensity, range, radius, shadow_maps_enabled: false, ..default() };
            let transform = Transform::from_translation(position).looking_to(Vec3::Y, Vec3::X);
            let cone = SolarikLightCone { inner, outer };
            place_point(&mut commands, &mut points, &mut last_points, &mut pools.free_points, &mut pools.points, key(light), point_light, transform, Some(cone));
        } else {
            let point_light = PointLight { color, intensity, range, radius, shadow_maps_enabled: false, ..default() };
            let transform = Transform::from_translation(position);
            place_point(&mut commands, &mut points, &mut last_points, &mut pools.free_points, &mut pools.points, key(light), point_light, transform, None);
        }
    }
    if !test_lights.is_empty() {
        *test_logged = true;
    }
    // (the rest go dark, free for the lights to come)
    for (_, entity) in last_points {
        if let Ok((_, _, mut visibility, _)) = points.get_mut(entity) {
            visibility.set_if_neq(Visibility::Hidden);
        }
        pools.free_points.push(entity);
    }
    for (_, entity) in last_spots {
        if let Ok((_, _, mut visibility)) = spots.get_mut(entity) {
            visibility.set_if_neq(Visibility::Hidden);
        }
        pools.free_spots.push(entity);
    }
}
