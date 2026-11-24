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
use bevy_solarik::scene::{SolarikHollowLight, SolarikLightCone, SolarikUnreflectedLight};
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
/// How far ahead of the game's position a fixed spot light shines from
/// (metres): the light stands at its marker on the object. A fixed spot
/// light is one carried by scenery, a machine, a control or a light fixture
/// ([`LightHolder::Fixed`]; a30's holo control indicator, two spots of
/// `holo indicator` on the wall beside it): it shines along its own forward
/// in its tag's cone at a spot light's brightness, wherever the camera is
/// (the user's play of 20 November 2026: "the light on the wall changes
/// based on your proximity to it": near the camera it was taken for the
/// flashlight and aimed by the view, further off for a vehicle's headlight,
/// pushed ahead and dropped). `HALO_RT_FIXED_SPOT_FORWARD=<metres>` sets
/// this for a run ([`fixed_spot_forward`]); `HALO_RT_FIXED_SPOTS=0` the old
/// reading, such a light a flashlight within [`FLASHLIGHT_REACH`] of the
/// camera and a headlight further off ([`fixed_spots`]). Each read once.
const FIXED_SPOT_FORWARD: f32 = 0.0;
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
/// (world units, as a light tag's radius), and the glow's share of that
/// brightness (as a light tag's [`LIGHT_DIMMING`]; set for a run by the
/// projectile's tag's name with [`light_dim`]'s `HALO_RT_LIGHT_DIM`).
const PROJECTILE_GLOWS: [(&str, [f32; 3], f32, f32); 3] = [
    // (its exhaust's flame)
    ("weapons\\rocket launcher\\rocket", [1.0, 0.55, 0.2], 2.0, 1.0),
    // (pink crystal, in flight and stuck in what it hit; play feedback of
    // 15 November 2026: 50% less)
    ("weapons\\needler\\needle", [1.0, 0.3, 0.8], 0.75, 0.5),
    // (a ball of blue plasma)
    ("vehicles\\wraith\\wraith mortar", [0.5, 0.45, 1.0], 2.0, 1.0),
];

/// Whether a projectile's glow at exactly the world's origin is a light, as
/// before (`HALO_RT_ORIGIN_GLOWS=1`): by default it is left out, an object
/// the game holds there, not in the world yet or already gone, whose glow lit
/// whatever was near the map's origin. Read once.
fn origin_glows() -> bool {
    static ORIGIN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ORIGIN.get_or_init(|| std::env::var("HALO_RT_ORIGIN_GLOWS").is_ok_and(|v| v.trim() == "1"))
}

/// Whether spot lights carried by fixed objects are read as fixed (see
/// [`FIXED_SPOT_FORWARD`]): `HALO_RT_FIXED_SPOTS=0` the old, every one a
/// flashlight near the camera and a headlight further off. Read once.
fn fixed_spots() -> bool {
    static FIXED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FIXED.get_or_init(|| !std::env::var("HALO_RT_FIXED_SPOTS").is_ok_and(|v| v.trim() == "0"))
}

/// How far ahead of the game's position a fixed spot light shines from
/// (metres): [`FIXED_SPOT_FORWARD`], or `HALO_RT_FIXED_SPOT_FORWARD`'s (a
/// value that is not a finite number ignored with a warning). Read once.
fn fixed_spot_forward() -> f32 {
    static FORWARD: std::sync::LazyLock<f32> = std::sync::LazyLock::new(|| {
        let text = std::env::var("HALO_RT_FIXED_SPOT_FORWARD").ok();
        parse_fixed_spot_forward(text.as_deref()).unwrap_or_else(|text| {
            warn!("HALO_RT_FIXED_SPOT_FORWARD: {text:?} is not <metres>");
            FIXED_SPOT_FORWARD
        })
    });
    *FORWARD
}

/// [`fixed_spot_forward`]'s metres from the variable's text: none set
/// [`FIXED_SPOT_FORWARD`], a text that is not a finite number the error.
fn parse_fixed_spot_forward(text: Option<&str>) -> Result<f32, String> {
    match text {
        None => Ok(FIXED_SPOT_FORWARD),
        Some(text) => text.trim().parse::<f32>().ok().filter(|metres| metres.is_finite()).ok_or_else(|| text.to_string()),
    }
}

/// What carries a light tag, by the groups of the object tags whose
/// attachments hold it (the maps' attached spot lights: Chief's and the
/// assault rifle's flashlights, the Warthog's, the Scorpion's and
/// the lifepods' lights, a30's holo control indicator's).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LightHolder {
    /// Scenery, a machine, a control or a light fixture: the light stays
    /// where the game puts it.
    Fixed,
    /// A vehicle: its headlights.
    Vehicle,
    /// A biped or a weapon: a flashlight.
    Carried,
}

/// The holder of a light tag held by object tags of `groups`: a biped's or
/// a weapon's [`LightHolder::Carried`], a vehicle's
/// [`LightHolder::Vehicle`], scenery's, a machine's, a control's or a light
/// fixture's [`LightHolder::Fixed`], the first winning over the second over
/// the third where several hold it; none by any other group or none at all.
fn light_holder(groups: impl IntoIterator<Item = u32>) -> Option<LightHolder> {
    use halo_map::fourcc;
    groups
        .into_iter()
        .filter_map(|group| match group {
            g if g == fourcc(b"bipd") || g == fourcc(b"weap") => Some(LightHolder::Carried),
            g if g == fourcc(b"vehi") => Some(LightHolder::Vehicle),
            g if g == fourcc(b"scen") || g == fourcc(b"mach") || g == fourcc(b"ctrl") || g == fourcc(b"lifi") => Some(LightHolder::Fixed),
            _ => None,
        })
        .max()
}

/// Each light tag's holder in a map (by the light tag's datum), from every
/// object tag's attachments ([`light_holder`]); a light tag no object holds
/// is not in it.
fn light_holders(cache: &halo_map::CacheFile) -> HashMap<u32, LightHolder> {
    let mut groups: HashMap<u32, Vec<u32>> = HashMap::new();
    for tag in cache.tags.iter() {
        let Ok(root) = cache.tag_root(tag.datum, halo_map::groups::OBJECT) else { continue };
        let Ok(attachments) = root.block(halo_map::layout::_object_definition::ATTACHMENTS, 0x48) else { continue };
        for attachment in attachments.iter() {
            let Ok(reference) = attachment.reference(0) else { continue };
            if reference.group != halo_map::groups::LIGHT {
                continue;
            }
            if let Some(light) = reference.datum {
                groups.entry(light).or_default().push(tag.group);
            }
        }
    }
    groups.into_iter().filter_map(|(light, groups)| Some((light, light_holder(groups)?))).collect()
}

/// What a spot light is taken as ([`spot_reading`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SpotKind {
    /// Not a spot light, or a muzzle flash.
    #[default]
    Neither,
    /// The player's flashlight: aimed by the camera.
    Flashlight,
    /// A vehicle's headlight: further ahead, dropped, narrower, dimmer.
    Vehicle,
    /// A fixed object's ([`FIXED_SPOT_FORWARD`]): as the game has it.
    Fixed,
}

/// [`spot_reading`]'s answer: what the light is taken as, and whether it is
/// aimed by the camera (the player's flashlight, or their muzzle flash).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SpotReading {
    kind: SpotKind,
    aimed: bool,
}

/// What a light is taken as: `camera` whether the game sent a camera,
/// `spot` and `muzzle` whether it is a spot light and a muzzle flash, `own`
/// marked first person, `near` within [`FLASHLIGHT_REACH`] of the camera,
/// `holder` its tag's ([`light_holders`]), `fixed_rule` [`fixed_spots`]'s.
/// A spot light that is the player's (first person, or near the camera and
/// not a muzzle flash) is aimed by the camera; one not a muzzle flash is the
/// flashlight if aimed and a vehicle's headlight if not; but with the rule,
/// one not a muzzle flash, not first person and held by a fixed object is
/// fixed wherever the camera is.
fn spot_reading(camera: bool, spot: bool, muzzle: bool, own: bool, near: bool, holder: Option<LightHolder>, fixed_rule: bool) -> SpotReading {
    if fixed_rule && spot && !muzzle && !own && holder == Some(LightHolder::Fixed) {
        return SpotReading { kind: SpotKind::Fixed, aimed: false };
    }
    let aimed = camera && spot && (own || (!muzzle && near));
    let kind = match (spot && !muzzle, aimed) {
        (false, _) => SpotKind::Neither,
        (true, true) => SpotKind::Flashlight,
        (true, false) => SpotKind::Vehicle,
    };
    SpotReading { kind, aimed }
}

/// The contrails that glow (by their contrail tag's name), with the colour
/// and reach (world units) of the lights along them and their share of that
/// brightness (as a light tag's [`LIGHT_DIMMING`]; set for a run by the
/// contrail's tag's name with [`light_dim`]'s `HALO_RT_LIGHT_DIM`): one each
/// this far apart (world units), up to this many. The share is the light
/// the contrail casts alone; the beam itself is drawn as the game has it.
const CONTRAIL_GLOWS: [(&str, [f32; 3], f32, f32); 1] = [
    // (the Sentinels' beam: an orange laser; play feedback of 19 November
    // 2026: "reduce the sentinel beam brightness just a bit", 30% less;
    // `HALO_RT_LIGHT_DIM="characters\sentinel\beam=1"` the old)
    ("characters\\sentinel\\beam", [1.0, 0.45, 0.15], 1.0, 0.7),
];
const CONTRAIL_GLOW_SPACING: f32 = 3.0;
const CONTRAIL_GLOW_MOST: usize = 4;

/// Lights dimmed from their tags' brightness (by the light tag's name, or
/// a folder's, ending in a backslash): a wide reach is a bright light here
/// (brightness grows with its square). Dimmed to 0, left out. Each share
/// can be set for a run with [`light_dim`]'s `HALO_RT_LIGHT_DIM`.
const LIGHT_DIMMING: [(&str, f32); 19] = [
    // (Guilty Spark's, reaching 5.5 world units: it lit rooms like
    // floodlights; play feedback of 15 November 2026: "something like 50%"
    // less, here 60%: in c40's control room cutscene he hangs half a metre
    // from the console, whose top was 96% clipped at 0.15, 43% at half and
    // 29% at this. His own shell, a white ball since the light's sphere
    // lights what is inside it (14 November), is `HOLLOW_LIGHTS`' matter:
    // no share of his light shows his insides)
    ("characters\\monitor\\monitor dynamic", 0.06),
    // (the grenades' glow in flight, reaching 10, the frag's 50% less and
    // the plasma's 20% less by play feedback of 15 November 2026, and their
    // explosions', as before)
    ("weapons\\frag grenade\\frag grenade", 0.35),
    ("weapons\\plasma grenade\\plasma grenade", 0.56),
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
    // (a10's crewman's pair, the other baton light tag, reaching 0.5 from
    // each stick, blinking in turn, 3,700 lm of blue at its brightest: a
    // bright blue pool on the deck under him in the opening cutscene; play
    // feedback of 18 November: "tone down the brightness on his batons
    // significantly? They cast enough light that its odd looking". A
    // twentieth: the sticks as bright, a faint glow on the deck and his
    // legs)
    ("scenery\\baton\\baton light", 0.05),
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
    // (c40's control room console's light in the cutscene, reaching 3,
    // green, 116,000 lm of it at the console's own top: with the Monitor's
    // it blew the console out white; play feedback of 15 November: dimmer.
    // A twentieth leaves the console its green cast and its texture)
    ("cinematics\\effects\\cortana powerup fx\\cortana powerup", 0.05),
    // (the pulse generators' two lights on b40 and c40, `generator` and
    // `generator overload`, reaching 3 from the same marker, one cyan and
    // one white whenever the other's function is 0 (the game's colour
    // interpolation gives a light with no lower alpha white): the pylons and
    // the room round them were washed out; play feedback of 15 November:
    // dimmer. A twentieth: the room still lit by the beam, the walls and
    // the floor with their texture, the picture's mean 154 to 86 of 255,
    // 37 with the lights off)
    ("levels\\b40\\devices\\b40_generator_fx\\", 0.05),
    // (b30's two hologram rooms' lights: the violet holo beam's, reaching 5
    // in shaft A's hologram room, and the teal control ring's, reaching 3 in
    // the map room; play feedback of 19 November 2026: each washed its room
    // out, "reduce the brightness". A tenth each, from captures in game at
    // the user's two places (the picture's mean of 255, green: the violet
    // room 121 as it was, 97 at 0.4, 85 at 0.25, 67 at 0.1, 56 at 0.05 and
    // 33 with the light off; the map room 147, 116, 93 at 0.2, 73, 56 and
    // 15): the walls with their texture and the map room's hologram seen;
    // the faces a few units from the beam stay near white whatever the
    // share. `HALO_RT_LIGHT_DIM="holo beam=1;holo control ring=1"` the
    // old)
    ("levels\\b30\\devices\\interior tech objects\\holo beam tube\\holo beam", 0.1),
    ("levels\\b30\\devices\\interior tech objects\\holo control ring\\holo control ring", 0.1),
    // (a30's light bridge control's two indicator lamps, teal spot lights
    // of 16,000 lm on the posts either side of it, blinking, turned from the
    // wall toward the bridge; play feedback of 20 November 2026: "cast a
    // light on the wall (brightness should be reduced)". The light on the
    // wall was the flashlight's reading of them (`SpotKind::Fixed` now);
    // standing at the lamp's own place each lit its post's top white. A
    // thirtieth, from captures in game from before the posts (0.3 still
    // white, 0.1 pale): the top a teal lamp under the game's flare.
    // `HALO_RT_LIGHT_DIM="holo indicator=1"` the old)
    ("levels\\a30\\scenery\\holo control indicator\\holo indicator", 0.03),
];
/// Placed lights dimmed further, one placement each, over their tag's
/// [`LIGHT_DIMMING`] share: the map's name, the light tag's name, the
/// light's place (the game's world units, as the F9 capture note and
/// `HALO_RT_LIGHT_LOG` print it), how near the light must be to it (world
/// units: the light is at its object's marker, a few tenths from the
/// object's origin) and the share that multiplies the tag's. Set for a run
/// by place with [`light_dim_at`]'s `HALO_RT_LIGHT_DIM_AT`.
const PLACED_LIGHT_DIMMING: [(&str, &str, [f32; 3], f32, f32); 2] = [
    // (a10's two alarm klaxons on the bridge flanking where its two opening
    // cutscenes play, round 0.9,-0.6,0.4; the user on 18 November 2026:
    // "significantly less bright". The other pair, at -4.1, is 4.2 away
    // and kept)
    ("a10", "levels\\a10\\devices\\alert_light\\alert_light", [0.10, 2.20, 0.99], PLACED_LIGHT_WITHIN, BRIDGE_ALARM_SHARE),
    ("a10", "levels\\a10\\devices\\alert_light\\alert_light", [0.10, -2.20, 0.99], PLACED_LIGHT_WITHIN, BRIDGE_ALARM_SHARE),
];
/// How near a placed light must be to its [`PLACED_LIGHT_DIMMING`] (or
/// `HALO_RT_LIGHT_DIM_AT`) place, by default (world units).
const PLACED_LIGHT_WITHIN: f32 = 0.5;
/// a10's bridge alarm klaxons' share over their tag's 0.7 (see
/// [`PLACED_LIGHT_DIMMING`]).
const BRIDGE_ALARM_SHARE: f32 = 0.3;

/// A light's share of its brightness by its place, set for a run (see
/// [`PLACED_LIGHT_DIMMING`]): `HALO_RT_LIGHT_DIM_AT="x,y,z[,within]=share[;...]"`,
/// the place in the game's world units, `within` the radius about it
/// ([`PLACED_LIGHT_WITHIN`] if not given), any map's and any tag's light
/// (but the test lights and the projectiles' and contrails' glows); the
/// first place the light is within wins over the table, and the share
/// multiplies the tag's as the table's does (0 leaves the light out).
/// `HALO_RT_LIGHT_DIM_AT=none` leaves the table out (the old: the tag's
/// share alone). Read once.
fn light_dim_at() -> &'static DimAt {
    static DIM_AT: std::sync::LazyLock<DimAt> = std::sync::LazyLock::new(|| parse_light_dim_at(&std::env::var("HALO_RT_LIGHT_DIM_AT").unwrap_or_default()));
    &DIM_AT
}

/// [`light_dim_at`]'s places and whether the table is used.
#[derive(Debug, PartialEq)]
struct DimAt {
    /// [`PLACED_LIGHT_DIMMING`] used (not `none`).
    table: bool,
    /// Each place, its radius and its share.
    places: Vec<([f32; 3], f32, f32)>,
}

/// [`light_dim_at`]'s places from the variable's text: an entry that is not
/// `x,y,z[,within]=share` of finite numbers skipped with a warning, a
/// negative share or radius 0.
fn parse_light_dim_at(text: &str) -> DimAt {
    if text.trim().eq_ignore_ascii_case("none") {
        return DimAt { table: false, places: Vec::new() };
    }
    let places = text
        .split(';')
        .filter(|entry| !entry.trim().is_empty())
        .filter_map(|entry| {
            let parsed = entry.split_once('=').and_then(|(place, share)| {
                let share = share.trim().parse::<f32>().ok().filter(|share| share.is_finite())?;
                let numbers: Vec<f32> = place.split(',').map(|n| n.trim().parse::<f32>().ok().filter(|n| n.is_finite())).collect::<Option<_>>()?;
                let (x, y, z, within) = match numbers.as_slice() {
                    &[x, y, z] => (x, y, z, PLACED_LIGHT_WITHIN),
                    &[x, y, z, within] => (x, y, z, within.max(0.0)),
                    _ => return None,
                };
                Some(([x, y, z], within, share.max(0.0)))
            });
            if parsed.is_none() {
                warn!("HALO_RT_LIGHT_DIM_AT: {entry:?} is not x,y,z[,within]=share");
            }
            parsed
        })
        .collect();
    DimAt { table: true, places }
}

/// The share by place of a light at `position` (world units) on `map`:
/// the first of `dim_at`'s places it is within, else the first of `table`'s
/// entries for the map and for `tag` (the table's tag name the light's
/// equals, [`LightTag::placed`]) it is within, if the table is used.
fn light_dim_at_in(dim_at: &DimAt, table: &[(&str, &str, [f32; 3], f32, f32)], map: &str, tag: Option<&str>, position: [f32; 3]) -> Option<f32> {
    let within = |place: [f32; 3], radius: f32| Vec3::from(place).distance_squared(Vec3::from(position)) <= radius * radius;
    if let Some(&(_, _, share)) = dim_at.places.iter().find(|&&(place, radius, _)| within(place, radius)) {
        return Some(share);
    }
    let tag = tag.filter(|_| dim_at.table)?;
    table
        .iter()
        .find(|&&(table_map, table_tag, place, radius, _)| table_map.eq_ignore_ascii_case(map) && table_tag.eq_ignore_ascii_case(tag) && within(place, radius))
        .map(|&(.., share)| share)
}

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

/// Light tags (by name) whose light lights nothing inside its own source
/// sphere (`SolarikHollowLight`): the Monitor's light is at his centre and
/// his body within its sphere, which since the inside term of 14 November
/// lit his shell from all round, a featureless white ball; before it his
/// shell got nothing from his own light and his eye and the shell's detail
/// showed. `HALO_RT_HOLLOW_LIGHTS=0` none (the old);
/// `HALO_RT_HOLLOW_LIGHTS="<tag name part>[;...]"` those instead of the
/// table, for a run.
const HOLLOW_LIGHTS: [&str; 1] = ["characters\\monitor\\monitor dynamic"];

/// Whether a light tag's light is hollow ([`HOLLOW_LIGHTS`], or
/// `HALO_RT_HOLLOW_LIGHTS`'s parts, matched without regard to case). Read
/// once.
fn hollow_light(name: &str) -> bool {
    static PARTS: std::sync::LazyLock<Option<Vec<String>>> = std::sync::LazyLock::new(|| {
        let text = std::env::var("HALO_RT_HOLLOW_LIGHTS").ok()?;
        // (0: none)
        Some(if text.trim() == "0" {
            Vec::new()
        } else {
            text.split(';').map(|part| part.trim().to_ascii_lowercase()).filter(|part| !part.is_empty()).collect()
        })
    });
    match PARTS.as_deref() {
        None => HOLLOW_LIGHTS.iter().any(|hollow| name.eq_ignore_ascii_case(hollow)),
        Some(parts) => {
            let name = name.to_ascii_lowercase();
            parts.iter().any(|part| name.contains(part.as_str()))
        }
    }
}

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
    /// Its share of its brightness ([`LIGHT_DIMMING`], or [`light_dim`]'s).
    dimming: f32,
    /// Its size, if small ([`SMALL_LIGHTS`]).
    small: Option<f32>,
    /// Its upward cone, if it shines every way but down ([`UPWARD_LIGHTS`]).
    upward: Option<Upward>,
    /// Its sphere and brightness as tuned, a point light's ([`light_tune`]).
    tune: Option<Tune>,
    /// Lighting nothing inside its sphere, a point light's
    /// ([`HOLLOW_LIGHTS`]).
    hollow: bool,
    /// Its name as [`PLACED_LIGHT_DIMMING`] has it, if some of its
    /// placements are dimmed there (only then are places compared, but for
    /// `HALO_RT_LIGHT_DIM_AT`'s).
    placed: Option<&'static str>,
}

/// What a light's tag says of it here, by the tag's name (none: a plain
/// light).
fn light_tag(name: Option<&str>) -> LightTag {
    LightTag {
        muzzle: name.is_some_and(|name| name.ends_with("muzzle flash")),
        dimming: name
            .and_then(|name| light_dim(name).or_else(|| LIGHT_DIMMING.iter().find(|(dimmed, _)| dimmed_by(name, dimmed)).map(|&(_, dimming)| dimming)))
            .unwrap_or(1.0),
        small: name.and_then(|name| SMALL_LIGHTS.iter().find(|(small, _)| name.eq_ignore_ascii_case(small))).map(|&(_, size)| size),
        upward: name.and_then(upward_light),
        tune: name.and_then(light_tune),
        hollow: name.is_some_and(hollow_light),
        placed: name.and_then(|name| PLACED_LIGHT_DIMMING.iter().find(|(_, placed, ..)| name.eq_ignore_ascii_case(placed))).map(|&(_, placed, ..)| placed),
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

/// A light's share of its brightness as set for a run, by its light tag's
/// name (a projectile's glow by the projectile's tag's name, a contrail's by
/// the contrail's), in place of [`LIGHT_DIMMING`]'s, [`PROJECTILE_GLOWS`]' or
/// [`CONTRAIL_GLOWS`]' share, whether or not the
/// tables have it: to try a value in play before the table is changed, or
/// to get an old one back.
/// `HALO_RT_LIGHT_DIM="<tag name part>=<share>[;...]"`, the part matched
/// without regard to case, the first that matches winning (as
/// [`light_tune`]'s); a share of 0 leaves the light out, as a tabled 0 does,
/// and a negative one is 0. The shares before 15 November 2026, that day's
/// changes undone:
/// `HALO_RT_LIGHT_DIM="monitor dynamic=0.15;frag grenade\frag grenade=0.7;plasma grenade\plasma grenade=0.7;needler\needle=1;cortana powerup=1;b40_generator_fx\=1"`.
/// (`HALO_RT_LIGHT_TUNE`'s share multiplies the power after this one.) Read
/// once.
fn light_dim(name: &str) -> Option<f32> {
    static DIMS: std::sync::LazyLock<Vec<(String, f32)>> =
        std::sync::LazyLock::new(|| parse_light_dim(&std::env::var("HALO_RT_LIGHT_DIM").unwrap_or_default()));
    light_dim_in(&DIMS, name)
}

/// [`light_dim`]'s entries from the variable's text, each part in lower
/// case: an entry that is not `<part>=<share>`, its part empty or its share
/// not a finite number, skipped with a warning.
fn parse_light_dim(text: &str) -> Vec<(String, f32)> {
    text.split(';')
        .filter(|entry| !entry.trim().is_empty())
        .filter_map(|entry| {
            let parsed = entry.split_once('=').and_then(|(part, share)| {
                let part = part.trim();
                let share = share.trim().parse::<f32>().ok().filter(|share| share.is_finite())?;
                (!part.is_empty()).then(|| (part.to_ascii_lowercase(), share.max(0.0)))
            });
            if parsed.is_none() {
                warn!("HALO_RT_LIGHT_DIM: {entry:?} is not <tag name part>=<share>");
            }
            parsed
        })
        .collect()
}

/// The share of the first of `dims` whose part the tag's name holds.
fn light_dim_in(dims: &[(String, f32)], name: &str) -> Option<f32> {
    if dims.is_empty() {
        return None;
    }
    let name = name.to_ascii_lowercase();
    dims.iter().find(|(part, _)| name.contains(part.as_str())).map(|&(_, share)| share)
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

/// Lights of fixed objects that the game does not light with (their light
/// tags lack the dynamic flag: in the game their light is the lightmaps',
/// baked, and the tag is a lens flare alone, so the bridge never sends
/// them), given a cast light here, by the light tag's name: each object the
/// level has whose definition attaches such a light gets a point light at
/// the attachment's marker, at the tag's upper colour and its radius as the
/// reach, as bright as the game's dynamic light of that colour and reach
/// would be here, times the share, from a source sphere of the given radius
/// (metres), raised by the given height (metres, the world's up). Set for a
/// run, or added to, with [`static_light`]'s `HALO_RT_STATIC_LIGHTS`;
/// `HALO_RT_STATIC_LIGHTS=0` none (the old).
const STATIC_OBJECT_LIGHTS: [(&str, StaticLight); 1] = [
    // (the Covenant torches standing in b40's and c40's snow, a teal flame
    // on a pole, its light a point at the flame reaching 1 world unit; play
    // feedback of 23 November 2026, c40: "these kinds of lights don't
    // actually seem to cast light, can we update them to do so?")
    ("levels\\b40\\scenery\\b40_ctorch\\ctorch", StaticLight { share: STATIC_TORCH_SHARE, source_radius: 0.1, raise: 0.0 }),
];
/// The torch's share (see [`STATIC_OBJECT_LIGHTS`]): chosen from a series
/// at the user's camera (c40 at night, exposure EV 9.7; 0.5, 0.1, 0.05, 0.02,
/// 0.01): at 0.1 and up the cup glared white and the snow round the next
/// torch was a white floodlit patch; at 0.02 the pole and the snow at its
/// foot read lit, the cup's rim bright but not blown out.
const STATIC_TORCH_SHARE: f32 = 0.02;

/// A static light's treatment ([`STATIC_OBJECT_LIGHTS`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StaticLight {
    /// Its share of the power a game light of its colour and reach gets.
    pub share: f32,
    /// Its source sphere's radius (metres).
    pub source_radius: f32,
    /// How far it is raised from its marker (metres, the world's up).
    pub raise: f32,
}

/// [`static_light`]'s setting: the table, none, or entries by tag name part
/// (a share, and the source radius and raise if given), tried before the
/// table.
#[derive(Clone, Debug, PartialEq)]
enum StaticLightsSetting {
    Table,
    Off,
    Entries(Vec<(String, f32, Option<f32>, Option<f32>)>),
}

/// `HALO_RT_STATIC_LIGHTS`'s setting from its text: unset the table, `0`
/// none, else `<tag name part>=<share>[,<source radius m>[,<raise m>]][;...]`
/// (the part in lower case; an entry that does not parse skipped with a
/// warning).
fn parse_static_lights(text: Option<&str>) -> StaticLightsSetting {
    let Some(text) = text else { return StaticLightsSetting::Table };
    if text.trim() == "0" {
        return StaticLightsSetting::Off;
    }
    StaticLightsSetting::Entries(
        text.split(';')
            .filter(|entry| !entry.trim().is_empty())
            .filter_map(|entry| {
                let parsed = entry.split_once('=').and_then(|(part, numbers)| {
                    let part = part.trim();
                    let numbers: Vec<f32> =
                        numbers.split(',').map(|n| n.trim().parse::<f32>().ok().filter(|n| n.is_finite())).collect::<Option<_>>()?;
                    if part.is_empty() || numbers.is_empty() || numbers.len() > 3 {
                        return None;
                    }
                    Some((part.to_ascii_lowercase(), numbers[0].max(0.0), numbers.get(1).map(|r| r.max(0.001)), numbers.get(2).copied()))
                });
                if parsed.is_none() {
                    warn!("HALO_RT_STATIC_LIGHTS: {entry:?} is not <tag name part>=<share>[,<source radius m>[,<raise m>]]");
                }
                parsed
            })
            .collect(),
    )
}

/// Whether any static light can be made (`HALO_RT_STATIC_LIGHTS` is not
/// `0`). Read once.
pub fn static_lights_on() -> bool {
    *static_lights_setting() != StaticLightsSetting::Off
}

fn static_lights_setting() -> &'static StaticLightsSetting {
    static SETTING: std::sync::LazyLock<StaticLightsSetting> =
        std::sync::LazyLock::new(|| parse_static_lights(std::env::var("HALO_RT_STATIC_LIGHTS").ok().as_deref()));
    &SETTING
}

/// A light tag's static light, by its name ([`STATIC_OBJECT_LIGHTS`], or
/// `HALO_RT_STATIC_LIGHTS`'s).
fn static_light(name: &str) -> Option<StaticLight> {
    static_light_in(static_lights_setting(), name)
}

/// [`static_light`] by a setting: an entry's part the name holds (without
/// regard to case, the first winning) before the table's whole name; an
/// entry's missing radius and raise the table's for that name, else
/// [`LIGHT_RADIUS`] and none; a share of 0 none.
fn static_light_in(setting: &StaticLightsSetting, name: &str) -> Option<StaticLight> {
    let tabled = STATIC_OBJECT_LIGHTS.iter().find(|(tabled, _)| name.eq_ignore_ascii_case(tabled)).map(|&(_, light)| light);
    let light = match setting {
        StaticLightsSetting::Off => None,
        StaticLightsSetting::Table => tabled,
        StaticLightsSetting::Entries(entries) => {
            let lower = name.to_ascii_lowercase();
            match entries.iter().find(|(part, ..)| lower.contains(part.as_str())) {
                Some(&(_, share, radius, raise)) => Some(StaticLight {
                    share,
                    source_radius: radius.or(tabled.map(|t| t.source_radius)).unwrap_or(LIGHT_RADIUS),
                    raise: raise.or(tabled.map(|t| t.raise)).unwrap_or(0.0),
                }),
                None => tabled,
            }
        }
    };
    light.filter(|light| light.share > 0.0)
}

/// A static light an object definition carries ([`STATIC_OBJECT_LIGHTS`]).
#[derive(Clone, Copy, Debug)]
struct StaticLightDef {
    /// The light tag's datum.
    light: u32,
    /// The marker's node, and the marker in the node's space.
    node: usize,
    local: halo_map::model::Matrix4x3,
    /// The marker in the object's space at rest (the node's rest pose).
    rest: halo_map::model::Matrix4x3,
    color: [f32; 3],
    radius: f32,
    cutoff: f32,
    falloff: f32,
    flags: u32,
    treatment: StaticLight,
}

/// Each object definition's static lights in a map (by the definition's
/// datum), with the light tags' names: the light attachments whose tag is
/// one of [`static_light`]'s and has no dynamic flag (the game lights with
/// those itself), at the attachment's marker (its first instance; the
/// object's origin if the model has no such marker), coloured by the tag's
/// upper colour (the game's colour for an attached light with no function
/// to drive it) and reaching its radius times its upper radius modifier.
fn static_light_defs(cache: &halo_map::CacheFile) -> (HashMap<u32, Vec<StaticLightDef>>, HashMap<u32, String>) {
    use halo_map::{collision::matrix_inverse, markers, pose::matrix_multiply};
    let mut defs: HashMap<u32, Vec<StaticLightDef>> = HashMap::new();
    let mut names = HashMap::new();
    if !static_lights_on() {
        return (defs, names);
    }
    let identity = halo_map::model::Matrix4x3 { scale: 1.0, forward: [1.0, 0.0, 0.0], left: [0.0, 1.0, 0.0], up: [0.0, 0.0, 1.0], position: [0.0; 3] };
    for tag in cache.tags.iter() {
        let Ok(root) = cache.tag_root(tag.datum, halo_map::groups::OBJECT) else { continue };
        let Ok(attachments) = root.block(halo_map::layout::_object_definition::ATTACHMENTS, 0x48) else { continue };
        let mut model: Option<(Option<markers::ModelMarkers>, Vec<halo_map::model::Node>)> = None;
        for attachment in attachments.iter() {
            let Ok(reference) = attachment.reference(0) else { continue };
            let Some(light) = reference.datum.filter(|_| reference.group == halo_map::groups::LIGHT) else { continue };
            let Ok(light_tag) = cache.tag(light) else { continue };
            let Some(treatment) = static_light(&light_tag.name) else { continue };
            let Ok(definition) = cache.tag_root(light, halo_map::groups::LIGHT) else { continue };
            let f = |offset: usize| definition.f32(offset).unwrap_or(0.0);
            // (struct point_light_definition: flags, radius, its modifiers'
            // bounds, the falloff and cutoff angles; the upper colour, argb)
            let flags = definition.u32(0x0).unwrap_or(0);
            if flags & 1 != 0 {
                continue;
            }
            let modifier = if f(0xc) > 0.0 { f(0xc) } else { 1.0 };
            let radius = f(0x4) * modifier;
            if radius <= 0.0 {
                continue;
            }
            let (model_markers, nodes) = model.get_or_insert_with(|| {
                let definition = halo_map::TagRef { group: tag.group, datum: Some(tag.datum) };
                let model = halo_map::objects::object_model(cache, definition).ok().flatten().and_then(|m| m.datum);
                (
                    model.and_then(|model| markers::ModelMarkers::load(cache, model).ok()),
                    model.and_then(|model| halo_map::model::Model::load(cache, model).ok()).map(|m| m.nodes).unwrap_or_default(),
                )
            });
            let marker = attachment.bytes(0x10, 32).ok().map(|bytes| bytes.split(|&b| b == 0).next().unwrap_or(&[]).to_vec()).unwrap_or_default();
            let found = (!marker.is_empty())
                .then(|| model_markers.as_ref()?.markers.iter().find(|m| m.name.eq_ignore_ascii_case(&marker))?.instances.first().copied())
                .flatten();
            let (node, local, rest) = match found {
                Some(instance) => {
                    let local = markers::matrix_from_point_and_quaternion(instance.translation, instance.rotation);
                    let rest = nodes.get(instance.node as usize).map_or(local, |node| matrix_multiply(&matrix_inverse(&node.default_inverse), &local));
                    (instance.node as usize, local, rest)
                }
                None => {
                    warn!("static light {:?} of {:?}: no marker {:?} on its model, at the object's origin", light_tag.name, tag.name, String::from_utf8_lossy(&marker));
                    (0, identity, identity)
                }
            };
            names.insert(light, light_tag.name.clone());
            defs.entry(tag.datum).or_default().push(StaticLightDef {
                light,
                node,
                local,
                rest,
                color: [f(0x4c), f(0x50), f(0x54)],
                radius,
                cutoff: f(0x14),
                falloff: f(0x10),
                flags,
                treatment,
            });
        }
    }
    (defs, names)
}

/// A static light as a game light would be, at a marker's place and facing
/// in the world (`matrix`, Halo's units), on the object `object_index`.
fn static_light_at(def: &StaticLightDef, matrix: &halo_map::model::Matrix4x3, object_index: u32) -> Light {
    Light {
        position: matrix.position,
        forward: matrix.forward,
        up: matrix.up,
        color: def.color,
        radius: def.radius,
        cutoff_angle: def.cutoff,
        falloff_angle: def.falloff,
        definition_index: def.light,
        definition_flags: def.flags,
        object_index,
        ..default()
    }
}

/// The objects of the level viewer's static lights (none in the game), from
/// this one up by placement: below the test lights' and the game's
/// first-person models' range.
const STATIC_LIGHT_OBJECT: u32 = 0xFFFE_0000;

/// The level's static lights ([`STATIC_OBJECT_LIGHTS`]): what each object
/// definition carries, and in the level viewer, which has no game to say
/// where its objects are, the lights of its placed objects (those
/// `LevelBuilder::spawn_objects` places), made once. Bridged, `sync_lights`
/// puts them on the game's objects each frame. Made at each level's load.
#[derive(Resource, Default)]
pub struct StaticLights {
    defs: HashMap<u32, Vec<StaticLightDef>>,
    names: HashMap<u32, String>,
    placed: Vec<(Light, StaticLight)>,
}

impl StaticLights {
    /// The map's static lights; with `placements` (the level viewer's
    /// placed objects) their lights too.
    pub fn from_map(cache: &halo_map::CacheFile, placements: Option<&[halo_map::objects::Placement]>) -> Self {
        let (defs, names) = static_light_defs(cache);
        let mut placed = Vec::new();
        for (index, placement) in placements.unwrap_or(&[]).iter().enumerate() {
            let Some(carried) = placement.definition.datum.and_then(|datum| defs.get(&datum)) else { continue };
            let [forward, left, up] = placement.axes();
            let object = halo_map::model::Matrix4x3 { scale: 1.0, forward, left, up, position: placement.position };
            for def in carried {
                let matrix = halo_map::pose::matrix_multiply(&object, &def.rest);
                placed.push((static_light_at(def, &matrix, STATIC_LIGHT_OBJECT + index as u32), def.treatment));
            }
        }
        if placements.is_some() {
            static_lights_log(&cache.header.name, &names, placed.iter().map(|(light, _)| light));
        }
        Self { defs, names, placed }
    }

    /// A static light's tag's name, by its datum.
    pub(crate) fn name(&self, definition_index: u32) -> Option<&str> {
        self.names.get(&definition_index).map(String::as_str)
    }

    /// The static lights on the game's objects this frame: each object not
    /// hidden, not a model and not outside the map whose definition carries
    /// some, at its marker on its node as the game posed it.
    fn on_objects(&self, snapshot: &halo_bridge::Snapshot) -> Vec<(Light, StaticLight)> {
        let mut lights = Vec::new();
        if self.defs.is_empty() {
            return lights;
        }
        for object in &snapshot.objects {
            if object.flags & (halo_bridge::OBJECT_HIDDEN | halo_bridge::OBJECT_MODEL) != 0 || object.object_flags & OBJECT_OUTSIDE_OF_MAP != 0 {
                continue;
            }
            let Some(carried) = self.defs.get(&object.definition_index) else { continue };
            for def in carried {
                let node = (def.node < object.node_count as usize).then(|| snapshot.nodes.get(object.first_node as usize + def.node)).flatten();
                let Some(node) = node else { continue };
                let node = halo_map::model::Matrix4x3 { scale: node.scale, forward: node.forward, left: node.left, up: node.up, position: node.position };
                let matrix = halo_map::pose::matrix_multiply(&node, &def.local);
                lights.push((static_light_at(def, &matrix, object.object_index), def.treatment));
            }
        }
        lights
    }
}

/// The level's one line on its static lights: how many, of which tags.
fn static_lights_log<'a>(map: &str, names: &HashMap<u32, String>, lights: impl Iterator<Item = &'a Light>) {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for light in lights {
        let name = names.get(&light.definition_index).cloned().unwrap_or_default();
        match counts.iter_mut().find(|(n, _)| *n == name) {
            Some((_, count)) => *count += 1,
            None => counts.push((name, 1)),
        }
    }
    let total: usize = counts.iter().map(|(_, count)| count).sum();
    let each: Vec<String> = counts.iter().map(|(name, count)| format!("{count} of {name}")).collect();
    info!("static lights in {map}: {total}{}{}", if each.is_empty() { "" } else { ": " }, each.join(", "));
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

/// The point lights' query: each one's cone, if it has one, and whether it
/// is hollow.
type PointLights<'w, 's> = Query<
    'w,
    's,
    (&'static mut PointLight, &'static mut Transform, &'static mut Visibility, Option<&'static SolarikLightCone>, Has<SolarikHollowLight>),
    Without<SpotLight>,
>;

/// A point light's entity (as [`place_spot`]), with its cone or none and
/// hollow or not (a reused entity's given or taken away when it differs).
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
    hollow: bool,
) {
    let reused = last.remove(&key).or_else(|| free.pop());
    let entity = match reused.map(|entity| (entity, points.get_mut(entity))) {
        Some((entity, Ok((mut light_component, mut light_transform, mut visibility, had, was_hollow)))) => {
            *light_component = light;
            *light_transform = transform;
            visibility.set_if_neq(Visibility::Inherited);
            if had.copied() != cone {
                match cone {
                    Some(cone) => commands.entity(entity).insert(cone),
                    None => commands.entity(entity).remove::<SolarikLightCone>(),
                };
            }
            if was_hollow != hollow {
                if hollow {
                    commands.entity(entity).insert(SolarikHollowLight);
                } else {
                    commands.entity(entity).remove::<SolarikHollowLight>();
                }
            }
            entity
        }
        _ => {
            let mut spawned = commands.spawn((Name::new("light"), light, transform, Visibility::Inherited, NoFrustumCulling, SolarikUnreflectedLight));
            if let Some(cone) = cone {
                spawned.insert(cone);
            }
            if hollow {
                spawned.insert(SolarikHollowLight);
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
    /// Its share of its brightness ([`LIGHT_DIMMING`], a projectile's glow's
    /// [`PROJECTILE_GLOWS`], a contrail's [`CONTRAIL_GLOWS`], or
    /// [`light_dim`]'s).
    pub dimming: f32,
    /// Its power (lumens a channel, at an emissive scale of one).
    pub power: Vec3,
    pub tuned: bool,
    /// Lighting nothing inside its sphere ([`HOLLOW_LIGHTS`]).
    pub hollow: bool,
    /// What a spot light was taken as ([`spot_reading`]; a muzzle flash and
    /// a light not a spot neither).
    pub spot_kind: SpotKind,
    /// A static light ([`STATIC_OBJECT_LIGHTS`]), not one the game sent.
    pub static_light: bool,
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
            match self.spot_kind {
                SpotKind::Fixed => "spot (fixed)",
                SpotKind::Flashlight => "spot (flashlight)",
                SpotKind::Vehicle => "spot (vehicle)",
                SpotKind::Neither => "spot",
            }
        } else if self.upward {
            "upward"
        } else {
            "point"
        };
        let [pr, pg, pb] = self.power.to_array();
        format!(
            "{distance:.2} m | at {x:.3},{y:.3},{z:.3} | colour {r:.3},{g:.3},{b:.3} | radius {:.3} | flags {:#x}{}{}{} | tag {tag} | object {object} | {kind}, source {:.3} m, raise {:.3} m, dimming {:.2}, power {pr:.0},{pg:.0},{pb:.0} lm, {}{}",
            light.radius,
            light.definition_flags,
            if light.first_person != 0 { " first person" } else { "" },
            if self.test { " test light" } else { "" },
            if self.static_light { " static" } else { "" },
            self.radius,
            self.raise,
            self.dimming,
            if self.tuned { "tuned" } else { "untuned" },
            if self.hollow { " | hollow" } else { "" },
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
    statics: Option<Res<StaticLights>>,
    in_solid: Option<Res<crate::bridge::LightsInSolid>>,
    // (each light tag's reading, and what carries each: light_holders)
    mut tags: Local<(HashMap<u32, LightTag>, Option<HashMap<u32, LightHolder>>)>,
    mut gels: Local<HashMap<u32, Vec<GelBeam>>>,
    // (the projectiles' glows and the contrails', by their tags)
    mut all_glows: Local<(HashMap<u32, Option<([f32; 3], f32, f32)>>, HashMap<u32, Option<([f32; 3], f32, f32)>>)>,
    mut scenario: Local<String>,
    // (whether the test lights were logged as built, and the level whose
    // static lights were counted in the log and their count, bridged)
    mut logged: Local<(bool, (String, usize))>,
    mut shown: ResMut<ShownLights>,
) {
    shown.0.clear();
    let (test_logged, statics_logged) = &mut *logged;
    let (glows, contrail_glows) = &mut *all_glows;
    let (light_tags, holders) = &mut *tags;
    static LIGHT_LOG: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var_os("HALO_RT_LIGHT_LOG").is_some());
    let snapshot = link.as_ref().and_then(|link| link.latest.as_ref());
    let test_lights = test.as_deref().map_or(&[][..], |test| test.lights.as_slice());
    // (a level's test lights, anew: logged as built, their tags looked up)
    if test.as_ref().is_some_and(|test| test.is_changed()) {
        *test_logged = false;
        light_tags.clear();
        *holders = None;
    }
    // (the level viewer's static lights, placed once at the level's load)
    let placed_statics = statics.as_deref().filter(|_| snapshot.is_none()).map_or(&[][..], |statics| statics.placed.as_slice());
    if snapshot.is_none() && test_lights.is_empty() && placed_statics.is_empty() {
        return;
    }
    // (a new level: its own tags)
    if let Some(snapshot) = snapshot
        && snapshot.scenario_name != *scenario
    {
        light_tags.clear();
        *holders = None;
        gels.clear();
        glows.clear();
        contrail_glows.clear();
        *scenario = snapshot.scenario_name.clone();
    }
    // (what carries each light tag, once a level from its map's object tags,
    // with the fixed spots' rule alone: light_holders; none without a map)
    let fixed_rule = fixed_spots();
    if fixed_rule
        && holders.is_none()
        && let Some(level) = level.as_ref()
    {
        *holders = Some(light_holders(&level.builder.cache));
    }
    let camera = snapshot.and_then(|snapshot| snapshot.camera.as_ref());
    // the glowing projectiles' lights, round from their middles (by no tag:
    // neither muzzle flashes nor gelled; hidden ones too: a projectile
    // without a model, a Sentinel's beam, the game hides; not those outside
    // the map: b40's two Wraith mortars, kept at the world's origin), each
    // with its share of its brightness (PROJECTILE_GLOWS, or light_dim's)
    let projectile_lights: Vec<(Light, f32)> = snapshot
        .into_iter()
        .flat_map(|snapshot| snapshot.objects.iter())
        .filter(|object| object.object_flags & OBJECT_OUTSIDE_OF_MAP == 0)
        .filter_map(|object| {
            let (color, radius, share) = (*glows.entry(object.definition_index).or_insert_with(|| {
                let tag = level.as_ref()?.builder.cache.tag(object.definition_index).ok()?;
                PROJECTILE_GLOWS
                    .iter()
                    .find(|(name, ..)| tag.name.eq_ignore_ascii_case(name))
                    .map(|&(_, color, radius, share)| (color, radius, light_dim(&tag.name).unwrap_or(share)))
            }))?;
            let light = Light {
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
            };
            Some((light, share))
        })
        .collect();
    // the glowing contrails' lights, evenly along each (the rest of the
    // contrails light nothing), each with its share of its brightness
    // (CONTRAIL_GLOWS, or light_dim's)
    let mut contrail_lights: Vec<(Light, f32)> = Vec::new();
    for contrail in snapshot.into_iter().flat_map(|snapshot| snapshot.lights.iter()).filter(|light| light.definition_flags & LIGHT_CONTRAIL != 0) {
        let Some((color, radius, share)) = *contrail_glows.entry(contrail.definition_index).or_insert_with(|| {
            let tag = level.as_ref()?.builder.cache.tag(contrail.definition_index).ok()?;
            CONTRAIL_GLOWS
                .iter()
                .find(|(name, ..)| tag.name.eq_ignore_ascii_case(name))
                .map(|&(_, color, radius, share)| (color, radius, light_dim(&tag.name).unwrap_or(share)))
        }) else {
            continue;
        };
        let (middle, along, length) = (Vec3::from(contrail.position), Vec3::from(contrail.forward), 2.0 * contrail.radius);
        let count = ((length / CONTRAIL_GLOW_SPACING).ceil() as usize).clamp(1, CONTRAIL_GLOW_MOST);
        for index in 0..count {
            let offset = ((index as f32 + 0.5) / count as f32 - 0.5) * length;
            let light = Light {
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
            };
            contrail_lights.push((light, share));
        }
    }
    // the static lights (STATIC_OBJECT_LIGHTS): on the game's objects as it
    // posed them, or the level viewer's placed; each with its treatment
    let static_lights: Vec<(Light, StaticLight)> = match (snapshot, statics.as_deref()) {
        (Some(snapshot), Some(statics)) => {
            let lights = statics.on_objects(snapshot);
            // (counted in the log when its objects have come, and again when
            // the count changes: a switch of BSP puts the objects of
            // another outside the map)
            if !snapshot.objects.is_empty() && (statics_logged.0 != snapshot.scenario_name || statics_logged.1 != lights.len()) {
                *statics_logged = (snapshot.scenario_name.clone(), lights.len());
                static_lights_log(&snapshot.scenario_name, &statics.names, lights.iter().map(|(light, _)| light));
            }
            lights
        }
        _ => placed_statics.to_vec(),
    };
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
    // (the test lights after the game's, the static lights last; each with
    // whether it is a test light and a projectile's glow, a projectile's or
    // a contrail's glow's share of its brightness, the rest's 1: theirs is
    // their tag's, and a static light's treatment)
    let all = lights
        .map(|light| (light, false, false, 1.0, None))
        .chain(projectile_lights.iter().map(|(light, share)| (light, false, true, *share, None)))
        .chain(contrail_lights.iter().map(|(light, share)| (light, false, false, *share, None)))
        .chain(test_lights.iter().map(|light| (light, true, false, 1.0, None)))
        .chain(static_lights.iter().map(|(light, treatment)| (light, false, false, treatment.share, Some(*treatment))));
    // (the loaded level's map, whose tags these are, for the dimming by
    // place)
    let map = level.as_ref().map_or("", |level| level.builder.stats.map_name.as_str());
    let dim_at = light_dim_at();
    for (light, test_light, projectile, share, statik) in all {
        let near = camera.filter(|camera| Vec3::from(camera.position).distance(Vec3::from(light.position)) < FLASHLIGHT_REACH);
        let own = light.first_person != 0;
        // (a muzzle flash: its tag's flag, or its name; a test light's tag
        // by the name it was given, the level viewer having no map)
        let tag = *light_tags.entry(light.definition_index).or_insert_with(|| {
            let name = level
                .as_ref()
                .and_then(|level| level.builder.cache.tag(light.definition_index).ok())
                .map(|tag| tag.name.as_str())
                .or_else(|| test.as_deref().and_then(|test| test.name(light.definition_index)))
                .or_else(|| statics.as_deref().and_then(|statics| statics.name(light.definition_index)));
            light_tag(name)
        });
        // (its share of its brightness: its tag's, times a projectile's
        // glow's, times its placement's if dimmed by place: a game light's
        // alone, PLACED_LIGHT_DIMMING or HALO_RT_LIGHT_DIM_AT)
        let by_place = (!test_light && light.definition_index != u32::MAX && (tag.placed.is_some() || !dim_at.places.is_empty()))
            .then(|| light_dim_at_in(dim_at, &PLACED_LIGHT_DIMMING, map, tag.placed, light.position))
            .flatten();
        let dimming = tag.dimming * share * by_place.unwrap_or(1.0);
        let muzzle_tag = light.definition_flags & LIGHT_MUZZLE_FLASH != 0 || tag.muzzle;
        let muzzle = light.cutoff_angle > SPOT_CUTOFF && muzzle_tag;
        let spot = light.cutoff_angle <= SPOT_CUTOFF || muzzle;
        // (the player's spot lights follow the camera's aim; a fixed
        // object's never: spot_reading)
        let holder = holders.as_ref().and_then(|holders| holders.get(&light.definition_index).copied());
        let reading = spot_reading(camera.is_some(), spot, muzzle, own, near.is_some(), holder, fixed_rule);
        let near_camera = camera.filter(|_| reading.aimed);
        // (one shining upward: a point light's but for its cone)
        let upward = tag.upward.filter(|_| !spot);
        // (a point light's tuning: HALO_RT_LIGHT_TUNE)
        let tune = tag.tune.filter(|_| !spot);
        // (hollow: a point light's, HOLLOW_LIGHTS)
        let hollow = tag.hollow && !spot;
        let forward = Vec3::from(halo_to_bevy(near_camera.map_or(light.forward, |camera| camera.forward), 1.0)).normalize_or(Vec3::NEG_Z);
        let vehicle = reading.kind == SpotKind::Vehicle;
        let flashlight = reading.kind == SpotKind::Flashlight;
        let fixed = reading.kind == SpotKind::Fixed;
        let ahead = match (muzzle, spot) {
            (true, _) if near_camera.is_some() => MUZZLE_FORWARD,
            (true, _) => OTHERS_MUZZLE_FORWARD,
            (false, true) if vehicle => SPOT_FORWARD + VEHICLE_SPOT_FORWARD,
            (false, true) if fixed => fixed_spot_forward(),
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
        } * dimming;
        let reach2 = if spot && !muzzle { light.radius * SPOT_REFERENCE_REACH } else { light.radius * light.radius };
        let radiance = Vec3::from(light.color) * RADIANCE_PER_REACH2 * scale * reach2;
        let range = light.radius * METERS_PER_WORLD_UNIT * RANGE_PER_RADIUS;
        let radius = if spot { SPOT_RADIUS } else { tag.small.unwrap_or(LIGHT_RADIUS) };
        // (an upward light at its own height, the rest clear of the ground)
        let raise = upward.map_or_else(|| tag.small.map_or(0.0, |size| size * 1.2), |upward| upward.raise);
        let power = radiance * sphere_lumens_per_radiance(radius);
        // (tuned: its sphere's size and height, its power its untuned
        // size's times the share, see light_tune; a static light's point
        // light as its treatment has it, its share in the dimming)
        let (radius, raise, power) = match (tune, statik.filter(|_| !spot)) {
            (Some(tune), _) => (tune.radius, tune.raise, power * tune.share),
            (None, Some(statik)) => (statik.source_radius, statik.raise, power),
            (None, None) => (radius, raise, power),
        };
        let position = position + Vec3::Y * raise;
        let source = LightSource { power };
        let (color, intensity) = source.color_and_intensity(settings.emissive_scale);
        // (HALO_RT_LIGHT_LOG: each light, in the log)
        if *LIGHT_LOG {
            let distance = camera.map(|c| Vec3::from(c.position).distance(Vec3::from(light.position)));
            info!(
                "light at tick {}: position {:?} forward {:?} colour {:?} radius {} cutoff {:.1} falloff {:.1} deg, {distance:?} from the camera (forward {:?}), own {own} muzzle {muzzle} aimed by the camera {} upward {:?} tag {:#x} flags {:#x} object {:#x}; source radius {radius} m raise {raise} m power {power:.1?} lm ({intensity:.1} lm at the emissive scale) dimming {dimming:.3} (by place {by_place:?}) tuned {tune:?} test {test_light}",
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
                if dimming <= 0.0 { ", dimmed to none: left out" } else { "" },
            );
        }
        // (dimmed to none, or shining upward at no share: left out; and a
        // projectile's glow at exactly the world's origin, where the game
        // holds objects not in the world: three wraith mortar glows at 0,0,0
        // in b40's notes of 13 November; HALO_RT_ORIGIN_GLOWS=1 the old)
        let at_origin = projectile && light.position == [0.0; 3] && !origin_glows();
        let left_out = dimming <= 0.0 || upward.is_some_and(|upward| upward.share <= 0.0) || at_origin;
        shown.0.push(ShownLight {
            light: *light,
            test: test_light,
            spot,
            upward: upward.is_some(),
            gelled: false,
            left_out,
            radius,
            raise,
            dimming,
            power,
            tuned: tune.is_some(),
            hollow,
            spot_kind: reading.kind,
            static_light: statik.is_some(),
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
                shown.hollow = false;
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
            // (a spot light shines along its -Z; a fixed one's up its own,
            // as the game has it, unless along its forward)
            let own_up = Vec3::from(halo_to_bevy(light.up, 1.0)).normalize_or_zero();
            let up = if fixed && own_up != Vec3::ZERO && own_up.dot(forward).abs() < 0.99 {
                own_up
            } else if forward.y.abs() > 0.99 {
                Vec3::X
            } else {
                Vec3::Y
            };
            let transform = Transform::from_translation(position).looking_to(forward, up);
            place_spot(&mut commands, &mut spots, &mut last_spots, &mut pools.free_spots, &mut pools.spots, key(light), spot_light, transform);
        } else if let Some(Upward { inner, outer, .. }) = upward {
            // (a point light in a cone wider than a spot light's: Solarik's
            // cone, along the transform's forward, its -Z, the world's up)
            let point_light = PointLight { color, intensity, range, radius, shadow_maps_enabled: false, ..default() };
            let transform = Transform::from_translation(position).looking_to(Vec3::Y, Vec3::X);
            let cone = SolarikLightCone { inner, outer };
            place_point(&mut commands, &mut points, &mut last_points, &mut pools.free_points, &mut pools.points, key(light), point_light, transform, Some(cone), hollow);
        } else {
            let point_light = PointLight { color, intensity, range, radius, shadow_maps_enabled: false, ..default() };
            let transform = Transform::from_translation(position);
            place_point(&mut commands, &mut points, &mut last_points, &mut pools.free_points, &mut pools.points, key(light), point_light, transform, None, hollow);
        }
    }
    if !test_lights.is_empty() {
        *test_logged = true;
    }
    // (the rest go dark, free for the lights to come)
    for (_, entity) in last_points {
        if let Ok((_, _, mut visibility, _, _)) = points.get_mut(entity) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_dim_parses_and_matches() {
        let dims = parse_light_dim(" Monitor Dynamic = 0.15 ;frag grenade\\frag grenade=0.7;bad;=0.5;needle=x;needle=inf;grenade=-1;;needler\\needle=1");
        assert_eq!(
            dims,
            vec![
                ("monitor dynamic".to_string(), 0.15),
                ("frag grenade\\frag grenade".to_string(), 0.7),
                ("grenade".to_string(), 0.0),
                ("needler\\needle".to_string(), 1.0),
            ]
        );
        // (without regard to case, the first that matches winning)
        assert_eq!(light_dim_in(&dims, "characters\\monitor\\MONITOR DYNAMIC"), Some(0.15));
        assert_eq!(light_dim_in(&dims, "weapons\\frag grenade\\frag grenade"), Some(0.7));
        assert_eq!(light_dim_in(&dims, "weapons\\frag grenade\\explosion"), Some(0.0));
        assert_eq!(light_dim_in(&dims, "weapons\\needler\\needle"), Some(1.0));
        assert_eq!(light_dim_in(&dims, "vehicles\\ghost\\ghost bolt"), None);
        assert_eq!(light_dim_in(&[], "characters\\monitor\\monitor dynamic"), None);
    }

    #[test]
    fn b30_room_lights_and_sentinel_beam_dimmed() {
        let tabled = |name: &str| LIGHT_DIMMING.iter().find(|(dimmed, _)| dimmed_by(name, dimmed)).map(|&(_, share)| share);
        let beam = "levels\\b30\\devices\\interior tech objects\\holo beam tube\\holo beam";
        let ring = "levels\\b30\\devices\\interior tech objects\\holo control ring\\holo control ring";
        assert_eq!(tabled(beam), Some(0.1));
        assert_eq!(tabled(ring), Some(0.1));
        // (the control room's own light keeps its row)
        assert_eq!(tabled("levels\\b30\\devices\\interior tech objects\\holo control room display\\light"), Some(0.15));
        // (the old, by HALO_RT_LIGHT_DIM's parts, each matching its own)
        let old = parse_light_dim("holo beam=1;holo control ring=1;characters\\sentinel\\beam=1");
        assert_eq!(light_dim_in(&old, beam), Some(1.0));
        assert_eq!(light_dim_in(&old, ring), Some(1.0));
        assert_eq!(light_dim_in(&old, "levels\\b30\\devices\\interior tech objects\\holo control room display\\light"), None);
        let (name, _, _, share) = CONTRAIL_GLOWS[0];
        assert_eq!((name, share), ("characters\\sentinel\\beam", 0.7));
        assert_eq!(light_dim_in(&old, name), Some(1.0));
    }

    #[test]
    fn light_dim_at_parses_and_matches() {
        assert_eq!(parse_light_dim_at(" NONE "), DimAt { table: false, places: Vec::new() });
        assert_eq!(parse_light_dim_at(""), DimAt { table: true, places: Vec::new() });
        let dim_at = parse_light_dim_at(" 1, 2 ,3 = 0.5 ;4,5,6,2=0.25;bad;1,2=0.5;1,2,3,4,5=1;1,2,3=x;1,2,3=inf;7,8,9,-1=-2;;");
        assert_eq!(
            dim_at,
            DimAt { table: true, places: vec![([1.0, 2.0, 3.0], PLACED_LIGHT_WITHIN, 0.5), ([4.0, 5.0, 6.0], 2.0, 0.25), ([7.0, 8.0, 9.0], 0.0, 0.0)] }
        );
        let alarm = "levels\\a10\\devices\\alert_light\\alert_light";
        let tag = Some(alarm);
        // (the table: by map, tag and place, the light a little off the
        // machine's origin; the other pair and other maps not)
        let none = DimAt { table: true, places: Vec::new() };
        assert_eq!(light_dim_at_in(&none, &PLACED_LIGHT_DIMMING, "A10", tag, [0.10, 2.20, 1.30]), Some(BRIDGE_ALARM_SHARE));
        assert_eq!(light_dim_at_in(&none, &PLACED_LIGHT_DIMMING, "a10", tag, [0.30, -2.0, 0.99]), Some(BRIDGE_ALARM_SHARE));
        assert_eq!(light_dim_at_in(&none, &PLACED_LIGHT_DIMMING, "a10", tag, [-4.10, 2.20, 0.99]), None);
        assert_eq!(light_dim_at_in(&none, &PLACED_LIGHT_DIMMING, "a50", tag, [0.10, 2.20, 0.99]), None);
        assert_eq!(light_dim_at_in(&none, &PLACED_LIGHT_DIMMING, "a10", None, [0.10, 2.20, 0.99]), None);
        // (none: the table left out)
        let off = parse_light_dim_at("none");
        assert_eq!(light_dim_at_in(&off, &PLACED_LIGHT_DIMMING, "a10", tag, [0.10, 2.20, 0.99]), None);
        // (a place wins over the table, any map's and tag's light; the
        // first it is within)
        let set = parse_light_dim_at("0.1,2.2,0.99=0.6;0,0,0,10=0.1");
        assert_eq!(light_dim_at_in(&set, &PLACED_LIGHT_DIMMING, "a10", tag, [0.10, 2.20, 0.99]), Some(0.6));
        assert_eq!(light_dim_at_in(&set, &PLACED_LIGHT_DIMMING, "a10", tag, [0.10, -2.20, 0.99]), Some(0.1));
        assert_eq!(light_dim_at_in(&set, &PLACED_LIGHT_DIMMING, "c40", None, [5.0, 5.0, 5.0]), Some(0.1));
        assert_eq!(light_dim_at_in(&set, &PLACED_LIGHT_DIMMING, "c40", None, [50.0, 5.0, 5.0]), None);
        // (the table's tags are the names light_tag finds)
        assert_eq!(light_tag(Some("LEVELS\\a10\\devices\\alert_light\\alert_light")).placed, Some(alarm));
        assert_eq!(light_tag(Some("levels\\a10\\devices\\command display\\command display")).placed, None);
    }

    #[test]
    fn light_holder_by_group_and_precedence() {
        use halo_map::fourcc;
        assert_eq!(light_holder([fourcc(b"bipd")]), Some(LightHolder::Carried));
        assert_eq!(light_holder([fourcc(b"weap")]), Some(LightHolder::Carried));
        assert_eq!(light_holder([fourcc(b"vehi")]), Some(LightHolder::Vehicle));
        for group in [b"scen", b"mach", b"ctrl", b"lifi"] {
            assert_eq!(light_holder([fourcc(group)]), Some(LightHolder::Fixed));
        }
        // (carried over vehicle over fixed, in any order)
        assert_eq!(light_holder([fourcc(b"scen"), fourcc(b"vehi")]), Some(LightHolder::Vehicle));
        assert_eq!(light_holder([fourcc(b"vehi"), fourcc(b"weap"), fourcc(b"mach")]), Some(LightHolder::Carried));
        assert_eq!(light_holder([fourcc(b"lifi"), fourcc(b"bipd")]), Some(LightHolder::Carried));
        // (another group counts for nothing; none held, none)
        assert_eq!(light_holder([fourcc(b"eqip"), fourcc(b"ctrl")]), Some(LightHolder::Fixed));
        assert_eq!(light_holder([fourcc(b"eqip"), fourcc(b"proj")]), None);
        assert_eq!(light_holder([]), None);
    }

    #[test]
    fn spot_reading_as_before_but_fixed() {
        let bools = [false, true];
        for camera in bools {
            for spot in bools {
                for muzzle in bools {
                    for own in bools {
                        for near in bools {
                            // (near is by the camera: never without it)
                            let near = near && camera;
                            // (the old reading, as sync_lights had it)
                            let aimed = camera && spot && (own || (!muzzle && near));
                            let vehicle = spot && !muzzle && !aimed;
                            let flashlight = spot && !muzzle && aimed;
                            let kind = if vehicle {
                                SpotKind::Vehicle
                            } else if flashlight {
                                SpotKind::Flashlight
                            } else {
                                SpotKind::Neither
                            };
                            let old = SpotReading { kind, aimed };
                            let context = format!("camera {camera} spot {spot} muzzle {muzzle} own {own} near {near}");
                            for holder in [None, Some(LightHolder::Carried), Some(LightHolder::Vehicle)] {
                                for rule in bools {
                                    assert_eq!(spot_reading(camera, spot, muzzle, own, near, holder, rule), old, "{context} holder {holder:?} rule {rule}");
                                }
                            }
                            // (a fixed object's: the old without the rule;
                            // with it fixed at any distance, but a muzzle
                            // flash, a light not a spot and one first person
                            // as before)
                            assert_eq!(spot_reading(camera, spot, muzzle, own, near, Some(LightHolder::Fixed), false), old, "{context}");
                            let fixed = spot_reading(camera, spot, muzzle, own, near, Some(LightHolder::Fixed), true);
                            if spot && !muzzle && !own {
                                assert_eq!(fixed, SpotReading { kind: SpotKind::Fixed, aimed: false }, "{context}");
                            } else {
                                assert_eq!(fixed, old, "{context}");
                                assert_ne!(fixed.kind, SpotKind::Fixed, "{context}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn fixed_spot_forward_parses() {
        assert_eq!(parse_fixed_spot_forward(None), Ok(FIXED_SPOT_FORWARD));
        assert_eq!(parse_fixed_spot_forward(Some(" 0.25 ")), Ok(0.25));
        assert_eq!(parse_fixed_spot_forward(Some("-0.1")), Ok(-0.1));
        assert_eq!(parse_fixed_spot_forward(Some("x")), Err("x".to_string()));
        assert_eq!(parse_fixed_spot_forward(Some("inf")), Err("inf".to_string()));
        assert_eq!(parse_fixed_spot_forward(Some("")), Err(String::new()));
    }

    #[test]
    fn note_names_the_spot_kind() {
        let shown = |spot, spot_kind| ShownLight {
            light: Light { definition_index: 7, object_index: u32::MAX, ..default() },
            test: false,
            spot,
            upward: false,
            gelled: false,
            left_out: false,
            radius: SPOT_RADIUS,
            raise: 0.0,
            dimming: 1.0,
            power: Vec3::ONE,
            tuned: false,
            hollow: false,
            spot_kind,
            static_light: false,
        };
        let note = |spot, kind| shown(spot, kind).note(Vec3::ZERO, Some("tag"), None);
        assert!(note(true, SpotKind::Fixed).contains("| spot (fixed), source 0.150 m, raise 0.000 m, dimming 1.00, power 1,1,1 lm, untuned"));
        assert!(note(true, SpotKind::Flashlight).contains("| spot (flashlight), source"));
        assert!(note(true, SpotKind::Vehicle).contains("| spot (vehicle), source"));
        // (a muzzle flash as before)
        assert!(note(true, SpotKind::Neither).contains("| spot, source"));
        assert!(note(false, SpotKind::Neither).contains("| point, source"));
        // (a static light marked as such, a game light not)
        assert!(!note(false, SpotKind::Neither).contains("static"));
        let statik = ShownLight { static_light: true, ..shown(false, SpotKind::Neither) };
        assert!(statik.note(Vec3::ZERO, Some("tag"), None).contains("| flags 0x0 static | tag tag |"));
    }

    #[test]
    fn static_lights_parse() {
        use StaticLightsSetting::*;
        assert_eq!(parse_static_lights(None), Table);
        assert_eq!(parse_static_lights(Some(" 0 ")), Off);
        assert_eq!(parse_static_lights(Some("")), Entries(vec![]));
        assert_eq!(
            parse_static_lights(Some(" CTorch = 0.3 ;lamp=1,0.2;lamp2=0.5,0.05,-0.1;bad;=1;x=;y=a;z=1,2,3,4;neg=-1,0")),
            Entries(vec![
                ("ctorch".to_string(), 0.3, None, None),
                ("lamp".to_string(), 1.0, Some(0.2), None),
                ("lamp2".to_string(), 0.5, Some(0.05), Some(-0.1)),
                ("neg".to_string(), 0.0, Some(0.001), None),
            ])
        );
    }

    #[test]
    fn static_lights_match() {
        use StaticLightsSetting::*;
        let torch = r"levels\b40\scenery\b40_ctorch\ctorch";
        let tabled = StaticLight { share: STATIC_TORCH_SHARE, source_radius: 0.1, raise: 0.0 };
        // (the table by the whole name, without regard to case)
        assert_eq!(static_light_in(&Table, torch), Some(tabled));
        assert_eq!(static_light_in(&Table, &torch.to_ascii_uppercase()), Some(tabled));
        assert_eq!(static_light_in(&Table, r"levels\b40\scenery\b40_ctorch\ctorch2"), None);
        assert_eq!(static_light_in(&Table, r"levels\c10\scenery\lamp"), None);
        // (none at all)
        assert_eq!(static_light_in(&Off, torch), None);
        // (an entry by part first, its missing numbers the table's for that
        // name, else the plain light's; a share of 0 none)
        let set = parse_static_lights(Some("ctorch=0.2;lamp=0.4;lit=1,0.3,0.1"));
        assert_eq!(static_light_in(&set, torch), Some(StaticLight { share: 0.2, ..tabled }));
        assert_eq!(static_light_in(&set, r"levels\c10\scenery\lamp"), Some(StaticLight { share: 0.4, source_radius: LIGHT_RADIUS, raise: 0.0 }));
        assert_eq!(static_light_in(&set, r"levels\x\lit"), Some(StaticLight { share: 1.0, source_radius: 0.3, raise: 0.1 }));
        assert_eq!(static_light_in(&parse_static_lights(Some("lamp=1")), torch), Some(tabled));
        assert_eq!(static_light_in(&parse_static_lights(Some("ctorch=0")), torch), None);
    }
}
