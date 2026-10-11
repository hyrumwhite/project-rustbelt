//! Particle updates (`particles.c`'s `particles_update`, each particle's:
//! its age, its death, its animation's frames and sequences, and its physics,
//! with `particle_update_frame_time`, `particle_next_frame`,
//! `particle_next_sequence`, `particle_update_physics` and `particle_die`).
//! They run once a frame, from the frame's time.
//!
//! What a particle does to the rest of the game goes through
//! [`ParticleHost::call`] (particles.c's `particle_call`, shared with the C):
//! deleting it, its effects (a collision's, its death's), the material effect
//! of its impact, whether the object it is attached to is still there, and
//! its point physics (the game's, or Rust's under `point_physics`).

// (the C's shape: its operand order, and PIN's comparisons, which `clamp`
// doesn't keep for NaN)
#![allow(clippy::manual_clamp, clippy::assign_op_pattern)]

use crate::cache::CacheFile;
use crate::physics::PointPhysicsDefinition;
use crate::{Result, layout};

const NONE: i32 = -1;

/// `MAXIMUM_PARTICLE_UNRENDERED_FRAME_COUNT`.
const MAXIMUM_PARTICLE_UNRENDERED_FRAME_COUNT: i32 = 15;

/// `enum particle_flags`.
const PARTICLE_ANIMATES_BACKWARDS: u16 = 1 << 0;
const PARTICLE_AT_REST: u16 = 1 << 1;
const PARTICLE_U_MIRROR: u16 = 1 << 2;
const PARTICLE_V_MIRROR: u16 = 1 << 3;
const PARTICLE_DONT_DRAW_FIRST_PERSON: u16 = 1 << 4;
const PARTICLE_DONT_DRAW_THIRD_PERSON: u16 = 1 << 5;
const PARTICLE_ATTACHED_TO_LOCAL_PLAYER: u16 = 1 << 6;

/// `PARTICLE_SHARED_LIFESPAN`.
const PARTICLE_SHARED_LIFESPAN: f32 = 0.7;

/// `enum particle_definition_flags`.
const DEFINITION_CAN_ANIMATE_BACKWARDS: u32 = 1 << 0;
const DEFINITION_ANIMATION_STOPS_AT_REST: u32 = 1 << 1;
const DEFINITION_ANIMATION_STARTS_ON_RANDOM_FRAME: u32 = 1 << 2;
const DEFINITION_TINT_FROM_DIFFUSE_TEXTURE: u32 = 1 << 6;
const DEFINITION_SELF_ILLUMINATED: u32 = 1 << 9;
const DEFINITION_RANDOM_HORIZONTAL_MIRRORING: u32 = 1 << 10;
const DEFINITION_RANDOM_VERTICAL_MIRRORING: u32 = 1 << 11;
const DEFINITION_ANIMATE_ONCE_PER_FRAME: u32 = 1 << 3;
const DEFINITION_DIES_AT_REST: u32 = 1 << 4;
const DEFINITION_DIES_ON_CONTACT_WITH_STRUCTURE: u32 = 1 << 5;
const DEFINITION_DIES_ON_CONTACT_WITH_WATER: u32 = 1 << 7;
const DEFINITION_DIES_ON_CONTACT_WITH_AIR: u32 = 1 << 8;

/// `enum particle_states`.
const STATE_NEXT_SEQUENCE_INITIAL: u8 = 0;
const STATE_NEXT_SEQUENCE_LOOPING: u8 = 1;
const STATE_STILL_LOOPING: u8 = 2;
const STATE_NEXT_SEQUENCE_FINAL: u8 = 3;

/// `point_physics_update`'s result flags.
const PHYSICS_IN_AIR: i32 = 1 << 0;
const PHYSICS_IN_WATER: i32 = 1 << 1;
const PHYSICS_COLLIDED_WITH_STRUCTURE: i32 = 1 << 2;
const PHYSICS_COLLIDED_WITH_WATER: i32 = 1 << 3;

/// `particle_collision_effect_scale_bounds`: {upper, lower}.
const COLLISION_EFFECT_SCALE_BOUNDS: [f32; 2] = [1.5, 0.5];

/// A particle's calls of the rest of the game (halo_sim.h's
/// `_particle_call_*`).
pub mod call {
    /// `datum_delete`.
    pub const DELETE: i32 = 1;
    /// `particle_effect_new`: `a` the group, `b` the effect or sound, `f[0]`
    /// the scale.
    pub const EFFECT: i32 = 2;
    /// Whether the object `a` is still there.
    pub const OBJECT_EXISTS: i32 = 3;
    /// `point_physics_update` on the particle (its location, position and
    /// velocity): `a` the point physics, `f[0]` the radius, `f[1]` the time;
    /// the result its flags, `out` the collision normal,
    /// `out_material_type` the material.
    pub const PHYSICS: i32 = 4;
    /// The impact's material effect if it is visible: `a` the material
    /// effects, `b` the material; `f` the normal and scale.
    pub const MATERIAL_EFFECT: i32 = 5;
    /// Making a particle: the point `f` in the node `b` of the object `a`
    /// (`c` 1 its first person weapon's, for the local player `d`), in `out`.
    pub const NODE_POINT: i32 = 6;
    /// `scenario_location_from_point` at `f` (from a location of zeros), in
    /// `out_location_*`.
    pub const LOCATION_FROM_POINT: i32 = 7;
    /// Whether the location in `a` (leaf), `b` (cluster), `c` (bonus) is
    /// potentially visible to the local players.
    pub const VISIBLE: i32 = 8;
    /// The particle's datum: the result its index (NONE for none).
    pub const NEW: i32 = 9;
    /// `light_particle` at `f`: the light and the diffuse colour in `out`.
    pub const LIGHT: i32 = 10;
}

/// halo_sim.h's `struct halo_sim_particle_call`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleCall {
    pub kind: i32,
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub d: i32,
    pub f: [f32; 4],
    pub result: i32,
    pub out: [f32; 6],
    pub out_material_type: i32,
    pub out_location_leaf: i32,
    pub out_location_cluster: i16,
    pub out_location_bonus: u16,
}

impl ParticleCall {
    pub fn new(kind: i32) -> Self {
        ParticleCall {
            kind,
            a: 0,
            b: 0,
            c: 0,
            d: 0,
            f: [0.0; 4],
            result: 0,
            out: [0.0; 6],
            out_material_type: 0,
            out_location_leaf: 0,
            out_location_cluster: 0,
            out_location_bonus: 0,
        }
    }
}

/// What a particle's update changes of it (`struct particle_datum`), and
/// the game's local random seed.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ParticleState {
    pub particle_index: u32,
    pub definition_index: u32,
    pub flags: u16,
    pub state: u8,
    pub local_player_index: u8,
    pub object_index: i32,
    pub node_index: i16,
    pub sequence_index: i16,
    pub frame_index: i16,
    pub pad: i16,
    pub last_rendered_frame_index: i32,
    pub age: f32,
    pub lifespan: f32,
    pub frame_time: f32,
    pub frame_span: f32,
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub location_bonus: u16,
    pub position: [f32; 3],
    pub direction: [f32; 3],
    pub translational_velocity: [f32; 3],
    pub rotation: f32,
    pub angular_velocity: f32,
    pub radius: f32,
    /// alpha, red, green, blue.
    pub color: [f32; 4],
    /// `get_global_local_random_seed_address`'s.
    pub local_random_seed: u32,
}

const _: () = assert!(std::mem::size_of::<ParticleState>() == 120);
const _: () = assert!(std::mem::size_of::<ParticleCall>() == 76);

/// What a frame's particle updates read of the game besides the particle.
pub struct ParticleInputs {
    /// `render.frame_index`.
    pub frame_index: i32,
    /// The frame drawn `MAXIMUM_PARTICLE_UNRENDERED_FRAME_COUNT` ticks ago
    /// (`particles_tick_frame_indices`' oldest).
    pub oldest_tick_frame_index: i32,
    /// `particles_update_ticks`: the ticks this frame ends.
    pub update_ticks: i32,
    /// `local_player_count()` (making a particle).
    pub local_player_count: i16,
}

/// The game's side of a particle's update.
pub trait ParticleHost {
    /// A call: the particle (and the seed) written to the game first and read
    /// back after.
    fn call(&self, state: &mut ParticleState, call: &mut ParticleCall);
}

/// An effect or sound a particle makes.
#[derive(Debug, Clone, Copy, Default)]
pub struct ParticleEffect {
    pub group: u32,
    pub index: i32,
}

/// `struct particle_definition`, as an update reads it.
#[derive(Debug, Clone, Default)]
pub struct ParticleDefinition {
    pub flags: u32,
    pub physics_index: i32,
    pub physics: Option<PointPhysicsDefinition>,
    pub collision_material_effects: i32,
    pub collision_effect: ParticleEffect,
    pub effect: ParticleEffect,
    pub radius_lower_bound: f32,
    pub radius_upper_bound: f32,
    pub frames_per_second_contact_deterioration: f32,
    pub life_span_lower_bound: f32,
    pub life_span_upper_bound: f32,
    pub frames_per_second_lower_bound: f32,
    pub frames_per_second_upper_bound: f32,
    pub first_sequence_index: i16,
    pub initial_sequence_count: i16,
    pub looping_sequence_count: i16,
    pub final_sequence_count: i16,
    /// Its bitmap's sequences' sprite counts.
    pub sequence_sprite_counts: Vec<i32>,
}

impl ParticleDefinition {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{bitmap_group as b, bitmap_group_sequence as s, particle_definition as p};
        let root = cache.tag_root(datum, crate::fourcc(b"part"))?;
        let reference = |rel: usize| -> Result<ParticleEffect> {
            let r = root.reference(rel)?;
            Ok(ParticleEffect { group: r.group, index: r.datum.map_or(NONE, |d| d as i32) })
        };
        let bitmap = reference(p::BITMAP)?;
        let mut sequence_sprite_counts = Vec::new();
        if bitmap.index != NONE {
            let group = cache.tag_root(bitmap.index as u32, crate::groups::BITMAP)?;
            for sequence in group.block(b::SEQUENCES, s::SIZEOF)?.iter() {
                sequence_sprite_counts.push(sequence.block(s::SPRITES, layout::bitmap_group_sprite::SIZEOF)?.count as i32);
            }
        }
        let physics_index = reference(p::PHYSICS)?.index;
        let physics =
            if physics_index == NONE { None } else { Some(PointPhysicsDefinition::load(cache, physics_index as u32)?) };
        Ok(ParticleDefinition {
            flags: root.u32(p::FLAGS)?,
            physics_index,
            physics,
            collision_material_effects: reference(p::COLLISION_MATERIAL_EFFECTS)?.index,
            collision_effect: reference(p::COLLISION_EFFECT)?,
            effect: reference(p::EFFECT)?,
            radius_lower_bound: root.f32(p::RADIUS_LOWER_BOUND)?,
            radius_upper_bound: root.f32(p::RADIUS_UPPER_BOUND)?,
            frames_per_second_contact_deterioration: root.f32(p::FRAMES_PER_SECOND_CONTACT_DETERIORATION)?,
            life_span_lower_bound: root.f32(p::LIFE_SPAN_LOWER_BOUND)?,
            life_span_upper_bound: root.f32(p::LIFE_SPAN_UPPER_BOUND)?,
            frames_per_second_lower_bound: root.f32(p::FRAMES_PER_SECOND_LOWER_BOUND)?,
            frames_per_second_upper_bound: root.f32(p::FRAMES_PER_SECOND_UPPER_BOUND)?,
            first_sequence_index: root.i16(p::FIRST_SEQUENCE_INDEX)?,
            initial_sequence_count: root.i16(p::INITIAL_SEQUENCE_COUNT)?,
            looping_sequence_count: root.i16(p::LOOPING_SEQUENCE_COUNT)?,
            final_sequence_count: root.i16(p::FINAL_SEQUENCE_COUNT)?,
            sequence_sprite_counts,
        })
    }
}

/// `local_random_range`: `seed_random_range` on the local seed.
fn local_random_range(seed: &mut u32, lower_bound: i16, upper_bound: i16) -> i16 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let random = (*seed >> 16) as u16 as u32;
    let span = (upper_bound as i32 - lower_bound as i32) as u32;
    lower_bound.wrapping_add((span.wrapping_mul(random) >> 16) as i16)
}

/// `magnitude_squared3d`.
fn magnitude_squared3(v: [f32; 3]) -> f32 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

/// A particle's update.
struct Update<'a, H: ParticleHost> {
    definition: &'a ParticleDefinition,
    inputs: &'a ParticleInputs,
    host: &'a H,
}

impl<H: ParticleHost> Update<'_, H> {
    fn call(&self, state: &mut ParticleState, call: &mut ParticleCall) {
        self.host.call(state, call);
    }

    fn delete(&self, state: &mut ParticleState) {
        self.call(state, &mut ParticleCall::new(call::DELETE));
    }

    /// `particle_effect_new`.
    fn effect(&self, state: &mut ParticleState, effect: ParticleEffect, scale: f32) {
        let mut call = ParticleCall::new(call::EFFECT);
        call.a = effect.group as i32;
        call.b = effect.index;
        call.f[0] = scale;
        self.call(state, &mut call);
    }

    fn object_exists(&self, state: &mut ParticleState, object_index: i32) -> bool {
        let mut call = ParticleCall::new(call::OBJECT_EXISTS);
        call.a = object_index;
        self.call(state, &mut call);
        call.result != 0
    }

    /// `particle_get_radius`.
    fn radius(&self, state: &ParticleState) -> f32 {
        let definition = self.definition;
        ((definition.radius_upper_bound - definition.radius_lower_bound) * (state.age / state.lifespan)
            + definition.radius_lower_bound)
            * state.radius
    }

    /// `particle_die`.
    fn die(&self, state: &mut ParticleState) {
        if self.definition.effect.index != NONE {
            self.effect(state, self.definition.effect, 0.0);
        }
        self.delete(state);
    }

    /// `particle_next_sequence`: false if the particle died.
    fn next_sequence(&self, state: &mut ParticleState) -> bool {
        let definition = self.definition;
        state.sequence_index = NONE as i16;

        if state.state == STATE_NEXT_SEQUENCE_INITIAL {
            if definition.initial_sequence_count > 0 {
                state.sequence_index = definition.first_sequence_index.wrapping_add(local_random_range(
                    &mut state.local_random_seed,
                    0,
                    definition.initial_sequence_count,
                ));
            }
            state.state = state.state.wrapping_add(1);
        }

        if (state.sequence_index == NONE as i16 && state.state == STATE_NEXT_SEQUENCE_LOOPING)
            || state.state == STATE_STILL_LOOPING
        {
            if state.state == STATE_NEXT_SEQUENCE_LOOPING {
                state.state = STATE_STILL_LOOPING;
            }
            if state.age < state.lifespan && definition.looping_sequence_count > 0 {
                state.sequence_index = definition
                    .initial_sequence_count
                    .wrapping_add(definition.first_sequence_index)
                    .wrapping_add(local_random_range(&mut state.local_random_seed, 0, definition.looping_sequence_count));
            } else {
                state.state = state.state.wrapping_add(1);
            }
        }

        if state.sequence_index == NONE as i16 && state.state == STATE_NEXT_SEQUENCE_FINAL {
            if definition.final_sequence_count > 0 {
                state.sequence_index = definition
                    .looping_sequence_count
                    .wrapping_add(definition.initial_sequence_count)
                    .wrapping_add(definition.first_sequence_index)
                    .wrapping_add(local_random_range(&mut state.local_random_seed, 0, definition.final_sequence_count));
            }
            state.state = state.state.wrapping_add(1);
        }

        let sequence_count = definition.sequence_sprite_counts.len() as i32;
        if state.sequence_index == NONE as i16 || sequence_count == 0 {
            self.die(state);
            return false;
        }

        // PIN(sequence_index, 0, count - 1)
        let index = state.sequence_index as i32;
        state.sequence_index = (if index < 0 {
            0
        } else if index > sequence_count - 1 {
            sequence_count - 1
        } else {
            index
        }) as i16;
        true
    }

    fn sprite_count(&self, state: &ParticleState) -> i32 {
        self.definition.sequence_sprite_counts.get(state.sequence_index as usize).copied().unwrap_or(0)
    }

    /// `particle_next_frame`: false if the particle died.
    fn next_frame(&self, state: &mut ParticleState) -> bool {
        let mut result = true;
        state.frame_time = 0.0;
        if state.flags & PARTICLE_ANIMATES_BACKWARDS != 0 {
            if state.frame_index > 0 {
                state.frame_index -= 1;
            } else {
                result = self.next_sequence(state);
                if result {
                    state.frame_index = (self.sprite_count(state) - 1) as i16;
                }
            }
        } else if state.frame_index as i32 + 1 < self.sprite_count(state) {
            state.frame_index += 1;
        } else {
            result = self.next_sequence(state);
            state.frame_index = 0;
        }
        result
    }

    /// `particle_update_frame_time`: false if the particle died.
    fn update_frame_time(&self, state: &mut ParticleState, mut dt: f32) -> bool {
        let definition = self.definition;
        let mut result = true;
        if definition.flags & DEFINITION_ANIMATION_STOPS_AT_REST == 0 || state.flags & PARTICLE_AT_REST == 0 {
            if definition.flags & DEFINITION_ANIMATE_ONCE_PER_FRAME != 0 {
                if dt != 0.0 && self.inputs.update_ticks > 0 {
                    return self.next_frame(state);
                }
            } else {
                if state.frame_time == -1.0 {
                    result = self.next_frame(state);
                    state.frame_time = 0.0;
                }
                while dt > 0.0 && result {
                    let frame_time_remaining = state.frame_span - state.frame_time;
                    if frame_time_remaining <= dt {
                        result = self.next_frame(state);
                        dt -= frame_time_remaining;
                    } else {
                        state.frame_time = dt + state.frame_time;
                        break;
                    }
                }
            }
        }
        result
    }

    /// `particle_update_physics`: false if the particle is gone.
    fn update_physics(&self, state: &mut ParticleState, dt: f32) -> bool {
        let definition = self.definition;
        if state.flags & PARTICLE_AT_REST == 0 {
            let mut settled = false;
            if state.object_index == NONE {
                let mut call = ParticleCall::new(call::PHYSICS);
                call.a = definition.physics_index;
                call.f[0] = self.radius(state);
                call.f[1] = dt;
                self.call(state, &mut call);
                let collision_flags = call.result;
                let collision_normal = [call.out[0], call.out[1], call.out[2]];
                let collision_material_type = call.out_material_type;

                if collision_flags & PHYSICS_COLLIDED_WITH_STRUCTURE != 0 {
                    let collision_effect_index = definition.collision_effect.index;
                    // (the game tests the material effects' index for being
                    // non-zero here, NONE included)
                    if collision_effect_index != NONE || definition.collision_material_effects != 0 {
                        let mut scale = (magnitude_squared3(state.translational_velocity).sqrt()
                            - COLLISION_EFFECT_SCALE_BOUNDS[1])
                            / (COLLISION_EFFECT_SCALE_BOUNDS[0] - COLLISION_EFFECT_SCALE_BOUNDS[1]);
                        if scale < 0.0 {
                            scale = 0.0;
                        } else if scale > 1.0 {
                            scale = 1.0;
                        }
                        if collision_effect_index != NONE {
                            self.effect(state, definition.collision_effect, scale);
                        }
                        if definition.collision_material_effects != NONE {
                            let mut call = ParticleCall::new(call::MATERIAL_EFFECT);
                            call.a = definition.collision_material_effects;
                            call.b = collision_material_type;
                            call.f[..3].copy_from_slice(&collision_normal);
                            call.f[3] = scale;
                            self.call(state, &mut call);
                        }
                    }
                    if definition.flags & DEFINITION_DIES_ON_CONTACT_WITH_STRUCTURE != 0 {
                        if definition.collision_effect.index == NONE {
                            self.die(state);
                        } else {
                            self.delete(state);
                        }
                        return false;
                    }
                }

                if (collision_flags & PHYSICS_IN_AIR != 0 && definition.flags & DEFINITION_DIES_ON_CONTACT_WITH_AIR != 0)
                    || (collision_flags & PHYSICS_IN_WATER != 0
                        && definition.flags & DEFINITION_DIES_ON_CONTACT_WITH_WATER != 0)
                {
                    self.die(state);
                    return false;
                }

                if collision_flags & (PHYSICS_COLLIDED_WITH_STRUCTURE | PHYSICS_COLLIDED_WITH_WATER) != 0 {
                    if collision_normal[2] > 0.8 {
                        settled = true;
                    }
                    state.frame_span = definition.frames_per_second_contact_deterioration + state.frame_span;
                }
            } else {
                if state.flags & PARTICLE_ATTACHED_TO_LOCAL_PLAYER == 0 && !self.object_exists(state, state.object_index) {
                    self.delete(state);
                    return false;
                }
                let radius = self.radius(state);
                let (air_friction, mass_over_radius_cubed) =
                    definition.physics.as_ref().map_or((0.0, 0.0), |p| (p.air_friction, p.runtime_mass_over_radius_cubed));
                let friction = air_friction * radius * radius;
                let mass = mass_over_radius_cubed * radius * radius * radius;
                let scale = if mass == 0.0 {
                    if friction == 0.0 { 1.0 } else { 0.0 }
                } else {
                    let scale = 1.0 - friction / mass * dt;
                    if scale < 0.0 {
                        0.0
                    } else if scale > 1.0 {
                        1.0
                    } else {
                        scale
                    }
                };
                let v = &mut state.translational_velocity;
                *v = [scale * v[0], scale * v[1], scale * v[2]];
                for c in 0..3 {
                    state.position[c] = state.translational_velocity[c] * dt + state.position[c];
                }
                settled = true;
            }

            if magnitude_squared3(state.translational_velocity) < 0.0625 {
                if settled {
                    if definition.flags & DEFINITION_DIES_AT_REST != 0 {
                        self.die(state);
                        return false;
                    }
                    state.flags |= PARTICLE_AT_REST;
                }
            } else {
                state.direction = state.translational_velocity;
            }
            state.rotation = dt * state.angular_velocity + state.rotation;
        } else if state.object_index != NONE && !self.object_exists(state, state.object_index) {
            self.delete(state);
            return false;
        }
        true
    }

    /// The loop body of `particles_update`.
    fn update(&self, state: &mut ParticleState, dt: f32) {
        let was_new = state.age == 0.0;
        if self.inputs.frame_index.wrapping_sub(state.last_rendered_frame_index) > MAXIMUM_PARTICLE_UNRENDERED_FRAME_COUNT
            && state.last_rendered_frame_index < self.inputs.oldest_tick_frame_index
        {
            self.delete(state);
        } else {
            state.age = dt + state.age;
            if state.age >= state.lifespan && !was_new && self.definition.final_sequence_count == 0 {
                self.die(state);
            } else if self.update_frame_time(state, dt) {
                self.update_physics(state, dt);
            }
        }
    }
}

/// A particle's update over the frame's `dt` seconds (the loop body of
/// `particles_update`), from `state`, where it is left. Whether the particle
/// is still there is the host's to say.
pub fn particle_update(
    definition: &ParticleDefinition,
    inputs: &ParticleInputs,
    host: &impl ParticleHost,
    state: &mut ParticleState,
    dt: f32,
) {
    Update { definition, inputs, host }.update(state, dt);
}

/// `struct new_particle_data`: what `particle_new` is given.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct NewParticleData {
    pub definition_index: i32,
    pub object_index: i32,
    pub node_index: i16,
    pub local_player_index: i16,
    pub attached_to_local_player: u8,
    pub dont_draw_first_person: u8,
    pub dont_draw_third_person: u8,
    pub pad: u8,
    pub position: [f32; 3],
    pub direction: [f32; 3],
    pub velocity: [f32; 3],
    pub initial_impulse: [f32; 3],
    pub rotation: f32,
    pub angular_velocity: f32,
    pub radius: f32,
    /// alpha, red, green, blue.
    pub color: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<NewParticleData>() == 92);

/// `local_random`: `seed_random` on the local seed.
fn local_random(seed: &mut u32) -> u16 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*seed >> 16) as u16
}

/// `real_local_random_range`.
fn real_local_random_range(seed: &mut u32, lower_bound: f32, upper_bound: f32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let random = (*seed >> 16) as f32 / 65535.0;
    lower_bound + (upper_bound - lower_bound) * random
}

impl<H: ParticleHost> Update<'_, H> {
    /// `particle_new`, from `state` (the seed; its index NONE): the new
    /// particle in `state`, or NONE.
    fn new_particle(&self, state: &mut ParticleState, data: &NewParticleData) -> i32 {
        let definition = self.definition;
        let position = if data.object_index != NONE {
            let mut call = ParticleCall::new(call::NODE_POINT);
            call.a = data.object_index;
            call.b = data.node_index as i32;
            call.c = data.attached_to_local_player as i32;
            call.d = data.local_player_index as i32;
            call.f[..3].copy_from_slice(&data.position);
            self.call(state, &mut call);
            [call.out[0], call.out[1], call.out[2]]
        } else {
            data.position
        };
        let mut call = ParticleCall::new(call::LOCATION_FROM_POINT);
        call.f[..3].copy_from_slice(&position);
        self.call(state, &mut call);
        let location = (call.out_location_leaf, call.out_location_cluster, call.out_location_bonus);
        if location.0 == NONE {
            return NONE;
        }
        let mut call = ParticleCall::new(call::VISIBLE);
        call.a = location.0;
        call.b = location.1 as i32;
        call.c = location.2 as i32;
        self.call(state, &mut call);
        if call.result == 0 {
            return NONE;
        }
        let mut call = ParticleCall::new(call::NEW);
        self.call(state, &mut call);
        let particle_index = call.result;
        if particle_index == NONE {
            return NONE;
        }

        // (the datum is new: zeros)
        let seed = state.local_random_seed;
        *state = ParticleState { particle_index: particle_index as u32, local_random_seed: seed, ..Default::default() };
        let mut flags: u16 = 0;
        if definition.flags & DEFINITION_CAN_ANIMATE_BACKWARDS != 0 {
            flags |= local_random(&mut state.local_random_seed) & PARTICLE_ANIMATES_BACKWARDS;
        }
        if definition.flags & DEFINITION_RANDOM_HORIZONTAL_MIRRORING != 0 {
            flags |= local_random(&mut state.local_random_seed) & PARTICLE_U_MIRROR;
        }
        if definition.flags & DEFINITION_RANDOM_VERTICAL_MIRRORING != 0 {
            flags |= local_random(&mut state.local_random_seed) & PARTICLE_V_MIRROR;
        }
        for (set, bit) in [
            (data.dont_draw_first_person, PARTICLE_DONT_DRAW_FIRST_PERSON),
            (data.dont_draw_third_person, PARTICLE_DONT_DRAW_THIRD_PERSON),
            (data.attached_to_local_player, PARTICLE_ATTACHED_TO_LOCAL_PLAYER),
        ] {
            if set != 0 {
                flags |= bit;
            } else {
                flags &= !bit;
            }
        }
        state.flags = flags;
        state.definition_index = data.definition_index as u32;
        state.local_player_index = data.local_player_index as u8;
        state.object_index = data.object_index;
        state.node_index = data.node_index;
        state.state = STATE_NEXT_SEQUENCE_INITIAL;
        state.last_rendered_frame_index = self.inputs.frame_index;

        state.lifespan = real_local_random_range(
            &mut state.local_random_seed,
            definition.life_span_lower_bound,
            definition.life_span_upper_bound,
        );
        if state.lifespan > PARTICLE_SHARED_LIFESPAN {
            state.lifespan = (state.lifespan - PARTICLE_SHARED_LIFESPAN) / self.inputs.local_player_count as f32
                + PARTICLE_SHARED_LIFESPAN;
        }
        state.frame_span = if definition.frames_per_second_upper_bound != 0.0 {
            1.0 / real_local_random_range(
                &mut state.local_random_seed,
                definition.frames_per_second_lower_bound,
                definition.frames_per_second_upper_bound,
            )
        } else {
            f32::MAX
        };
        state.frame_time = -1.0;

        (state.location_leaf, state.location_cluster, state.location_bonus) = location;
        state.position = data.position;
        state.direction = data.direction;
        state.rotation = data.rotation;
        state.radius = data.radius;
        state.color = data.color;
        state.translational_velocity = data.velocity;
        if state.object_index == NONE {
            let radius = self.radius(state);
            let mass =
                definition.physics.as_ref().map_or(0.0, |p| p.runtime_mass_over_radius_cubed) * radius * radius * radius;
            for c in 0..3 {
                state.translational_velocity[c] = data.initial_impulse[c] * mass + state.translational_velocity[c];
            }
        }
        state.angular_velocity = data.angular_velocity;

        if definition.flags & DEFINITION_SELF_ILLUMINATED == 0 || definition.flags & DEFINITION_TINT_FROM_DIFFUSE_TEXTURE != 0
        {
            let mut call = ParticleCall::new(call::LIGHT);
            call.f[..3].copy_from_slice(&position);
            self.call(state, &mut call);
            if definition.flags & DEFINITION_SELF_ILLUMINATED == 0 {
                for c in 0..3 {
                    state.color[1 + c] *= call.out[c];
                }
            }
            if definition.flags & DEFINITION_TINT_FROM_DIFFUSE_TEXTURE != 0 {
                for c in 0..3 {
                    state.color[1 + c] *= call.out[3 + c];
                }
            }
        }

        if self.next_sequence(state) {
            let sprite_count = self.sprite_count(state);
            if definition.flags & DEFINITION_ANIMATION_STARTS_ON_RANDOM_FRAME != 0 {
                let start: i16 = if state.flags & PARTICLE_ANIMATES_BACKWARDS != 0 { 1 } else { NONE as i16 };
                state.frame_index =
                    start.wrapping_add(local_random_range(&mut state.local_random_seed, 0, sprite_count as i16));
            } else if state.flags & PARTICLE_ANIMATES_BACKWARDS != 0 {
                state.frame_index = sprite_count as i16;
            } else {
                state.frame_index = NONE as i16;
            }
        }
        particle_index
    }
}

/// `particle_new`: the new particle (NONE for none), from `state` (the
/// seed; its index NONE), where it is left. `definition` is the particle's
/// (None when `data` names none).
pub fn particle_new(
    definition: Option<&ParticleDefinition>,
    inputs: &ParticleInputs,
    host: &impl ParticleHost,
    state: &mut ParticleState,
    data: &NewParticleData,
) -> i32 {
    let Some(definition) = definition else { return NONE };
    if data.definition_index == NONE {
        return NONE;
    }
    Update { definition, inputs, host }.new_particle(state, data)
}
