//! Halo's biped movement physics (ROADMAP phase 7, objects and physics):
//! `biped_update_physics` in OpenCE's `source/units/bipeds.c`, ported as it
//! is, in the same single-precision arithmetic.
//!
//! Each tick a biped's update turns its throttle, animation and state into
//! a [`BipedPhysics`] (where it is, how it wants to move, how fast it may
//! accelerate, the ground it stands on); this works out its new velocity
//! (walking along the ground, flying, or falling), moves its pill through
//! the structure, sticking to climbable ledges, and finds what it now
//! stands on.
//!
//! Collisions with objects (other bipeds, vehicles, scenery, elevators)
//! are not ported yet: only the structure is collided with.

use crate::collision::{CollisionBsp, surface_flags};
use crate::features::{
    self, CollisionPlane, cross3, dot3, magnitude3, magnitude_squared3, normalize3, plane_from_designator, scale3, sub3,
    valid_surface,
};

/// `_biped_physics_in` flags.
pub mod in_flags {
    pub const AIRBORNE: u16 = 1 << 0;
    pub const SLIPPING: u16 = 1 << 1;
    pub const CROUCHED: u16 = 1 << 2;
    pub const TRYING_TO_STAND: u16 = 1 << 3;
    pub const FLYING: u16 = 1 << 4;
    pub const ABSOLUTE_MOVEMENT: u16 = 1 << 5;
    pub const NO_COLLISION: u16 = 1 << 6;
    pub const DEAD: u16 = 1 << 7;
    pub const PASS_THROUGH_BIPEDS: u16 = 1 << 8;
    pub const CLIMB_ANYTHING: u16 = 1 << 9;
    pub const PASS_THROUGH_PLAYERS: u16 = 1 << 10;
}

/// `_biped_physics_out` flags.
pub mod out_flags {
    pub const AIRBORNE: u16 = 1 << 0;
    pub const SLIPPING: u16 = 1 << 1;
    pub const CANNOT_STAND: u16 = 1 << 2;
    pub const SPLATTER: u16 = 1 << 3;
    pub const VOLATILE_COLLISION: u16 = 1 << 4;
}

/// `struct biped_physics`, laid out as the game's (the bridge shares it).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BipedPhysics {
    pub biped_index: i32,
    pub in_flags: u16,
    pub pad6: u16,
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub aiming: [f32; 3],
    pub velocity: [f32; 3],
    pub crouch_velocity: f32,
    pub movement_desired: [f32; 3],
    pub movement_penalty: f32,
    pub acceleration_maximum: f32,
    pub airborne_acceleration_maximum: f32,
    pub height: f32,
    pub width: f32,
    pub ground_tangential_velocity_max: f32,
    pub ground_tangential_angle: f32,
    pub minimum_normal_k: f32,
    pub downhill_k0: f32,
    pub downhill_k1: f32,
    pub downhill_velocity_scale: f32,
    pub uphill_k0: f32,
    pub uphill_k1: f32,
    pub uphill_velocity_scale: f32,
    pub ground_plane: [f32; 4],
    pub existing_support_surface_index: i32,
    pub gravity: f32,
    pub bumped_object_index: i32,
    pub elevator_object_index: i32,
    pub out_flags: u16,
    pub pad_a2: u16,
    pub support_surface_index: i32,
    pub stick_surface_index: i32,
    pub new_position: [f32; 3],
    pub new_velocity: [f32; 3],
    pub landing_velocity: f32,
    pub collision_velocity: f32,
}

const _: () = assert!(std::mem::size_of::<BipedPhysics>() == 0xcc);

/// What the crouching player's stand test needs of the biped's definition
/// (done only for a player's biped whose pill stands on its origin).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StandTest {
    pub collision_height_standing: f32,
    pub collision_radius: f32,
}

const GLOBAL_GRAVITY: f32 = crate::physics::GLOBAL_GRAVITY;
const EPSILON: f32 = 0.0001;
const NONE: i32 = -1;
const UP: [f32; 3] = [0.0, 0.0, 1.0];
const FORWARD: [f32; 3] = [1.0, 0.0, 0.0];
/// `depths_of_hell`: the ground plane of a biped standing on nothing.
const DEPTHS_OF_HELL: [f32; 4] = [0.0, 0.0, 1.0, -256.0];
const MAXIMUM_COLLISIONS: usize = 16;
const TICKS_PER_SECOND: f32 = 30.0;

/// `collision_test_flags` the biped movement uses.
pub const COLLISION_FLAGS_LIVING: u32 = 0x20_c3a0;
pub const COLLISION_FLAGS_PASSTHROUGH_LIVING: u32 = 0xc2a0;
pub const COLLISION_FLAGS_DEAD: u32 = 0xc0a0;
const SKIP_PLAYER_BIPEDS: u32 = 1 << 23;

/// `biped_build_flying_axes`: left and up for a flying biped's forward.
fn flying_axes(forward: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let mut up = UP;
    let mut left = cross3(up, forward);
    if normalize3(&mut left) == 0.0 {
        up = FORWARD;
        left = cross3(up, forward);
        normalize3(&mut left);
    }
    up = cross3(forward, left);
    normalize3(&mut up);
    (left, up)
}

/// `biped_update_physics`, against the structure only. `stand_test` is
/// what the stand test needs, for a player's biped it applies to.
pub fn biped_update_physics(bsp: &CollisionBsp, physics: &mut BipedPhysics, stand_test: Option<StandTest>) {
    let mut facing = [0.0f32; 2];
    let climbing = physics.in_flags & in_flags::CLIMB_ANYTHING != 0;
    let in_flags = physics.in_flags;
    let md = physics.movement_desired;
    let f = physics.forward;
    physics.out_flags = 0;

    if in_flags & in_flags::FLYING != 0 {
        let (left, up) = flying_axes(f);
        let mut desired = [
            f[0] * md[0] + left[0] * md[1] + up[0] * md[2],
            f[1] * md[0] + left[1] * md[1] + up[1] * md[2],
            f[2] * md[0] + left[2] * md[1] + up[2] * md[2],
        ];
        desired = scale3(desired, 1.0 - physics.movement_penalty);
        let acceleration = sub3(desired, physics.velocity);
        let mut direction = acceleration;
        let change = if normalize3(&mut direction) > physics.acceleration_maximum {
            scale3(direction, physics.acceleration_maximum)
        } else {
            acceleration
        };
        let v = physics.velocity;
        physics.new_velocity = [change[0] + v[0], change[1] + v[1], change[2] + v[2]];
        physics.out_flags |= out_flags::AIRBORNE;
        physics.out_flags &= !out_flags::SLIPPING;
    } else if in_flags & in_flags::ABSOLUTE_MOVEMENT != 0 {
        physics.new_velocity[2] = md[2];
        physics.new_velocity[0] = md[0] * f[0] - f[1] * md[1];
        physics.new_velocity[1] = md[1] * f[0] + md[0] * f[1];
        facing = [physics.new_velocity[0], physics.new_velocity[1]];
    } else if in_flags & in_flags::AIRBORNE != 0 {
        let scale = 1.0 - physics.movement_penalty;
        let desired = [scale * (md[0] * f[0] - f[1] * md[1]), scale * (md[1] * f[0] + md[0] * f[1])];
        let acceleration = [desired[0] - physics.velocity[0], desired[1] - physics.velocity[1]];
        let mut direction = acceleration;
        // (normalize2d)
        let magnitude = (direction[0] * direction[0] + direction[1] * direction[1]).sqrt();
        let magnitude = if !(EPSILON > magnitude.abs()) {
            let inverse = 1.0 / magnitude;
            direction = [inverse * direction[0], inverse * direction[1]];
            magnitude
        } else {
            0.0
        };
        let change = if magnitude > physics.airborne_acceleration_maximum {
            let c = physics.airborne_acceleration_maximum;
            [c * direction[0], c * direction[1]]
        } else {
            acceleration
        };
        physics.new_velocity[0] = change[0] + physics.velocity[0];
        physics.new_velocity[1] = change[1] + physics.velocity[1];
        physics.new_velocity[2] = physics.velocity[2] - GLOBAL_GRAVITY;
        physics.out_flags = if in_flags & in_flags::SLIPPING != 0 { out_flags::SLIPPING } else { 0 };
    } else {
        let mut speed = magnitude3(md);
        let n = [physics.ground_plane[0], physics.ground_plane[1], physics.ground_plane[2]];
        let mut move_direction;
        if in_flags & in_flags::CLIMB_ANYTHING != 0 {
            let mut forward = physics.aiming;
            let mut left = cross3(n, forward);
            if normalize3(&mut left) == 0.0 {
                left = cross3(n, UP);
                if normalize3(&mut left) == 0.0 {
                    left = cross3(n, FORWARD);
                    normalize3(&mut left);
                }
            }
            forward = cross3(left, n);
            normalize3(&mut forward);
            move_direction = [
                forward[0] * md[0] + left[0] * md[1],
                forward[1] * md[0] + left[1] * md[1],
                forward[2] * md[0] + left[2] * md[1] + md[2],
            ];
            normalize3(&mut move_direction);
        } else if n[2] > EPSILON {
            facing = [md[0] * f[0] - f[1] * md[1], md[1] * f[0] + md[0] * f[1]];
            move_direction = [facing[0], facing[1], md[2] - (facing[1] * n[1] + facing[0] * n[0]) / n[2]];
            normalize3(&mut move_direction);
        } else {
            let mut forward = physics.aiming;
            let mut left = cross3(UP, physics.aiming);
            normalize3(&mut left);
            let t = -dot3(forward, n);
            forward = [n[0] * t + forward[0], n[1] * t + forward[1], n[2] * t + forward[2]];
            let t = -dot3(left, n);
            left = [n[0] * t + left[0], n[1] * t + left[1], n[2] * t + left[2]];
            facing = [md[0] * f[0] - f[1] * md[1], md[1] * f[0] + md[0] * f[1]];
            move_direction = [
                forward[0] * md[0] + left[0] * md[1],
                forward[1] * md[0] + left[1] * md[1],
                forward[2] * md[0] + left[2] * md[1] + md[2],
            ];
            if !climbing {
                move_direction[2] *= 5.0;
            }
            normalize3(&mut move_direction);
        }

        if in_flags & in_flags::CLIMB_ANYTHING == 0 {
            let k = move_direction[2];
            if k <= physics.downhill_k1 {
                speed *= physics.downhill_velocity_scale;
            } else if k < physics.downhill_k0 {
                speed *= (k - physics.downhill_k0) * (physics.downhill_velocity_scale - 1.0) / (physics.downhill_k1 - physics.downhill_k0) + 1.0;
            } else if k >= physics.uphill_k1 {
                speed *= physics.uphill_velocity_scale;
            } else if k > physics.uphill_k0 {
                speed *= (k - physics.uphill_k0) * (physics.uphill_velocity_scale - 1.0) / (physics.uphill_k1 - physics.uphill_k0) + 1.0;
            }
        }

        let desired = scale3(move_direction, speed * (1.0 - physics.movement_penalty));
        let acceleration = sub3(desired, physics.velocity);
        let mut direction = acceleration;
        let mut slipping = false;
        let mut change = if normalize3(&mut direction) > physics.acceleration_maximum {
            if in_flags & in_flags::CLIMB_ANYTHING == 0 {
                slipping = in_flags & in_flags::SLIPPING != 0;
            }
            scale3(direction, physics.acceleration_maximum)
        } else {
            acceleration
        };
        change[0] -= n[0] * (1.0 / 128.0);
        change[1] -= n[1] * (1.0 / 128.0);
        change[2] -= n[2] * (1.0 / 128.0);
        physics.out_flags = if slipping { out_flags::SLIPPING } else { 0 };
        let v = physics.velocity;
        physics.new_velocity = [change[0] + v[0], change[1] + v[1], change[2] + v[2]];
        if physics.out_flags & out_flags::SLIPPING != 0 {
            physics.new_velocity[2] -= GLOBAL_GRAVITY;
        }
    }

    let collision_flags = if physics.in_flags & in_flags::NO_COLLISION != 0 {
        0
    } else if physics.in_flags & in_flags::DEAD != 0 {
        COLLISION_FLAGS_DEAD
    } else {
        let mut flags = if physics.in_flags & in_flags::PASS_THROUGH_BIPEDS != 0 {
            COLLISION_FLAGS_PASSTHROUGH_LIVING
        } else {
            COLLISION_FLAGS_LIVING
        };
        if physics.in_flags & in_flags::PASS_THROUGH_PLAYERS != 0 {
            flags |= SKIP_PLAYER_BIPEDS;
        }
        flags
    };

    let position = physics.position;
    let mut velocity = physics.new_velocity;
    velocity[2] += physics.crouch_velocity;
    let (mut clipped_position, mut clipped_velocity, mut collisions) =
        features::move_pill(bsp, collision_flags, position, velocity, physics.height, physics.width, MAXIMUM_COLLISIONS);

    if collisions.len() >= MAXIMUM_COLLISIONS {
        physics.out_flags |= out_flags::SPLATTER;
    } else {
        physics.out_flags &= !out_flags::SPLATTER;
    }
    physics.stick_surface_index = NONE;
    if collisions.is_empty() && physics.existing_support_surface_index != NONE {
        stick_to_ledge(bsp, physics, &mut clipped_position, &mut clipped_velocity, &mut collisions);
    }

    let magnitude_squared = facing[1] * facing[1] + facing[0] * facing[0];
    if magnitude_squared > EPSILON * EPSILON {
        // (1.0 / square_root(...): a double division)
        let inverse = (1.0f64 / magnitude_squared.sqrt() as f64) as f32;
        facing = [facing[0] * inverse, facing[1] * inverse];
    }

    // the plane it stands on
    let mut best: Option<usize> = None;
    let mut best_walkable = false;
    let mut best_stuck = false;
    let mut best_normal_k = -f32::MAX;
    let mut best_velocity_dot = -f32::MAX;
    if physics.in_flags & in_flags::FLYING == 0 {
        for (index, collision) in collisions.iter().enumerate() {
            let walkable = physics.in_flags & in_flags::DEAD == 0 && (climbing || collision.source.flags & surface_flags::CLIMBABLE != 0);
            let stuck = physics.stick_surface_index != NONE
                && best.is_some_and(|b| collisions[b].source.surface == physics.stick_surface_index);
            let n = [collision.plane[0], collision.plane[1], collision.plane[2]];
            let velocity_dot = -dot3(n, physics.new_velocity);
            let better = if walkable {
                if !climbing && n[0] * facing[0] + n[1] * facing[1] > 0.5 {
                    false
                } else if !best_walkable || stuck {
                    true
                } else if best_stuck {
                    false
                } else {
                    velocity_dot > best_velocity_dot
                }
            } else {
                !best_walkable && n[2] > best_normal_k
            };
            if better {
                best_velocity_dot = velocity_dot;
                best_walkable = walkable;
                best = Some(index);
                best_stuck = stuck;
                best_normal_k = n[2];
            }
            // (and objects other than scenery: not ported)
            if physics.out_flags & out_flags::VOLATILE_COLLISION == 0 && collision.source.flags & surface_flags::BREAKABLE != 0 {
                physics.out_flags |= out_flags::VOLATILE_COLLISION;
            }
        }
    }

    let supported_by = best.and_then(|b| {
        let collision = collisions[b];
        let plane = collision.plane;
        let n = [plane[0], plane[1], plane[2]];
        let velocity_dot = -dot3(n, velocity);
        let mut supported = true;
        if !best_walkable && !best_stuck {
            if best_normal_k < physics.minimum_normal_k {
                supported = false;
            } else if physics.in_flags & in_flags::AIRBORNE != 0 && physics.ground_tangential_velocity_max < f32::MAX {
                let maximum = physics.ground_tangential_velocity_max;
                let tangential = [n[0] * velocity_dot + velocity[0], n[1] * velocity_dot + velocity[1], n[2] * velocity_dot + velocity[2]];
                if magnitude_squared3(tangential) > maximum * maximum && velocity_dot / magnitude3(velocity) < physics.ground_tangential_angle {
                    supported = false;
                }
            }
        }
        supported.then_some(collision)
    });
    match supported_by {
        Some(collision) => {
            physics.out_flags &= !out_flags::AIRBORNE;
            physics.ground_plane = collision.plane;
            physics.support_surface_index = collision.source.surface;
            physics.landing_velocity =
                if physics.support_surface_index == NONE || physics.support_surface_index != physics.stick_surface_index {
                    -dot3(velocity, [collision.plane[0], collision.plane[1], collision.plane[2]])
                } else {
                    0.0
                };
        }
        None => {
            physics.out_flags |= out_flags::AIRBORNE;
            physics.ground_plane = DEPTHS_OF_HELL;
            physics.support_surface_index = NONE;
            physics.landing_velocity = 0.0;
        }
    }

    // (what it bumped, and the elevator it stands on: objects, not ported)
    physics.bumped_object_index = NONE;
    physics.elevator_object_index = NONE;

    physics.new_position = clipped_position;
    physics.new_velocity = clipped_velocity;
    physics.collision_velocity = magnitude3(sub3(clipped_velocity, velocity));
    physics.new_velocity[2] -= physics.crouch_velocity;

    if physics.in_flags & in_flags::CROUCHED != 0
        && physics.in_flags & in_flags::TRYING_TO_STAND != 0
        && let Some(stand) = stand_test
    {
        let radius = stand.collision_height_standing * 0.5;
        let mut center = physics.new_position;
        center[2] += radius;
        let flags = if physics.in_flags & in_flags::DEAD != 0 { COLLISION_FLAGS_DEAD } else { COLLISION_FLAGS_LIVING };
        let features = features::features_in_sphere(bsp, flags, center, radius, 0.0, stand.collision_radius);
        if !features.is_empty() {
            let standing_vector = scale3(UP, stand.collision_height_standing - 2.0 * stand.collision_radius);
            if features.test_vector(physics.new_position, standing_vector).0 {
                physics.out_flags |= out_flags::CANNOT_STAND;
            }
        }
    }
}

/// With nothing hit, a biped walking off its support surface onto a
/// climbable neighbour (a ledge, a step down) is held against it.
fn stick_to_ledge(
    bsp: &CollisionBsp,
    physics: &mut BipedPhysics,
    clipped_position: &mut [f32; 3],
    clipped_velocity: &mut [f32; 3],
    collisions: &mut Vec<CollisionPlane>,
) {
    let existing = physics.existing_support_surface_index;
    let Some(surface) = valid_surface(bsp, existing) else { return };
    let cp = *clipped_position;
    let plane = plane_from_designator(bsp, surface.plane_designator);
    let distance = -(plane[0] * cp[0] + plane[1] * cp[1] + plane[2] * cp[2] - plane[3]);
    let point = [plane[0] * distance + cp[0], plane[1] * distance + cp[1], plane[2] * distance + cp[2]];

    let mut best_surface = NONE;
    let mut best_distance_squared = f32::MAX;
    let mut best_plane = [0.0f32; 4];
    let mut best_velocity_dot = 0.0f32;
    let mut edge_index = surface.first_edge;
    let mut edge_count = 0;
    loop {
        if !bsp.ring_continues(edge_index, edge_count) {
            break;
        }
        edge_count += 1;
        let edge = bsp.edges[edge_index as usize];
        if edge.vertices.iter().any(|&v| v < 0 || v as usize >= bsp.vertices.len()) {
            break;
        }
        let reverse = existing == edge.surfaces[1];
        let neighbor_index = edge.surfaces[usize::from(!reverse)];
        if neighbor_index != NONE
            && let Some(neighbor) = valid_surface(bsp, neighbor_index)
            && (physics.in_flags & in_flags::CLIMB_ANYTHING != 0 || neighbor.flags & surface_flags::CLIMBABLE != 0)
        {
            let plane = plane_from_designator(bsp, neighbor.plane_designator);
            let n = [plane[0], plane[1], plane[2]];
            let velocity_dot = dot3(n, *clipped_velocity);
            if velocity_dot > 0.0 && plane[0] * cp[0] + plane[1] * cp[1] + plane[2] * cp[2] - plane[3] > physics.width * -0.5 {
                let vertex0 = bsp.vertices[edge.vertices[0] as usize];
                let vertex1 = bsp.vertices[edge.vertices[1] as usize];
                let edge_vector = sub3(vertex1, vertex0);
                let t = ((point[0] - vertex0[0]) * edge_vector[0]
                    + (point[1] - vertex0[1]) * edge_vector[1]
                    + (point[2] - vertex0[2]) * edge_vector[2])
                    / magnitude_squared3(edge_vector);
                let closest = if t < 0.0 {
                    vertex0
                } else if t > 1.0 {
                    vertex1
                } else {
                    [edge_vector[0] * t + vertex0[0], edge_vector[1] * t + vertex0[1], edge_vector[2] * t + vertex0[2]]
                };
                let distance_squared = magnitude_squared3(sub3(closest, point));
                if distance_squared < best_distance_squared {
                    best_distance_squared = distance_squared;
                    best_surface = neighbor_index;
                    best_plane = plane;
                    best_velocity_dot = velocity_dot;
                }
            }
        }
        edge_index = edge.edges[usize::from(reverse)];
        if edge_index == surface.first_edge {
            break;
        }
    }

    let width = physics.width;
    if best_surface != NONE && best_distance_squared <= (width * 2.0) * (width * 2.0) && best_velocity_dot <= 1.6 / TICKS_PER_SECOND {
        let n = [best_plane[0], best_plane[1], best_plane[2]];
        let distance = best_plane[0] * cp[0] + best_plane[1] * cp[1] + best_plane[2] * cp[2] - (best_plane[3] + width);
        if !(distance.abs() > width * 0.5) {
            let new_velocity_dot = dot3(n, *clipped_velocity);
            *clipped_position = [n[0] * -distance + cp[0], n[1] * -distance + cp[1], n[2] * -distance + cp[2]];
            if new_velocity_dot > -1.0 / TICKS_PER_SECOND {
                let t = -(new_velocity_dot + 1.0 / TICKS_PER_SECOND);
                let v = *clipped_velocity;
                *clipped_velocity = [n[0] * t + v[0], n[1] * t + v[1], n[2] * t + v[2]];
            }
            let p = *clipped_position;
            collisions.clear();
            collisions.push(CollisionPlane {
                t: 0.0,
                point: [n[0] * -width + p[0], n[1] * -width + p[1], n[2] * -width + p[2]],
                plane: best_plane,
                source: features::FeatureSource { object: NONE, surface: best_surface, flags: 0, breakable_surface: 0, material: NONE as i16 },
            });
            physics.stick_surface_index = best_surface;
        }
    }
}
