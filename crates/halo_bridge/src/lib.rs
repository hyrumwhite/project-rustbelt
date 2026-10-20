//! The reading side of OpenCE's Bevy bridge.
//!
//! OpenCE (branch `bevy-bridge`, `port/linux/game/bevy_bridge.c`) writes each
//! rendered frame's camera and every object's node matrices into shared memory,
//! laid out as `bevy_bridge_protocol.h`. This crate mirrors that layout
//! (`#[repr(C)]`, fixed-size types, the same in the 32-bit game and a 64-bit
//! reader) and copies out consistent snapshots: two frame buffers, each a
//! sequence lock.
//!
//! Positions and matrices are the game's: world units, Z up.
//!
//! The other way, [`Bridge::write_input`] forwards a window's keyboard and
//! mouse for the game to read as its own (the game's
//! `port/linux/src/bevy_bridge_input.c` turns them into SDL events).
//!
//! With the game's `game.bevy_bridge_overlay` on, it also shares its own
//! picture without the opaque world ([`Bridge::read_overlay`]): effects,
//! transparent surfaces, the HUD and menus, premultiplied, to draw over the
//! renderer's picture.

use std::sync::atomic::{fence, Ordering};

pub const MAGIC: u32 = 0x4252_4248; // "HBRB"
pub const VERSION: u32 = 43;
pub const SPRITE_BATCHES: usize = 512;
pub const SPRITE_VERTICES: usize = 32768;
pub const DECALS: usize = 2048;
pub const DECAL_VERTICES: usize = 10240;
pub const VEHICLE_UPDATE_RECORDS: usize = 512;
pub const VEHICLE_PHYSICS_RECORDS: usize = 256;
pub const MASS_POINTS: usize = 8192;
pub const PHYSICS_VEHICLES: usize = 128;
pub const VEHICLE_RAY_TESTS: usize = 16;
pub const POSE_RECORDS: usize = 2048;
pub const MELEE_RECORDS: usize = 512;
pub const POSE_NODES: usize = 65536;
pub const UNIT_ANIMATION_RECORDS: usize = 1024;
pub const BIPED_UPDATE_RECORDS: usize = 1024;
pub const BIPED_MOVING_RECORDS: usize = 1024;
pub const FEATURE_OBJECTS: usize = 16384;
pub const FEATURE_MATRICES: usize = 65536;
pub const BIPED_RECORDS: usize = 1024;
pub const PROJECTILE_RECORDS: usize = 1024;
pub const RANDOM_DIRECTIONS: usize = 4096;
/// The steps of each ray test's point physics.
pub const POINT_PHYSICS_STEPS: usize = 90;
pub const RAY_TESTS: usize = 16;
pub const MAXIMUM_LIGHTS: usize = 64;
pub const MAXIMUM_OBJECTS: usize = 4096;
pub const MAXIMUM_NODES: usize = 49152;
const FRAMES: usize = 2;
/// The largest overlay picture, in pixels.
pub const OVERLAY_MAXIMUM_PIXELS: usize = 2560 * 1440;
const OVERLAYS: usize = 2;
/// The game's default bridge name (`game.bevy_bridge_name`). Each game
/// shares under its own name, so several can run side by side.
pub const DEFAULT_NAME: &str = "HaloBevyBridge";

pub const OBJECT_HIDDEN: u32 = 1 << 0;
/// An object's `object_flags` bit for one outside the map the game is in
/// (`_object_outside_of_map_bit`): in no cluster of the BSP, which the game
/// doesn't draw.
pub const OBJECT_OUTSIDE_OF_MAP: u32 = 1 << 21;
pub const OBJECT_FIRST_PERSON: u32 = 1 << 1;
/// Not an object but a model the game draws by itself (the first-person
/// weapon and hands): [`Object::definition_index`] is the model tag.
pub const OBJECT_MODEL: u32 = 1 << 2;
/// (Protocol 40) The first-person unit or something it holds, not itself
/// invisible ([`OBJECT_HIDDEN`] is set too): hidden from the camera, but its
/// body can still shade the world.
pub const OBJECT_FIRST_PERSON_BODY: u32 = 1 << 3;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Camera {
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    /// Radians.
    pub vertical_field_of_view: f32,
}

/// (Protocol 32) the fog the first player's window was drawn with
/// (`rasterizer_window_set_fog`'s, as it left it: off, each has no
/// density), in the game's coordinates. Atmospheric fog thickens with the
/// view depth from the minimum to the maximum distance; planar fog with the
/// view depth over its maximum distance and the depth under its plane
/// (whose normal points out of the fog) over its maximum depth
/// (`rasterizer_xbox_environment_fog.c`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Fog {
    pub valid: u32,
    /// `fog_definition` flags: bit 0 water, 1 atmosphere dominant, 2 screen
    /// effect only.
    pub flags: u16,
    /// 0 off, 1 normal, 2 fully fogged.
    pub planar_mode: i16,
    pub atmospheric_color: [f32; 3],
    pub atmospheric_maximum_density: f32,
    pub atmospheric_minimum_distance: f32,
    pub atmospheric_maximum_distance: f32,
    /// Normal and d: n.p = d on the plane.
    pub plane: [f32; 4],
    pub planar_color: [f32; 3],
    pub planar_maximum_density: f32,
    pub planar_maximum_distance: f32,
    pub planar_maximum_depth: f32,
}

/// (Protocol 33) a fog screen layer: a frame of the screen's bitmap over
/// the picture, moved on by the game.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FogScreenLayer {
    /// Texture coordinates from the picture's (x right, y up, -1 to 1):
    /// u = t0 x + t1 y + t2, v = t3 x + t4 y + t5.
    pub transform: [f32; 6],
    /// The frame's red, green and blue's weights: the layer's density is
    /// their dot product with its colour.
    pub weights: [f32; 3],
    /// The frame: a bitmap of [`FogScreen::map`].
    pub bitmap_index: u32,
}

/// (Protocol 33) the fog screen the first player's window would have drawn
/// over its fog (`rasterizer_xbox_environment_fog.c`). Its layers cover
/// 1 - the product of (1 - each density), in its colour: with `mode` 1
/// added to the picture, times the surface's density (`near_density` at
/// `near_distance` up to 1 at `far_distance`, along the planar fog
/// density's smoothstep; 1 where nothing is drawn); with 2, blended over
/// the whole picture.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FogScreen {
    /// 0 none, 1 added by depth, 2 blended over.
    pub mode: u16,
    /// `fog_screen` flags (bit 2: no texture, its depth's density flat).
    pub flags: u16,
    /// The bitmap tag's datum index.
    pub map: u32,
    pub layer_count: i16,
    pub pad: i16,
    pub color: [f32; 3],
    /// World units.
    pub near_distance: f32,
    pub far_distance: f32,
    /// The near density over the far.
    pub near_density: f32,
    pub layers: [FogScreenLayer; 4],
}

/// (Protocol 34) a decal the first player's window would have drawn (with
/// `game.bevy_bridge_viewer_decals`): its quads (four vertices each, from
/// `first_vertex` of the frame's decal vertices), its bitmap, and how it
/// blends: its shader's framebuffer blend function ([`DECAL_BLEND_ALPHA`]
/// and on) with its colour and intensity, as the game's combiners take
/// them (the bitmap's colour times the decal's, by the intensity toward
/// nothing: multiply's white, double multiply's grey, add's black).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Decal {
    /// Its datum index: the same decal from frame to frame.
    pub decal_index: u32,
    /// The bitmap tag's datum index, and the bitmap of it.
    pub map: u32,
    pub bitmap_index: u8,
    /// 0 primary, 1 secondary, 2 light, 3 alpha tested, 4 water.
    pub layer: u8,
    pub blend_function: u8,
    /// 0..255.
    pub intensity: u8,
    /// Red, green, blue (0..255), and padding.
    pub color: [u8; 4],
    pub first_vertex: u32,
    pub quad_count: u32,
}

pub const DECAL_BLEND_ALPHA: u8 = 0;
pub const DECAL_BLEND_MULTIPLY: u8 = 1;
pub const DECAL_BLEND_DOUBLE_MULTIPLY: u8 = 2;
pub const DECAL_BLEND_ADD: u8 = 3;
pub const DECAL_BLEND_REVERSE_SUBTRACT: u8 = 4;
pub const DECAL_BLEND_MIN: u8 = 5;
pub const DECAL_BLEND_MAX: u8 = 6;
pub const DECAL_BLEND_ALPHA_MULTIPLY_ADD: u8 = 7;

/// (Protocol 35) a batch of sprites the first player's window would have
/// drawn (with `game.bevy_bridge_viewer_effects`: particles, particle
/// systems, weather, glows, antennas): quads (four vertices each, from
/// `first_vertex` of the frame's sprite vertices) of one bitmap of `map`,
/// with an effect shader. Its combiners take the bitmap's colour times the
/// vertex's (with nonlinear tint, (1 - v) t^4 + v t) and blend it by the
/// framebuffer blend function (as a decal's, [`DECAL_BLEND_ALPHA`] and on),
/// faded by the vertex's alpha toward nothing.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpriteBatch {
    pub map: u32,
    pub bitmap_index: u16,
    /// Bit 0 sort bias, 1 nonlinear tint, 2 don't overdraw the first-person
    /// weapon.
    pub shader_flags: u16,
    pub blend_function: u16,
    /// Bit 0 point sampled, 1 u clamped, 2 v clamped.
    pub map_flags: u16,
    /// 1 drawn with the first-person weapon.
    pub first_person: u32,
    /// The secondary map's bitmap tag (`u32::MAX` for none), its anchor (0
    /// with the primary, 1 screen space, 2 zsprite) and flags.
    pub secondary_map: u32,
    pub secondary_anchor: u16,
    pub secondary_flags: u16,
    /// Where the game sorts the batch from (the game's coordinates).
    pub centroid: [f32; 3],
    pub first_vertex: u32,
    pub quad_count: u32,
    /// (Protocol 36) [`SPRITE_EFFECT`], or [`SPRITE_LENS_FLARE`]: a lens
    /// flare's reflection, drawn over everything (the game tests its
    /// occlusion itself), added by the vertex's alpha, its colour
    /// (1 - v) tint t^6 + v t; `shader_flags` and `blend_function` unused.
    pub kind: u32,
    pub tint: f32,
    /// (Protocol 42) A zsprite's radius (world units): how far toward the
    /// camera its secondary map (a z-map: green over blue its height) brings
    /// a pixel. 0 for other anchors.
    pub secondary_radius: f32,
}

pub const SPRITE_NONLINEAR_TINT: u16 = 1 << 1;
pub const SPRITE_EFFECT: u32 = 0;
pub const SPRITE_LENS_FLARE: u32 = 1;

/// A sprite's vertex: the game's coordinates, the bitmap's texture
/// coordinates and the colour (a8r8g8b8, alpha the fade).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpriteVertex {
    pub position: [f32; 3],
    pub texcoord: [f32; 2],
    pub color: u32,
}

/// A decal's vertex as the game keeps it: the game's coordinates, and the
/// bitmap's texture coordinates in 32768ths.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DecalVertex {
    pub position: [f32; 3],
    pub texcoord: [i16; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Object {
    /// The object's datum index: stable while the object lives.
    pub object_index: u32,
    /// Its definition tag's datum index.
    pub definition_index: u32,
    pub first_node: u32,
    pub node_count: u16,
    pub kind: i16,
    pub flags: u32,
    pub region_permutations: [u8; 8],
    /// The object's change colours A-D (team and armour colours); white for
    /// a model.
    pub change_colors: [[f32; 3]; 4],
    /// How camouflaged it is (0..1): a unit's active camouflage, shared by
    /// what it holds.
    pub active_camouflage: f32,
    /// The collision broad phase's state: the bounding sphere (as of the
    /// last tick), the object's flags (bit 0 invisible, 11 connected to the
    /// map, 25 with a collision model), its parent (`u32::MAX` for none) and
    /// its location's cluster (-1 outside the map).
    pub bounding_center: [f32; 3],
    pub bounding_radius: f32,
    pub object_flags: u32,
    pub parent_index: u32,
    pub location_cluster: i16,
    pub pad: i16,
    /// (Protocol 43) Its outgoing function values (A-D), which its shaders'
    /// animations, fades and numeric counters take as sources.
    pub function_values: [f32; 4],
}

/// `real_matrix4x3`: a node's world transform.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Node {
    pub scale: f32,
    pub forward: [f32; 3],
    pub left: [f32; 3],
    pub up: [f32; 3],
    pub position: [f32; 3],
}

/// A dynamic light (a muzzle flash, plasma, an explosion, the flashlight),
/// as the game lit the frame before.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Light {
    pub position: [f32; 3],
    pub forward: [f32; 3],
    /// 0..1 a channel.
    pub color: [f32; 3],
    /// How far it reaches (world units).
    pub radius: f32,
    /// A spotlight's cutoff angle (radians); pi or more shines every way.
    pub cutoff_angle: f32,
    /// (Protocol 37) the angle its light falls off from, to none at the
    /// cutoff (radians).
    pub falloff_angle: f32,
    /// (Protocol 38) its light tag (datum index), that tag's flags
    /// ([`LIGHT_MUZZLE_FLASH`] in the muzzle flashes'), the object it is
    /// attached to (datum index; `u32::MAX` for none), and 1 when that object
    /// is the player's first-person unit or something attached to it.
    pub definition_index: u32,
    pub definition_flags: u32,
    pub object_index: u32,
    pub first_person: u32,
    /// (Protocol 39) its up: a light's gel (a cube map round it, the a10
    /// alarm lights' two beams) is turned to its forward and up.
    pub up: [f32; 3],
}

/// A light tag flag that, in the retail maps, only the weapons' muzzle
/// flashes have (bit 5).
pub const LIGHT_MUZZLE_FLASH: u32 = 1 << 5;

/// (Protocol 41) a light's `definition_flags` bit no light tag has, for a
/// contrail's line, after the lights: its contrail tag as the definition,
/// its middle as the position, its direction from its oldest point to its
/// newest as the forward, half its length as the radius, no colour.
pub const LIGHT_CONTRAIL: u32 = 1 << 31;

/// A ray the game tested against its structure's collision geometry
/// (front-facing surfaces), with `game.bevy_bridge_ray_tests` on.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RayTest {
    pub point: [f32; 3],
    pub vector: [f32; 3],
    pub hit: u32,
    /// How far along `vector` (0..1).
    pub t: f32,
    pub surface_index: i32,
    /// A sphere tested half its radius short of the hit: what it touched
    /// (surfaces, edges, vertices, leaves) and an FNV-1a checksum of their
    /// indices in the order found.
    pub sphere_center: [f32; 3],
    pub sphere_radius: f32,
    pub sphere_counts: [u16; 4],
    pub sphere_checksum: u32,
    /// A sphere swept from the ray's start along `pill_vector` and where it
    /// first touched (`collision_bsp_test_pill`).
    pub pill_vector: [f32; 3],
    pub pill_radius: f32,
    pub pill_hit: u32,
    pub pill_t: f32,
    pub pill_normal: [f32; 3],
    pub pill_surface_index: i32,
    /// The game's whole ray test (`collision_test_vector`, the structure and
    /// every object type): the object it hit (`u32::MAX` for the structure
    /// or nothing; `object_surface_index` -1 for nothing), the collision
    /// node, bsp and surface, and how far along `vector`.
    pub object_index: u32,
    pub object_node_index: i16,
    pub object_bsp_index: i16,
    pub object_t: f32,
    pub object_surface_index: i32,
    /// A point launched from the ray's start along it at 15 world units a
    /// second (`point_physics_update`, no wind, under water if
    /// `point_underwater`): where it was after [`POINT_PHYSICS_STEPS`]
    /// steps of 1/30 s, its velocity, the steps' result flags or'd, and how
    /// many steps collided.
    pub point_radius: f32,
    pub point_underwater: u32,
    pub point_position: [f32; 3],
    pub point_velocity: [f32; 3],
    pub point_result_flags: u32,
    pub point_collisions: u32,
}

/// FNV-1a over 32-bit indices, as the game checksums a sphere test's.
pub fn feature_checksum(mut hash: u32, indices: &[i32]) -> u32 {
    for &index in indices {
        for byte in (index as u32).to_le_bytes() {
            hash ^= byte as u32;
            hash = hash.wrapping_mul(16_777_619);
        }
    }
    hash
}

#[repr(C)]
struct Frame {
    sequence: u32,
    frame_index: u32,
    game_tick: u32,
    structure_bsp_index: i32,
    scenario_name: [u8; 64],
    camera_valid: u32,
    camera: Camera,
    fog: Fog,
    fog_screen: FogScreen,
    decal_count: u32,
    decal_vertex_count: u32,
    decals: [Decal; DECALS],
    decal_vertices: [DecalVertex; DECAL_VERTICES],
    sprite_batch_count: u32,
    sprite_vertex_count: u32,
    sprite_batches: [SpriteBatch; SPRITE_BATCHES],
    sprite_vertices: [SpriteVertex; SPRITE_VERTICES],
    light_count: u32,
    lights: [Light; MAXIMUM_LIGHTS],
    ray_test_count: u32,
    ray_tests: [RayTest; RAY_TESTS],
    point_physics_definition: u32,
    object_count: u32,
    node_count: u32,
    objects: [Object; MAXIMUM_OBJECTS],
    nodes: [Node; MAXIMUM_NODES],
    physics_vehicle_count: u32,
    physics_vehicles: [PhysicsVehicle; PHYSICS_VEHICLES],
    vehicle_ray_test_count: u32,
    vehicle_ray_tests: [VehicleRayTest; VEHICLE_RAY_TESTS],
}

/// (Protocol 26) a vehicle as its physics shape is placed
/// (`physics_instance_new`: `object_get_origin`, `object_get_orientation`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PhysicsVehicle {
    pub object_index: u32,
    pub origin: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
}

/// (Protocol 26) a ray from the camera at a vehicle (through its bounding
/// sphere's centre, and above it), tested as `collision_test_vector` does
/// with `_collision_test_use_vehicle_physics_bit` (the structure and every
/// object type, front-facing; vehicles by their physics' mass points): the
/// result's type (-1 for nothing), object, t, plane, node and surface.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VehicleRayTest {
    pub point: [f32; 3],
    pub vector: [f32; 3],
    pub kind: i16,
    pub node_index: i16,
    pub object_index: u32,
    pub t: f32,
    pub plane: [f32; 4],
    pub surface_index: i32,
}

const _: () = assert!(std::mem::size_of::<PhysicsVehicle>() == 40);
const _: () = assert!(std::mem::size_of::<VehicleRayTest>() == 56);

/// What a renderer's window receives, for the game to read as its own
/// (`struct bevy_bridge_input`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Input {
    /// Nonzero while input is forwarded.
    pub active: u32,
    /// The keys held, a bit per SDL scancode (USB HID usage).
    pub keys: [u32; 16],
    /// Cumulative relative mouse motion (pixels) and wheel steps.
    pub mouse_x: i32,
    pub mouse_y: i32,
    pub wheel: i32,
    /// The mouse buttons held, a bit per SDL button number (1 left, 2
    /// middle, 3 right, 4 and 5 the side buttons).
    pub mouse_buttons: u32,
    /// Nonzero while the renderer flies its own camera: the first player
    /// hears from it. The game's coordinates (world units, Z up).
    pub listener_valid: u32,
    pub listener_position: [f32; 3],
    pub listener_forward: [f32; 3],
    pub listener_up: [f32; 3],
    /// The cursor over the window while the game's menus have a pointer
    /// ([`Bridge::menus_want_pointer`]): 0 for none, or [`Input::pointer_at`].
    pub pointer: u32,
}

impl Input {
    pub fn set_key(&mut self, scancode: u32, down: bool) {
        if scancode < 512 {
            let (word, bit) = ((scancode / 32) as usize, scancode % 32);
            if down {
                self.keys[word] |= 1 << bit;
            } else {
                self.keys[word] &= !(1 << bit);
            }
        }
    }

    /// A cursor at this fraction of the window across and down (0..1, from
    /// the top left), for [`Input::pointer`]: bit 30 set, x in bits 0..15 and
    /// y in bits 15..30, each a fraction of 32767.
    pub fn pointer_at(x: f32, y: f32) -> u32 {
        let fraction = |v: f32| (v.clamp(0.0, 1.0) * 32767.0).round() as u32;
        1 << 30 | fraction(y) << 15 | fraction(x)
    }
}

/// A gamepad forwarded to the game (`struct bevy_bridge_gamepad`), in SDL's
/// terms: a bit per `SDL_GamepadButton`, axes in `SDL_GamepadAxis` order
/// and ranges (sticks -32768..32767 with y down, triggers 0..32767).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Gamepad {
    pub buttons: u32,
    pub axes: [i16; 6],
    /// The game's rumble (low and high frequency motors, 0..65535); only the
    /// game writes it.
    pub rumble: [u16; 2],
}

pub const GAMEPADS: usize = 4;

/// `SDL_GamepadButton`.
pub mod sdl_button {
    pub const SOUTH: u32 = 0;
    pub const EAST: u32 = 1;
    pub const WEST: u32 = 2;
    pub const NORTH: u32 = 3;
    pub const BACK: u32 = 4;
    pub const GUIDE: u32 = 5;
    pub const START: u32 = 6;
    pub const LEFT_STICK: u32 = 7;
    pub const RIGHT_STICK: u32 = 8;
    pub const LEFT_SHOULDER: u32 = 9;
    pub const RIGHT_SHOULDER: u32 = 10;
    pub const DPAD_UP: u32 = 11;
    pub const DPAD_DOWN: u32 = 12;
    pub const DPAD_LEFT: u32 = 13;
    pub const DPAD_RIGHT: u32 = 14;
}

#[repr(C)]
struct Gamepads {
    active: u32,
    count: u32,
    pads: [Gamepad; GAMEPADS],
}

/// A projectile's state, as `projectile_update` reads and leaves it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProjectileState {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub projectile_flags: u32,
    pub object_flags: u32,
    pub action: i16,
    pub hit_material_type: i16,
    pub ignore_object_index: i32,
    pub target_object_index: i32,
    pub parent_object_index: i32,
    pub detonation_timer: f32,
    pub detonation_timer_delta: f32,
    pub arming_time: f32,
    pub arming_time_delta: f32,
    pub odometer: f32,
    pub deceleration_timer: f32,
    pub deceleration_timer_delta: f32,
    pub deceleration: f32,
    pub maximum_damage_distance: f32,
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub pad: i16,
}

/// One `projectile_update` the game recorded (with
/// `game.bevy_bridge_ray_tests` on): the state before and after, the
/// global random seed as it began and as each collision response began,
/// and whether a collision touched an object.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProjectileRecord {
    pub sequence: u32,
    pub projectile_index: u32,
    pub definition_index: u32,
    pub structure_bsp_index: i32,
    pub random_seed: u32,
    pub collision_count: u32,
    pub collision_seeds: [u32; 4],
    pub touched_object: u32,
    pub before: ProjectileState,
    pub after: ProjectileState,
    /// (Protocol 27) the objects its ray tests visited (`object_count` from
    /// `first_object` in the feature object ring), what each collision's
    /// impact damage reported (`object_cause_damage`'s material type, -2 for
    /// no damage done), and its guided steering, step by step.
    pub first_object: u32,
    pub object_count: u32,
    pub damage_material_types: [i16; 4],
    /// And the random seed the damage left (the response draws from it).
    pub damage_seeds: [u32; 4],
    pub guidance_count: u32,
    pub guidance: [ProjectileGuidance; 4],
}

/// One `biped_update_physics` the game recorded (with
/// `game.bevy_bridge_ray_tests` on): the biped's physics before and after,
/// which breakable surfaces stood (bit i of byte i / 8), and what the
/// crouching player's stand test reads (`stand_test`: whether it is made).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BipedRecord {
    pub sequence: u32,
    pub biped_index: u32,
    pub structure_bsp_index: i32,
    pub stand_test: u32,
    pub stand_height: f32,
    pub stand_radius: f32,
    pub breakable_surfaces: [u8; 32],
    /// The objects the broad phase visited during the update (in the
    /// feature object ring), and how many times features were gathered.
    pub first_feature_object: u32,
    pub feature_object_count: u32,
    pub feature_calls: u32,
    pub before: halo_map::biped::BipedPhysics,
    pub after: halo_map::biped::BipedPhysics,
}

const _: () = assert!(std::mem::size_of::<BipedRecord>() == 476);

/// An object the broad phase visited while a biped update gathered
/// collision features, as it was then (`call`: which gathering, from 0).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FeatureObjectRecord {
    pub sequence: u32,
    pub object_index: u32,
    pub definition_index: u32,
    pub call: u32,
    pub kind: i16,
    pub dead: i16,
    pub flags: u32,
    pub bounding_center: [f32; 3],
    pub bounding_radius: f32,
    pub parent_object_index: i32,
    pub translational_velocity: [f32; 3],
    pub elevator: u32,
    pub biped_passes_through: u32,
    pub player_index: i32,
    pub parent_seat_index: i32,
    pub pill_base: [f32; 3],
    pub pill_height: f32,
    pub pill_width: f32,
    pub region_permutations: [u8; 8],
    pub node_count: u32,
    pub first_matrix: u32,
    /// (Protocol 27) its location, where the broad phase registers it.
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub pad: i16,
    /// A projectile's own flags, 0 for other objects.
    pub projectile_flags: u32,
}

const _: () = assert!(std::mem::size_of::<FeatureObjectRecord>() == 120);

/// (Protocol 27) a guided projectile's steering in an update's step: its
/// bounding sphere's centre, its target's and the target's centre of mass
/// (`unit_get_center_of_mass`), the difficulty's guidance scale when the
/// target is a player's unit (1 otherwise), and the game time.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProjectileGuidance {
    pub projectile_center: [f32; 3],
    pub target_center: [f32; 3],
    pub target_center_of_mass: [f32; 3],
    pub guidance_scale: f32,
    pub game_time: i32,
}

/// A recorded projectile update with the objects its ray tests visited (each
/// with its node matrices).
#[derive(Debug, Clone)]
pub struct ProjectileUpdate {
    pub record: ProjectileRecord,
    pub objects: Vec<(FeatureObjectRecord, Vec<[f32; 13]>)>,
    /// Whether every object (and matrix) was still in the rings.
    pub complete: bool,
}

/// One `biped_update_moving` the game recorded: the biped's movement state
/// before and after, what it read of the biped and asked of the rest of the
/// game, what its actor's jump aim answered, whether it aborted the unit's
/// animation, and the biped physics record it made (-1 for none).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BipedMovingRecord {
    pub sequence: u32,
    pub biped_index: u32,
    pub definition_index: u32,
    pub structure_bsp_index: i32,
    pub physics_record: i32,
    pub dead: u32,
    pub ignore_translation: u32,
    pub suspended: u32,
    pub control_flags: u32,
    pub unit_animation_state: i8,
    pub base_seat_index: i8,
    pub aiming_speed: i8,
    pub actor: i8,
    pub player_index: i32,
    pub throttle: [f32; 3],
    pub body_stun: f32,
    pub recent_body_damage: f32,
    pub desired_facing_vector: [f32; 3],
    pub aiming_vector: [f32; 3],
    pub bounding_center: [f32; 3],
    pub animation_graph_index: u32,
    pub animation_index: i16,
    pub animation_frame_index: i16,
    pub difficulty_infection_forms: f32,
    pub speed_multiplier: f32,
    pub cinematic: u32,
    pub double_speed: u32,
    pub coop_player_collisions: u32,
    pub actor_leaping: u32,
    pub recorded_animation: u32,
    pub actor_jump_asked: u32,
    pub actor_jumped: u32,
    pub actor_jump_velocity: [f32; 3],
    pub aborted_animation: u32,
    pub before: halo_map::biped_moving::MovingState,
    pub after: halo_map::biped_moving::MovingState,
}

const _: () = assert!(std::mem::size_of::<BipedMovingRecord>() == 432);

/// One `biped_update` of an unparented biped up to its animation update:
/// the state before and after, what it read, the global random seed as its
/// airborne update began (if it had one), and its movement record (-1 for
/// none).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BipedUpdateRecord {
    pub sequence: u32,
    pub biped_index: u32,
    pub definition_index: u32,
    pub structure_bsp_index: i32,
    pub moving_record: i32,
    pub dead: u32,
    pub control_flags: u32,
    pub aim_without_turning: u32,
    pub unit_animation_state: i8,
    pub base_seat_index: i8,
    pub enhanced_animation: i8,
    pub limp_body: i8,
    pub player_index: i32,
    pub airborne_seed_taken: u32,
    pub airborne_seed: u32,
    pub before: halo_map::biped_update::UpdateState,
    pub after: halo_map::biped_update::UpdateState,
}

const _: () = assert!(std::mem::size_of::<BipedUpdateRecord>() == 376);

/// One `unit_update_animation` of a biped: the state before and after,
/// what it read, its side effects (`halo_map::unit_animation::side_effects`)
/// and what it returned.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UnitAnimationRecord {
    pub sequence: u32,
    pub unit_index: u32,
    pub definition_index: u32,
    pub random_seed: u32,
    pub state_desired: i8,
    pub crouching: i8,
    pub desired_state: i8,
    pub dead: i8,
    pub parented: i8,
    pub player: i8,
    pub force_alert: i8,
    pub flaming: i8,
    pub magic_base_seat_index: i16,
    pub magic_seat_index: i8,
    pub at_rest: i8,
    pub airborne: i8,
    pub pad: [i8; 3],
    pub weapon_label: [u8; 32],
    pub side_effects: u32,
    pub result: i16,
    pub pad2: i16,
    pub before: halo_map::unit_animation::UnitAnimationState,
    pub after: halo_map::unit_animation::UnitAnimationState,
}

const _: () = assert!(std::mem::size_of::<UnitAnimationRecord>() == 152);

/// A node of a recorded pose: its orientation (rotation i j k w,
/// translation, scale) from the animation, overlays and scale, after its
/// object type's preprocess, the original it interpolates from, and its
/// matrix (scale, forward, left, up, position).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PoseNodeRecord {
    pub animated: [f32; 8],
    pub preprocessed: [f32; 8],
    pub original: [f32; 8],
    pub matrix: [f32; 13],
}

/// One sampled `object_compute_node_matrices`: what it read, and where its
/// nodes are in the pose node ring.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PoseRecord {
    pub sequence: u32,
    pub object_index: u32,
    pub definition_index: u32,
    pub kind: i16,
    pub node_count: i16,
    pub first_node: u32,
    pub object_flags: u32,
    pub animation_graph_index: u32,
    pub animation_index: i16,
    pub frame_index: i16,
    pub interpolation_frame_index: i16,
    pub interpolation_frame_count: i16,
    pub world_relative: u32,
    pub scale: f32,
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub function_values: [f32; 4],
    pub has_parent: u32,
    pub parent_mirrored: u32,
    pub parent_node_matrix: [f32; 13],
    /// (Protocol 23) a biped's (`unit_recorded`): what its type's
    /// preprocess reads (its animation state, emotion, mouth, seat
    /// acceleration, aiming and looking, its orientation as
    /// `object_get_orientation` gives it, and for a grenade throw on the
    /// move whether its animations are enhanced, whether it flies through
    /// the air, its velocity, the game time and the legs' eased weights
    /// before) and what it sets (whether it aims and looks with euler
    /// screens, and their bounds).
    pub unit_recorded: u32,
    pub unit_enhanced: u32,
    pub unit_airborne: u32,
    pub unit_parented: u32,
    pub unit_has_weapon: u32,
    pub unit_player: u32,
    pub game_time: i32,
    pub unit_animation_flags: u16,
    pub aiming_screen_index: i16,
    pub looking_screen_index: i16,
    pub override_emotion_animation_index: i16,
    pub seat_index: i8,
    pub weapon_index: i8,
    pub state: i8,
    pub action: i8,
    pub base_seat_index: i8,
    pub emotion_index: i8,
    pub pad: i16,
    pub action_animation: [i16; 2],
    pub overlay_action_animation: [i16; 2],
    pub soft_ping_animation: [i16; 2],
    pub mouth_aperture: f32,
    pub seat_acceleration: [f32; 3],
    pub aiming_vector: [f32; 3],
    pub looking_vector: [f32; 3],
    pub orientation_forward: [f32; 3],
    pub orientation_up: [f32; 3],
    pub translational_velocity: [f32; 3],
    pub grenade_leg_weight: f32,
    pub grenade_leg_time: i32,
    pub grenade_air_weight: f32,
    pub grenade_air_time: i32,
    pub aiming_with_euler_screen: u32,
    pub looking_with_euler_screen: u32,
    pub aiming_screen_bounds: [f32; 4],
    pub looking_screen_bounds: [f32; 4],
    /// (Protocol 24; the game time is every pose's, for its overlays, and
    /// the unit fields every unit's.) A vehicle's (`vehicle_recorded`):
    /// its turn, speed, wheel and suspensions; a device's
    /// (`device_recorded`): its flags, position and power.
    pub vehicle_recorded: u32,
    pub vehicle_turn: f32,
    pub vehicle_speed: f32,
    pub vehicle_wheel: f32,
    pub vehicle_suspension: [u8; 8],
    pub device_recorded: u32,
    pub device_flags: u32,
    pub device_position: f32,
    pub device_power: f32,
}

/// (Protocol 25) a player biped's melee step in `biped_update`: what it
/// reads (its melee ticks, whether the player asks to melee, its weapon
/// (`u32::MAX` for none) and whether the weapon is overcharged, whether it
/// rides in a seat or zooms, its unit animation's seat, weapon class and
/// type and action, the random seed), whether it started a melee
/// (`unit_animation_start_action`: the action, its animation and the random
/// seed after) or caused its damage, and its melee ticks after.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MeleeRecord {
    pub sequence: u32,
    pub biped_index: u32,
    pub definition_index: u32,
    pub weapon_definition_index: u32,
    pub weapon_overcharged: u32,
    pub use_equipment: u32,
    pub in_seat: u32,
    pub zoomed: u32,
    pub enhanced: u32,
    pub random_seed_before: u32,
    pub random_seed_after_start: u32,
    pub started: u32,
    pub damaged: u32,
    pub ticks_before: i8,
    pub attack_tick_before: i8,
    pub ticks_after: i8,
    pub attack_tick_after: i8,
    pub seat_index: i8,
    pub weapon_index: i8,
    pub weapon_type_index: i8,
    pub action_before: i8,
    pub action_after: i8,
    pub pad: [i8; 3],
    pub action_animation_before: [i16; 2],
    pub action_animation_after: [i16; 2],
}

const _: () = assert!(std::mem::size_of::<MeleeRecord>() == 72);

/// (Protocol 28) a vehicle as its physics reads and leaves it: its place,
/// axes, velocities, flags, location, the collision force and torque other
/// vehicles gave it, the mass points that stuck, and (after) the bounding
/// sphere centre its new pose gave it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VehiclePhysicsState {
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub translational_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub object_flags: u32,
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub pad: i16,
    pub collision_force: [f32; 3],
    pub collision_torque: [f32; 3],
    pub stuck_mass_point_flags: u32,
    pub bounding_center: [f32; 3],
}

/// A powered mass point's controls (ground, water and air friction
/// velocities, water and air lift ratios, thrust and antigrav fractions;
/// rotation i j k w).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PoweredMassPointRecord {
    pub values: [f32; 7],
    pub rotation: [f32; 4],
}

/// A vehicle the unit collisions considered: its physics placement and
/// whether it was at rest.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VehiclePhysicsOther {
    pub object_index: u32,
    pub definition_index: u32,
    pub at_rest: u32,
    pub origin: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
}

/// (Protocol 28) one `physics_update` of a vehicle: its state before and
/// after, what its vehicle code gave it, which breakable surfaces stood,
/// its mass points after (in the mass point ring), the objects its
/// collision tests visited (in the feature object ring), the other
/// vehicles its unit collisions considered, and whether it took the old
/// physics.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VehiclePhysicsRecord {
    pub sequence: u32,
    pub vehicle_index: u32,
    pub definition_index: u32,
    pub structure_bsp_index: i32,
    pub old_physics: u32,
    pub powered_count: u32,
    pub powered: [PoweredMassPointRecord; 8],
    pub has_magic: u32,
    pub magic_force: [f32; 3],
    pub magic_torque: [f32; 3],
    pub breakable_surfaces: [u8; 32],
    pub before: VehiclePhysicsState,
    pub after: VehiclePhysicsState,
    pub mass_point_count: u32,
    pub first_mass_point: u32,
    pub first_object: u32,
    pub object_count: u32,
    pub feature_calls: u32,
    pub other_count: u32,
    pub others: [VehiclePhysicsOther; 8],
}

const _: () = assert!(std::mem::size_of::<VehiclePhysicsState>() == 112);
const _: () = assert!(std::mem::size_of::<VehiclePhysicsRecord>() == 1068);

/// (Protocol 30) a vehicle as `vehicle_update` reads and leaves it: its
/// place, axes, velocities, flags, location, the bounding sphere centre its
/// pose gave it, and its vehicle datum (`_vehicle_datum`, 88 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VehicleUpdateState {
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub translational_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub object_flags: u32,
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub pad: i16,
    pub bounding_center: [f32; 3],
    pub vehicle: [u8; 88],
}

/// (Protocol 30) one `vehicle_update`, up to its animation: its state
/// before and after, its driver's controls, whether it has a parent, the
/// physics record it made (its sequence + 1, 0 for none), which breakable
/// surfaces stood, and the objects its other ray tests visited.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VehicleUpdateRecord {
    pub sequence: u32,
    pub vehicle_index: u32,
    pub definition_index: u32,
    pub structure_bsp_index: i32,
    pub throttle: [f32; 3],
    pub control_flags: u32,
    pub desired_facing: [f32; 3],
    pub seat_power: [f32; 2],
    pub driven_by_ai: u32,
    pub parented: u32,
    pub physics_record: u32,
    pub breakable_surfaces: [u8; 32],
    pub before: VehicleUpdateState,
    pub after: VehicleUpdateState,
    pub first_object: u32,
    pub object_count: u32,
}

const _: () = assert!(std::mem::size_of::<VehicleUpdateState>() == 172);
const _: () = assert!(std::mem::size_of::<VehicleUpdateRecord>() == 448);

/// A recorded vehicle update with the objects its ray tests visited.
#[derive(Debug, Clone)]
pub struct VehicleUpdate {
    pub record: VehicleUpdateRecord,
    pub objects: Vec<(FeatureObjectRecord, Vec<[f32; 13]>)>,
    pub complete: bool,
}

/// A recorded vehicle physics update with its mass points (each the game's
/// `mass_point_datum`, 304 bytes) and the objects its tests visited.
#[derive(Debug, Clone)]
pub struct VehiclePhysicsUpdate {
    pub record: VehiclePhysicsRecord,
    pub mass_points: Vec<[u8; 304]>,
    pub objects: Vec<(FeatureObjectRecord, Vec<[f32; 13]>)>,
    /// Whether every mass point and object was still in the rings.
    pub complete: bool,
}
const _: () = assert!(std::mem::size_of::<PoseNodeRecord>() == 148);
const _: () = assert!(std::mem::size_of::<PoseRecord>() == 384);

/// A recorded biped update with the objects its features came from (each
/// with its node matrices: scale, forward, left, up, position).
#[derive(Debug, Clone)]
pub struct BipedUpdate {
    pub record: BipedRecord,
    pub objects: Vec<(FeatureObjectRecord, Vec<[f32; 13]>)>,
    /// Whether every object (and matrix) was still in the rings.
    pub complete: bool,
}
const _: () = assert!(std::mem::size_of::<ProjectileState>() == 92);
const _: () = assert!(std::mem::size_of::<ProjectileRecord>() == 228 + 16 + 16 + 4 + 44 * 4);

#[repr(C)]
struct OverlayHeader {
    sequence: u32,
    frame_index: u32,
    width: u32,
    height: u32,
    multiply: u32,
    reserved: [u32; 3],
}

#[repr(C)]
struct Shared {
    magic: u32,
    version: u32,
    size: u32,
    latest_frame: u32,
    writer_process_id: u32,
    /// nonzero while the game's menus are up and take a pointer
    menus_pointer: u32,
    reserved: [u32; 2],
    input: Input,
    gamepads: Gamepads,
    frames: [Frame; FRAMES],
    overlay_enabled: u32,
    latest_overlay: u32,
    overlay_reserved: [u32; 2],
    overlays: [OverlayHeader; OVERLAYS],
    projectile_record_count: u32,
    random_direction_count: u32,
    projectile_records: [ProjectileRecord; PROJECTILE_RECORDS],
    periodic_tables_valid: u32,
    periodic_tables: [[u8; 1024]; 12],
    random_directions: [[f32; 3]; RANDOM_DIRECTIONS],
    biped_record_count: u32,
    biped_pad: u32,
    biped_records: [BipedRecord; BIPED_RECORDS],
    feature_object_count: u32,
    feature_matrix_count: u32,
    feature_objects: [FeatureObjectRecord; FEATURE_OBJECTS],
    feature_matrices: [[f32; 13]; FEATURE_MATRICES],
    biped_moving_record_count: u32,
    biped_moving_pad: u32,
    biped_moving_records: [BipedMovingRecord; BIPED_MOVING_RECORDS],
    biped_update_record_count: u32,
    biped_update_pad: u32,
    biped_update_records: [BipedUpdateRecord; BIPED_UPDATE_RECORDS],
    unit_animation_record_count: u32,
    unit_animation_pad: u32,
    unit_animation_records: [UnitAnimationRecord; UNIT_ANIMATION_RECORDS],
    pose_record_count: u32,
    pose_node_count: u32,
    pose_records: [PoseRecord; POSE_RECORDS],
    pose_nodes: [PoseNodeRecord; POSE_NODES],
    melee_record_count: u32,
    melee_records: [MeleeRecord; MELEE_RECORDS],
    vehicle_physics_record_count: u32,
    mass_point_count: u32,
    vehicle_physics_records: [VehiclePhysicsRecord; VEHICLE_PHYSICS_RECORDS],
    mass_points: [[u8; 304]; MASS_POINTS],
    vehicle_update_record_count: u32,
    vehicle_update_records: [VehicleUpdateRecord; VEHICLE_UPDATE_RECORDS],
    // (each overlay's picture, then its multiply's)
    overlay_pixels: [[u8; OVERLAY_MAXIMUM_PIXELS * 4 * 2]; OVERLAYS],
}

/// The layout is the game's, measured there; keep these in step with
/// `bevy_bridge_protocol.h`.
const _: () = assert!(std::mem::size_of::<Object>() == 124);
const _: () = assert!(std::mem::size_of::<Node>() == 52);
const _: () = assert!(std::mem::size_of::<Camera>() == 40);
const _: () = assert!(std::mem::size_of::<Input>() == 128);
const _: () = assert!(std::mem::size_of::<Gamepad>() == 20);

/// One frame of the game's world.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub frame_index: u32,
    pub game_tick: u32,
    pub structure_bsp_index: i32,
    /// The scenario tag's name, e.g. `levels\b30\b30`.
    pub scenario_name: String,
    pub camera: Option<Camera>,
    /// The fog the camera's view was drawn with, once the game shares it.
    pub fog: Option<Fog>,
    /// The fog screen over it, when there is one.
    pub fog_screen: Option<FogScreen>,
    /// The decals it would have drawn (with the viewer drawing them), and
    /// their vertices.
    pub decals: Vec<Decal>,
    pub decal_vertices: Vec<DecalVertex>,
    /// The sprites it would have drawn (with the viewer drawing them), and
    /// their vertices.
    pub sprite_batches: Vec<SpriteBatch>,
    pub sprite_vertices: Vec<SpriteVertex>,
    pub lights: Vec<Light>,
    pub ray_tests: Vec<RayTest>,
    /// The point physics definition the ray tests' points use (`u32::MAX`
    /// for none).
    pub point_physics_definition: u32,
    pub objects: Vec<Object>,
    pub nodes: Vec<Node>,
    /// The vehicles' physics placement and rays at them tested with their
    /// physics shapes (with `game.bevy_bridge_ray_tests` on).
    pub physics_vehicles: Vec<PhysicsVehicle>,
    pub vehicle_ray_tests: Vec<VehicleRayTest>,
}

impl Snapshot {
    /// The map file's name: the scenario name's last part.
    pub fn map_name(&self) -> &str {
        self.scenario_name.rsplit('\\').next().unwrap_or("")
    }

    pub fn object_nodes(&self, object: &Object) -> &[Node] {
        let first = object.first_node as usize;
        self.nodes.get(first..first + object.node_count as usize).unwrap_or(&[])
    }
}

/// An overlay picture's size and the frame it shows; its pixels are RGBA,
/// 8 bits a channel, sRGB, premultiplied, the top row first. With
/// `multiply`, as many pixels again follow: what the game's multiplying
/// blends do to the world, as half the factor (128: nothing), display
/// encoded, for the world to be multiplied by twice it under the picture.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Overlay {
    pub frame_index: u32,
    pub width: u32,
    pub height: u32,
    pub multiply: bool,
}

#[derive(Debug)]
pub enum Error {
    /// No game is sharing frames (not running, or game.bevy_bridge is off).
    NotRunning,
    Incompatible(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotRunning => write!(f, "no game is sharing its frames (start OpenCE with HALO_BEVY_BRIDGE=1)"),
            Error::Incompatible(s) => write!(f, "incompatible bridge: {s}"),
        }
    }
}

impl std::error::Error for Error {}

/// The shared memory: frames read, input written.
pub struct Bridge {
    shared: *const Shared,
    #[cfg(windows)]
    _mapping: windows::Handle,
    last_frame_index: u32,
    last_overlay: Option<(u32, u32)>,
    last_projectile_record: Option<u32>,
    last_melee_record: Option<u32>,
    last_vehicle_physics_record: Option<u32>,
    last_vehicle_update_record: Option<u32>,
    last_biped_record: Option<u32>,
    last_biped_moving_record: Option<u32>,
    last_biped_update_record: Option<u32>,
    last_unit_animation_record: Option<u32>,
    last_pose_record: Option<u32>,
}

// Frames are read through volatile reads and a sequence check; the input
// block is only written by this side.
unsafe impl Send for Bridge {}
unsafe impl Sync for Bridge {}

impl Bridge {
    /// Opens the shared memory of the game using the default name.
    pub fn open() -> Result<Self, Error> {
        Self::open_named(DEFAULT_NAME)
    }

    /// Opens the shared memory of the game started with
    /// `HALO_BEVY_BRIDGE_NAME=name`.
    pub fn open_named(name: &str) -> Result<Self, Error> {
        #[cfg(windows)]
        {
            let mapping_name = format!("Local\\{name}\0");
            let (mapping, view) = windows::open(mapping_name.as_bytes(), std::mem::size_of::<Shared>())?;
            let bridge = Bridge { shared: view as *const Shared, _mapping: mapping, last_frame_index: 0, last_overlay: None, last_projectile_record: None, last_melee_record: None, last_vehicle_physics_record: None, last_vehicle_update_record: None, last_biped_record: None, last_biped_moving_record: None, last_biped_update_record: None, last_unit_animation_record: None, last_pose_record: None };
            bridge.check()?;
            Ok(bridge)
        }
        #[cfg(not(windows))]
        {
            let _ = name;
            Err(Error::NotRunning)
        }
    }

    fn check(&self) -> Result<(), Error> {
        // SAFETY: the view is at least size_of::<Shared>() bytes (mapped so)
        let (magic, version, size) = unsafe {
            (
                std::ptr::read_volatile(&(*self.shared).magic),
                std::ptr::read_volatile(&(*self.shared).version),
                std::ptr::read_volatile(&(*self.shared).size),
            )
        };
        if magic != MAGIC {
            return Err(Error::NotRunning);
        }
        if version != VERSION || size as usize != std::mem::size_of::<Shared>() {
            return Err(Error::Incompatible(format!(
                "version {version} size {size}; this reader is version {VERSION} size {}",
                std::mem::size_of::<Shared>()
            )));
        }
        Ok(())
    }

    /// Forwards input to the game (see [`Input`]).
    pub fn write_input(&mut self, input: &Input) {
        // SAFETY: the view is writable and at least size_of::<Shared>()
        // bytes; the game only reads this block
        unsafe {
            let target = &raw const (*self.shared).input as *mut Input;
            std::ptr::write_volatile(target, *input);
        }
        fence(Ordering::Release);
    }

    /// Forwards gamepads to the game, which then takes them in place of its
    /// own (the first shares port 0 with the keyboard). Their rumble fields
    /// are the game's and are left alone.
    pub fn write_gamepads(&mut self, pads: &[Gamepad]) {
        let count = pads.len().min(GAMEPADS);
        // SAFETY: the view is writable and at least size_of::<Shared>()
        // bytes; the game only reads these fields
        unsafe {
            let block = &raw const (*self.shared).gamepads as *mut Gamepads;
            for (index, pad) in pads.iter().take(count).enumerate() {
                let target = &raw mut (*block).pads[index];
                std::ptr::write_volatile(&raw mut (*target).buttons, pad.buttons);
                std::ptr::write_volatile(&raw mut (*target).axes, pad.axes);
            }
            for index in count..GAMEPADS {
                let target = &raw mut (*block).pads[index];
                std::ptr::write_volatile(&raw mut (*target).buttons, 0);
                std::ptr::write_volatile(&raw mut (*target).axes, [0; 6]);
            }
            std::ptr::write_volatile(&raw mut (*block).count, count as u32);
            fence(Ordering::Release);
            std::ptr::write_volatile(&raw mut (*block).active, 1);
        }
    }

    /// The game's rumble for each gamepad (low, high frequency motor).
    pub fn rumble(&self) -> [[u16; 2]; GAMEPADS] {
        // SAFETY: as in check()
        unsafe { std::array::from_fn(|i| std::ptr::read_volatile(&(*self.shared).gamepads.pads[i].rumble)) }
    }

    /// The projectile updates the game recorded since the last call (the
    /// first call starts from now). Records the ring overwrote meanwhile
    /// are skipped.
    pub fn read_projectile_records(&mut self) -> Vec<ProjectileRecord> {
        // SAFETY: as in check(); records are plain old data, checked by their
        // sequence number after copying
        unsafe {
            let count = std::ptr::read_volatile(&(*self.shared).projectile_record_count);
            fence(Ordering::Acquire);
            let first = match self.last_projectile_record {
                None => count,
                Some(last) => last.max(count.saturating_sub(PROJECTILE_RECORDS as u32 / 2)),
            };
            let mut out = Vec::new();
            for sequence in first..count {
                let record = std::ptr::read_volatile(&(*self.shared).projectile_records[sequence as usize % PROJECTILE_RECORDS]);
                if record.sequence == sequence {
                    out.push(record);
                }
            }
            self.last_projectile_record = Some(count);
            out
        }
    }

    /// Vehicles' physics updates the game recorded since the last call,
    /// with their mass points and the objects their tests visited.
    pub fn read_vehicle_physics(&mut self) -> Vec<VehiclePhysicsUpdate> {
        // SAFETY: as in check(); records are plain old data, checked by their
        // sequence numbers (and the rings' counts) after copying
        let records: Vec<VehiclePhysicsRecord> = unsafe {
            let count = std::ptr::read_volatile(&(*self.shared).vehicle_physics_record_count);
            fence(Ordering::Acquire);
            let first = match self.last_vehicle_physics_record {
                None => count,
                Some(last) => last.max(count.saturating_sub(VEHICLE_PHYSICS_RECORDS as u32 / 2)),
            };
            let mut out = Vec::new();
            for sequence in first..count {
                let record = std::ptr::read_volatile(&(*self.shared).vehicle_physics_records[sequence as usize % VEHICLE_PHYSICS_RECORDS]);
                if record.sequence == sequence {
                    out.push(record);
                }
            }
            self.last_vehicle_physics_record = Some(count);
            out
        };
        records
            .into_iter()
            .map(|record| {
                // SAFETY: as above
                let (mass_points, mass_points_complete) = unsafe {
                    let points: Vec<[u8; 304]> = (0..record.mass_point_count)
                        .map(|i| std::ptr::read_volatile(&(*self.shared).mass_points[record.first_mass_point.wrapping_add(i) as usize % MASS_POINTS]))
                        .collect();
                    fence(Ordering::Acquire);
                    let now = std::ptr::read_volatile(&(*self.shared).mass_point_count);
                    (points, now.wrapping_sub(record.first_mass_point) as usize <= MASS_POINTS)
                };
                let (objects, complete) = self.read_feature_objects(record.first_object, record.object_count);
                VehiclePhysicsUpdate { record, mass_points, objects, complete: complete && mass_points_complete }
            })
            .collect()
    }

    /// Vehicle updates the game recorded since the last call, with the
    /// objects their ray tests visited.
    pub fn read_vehicle_updates(&mut self) -> Vec<VehicleUpdate> {
        // SAFETY: as in check(); records are plain old data, checked by their
        // sequence numbers after copying
        let records: Vec<VehicleUpdateRecord> = unsafe {
            let count = std::ptr::read_volatile(&(*self.shared).vehicle_update_record_count);
            fence(Ordering::Acquire);
            let first = match self.last_vehicle_update_record {
                None => count,
                Some(last) => last.max(count.saturating_sub(VEHICLE_UPDATE_RECORDS as u32 / 2)),
            };
            let mut out = Vec::new();
            for sequence in first..count {
                let record = std::ptr::read_volatile(&(*self.shared).vehicle_update_records[sequence as usize % VEHICLE_UPDATE_RECORDS]);
                if record.sequence == sequence {
                    out.push(record);
                }
            }
            self.last_vehicle_update_record = Some(count);
            out
        };
        records
            .into_iter()
            .map(|record| {
                let (objects, complete) = self.read_feature_objects(record.first_object, record.object_count);
                VehicleUpdate { record, objects, complete }
            })
            .collect()
    }

    /// Players' melee steps the game recorded since the last call (as
    /// [`Self::read_projectile_records`]).
    pub fn read_melee_records(&mut self) -> Vec<MeleeRecord> {
        // SAFETY: as in check(); records are plain old data, checked by their
        // sequence number after copying
        unsafe {
            let count = std::ptr::read_volatile(&(*self.shared).melee_record_count);
            fence(Ordering::Acquire);
            let first = match self.last_melee_record {
                None => count,
                Some(last) => last.max(count.saturating_sub(MELEE_RECORDS as u32 / 2)),
            };
            let mut out = Vec::new();
            for sequence in first..count {
                let record = std::ptr::read_volatile(&(*self.shared).melee_records[sequence as usize % MELEE_RECORDS]);
                if record.sequence == sequence {
                    out.push(record);
                }
            }
            self.last_melee_record = Some(count);
            out
        }
    }

    /// The biped physics updates the game recorded since the last call (as
    /// [`Self::read_projectile_records`]).
    pub fn read_biped_records(&mut self) -> Vec<BipedRecord> {
        // SAFETY: as in check(); records are plain old data, checked by their
        // sequence number after copying
        unsafe {
            let count = std::ptr::read_volatile(&(*self.shared).biped_record_count);
            fence(Ordering::Acquire);
            let first = match self.last_biped_record {
                None => count,
                Some(last) => last.max(count.saturating_sub(BIPED_RECORDS as u32 / 2)),
            };
            let mut out = Vec::new();
            for sequence in first..count {
                let record = std::ptr::read_volatile(&(*self.shared).biped_records[sequence as usize % BIPED_RECORDS]);
                if record.sequence == sequence {
                    out.push(record);
                }
            }
            self.last_biped_record = Some(count);
            out
        }
    }

    /// The biped movement updates the game recorded since the last call (as
    /// [`Self::read_projectile_records`]).
    pub fn read_biped_moving_records(&mut self) -> Vec<BipedMovingRecord> {
        // SAFETY: as in check(); records are plain old data, checked by their
        // sequence number after copying
        unsafe {
            let count = std::ptr::read_volatile(&(*self.shared).biped_moving_record_count);
            fence(Ordering::Acquire);
            let first = match self.last_biped_moving_record {
                None => count,
                Some(last) => last.max(count.saturating_sub(BIPED_MOVING_RECORDS as u32 / 2)),
            };
            let mut out = Vec::new();
            for sequence in first..count {
                let record = std::ptr::read_volatile(&(*self.shared).biped_moving_records[sequence as usize % BIPED_MOVING_RECORDS]);
                if record.sequence == sequence {
                    out.push(record);
                }
            }
            self.last_biped_moving_record = Some(count);
            out
        }
    }

    /// The biped updates the game recorded since the last call (as
    /// [`Self::read_projectile_records`]).
    pub fn read_biped_update_records(&mut self) -> Vec<BipedUpdateRecord> {
        // SAFETY: as in check(); records are plain old data, checked by their
        // sequence number after copying
        unsafe {
            let count = std::ptr::read_volatile(&(*self.shared).biped_update_record_count);
            fence(Ordering::Acquire);
            let first = match self.last_biped_update_record {
                None => count,
                Some(last) => last.max(count.saturating_sub(BIPED_UPDATE_RECORDS as u32 / 2)),
            };
            let mut out = Vec::new();
            for sequence in first..count {
                let record = std::ptr::read_volatile(&(*self.shared).biped_update_records[sequence as usize % BIPED_UPDATE_RECORDS]);
                if record.sequence == sequence {
                    out.push(record);
                }
            }
            self.last_biped_update_record = Some(count);
            out
        }
    }

    /// The unit animation updates the game recorded since the last call (as
    /// [`Self::read_projectile_records`]).
    pub fn read_unit_animation_records(&mut self) -> Vec<UnitAnimationRecord> {
        // SAFETY: as in check(); records are plain old data, checked by their
        // sequence number after copying
        unsafe {
            let count = std::ptr::read_volatile(&(*self.shared).unit_animation_record_count);
            fence(Ordering::Acquire);
            let first = match self.last_unit_animation_record {
                None => count,
                Some(last) => last.max(count.saturating_sub(UNIT_ANIMATION_RECORDS as u32 / 2)),
            };
            let mut out = Vec::new();
            for sequence in first..count {
                let record = std::ptr::read_volatile(&(*self.shared).unit_animation_records[sequence as usize % UNIT_ANIMATION_RECORDS]);
                if record.sequence == sequence {
                    out.push(record);
                }
            }
            self.last_unit_animation_record = Some(count);
            out
        }
    }

    /// The poses the game recorded since the last call, each with its nodes
    /// and whether they were all still in the ring.
    pub fn read_poses(&mut self) -> Vec<(PoseRecord, Vec<PoseNodeRecord>, bool)> {
        // SAFETY: as in check(); records are plain old data, checked by their
        // sequence numbers (and the node ring's count) after copying
        unsafe {
            let count = std::ptr::read_volatile(&(*self.shared).pose_record_count);
            fence(Ordering::Acquire);
            let first = match self.last_pose_record {
                None => count,
                Some(last) => last.max(count.saturating_sub(POSE_RECORDS as u32 / 2)),
            };
            let mut out = Vec::new();
            for sequence in first..count {
                let record = std::ptr::read_volatile(&(*self.shared).pose_records[sequence as usize % POSE_RECORDS]);
                if record.sequence != sequence {
                    continue;
                }
                let nodes: Vec<PoseNodeRecord> = (0..record.node_count.max(0) as u32)
                    .map(|n| std::ptr::read_volatile(&(*self.shared).pose_nodes[record.first_node.wrapping_add(n) as usize % POSE_NODES]))
                    .collect();
                fence(Ordering::Acquire);
                let nodes_now = std::ptr::read_volatile(&(*self.shared).pose_node_count);
                let complete = (nodes_now.wrapping_sub(record.first_node) as usize) <= POSE_NODES;
                out.push((record, nodes, complete));
            }
            self.last_pose_record = Some(count);
            out
        }
    }

    /// The biped physics updates recorded since the last call, each with
    /// the objects its collision features came from.
    pub fn read_biped_updates(&mut self) -> Vec<BipedUpdate> {
        let records = self.read_biped_records();
        records
            .into_iter()
            .map(|record| {
                let (objects, complete) = self.read_feature_objects(record.first_feature_object, record.feature_object_count);
                BipedUpdate { record, objects, complete }
            })
            .collect()
    }

    /// The projectile updates the game recorded since the last call, with
    /// the objects their ray tests visited.
    pub fn read_projectile_updates(&mut self) -> Vec<ProjectileUpdate> {
        let records = self.read_projectile_records();
        records
            .into_iter()
            .map(|record| {
                let (objects, complete) = self.read_feature_objects(record.first_object, record.object_count);
                ProjectileUpdate { record, objects, complete }
            })
            .collect()
    }

    /// Objects in the feature object ring, with their node matrices, and
    /// whether all were still there.
    #[allow(clippy::type_complexity)]
    fn read_feature_objects(&self, first: u32, count: u32) -> (Vec<(FeatureObjectRecord, Vec<[f32; 13]>)>, bool) {
        // SAFETY: as in check(); entries are plain old data, checked by their
        // sequence numbers (and the matrix ring's count) after copying
        unsafe {
            let mut objects = Vec::new();
            let mut complete = true;
            for i in 0..count {
                let sequence = first.wrapping_add(i);
                let entry = std::ptr::read_volatile(&(*self.shared).feature_objects[sequence as usize % FEATURE_OBJECTS]);
                let matrices: Vec<[f32; 13]> = (0..entry.node_count)
                    .map(|n| {
                        std::ptr::read_volatile(
                            &(*self.shared).feature_matrices[entry.first_matrix.wrapping_add(n) as usize % FEATURE_MATRICES],
                        )
                    })
                    .collect();
                fence(Ordering::Acquire);
                let objects_now = std::ptr::read_volatile(&(*self.shared).feature_object_count);
                let matrices_now = std::ptr::read_volatile(&(*self.shared).feature_matrix_count);
                if entry.sequence != sequence
                    || objects_now.wrapping_sub(sequence) as usize > FEATURE_OBJECTS
                    || matrices_now.wrapping_sub(entry.first_matrix) as usize > FEATURE_MATRICES
                {
                    complete = false;
                }
                objects.push((entry, matrices));
            }
            (objects, complete)
        }
    }

    /// The periodic function tables the game built at startup, by function
    /// type, once a projectile has been recorded.
    pub fn periodic_tables(&self) -> Option<Box<[[u8; 1024]; 12]>> {
        // SAFETY: as in check()
        unsafe {
            if std::ptr::read_volatile(&(*self.shared).periodic_tables_valid) == 0 {
                return None;
            }
            fence(Ordering::Acquire);
            Some(Box::new(std::ptr::read_volatile(&(*self.shared).periodic_tables)))
        }
    }

    /// The game's random direction table (`random_vector_in_cone3d`), once
    /// a projectile has been recorded.
    pub fn random_directions(&self) -> Vec<[f32; 3]> {
        // SAFETY: as in check()
        unsafe {
            let count = (std::ptr::read_volatile(&(*self.shared).random_direction_count) as usize).min(RANDOM_DIRECTIONS);
            fence(Ordering::Acquire);
            (0..count).map(|i| std::ptr::read_volatile(&(*self.shared).random_directions[i])).collect()
        }
    }

    /// Whether the game's menus are up and take a mouse pointer (then the
    /// cursor is to be free, its place forwarded in [`Input::pointer`]).
    pub fn menus_want_pointer(&self) -> bool {
        // SAFETY: as in check()
        unsafe { std::ptr::read_volatile(&(*self.shared).menus_pointer) != 0 }
    }

    /// Whether the game shares overlays.
    pub fn overlay_enabled(&self) -> bool {
        // SAFETY: as in check()
        unsafe { std::ptr::read_volatile(&(*self.shared).overlay_enabled) != 0 }
    }

    /// The latest overlay, copied into `pixels`, if it is newer than the last
    /// one returned.
    pub fn read_overlay(&mut self, pixels: &mut Vec<u8>) -> Option<Overlay> {
        for _ in 0..4 {
            // SAFETY: as in read(); the pixels are copied as bytes and
            // validated by the sequence afterwards
            unsafe {
                let latest = std::ptr::read_volatile(&(*self.shared).latest_overlay) as usize % OVERLAYS;
                fence(Ordering::Acquire);
                let header = &(*self.shared).overlays[latest] as *const OverlayHeader;
                let sequence = std::ptr::read_volatile(&(*header).sequence);
                if sequence & 1 != 0 || sequence == 0 {
                    continue;
                }
                fence(Ordering::Acquire);
                if self.last_overlay == Some((latest as u32, sequence)) {
                    return None;
                }
                let overlay = Overlay {
                    frame_index: std::ptr::read_volatile(&(*header).frame_index),
                    width: std::ptr::read_volatile(&(*header).width),
                    height: std::ptr::read_volatile(&(*header).height),
                    multiply: std::ptr::read_volatile(&(*header).multiply) != 0,
                };
                let picture = overlay.width as usize * overlay.height as usize * 4;
                if picture == 0 || picture > OVERLAY_MAXIMUM_PIXELS * 4 {
                    return None;
                }
                let bytes = if overlay.multiply { picture * 2 } else { picture };
                pixels.resize(bytes, 0);
                std::ptr::copy_nonoverlapping((*self.shared).overlay_pixels[latest].as_ptr(), pixels.as_mut_ptr(), bytes);
                fence(Ordering::Acquire);
                if std::ptr::read_volatile(&(*header).sequence) != sequence {
                    continue;
                }
                self.last_overlay = Some((latest as u32, sequence));
                return Some(overlay);
            }
        }
        None
    }

    /// The next frame newer than the last one returned: the one before the
    /// latest, while it is still there, then the latest. (The overlay's
    /// picture is taken after its frame is shared, and the game has often
    /// shared the next frame by the time the viewer reads; the viewer shows
    /// the frame the picture was drawn for, so it must have read it.)
    pub fn read(&mut self) -> Option<Snapshot> {
        for _ in 0..8 {
            // SAFETY: as in check(); every field read is plain old data, read
            // volatile and validated by the sequence afterwards
            unsafe {
                let mut latest = std::ptr::read_volatile(&(*self.shared).latest_frame) as usize % FRAMES;
                fence(Ordering::Acquire);
                // (the one before, if it is newer than the last returned and
                // not being written)
                let older = (latest + FRAMES - 1) % FRAMES;
                let older_frame = &(*self.shared).frames[older] as *const Frame;
                let older_sequence = std::ptr::read_volatile(&(*older_frame).sequence);
                fence(Ordering::Acquire);
                let older_index = std::ptr::read_volatile(&(*older_frame).frame_index);
                let latest_index = std::ptr::read_volatile(&(*self.shared).frames[latest].frame_index);
                if older_sequence & 1 == 0
                    && (older_index.wrapping_sub(self.last_frame_index) as i32) > 0
                    && (latest_index.wrapping_sub(older_index) as i32) > 0
                {
                    latest = older;
                }
                let frame = &(*self.shared).frames[latest] as *const Frame;
                let sequence = std::ptr::read_volatile(&(*frame).sequence);
                if sequence & 1 != 0 {
                    continue;
                }
                fence(Ordering::Acquire);
                let frame_index = std::ptr::read_volatile(&(*frame).frame_index);
                if frame_index == self.last_frame_index {
                    return None;
                }
                let object_count = (std::ptr::read_volatile(&(*frame).object_count) as usize).min(MAXIMUM_OBJECTS);
                let light_count = (std::ptr::read_volatile(&(*frame).light_count) as usize).min(MAXIMUM_LIGHTS);
                let ray_test_count = (std::ptr::read_volatile(&(*frame).ray_test_count) as usize).min(RAY_TESTS);
                let node_count = (std::ptr::read_volatile(&(*frame).node_count) as usize).min(MAXIMUM_NODES);
                let name = std::ptr::read_volatile(&(*frame).scenario_name);
                let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
                let snapshot = Snapshot {
                    frame_index,
                    game_tick: std::ptr::read_volatile(&(*frame).game_tick),
                    structure_bsp_index: std::ptr::read_volatile(&(*frame).structure_bsp_index),
                    scenario_name: String::from_utf8_lossy(&name[..end]).into_owned(),
                    camera: (std::ptr::read_volatile(&(*frame).camera_valid) != 0)
                        .then(|| std::ptr::read_volatile(&(*frame).camera)),
                    fog: Some(std::ptr::read_volatile(&(*frame).fog)).filter(|fog| fog.valid != 0),
                    fog_screen: Some(std::ptr::read_volatile(&(*frame).fog_screen)).filter(|screen| screen.mode != 0),
                    decals: (0..(std::ptr::read_volatile(&(*frame).decal_count) as usize).min(DECALS))
                        .map(|i| std::ptr::read_volatile(&(*frame).decals[i]))
                        .collect(),
                    decal_vertices: (0..(std::ptr::read_volatile(&(*frame).decal_vertex_count) as usize).min(DECAL_VERTICES))
                        .map(|i| std::ptr::read_volatile(&(*frame).decal_vertices[i]))
                        .collect(),
                    sprite_batches: (0..(std::ptr::read_volatile(&(*frame).sprite_batch_count) as usize).min(SPRITE_BATCHES))
                        .map(|i| std::ptr::read_volatile(&(*frame).sprite_batches[i]))
                        .collect(),
                    sprite_vertices: (0..(std::ptr::read_volatile(&(*frame).sprite_vertex_count) as usize).min(SPRITE_VERTICES))
                        .map(|i| std::ptr::read_volatile(&(*frame).sprite_vertices[i]))
                        .collect(),
                    lights: (0..light_count).map(|i| std::ptr::read_volatile(&(*frame).lights[i])).collect(),
                    ray_tests: (0..ray_test_count).map(|i| std::ptr::read_volatile(&(*frame).ray_tests[i])).collect(),
                    point_physics_definition: std::ptr::read_volatile(&(*frame).point_physics_definition),
                    objects: (0..object_count).map(|i| std::ptr::read_volatile(&(*frame).objects[i])).collect(),
                    nodes: (0..node_count).map(|i| std::ptr::read_volatile(&(*frame).nodes[i])).collect(),
                    physics_vehicles: (0..(std::ptr::read_volatile(&(*frame).physics_vehicle_count) as usize).min(PHYSICS_VEHICLES))
                        .map(|i| std::ptr::read_volatile(&(*frame).physics_vehicles[i]))
                        .collect(),
                    vehicle_ray_tests: (0..(std::ptr::read_volatile(&(*frame).vehicle_ray_test_count) as usize).min(VEHICLE_RAY_TESTS))
                        .map(|i| std::ptr::read_volatile(&(*frame).vehicle_ray_tests[i]))
                        .collect(),
                };
                fence(Ordering::Acquire);
                if std::ptr::read_volatile(&(*frame).sequence) != sequence {
                    continue;
                }
                self.last_frame_index = frame_index;
                return Some(snapshot);
            }
        }
        None
    }
}

#[cfg(windows)]
mod windows {
    use super::Error;
    use std::ffi::c_void;

    const FILE_MAP_WRITE: u32 = 0x0002;
    const FILE_MAP_READ: u32 = 0x0004;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenFileMappingA(desired_access: u32, inherit_handle: i32, name: *const u8) -> *mut c_void;
        fn MapViewOfFile(mapping: *mut c_void, desired_access: u32, offset_high: u32, offset_low: u32, bytes: usize) -> *mut c_void;
        fn UnmapViewOfFile(base: *const c_void) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    /// A file mapping handle and its view, closed together.
    pub struct Handle {
        mapping: *mut c_void,
        view: *mut c_void,
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: both came from the calls in open() and are released once
            unsafe {
                UnmapViewOfFile(self.view);
                CloseHandle(self.mapping);
            }
        }
    }

    pub fn open(name: &[u8], size: usize) -> Result<(Handle, *mut c_void), Error> {
        // SAFETY: `name` is NUL-terminated; the results are checked
        unsafe {
            let mapping = OpenFileMappingA(FILE_MAP_READ | FILE_MAP_WRITE, 0, name.as_ptr());
            if mapping.is_null() {
                return Err(Error::NotRunning);
            }
            let view = MapViewOfFile(mapping, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, size);
            if view.is_null() {
                CloseHandle(mapping);
                return Err(Error::NotRunning);
            }
            Ok((Handle { mapping, view }, view))
        }
    }
}
