//! Halo's collision features and swept movement (ROADMAP phase 7, objects
//! and physics): OpenCE's `source/physics/collision_features.c` and
//! `collision_move_point`/`collision_move_pill` in `collisions.c`, ported as
//! they are, in the same single-precision arithmetic.
//!
//! A moving pill (a biped's collision shape) sweeps a point through the
//! world grown by the pill: each surface the sphere test finds becomes a
//! prism (the polygon pushed out by the pill's radius), each edge a pair of
//! cylinders and prisms, each vertex spheres and a cylinder. The point's
//! movement is clipped against what it hits, sliding along up to three
//! planes at once.
//!
//! Objects add theirs: a biped its pill (spheres and a cylinder), scenery,
//! machines, controls and vehicles their collision models' features in
//! their nodes' space. (A vehicle's physics shapes, which a test asking for
//! them would use instead, are not ported yet.)

use std::sync::Arc;

use crate::collision::{
    CollisionBsp, CollisionModel, SphereFeatures, matrix_inverse, test_flags, transform_plane, transform_point, transform_vector,
};
use crate::model::Matrix4x3;

/// `MAXIMUM_COLLISION_FEATURES_PER_TEST`.
pub const MAXIMUM_FEATURES: usize = 256;
const MAXIMUM_POINTS_PER_PRISM: usize = 8;
const EPSILON: f32 = 0.0001;
const NONE: i32 = -1;
const SIGN: i32 = i32::MIN;

/// What a feature belongs to (`collision_feature`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FeatureSource {
    pub object: i32,
    pub surface: i32,
    pub flags: u8,
    pub breakable_surface: u8,
    pub material: i16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sphere {
    pub source: FeatureSource,
    pub center: [f32; 3],
    pub radius: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cylinder {
    pub source: FeatureSource,
    pub base: [f32; 3],
    pub height: [f32; 3],
    pub width: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Prism {
    pub source: FeatureSource,
    pub plane: [f32; 4],
    pub height: f32,
    pub projection: usize,
    pub sign: bool,
    pub points: Vec<[f32; 2]>,
}

/// `collision_feature_list`: at most [`MAXIMUM_FEATURES`] of each kind.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FeatureList {
    pub spheres: Vec<Sphere>,
    pub cylinders: Vec<Cylinder>,
    pub prisms: Vec<Prism>,
}

/// `collision_plane`: where a movement met a feature.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CollisionPlane {
    pub t: f32,
    pub point: [f32; 3],
    pub plane: [f32; 4],
    pub source: FeatureSource,
}

impl FeatureList {
    pub fn is_empty(&self) -> bool {
        self.spheres.is_empty() && self.cylinders.is_empty() && self.prisms.is_empty()
    }

    fn add_sphere(&mut self, sphere: Sphere) {
        if self.spheres.len() < MAXIMUM_FEATURES {
            self.spheres.push(sphere);
        }
    }

    fn add_cylinder(&mut self, cylinder: Cylinder) {
        if self.cylinders.len() < MAXIMUM_FEATURES {
            self.cylinders.push(cylinder);
        }
    }

    fn add_prism(&mut self, prism: Prism) {
        if self.prisms.len() < MAXIMUM_FEATURES {
            self.prisms.push(prism);
        }
    }

    /// `collision_features_from_point`.
    pub fn from_point(&mut self, point: [f32; 3], height: f32, width: f32, source: FeatureSource) {
        self.add_sphere(Sphere { source, center: point, radius: width });
        if height > 0.0 {
            let new_height = point[2] - height;
            self.add_sphere(Sphere { source, center: [point[0], point[1], new_height], radius: width });
            self.add_cylinder(Cylinder { source, base: [point[0], point[1], new_height], height: [0.0, 0.0, height], width });
        }
    }

    /// `collision_features_from_line`.
    pub fn from_line(&mut self, point: [f32; 3], vector: [f32; 3], height: f32, width: f32, source: FeatureSource) {
        self.add_cylinder(Cylinder { source, base: point, height: vector, width });
        if height > 0.0 {
            self.add_cylinder(Cylinder { source, base: [point[0], point[1], point[2] - height], height: vector, width });
            // (perpendicular2d, normalize2d)
            let mut n = [-vector[1], vector[0]];
            let magnitude = (n[0] * n[0] + n[1] * n[1]).sqrt();
            if !(EPSILON > magnitude.abs()) {
                let inverse = 1.0 / magnitude;
                n = [inverse * n[0], inverse * n[1]];
                let plane_distance = point[0] * n[0] + point[1] * n[1];
                let p1 = [point[0] + vector[0], point[1] + vector[1], point[2] + vector[2]];
                let mut points = [point, p1, [p1[0], p1[1], p1[2] - height], [point[0], point[1], point[2] - height]];
                let prism = |normal: [f32; 3], d: f32, points: &[[f32; 3]; 4]| {
                    let projection = projection_from_vector(normal);
                    let sign = normal[projection] > 0.0;
                    Prism {
                        source,
                        plane: [normal[0], normal[1], normal[2], d],
                        height: width,
                        projection,
                        sign,
                        points: points.iter().map(|&p| project(p, projection, sign)).collect(),
                    }
                };
                self.add_prism(prism([n[0], n[1], 0.0], plane_distance, &points));
                points.swap(1, 3);
                self.add_prism(prism([-n[0], -n[1], 0.0], -plane_distance, &points));
            }
        }
    }

    /// `collision_features_from_polygon`.
    pub fn from_polygon(&mut self, points: &[[f32; 3]], plane: [f32; 4], height: f32, width: f32, source: FeatureSource) {
        let points = &points[..points.len().min(MAXIMUM_POINTS_PER_PRISM)];
        if points.len() < 3 || self.prisms.len() >= MAXIMUM_FEATURES {
            return;
        }
        let n = [plane[0], plane[1], plane[2]];
        let projection = projection_from_vector(n);
        let sign = n[projection] > 0.0;
        let mut prism = Prism {
            source,
            plane,
            height: width,
            projection,
            sign,
            points: points.iter().map(|&p| project(p, projection, sign)).collect(),
        };
        if height > 0.0 && plane[2] < 0.0 {
            prism.plane[3] -= height * prism.plane[2];
            if projection != 2 {
                let component = usize::from(PROJECTIONS[projection][usize::from(sign)][1] == 2);
                for p in &mut prism.points {
                    p[component] -= height;
                }
            }
        }
        self.prisms.push(prism);
    }

    /// `collision_bsp_get_features_in_sphere`: the features of what a
    /// sphere test found (a structure's with no matrix and no object, an
    /// object's collision model's with its node matrix).
    pub fn from_sphere_test(
        &mut self,
        bsp: &CollisionBsp,
        result: &SphereFeatures,
        matrix: Option<&Matrix4x3>,
        height: f32,
        width: f32,
        object: i32,
    ) {
        for &vertex in &result.vertices {
            self.from_vertex(bsp, vertex, matrix, height, width, object);
        }
        for &edge in &result.edges {
            self.from_edge(bsp, edge, matrix, height, width, object);
        }
        for &surface in &result.surfaces {
            self.from_surface(bsp, surface, matrix, height, width, object);
        }
    }

    /// `collision_features_from_vertex`.
    fn from_vertex(&mut self, bsp: &CollisionBsp, vertex: i32, matrix: Option<&Matrix4x3>, height: f32, width: f32, object: i32) {
        if vertex < 0 || vertex as usize >= bsp.vertices.len() {
            return;
        }
        let first_edge = bsp.vertex_first_edges[vertex as usize];
        if !bsp.ring_continues(first_edge, 0) {
            return;
        }
        let edge = bsp.edges[first_edge as usize];
        let Some(surface) = valid_surface(bsp, edge.surfaces[0]) else { return };
        let point = bsp.vertices[vertex as usize];
        let point = match matrix {
            Some(m) => transform_point(m, point),
            None => point,
        };
        let source = FeatureSource {
            object,
            surface: if object != NONE { NONE } else { edge.surfaces[0] },
            flags: surface.flags,
            breakable_surface: surface.breakable_surface,
            material: surface.material,
        };
        self.from_point(point, height, width, source);
    }

    /// `collision_features_from_edge`: an edge between two surfaces on
    /// different planes, where the solid is convex.
    fn from_edge(&mut self, bsp: &CollisionBsp, edge_index: i32, matrix: Option<&Matrix4x3>, height: f32, width: f32, object: i32) {
        if !bsp.ring_continues(edge_index, 0) {
            return;
        }
        let edge = bsp.edges[edge_index as usize];
        let (Some(surface0), Some(surface1)) = (valid_surface(bsp, edge.surfaces[0]), valid_surface(bsp, edge.surfaces[1])) else {
            return;
        };
        if surface0.plane_designator == surface1.plane_designator {
            return;
        }
        let vertex0 = bsp.vertex(edge.vertices[0]);
        let vertex1 = bsp.vertex(edge.vertices[1]);
        let plane0_index = surface0.plane_designator & i32::MAX;
        let plane1_index = surface1.plane_designator & i32::MAX;
        let plane0 = bsp.planes.get(plane0_index as usize).copied().unwrap_or_default();
        let plane1 = bsp.planes.get(plane1_index as usize).copied().unwrap_or_default();
        let plane0_negated = surface0.plane_designator & SIGN != 0;
        let plane1_negated = surface1.plane_designator & SIGN != 0;
        let vector = sub3(vertex1, vertex0);
        let n0 = [plane0[0], plane0[1], plane0[2]];
        let n1 = [plane1[0], plane1[1], plane1[2]];
        let valid_edge = if plane0_index == plane1_index {
            true
        } else if plane0_negated == plane1_negated {
            dot3(cross3(n0, n1), vector) > -EPSILON
        } else {
            dot3(cross3(n0, n1), vector) < EPSILON
        };
        if !valid_edge {
            return;
        }
        let (point, vector) = match matrix {
            Some(m) => (transform_point(m, vertex0), transform_vector(m, vector)),
            None => (vertex0, vector),
        };
        let source = FeatureSource {
            object,
            surface: if object == NONE { edge.surfaces[0] } else { NONE },
            flags: surface0.flags,
            breakable_surface: surface0.breakable_surface,
            material: surface0.material,
        };
        self.from_line(point, vector, height, width, source);
    }

    /// `collision_features_from_surface`.
    fn from_surface(&mut self, bsp: &CollisionBsp, surface_index: i32, matrix: Option<&Matrix4x3>, height: f32, width: f32, object: i32) {
        let Some(surface) = valid_surface(bsp, surface_index) else { return };
        let mut points = surface_polygon(bsp, surface_index);
        let mut plane = plane_from_designator(bsp, surface.plane_designator);
        if let Some(m) = matrix {
            for p in &mut points {
                *p = transform_point(m, *p);
            }
            plane = transform_plane(m, plane);
        }
        let source = FeatureSource {
            object,
            surface: if object != NONE { NONE } else { surface_index },
            flags: surface.flags,
            breakable_surface: surface.breakable_surface,
            material: surface.material,
        };
        self.from_polygon(&points, plane, height, width, source);
    }

    /// `collision_features_test_vector`: the first feature the vector
    /// from `point` enters (facing against it), or the vector's end.
    pub fn test_vector(&self, point: [f32; 3], vector: [f32; 3]) -> (bool, CollisionPlane) {
        let mut closest: Option<(f32, [f32; 4], FeatureSource)> = None;
        let mut closest_t = f32::MAX;
        let mut consider = |hit: Option<(f32, [f32; 4])>, source: FeatureSource| {
            if let Some((t, plane)) = hit
                && closest_t > t
                && dot3(vector, [plane[0], plane[1], plane[2]]) < -EPSILON
            {
                closest_t = t;
                closest = Some((t, plane, source));
            }
        };
        for sphere in &self.spheres {
            consider(sphere_test_vector(sphere, point, vector), sphere.source);
        }
        for cylinder in &self.cylinders {
            consider(cylinder_test_vector(cylinder, point, vector), cylinder.source);
        }
        for prism in &self.prisms {
            consider(prism_test_vector(prism, point, vector), prism.source);
        }
        match closest {
            Some((t, plane, source)) => (
                true,
                CollisionPlane {
                    t,
                    point: [vector[0] * t + point[0], vector[1] * t + point[1], vector[2] * t + point[2]],
                    plane,
                    source,
                },
            ),
            None => (
                false,
                CollisionPlane {
                    t: 1.0,
                    point: [point[0] + vector[0], point[1] + vector[1], point[2] + vector[2]],
                    ..Default::default()
                },
            ),
        }
    }
}

/// An object as `object_get_features_in_sphere` sees it.
#[derive(Debug, Clone)]
pub struct FeatureObject {
    pub index: i32,
    /// `object_type` (0 biped, 1 vehicle, 6 scenery, 7 machine, 8 control).
    pub kind: i16,
    /// The object's `flags` (bit 0 invisible, 24 no collisions).
    pub flags: u32,
    pub dead: bool,
    pub bounding_center: [f32; 3],
    pub bounding_radius: f32,
    pub parent: i32,
    pub velocity: [f32; 3],
    /// A machine that is an elevator.
    pub elevator: bool,
    /// A biped's: whether it passes through bipeds, is a player's, sits in a
    /// seat, and its physics pill (`biped_get_physics_pill`).
    pub biped: Option<BipedPill>,
    pub model: Option<Arc<CollisionModel>>,
    pub region_permutations: [u8; 8],
    pub matrices: Vec<Matrix4x3>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BipedPill {
    pub passes_through_bipeds: bool,
    pub player: bool,
    pub seated: bool,
    pub base: [f32; 3],
    pub height: f32,
    pub width: f32,
}

pub const OBJECT_TYPE_BIPED: i16 = 0;
pub const OBJECT_TYPE_VEHICLE: i16 = 1;
pub const OBJECT_TYPE_SCENERY: i16 = 6;
pub const OBJECT_TYPE_MACHINE: i16 = 7;
pub const OBJECT_TYPE_CONTROL: i16 = 8;
const OBJECT_INVISIBLE: u32 = 1 << 0;
const OBJECT_NO_COLLISIONS: u32 = 1 << 24;
const SKIP_PASSTHROUGH_BIPEDS: u32 = 1 << 21;
const USE_VEHICLE_PHYSICS: u32 = 1 << 22;
const SKIP_PLAYER_BIPEDS: u32 = 1 << 23;

/// `collision_get_features_in_sphere`: the features within `radius` of
/// `center` grown by a pill of `height` and `width`, the structure's and
/// then those of `objects`, the objects the broad phase visits (from the
/// clusters the sphere reaches, each with its children), in the order it
/// visits them.
#[allow(clippy::too_many_arguments)]
pub fn features_in_sphere(
    bsp: &CollisionBsp,
    mut flags: u32,
    center: [f32; 3],
    radius: f32,
    height: f32,
    width: f32,
    ignore_object: i32,
    objects: &[FeatureObject],
) -> FeatureList {
    let mut features = FeatureList::default();
    if flags & (test_flags::STRUCTURE | test_flags::MEDIA | test_flags::OBJECTS) != 0 {
        let radius = radius + 0.0625;
        let result = bsp.test_sphere(center, radius);
        if result.touches() && flags & test_flags::STRUCTURE != 0 {
            features.from_sphere_test(bsp, &result, None, height, width, NONE);
        }
        if flags & test_flags::OBJECTS != 0 && !result.leaves.is_empty() {
            if flags & test_flags::ALL_OBJECT_TYPES == 0 {
                flags |= test_flags::ALL_OBJECT_TYPES;
            }
            for object in objects {
                features.from_object(flags, object, center, radius, height, width, ignore_object);
            }
        }
    }
    features
}

impl FeatureList {
    /// `object_get_features_in_sphere`, for one object.
    #[allow(clippy::too_many_arguments)]
    fn from_object(&mut self, flags: u32, object: &FeatureObject, center: [f32; 3], radius: f32, height: f32, width: f32, ignore_object: i32) {
        if object.index == ignore_object
            || object.flags & OBJECT_INVISIBLE != 0
            || object.flags & OBJECT_NO_COLLISIONS != 0
            || (object.dead && object.kind == OBJECT_TYPE_BIPED)
        {
            return;
        }
        let reach = object.bounding_radius + radius;
        if !(magnitude_squared3(sub3(object.bounding_center, center)) <= reach * reach) {
            return;
        }
        if object.kind < 0 || flags & (1 << (object.kind as u32 + test_flags::FIRST_OBJECT_TYPE)) == 0 {
            return;
        }
        match object.kind {
            OBJECT_TYPE_BIPED => {
                if let Some(pill) = object.biped
                    && (flags & SKIP_PASSTHROUGH_BIPEDS == 0 || !pill.passes_through_bipeds)
                    && (flags & SKIP_PLAYER_BIPEDS == 0 || !pill.player)
                    && (object.parent == NONE || !pill.seated)
                {
                    let base = [pill.base[0], pill.base[1], pill.base[2] + pill.height];
                    let source = FeatureSource { object: object.index, surface: NONE, flags: 0, breakable_surface: 0xff, material: NONE as i16 };
                    self.from_point(base, pill.height + height, pill.width + width, source);
                }
            }
            OBJECT_TYPE_VEHICLE | OBJECT_TYPE_SCENERY | OBJECT_TYPE_MACHINE | OBJECT_TYPE_CONTROL => {
                if object.kind == OBJECT_TYPE_VEHICLE && flags & USE_VEHICLE_PHYSICS != 0 {
                    // (physics_get_features_in_sphere: not ported)
                } else if let Some(model) = &object.model {
                    self.from_collision_model(model, object, center, radius, height, width);
                }
            }
            _ => {}
        }
    }

    /// `collision_model_get_features_in_sphere`: each node's collision BSP
    /// for the object's permutation of its region, tested in the node's
    /// space, its features brought back out.
    fn from_collision_model(&mut self, model: &CollisionModel, object: &FeatureObject, center: [f32; 3], radius: f32, height: f32, width: f32) {
        for (node_index, node) in model.nodes.iter().enumerate().take(object.matrices.len()) {
            if node.region < 0 || node.region >= 8 || node.bsps.is_empty() {
                continue;
            }
            let permutation = (object.region_permutations[node.region as usize] as usize).min(node.bsps.len() - 1);
            let bsp = &node.bsps[permutation];
            if !bsp.has_nodes() {
                continue;
            }
            let matrix = &object.matrices[node_index];
            let inverse = matrix_inverse(matrix);
            let local_center = transform_point(&inverse, center);
            let scaled_radius = inverse.scale * radius;
            let result = bsp.test_sphere(local_center, scaled_radius);
            if result.touches() {
                self.from_sphere_test(bsp, &result, Some(matrix), height, width, object.index);
            }
        }
    }
}

/// `collision_move_pill`: a pill of `height` and `radius` standing at
/// `position` moved by `velocity` through the structure. Returns where it
/// ends, its velocity after sliding, and the planes it met (at most
/// `maximum_collision_count`).
#[allow(clippy::too_many_arguments)]
pub fn move_pill(
    bsp: &CollisionBsp,
    flags: u32,
    position: [f32; 3],
    velocity: [f32; 3],
    height: f32,
    radius: f32,
    ignore_object: i32,
    objects: &[FeatureObject],
    maximum_collision_count: usize,
) -> ([f32; 3], [f32; 3], Vec<CollisionPlane>) {
    let center = [
        position[0] + velocity[0] * 0.5,
        position[1] + velocity[1] * 0.5,
        height * 0.5 + (position[2] + velocity[2] * 0.5),
    ];
    let enclosing = magnitude3(velocity) * 0.5 + height * 0.5 + radius;
    let features = features_in_sphere(bsp, flags, center, enclosing, height, radius, ignore_object, objects);
    if !features.is_empty() {
        move_point(position, velocity, &features, maximum_collision_count)
    } else {
        ([position[0] + velocity[0], position[1] + velocity[1], position[2] + velocity[2]], velocity, Vec::new())
    }
}

/// `collision_move_point`: a point moved by `velocity` through the
/// features, sliding along the planes it meets (one, the line where two
/// meet, or stopping where three do).
pub fn move_point(
    old_position: [f32; 3],
    old_velocity: [f32; 3],
    features: &FeatureList,
    maximum_collision_count: usize,
) -> ([f32; 3], [f32; 3], Vec<CollisionPlane>) {
    let mut collisions: Vec<CollisionPlane> = Vec::new();
    let mut velocity = old_velocity;
    let mut clipped_position = old_position;
    let mut clipped_velocity = old_velocity;
    let mut clip_count = 0usize;
    let mut clip_indices = [0usize; 3];
    let mut clip_plane = [0.0f32; 4];
    let mut clip_line_point = [0.0f32; 3];
    let mut clip_line_vector = [0.0f32; 3];
    let mut clip_point = [0.0f32; 3];

    while !(clipped_velocity[0].abs() < EPSILON && clipped_velocity[1].abs() < EPSILON && clipped_velocity[2].abs() < EPSILON) {
        let (hit, collision) = features.test_vector(clipped_position, clipped_velocity);
        if !hit {
            clipped_position = collision.point;
            break;
        }
        collisions.push(collision);
        let collision_count = collisions.len();
        let position = collision.point;
        velocity = scale3(velocity, 1.0 - collision.t);

        let mut new_clip_indices = [0usize; 3];
        let mut new_clip_count = 1;
        new_clip_indices[0] = collision_count - 1;
        clip_plane = collisions[new_clip_indices[0]].plane;
        clipped_velocity = clip_velocity_to_plane(velocity, clip_plane);
        clipped_position = clip_position_to_plane(position, clip_plane);

        if clip_count > 0 {
            if dot3(clipped_velocity, normal(collisions[clip_indices[0]].plane)) < -EPSILON
                && line_from_planes(
                    collisions[new_clip_indices[0]].plane,
                    collisions[clip_indices[0]].plane,
                    &mut clip_line_point,
                    &mut clip_line_vector,
                )
            {
                new_clip_count = 2;
                new_clip_indices[1] = clip_indices[0];
                clipped_velocity = clip_velocity_to_line(velocity, clip_line_vector);
                clipped_position = clip_position_to_line(position, clip_line_point, clip_line_vector);
                if clip_count > 1
                    && dot3(clipped_velocity, normal(collisions[clip_indices[1]].plane)) < -EPSILON
                    && point_from_planes(
                        collisions[new_clip_indices[0]].plane,
                        collisions[new_clip_indices[1]].plane,
                        collisions[clip_indices[1]].plane,
                        &mut clip_point,
                    )
                {
                    new_clip_count = 3;
                    new_clip_indices[2] = clip_indices[1];
                    clipped_velocity = [0.0; 3];
                    clipped_position = clip_point;
                }
            } else if clip_count > 1
                && dot3(clipped_velocity, normal(collisions[clip_indices[1]].plane)) < -EPSILON
                && line_from_planes(
                    collisions[new_clip_indices[0]].plane,
                    collisions[clip_indices[1]].plane,
                    &mut clip_line_point,
                    &mut clip_line_vector,
                )
            {
                new_clip_count = 2;
                new_clip_indices[1] = clip_indices[1];
                clipped_velocity = clip_velocity_to_line(velocity, clip_line_vector);
                clipped_position = clip_position_to_line(position, clip_line_point, clip_line_vector);
            }
        }
        clip_count = new_clip_count;
        clip_indices[..clip_count].copy_from_slice(&new_clip_indices[..clip_count]);

        if collisions.len() >= maximum_collision_count {
            break;
        }
    }

    let new_position = clipped_position;
    let new_velocity = match clip_count {
        0 => old_velocity,
        1 => clip_velocity_to_plane(old_velocity, clip_plane),
        2 => clip_velocity_to_line(old_velocity, clip_line_vector),
        _ => [0.0; 3],
    };

    // (where two or three planes meet, a plane to stand on: across the
    // line or corner from the steepest overhang, or flat)
    if clip_count > 1 && collisions.len() < maximum_collision_count {
        let last = collisions[clip_indices[clip_count - 1]];
        let mut collision = CollisionPlane {
            t: last.t,
            point: last.point,
            plane: [0.0; 4],
            source: FeatureSource { object: NONE, surface: NONE, flags: 0, breakable_surface: 0, material: NONE as i16 },
        };
        let mut minimum_k = 0.0f32;
        let mut steepest: Option<usize> = None;
        for (clip_index, &index) in clip_indices[..clip_count].iter().enumerate() {
            if collisions[index].plane[2] < minimum_k {
                minimum_k = collisions[index].plane[2];
                steepest = Some(clip_index);
            }
        }
        let mut keep = true;
        let mut n;
        if clip_count == 2 {
            n = match steepest {
                Some(0) => cross3(clip_line_vector, normal(collisions[clip_indices[0]].plane)),
                Some(s) => cross3(normal(collisions[clip_indices[s]].plane), clip_line_vector),
                None => {
                    let t = -(clip_line_vector[2] / magnitude_squared3(clip_line_vector));
                    [clip_line_vector[0] * t + 0.0, clip_line_vector[1] * t + 0.0, clip_line_vector[2] * t + 1.0]
                }
            };
            if normalize3(&mut n) != 0.0 {
                collision.plane = [n[0], n[1], n[2], dot3(clip_line_point, n)];
            } else {
                keep = false;
            }
        } else if let Some(s) = steepest {
            let steepest_n = normal(collisions[clip_indices[s]].plane);
            let t = -steepest_n[2];
            n = [steepest_n[0] * t + 0.0, steepest_n[1] * t + 0.0, steepest_n[2] * t + 1.0];
            if normalize3(&mut n) != 0.0 {
                collision.plane = [n[0], n[1], n[2], dot3(clip_point, n)];
            } else {
                keep = false;
            }
        } else {
            n = [0.0, 0.0, 1.0];
            collision.plane = [n[0], n[1], n[2], dot3(clip_point, n)];
        }
        if keep {
            collisions.push(collision);
        }
    }

    (new_position, new_velocity, collisions)
}

// ---------- the feature tests

/// A point inside a feature: how deep, the feature's plane there, and what
/// the feature came from (`collision_features_test_point`'s result).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointCollision {
    pub depth: f32,
    pub plane: [f32; 4],
    pub source: FeatureSource,
}

impl FeatureList {
    /// `collision_features_test_point`: the feature a point is deepest
    /// inside (spheres, then cylinders, then prisms; the first of equals).
    pub fn test_point(&self, point: [f32; 3]) -> Option<PointCollision> {
        let mut deepest: Option<PointCollision> = None;
        let mut deepest_depth = f32::MIN;
        let mut consider = |hit: Option<(f32, [f32; 4])>, source: FeatureSource| {
            if let Some((depth, plane)) = hit
                && deepest_depth < depth
            {
                deepest_depth = depth;
                deepest = Some(PointCollision { depth, plane, source });
            }
        };
        for sphere in &self.spheres {
            consider(sphere_test_point(sphere, point), sphere.source);
        }
        for cylinder in &self.cylinders {
            consider(cylinder_test_point(cylinder, point), cylinder.source);
        }
        for prism in &self.prisms {
            consider(prism_test_point(prism, point), prism.source);
        }
        deepest
    }
}

/// `collision_sphere_test_point`.
fn sphere_test_point(sphere: &Sphere, point: [f32; 3]) -> Option<(f32, [f32; 4])> {
    let w = sub3(point, sphere.center);
    let distance_squared = magnitude_squared3(w);
    if !(distance_squared < sphere.radius * sphere.radius) {
        return None;
    }
    let distance = distance_squared.sqrt();
    let n = if distance > 0.0 { scale3(w, 1.0 / distance) } else { [0.0, 0.0, 1.0] };
    Some((sphere.radius - distance, [n[0], n[1], n[2], dot3(sphere.center, n) + sphere.radius]))
}

/// `collision_cylinder_test_point`.
fn cylinder_test_point(cylinder: &Cylinder, point: [f32; 3]) -> Option<(f32, [f32; 4])> {
    let w = sub3(point, cylinder.base);
    let height_distance = dot3(w, cylinder.height);
    if !(height_distance >= 0.0) {
        return None;
    }
    let height_squared = magnitude_squared3(cylinder.height);
    if !(height_distance <= height_squared
        && magnitude_squared3(w) * height_squared - height_distance * height_distance
            < cylinder.width * cylinder.width * height_squared)
    {
        return None;
    }
    let mut n = if height_squared > 0.0 { sub3(w, scale3(cylinder.height, height_distance / height_squared)) } else { w };
    let radial_distance = normalize3(&mut n);
    if radial_distance == 0.0 {
        n = [0.0, 0.0, 1.0];
    }
    Some((cylinder.width - radial_distance, [n[0], n[1], n[2], dot3(cylinder.base, n) + cylinder.width]))
}

/// `collision_prism_test_point`.
fn prism_test_point(prism: &Prism, point: [f32; 3]) -> Option<(f32, [f32; 4])> {
    let n = normal(prism.plane);
    let distance = dot3(point, n) - prism.plane[3];
    if !(distance >= 0.0) || !(distance < prism.height) {
        return None;
    }
    // (point_from_line3d)
    let on_plane = [n[0] * -distance + point[0], n[1] * -distance + point[1], n[2] * -distance + point[2]];
    let p = project(on_plane, prism.projection, prism.sign);
    let count = prism.points.len();
    for index in 0..count {
        let next = if index + 1 < count { index + 1 } else { 0 };
        let v0 = [prism.points[index][0] - p[0], prism.points[index][1] - p[1]];
        let v1 = [prism.points[next][0] - p[0], prism.points[next][1] - p[1]];
        // (cross_product2d)
        if v0[0] * v1[1] - v0[1] * v1[0] < 0.0 {
            return None;
        }
    }
    Some((prism.height - distance, [n[0], n[1], n[2], prism.plane[3] + prism.height]))
}

/// `collision_sphere_test_vector`.
fn sphere_test_vector(sphere: &Sphere, point: [f32; 3], vector: [f32; 3]) -> Option<(f32, [f32; 4])> {
    let w = sub3(sphere.center, point);
    let distance = magnitude_squared3(w) - sphere.radius * sphere.radius;
    let t = if distance <= 0.0 {
        0.0
    } else {
        let mut projection = dot3(w, vector);
        if !(projection > 0.0) {
            return None;
        }
        let vector_squared = magnitude_squared3(vector);
        let discriminant = projection * projection - vector_squared * distance;
        if !(discriminant >= 0.0) {
            return None;
        }
        projection -= discriminant.sqrt();
        if !(projection <= vector_squared) {
            return None;
        }
        projection / vector_squared
    };
    let point_on_vector = scale3(vector, t);
    let mut n = sub3(point_on_vector, w);
    if normalize3(&mut n) == 0.0 {
        n = [0.0, 0.0, 1.0];
    }
    Some((t, [n[0], n[1], n[2], dot3(sphere.center, n) + sphere.radius]))
}

/// `collision_cylinder_test_vector`.
fn cylinder_test_vector(cylinder: &Cylinder, point: [f32; 3], vector: [f32; 3]) -> Option<(f32, [f32; 4])> {
    let h = cylinder.height;
    let height_squared = magnitude_squared3(h);
    let height_vector = dot3(h, vector);
    let vector_squared = magnitude_squared3(vector);
    let quadratic_a = height_squared * vector_squared - height_vector * height_vector;
    if quadratic_a == 0.0 {
        return None;
    }
    let w = sub3(point, cylinder.base);
    let vector_w = dot3(vector, w);
    let height_w = dot3(h, w);
    let w_squared = magnitude_squared3(w);
    let width_squared = cylinder.width * cylinder.width;
    let quadratic_b = height_w * height_vector - height_squared * vector_w;
    let quadratic_c = (w_squared - width_squared) * height_squared - height_w * height_w;
    let discriminant = quadratic_b * quadratic_b - quadratic_a * quadratic_c;
    if discriminant < 0.0 {
        return None;
    }
    let discriminant = discriminant.sqrt();
    let inverse_quadratic_a = 1.0 / quadratic_a;
    let mut minimum_t = (quadratic_b - discriminant) * inverse_quadratic_a;
    let mut maximum_t = (quadratic_b + discriminant) * inverse_quadratic_a;
    if minimum_t > 1.0 || maximum_t < 0.0 {
        return None;
    }
    if minimum_t < 0.0 {
        minimum_t = 0.0;
    }
    if maximum_t > 1.0 {
        maximum_t = 1.0;
    }
    if height_vector != 0.0 {
        let bottom_t = -height_w / height_vector;
        let top_t = (height_squared - height_w) / height_vector;
        if height_vector > 0.0 {
            if minimum_t < bottom_t {
                minimum_t = bottom_t;
            }
            maximum_t = min(maximum_t, top_t);
        } else {
            if minimum_t < top_t {
                minimum_t = top_t;
            }
            maximum_t = min(maximum_t, bottom_t);
        }
        if minimum_t > maximum_t {
            return None;
        }
    } else if height_w < 0.0 || height_w > height_squared {
        return None;
    }
    let p = [vector[0] * minimum_t + w[0], vector[1] * minimum_t + w[1], vector[2] * minimum_t + w[2]];
    let height_t = -(dot3(p, h) / height_squared);
    let mut n = [h[0] * height_t + p[0], h[1] * height_t + p[1], h[2] * height_t + p[2]];
    if normalize3(&mut n) == 0.0 {
        n = [1.0, 0.0, 0.0];
    }
    Some((minimum_t, [n[0], n[1], n[2], dot3(cylinder.base, n) + cylinder.width]))
}

/// `collision_prism_test_vector`.
fn prism_test_vector(prism: &Prism, point: [f32; 3], vector: [f32; 3]) -> Option<(f32, [f32; 4])> {
    let n = normal(prism.plane);
    let mut t_out = 0.0f32;
    let mut t_in = 1.0f32;
    let d = dot3(point, n) - prism.plane[3];
    // (summed k, j, i, as the game does)
    let mut vn = n[2] * vector[2];
    vn += n[1] * vector[1];
    vn += n[0] * vector[0];
    if vn != 0.0 {
        let oovn = 1.0 / vn;
        let t0 = -oovn * d;
        let t1 = -oovn * (d - prism.height);
        if vn > 0.0 {
            if t_out < t0 {
                t_out = t0;
            }
            t_in = min(t_in, t1);
        } else {
            if t_out < t1 {
                t_out = t1;
            }
            t_in = min(t_in, t0);
        }
        if t_out > t_in {
            return None;
        }
    } else if d < 0.0 || d >= prism.height {
        return None;
    }
    let p3d = [n[0] * -d + point[0], n[1] * -d + point[1], n[2] * -d + point[2]];
    let v3d = [n[0] * -vn + vector[0], n[1] * -vn + vector[1], n[2] * -vn + vector[2]];
    let p2d = project(p3d, prism.projection, prism.sign);
    let v2d = project(v3d, prism.projection, prism.sign);
    let count = prism.points.len();
    for index in 0..count {
        let next = if index + 1 >= count { 0 } else { index + 1 };
        let p0 = prism.points[index];
        let p1 = prism.points[next];
        let w = [p1[0] - p0[0], p1[1] - p0[1]];
        let x = [p2d[0] - p0[0], p2d[1] - p0[1]];
        let vw = v2d[0] * w[1] - v2d[1] * w[0];
        let wx = w[0] * x[1] - w[1] * x[0];
        if vw != 0.0 {
            let t_edge = wx / vw;
            if vw < 0.0 {
                if t_out < t_edge {
                    t_out = t_edge;
                }
            } else if t_in > t_edge {
                t_in = t_edge;
            }
            if t_out > t_in {
                return None;
            }
        } else if wx < 0.0 {
            return None;
        }
    }
    Some((t_out, [n[0], n[1], n[2], prism.plane[3] + prism.height]))
}

// ---------- clipping

/// `clip_position_to_plane`.
fn clip_position_to_plane(position: [f32; 3], plane: [f32; 4]) -> [f32; 3] {
    let distance = -(dot3(position, normal(plane)) - plane[3]);
    [plane[0] * distance + position[0], plane[1] * distance + position[1], plane[2] * distance + position[2]]
}

/// `clip_velocity_to_plane`.
fn clip_velocity_to_plane(velocity: [f32; 3], plane: [f32; 4]) -> [f32; 3] {
    let distance = -dot3(velocity, normal(plane));
    [plane[0] * distance + velocity[0], plane[1] * distance + velocity[1], plane[2] * distance + velocity[2]]
}

/// `clip_position_to_line`.
fn clip_position_to_line(position: [f32; 3], point: [f32; 3], vector: [f32; 3]) -> [f32; 3] {
    let offset = sub3(position, point);
    let t = dot3(offset, vector) / magnitude_squared3(vector);
    [vector[0] * t + point[0], vector[1] * t + point[1], vector[2] * t + point[2]]
}

/// `clip_velocity_to_line`.
fn clip_velocity_to_line(velocity: [f32; 3], vector: [f32; 3]) -> [f32; 3] {
    scale3(vector, dot3(velocity, vector) / magnitude_squared3(vector))
}

/// `line_from_planes3d`: the line where two planes meet. The direction is
/// written even when the planes are parallel (as the game's is).
fn line_from_planes(plane0: [f32; 4], plane1: [f32; 4], point: &mut [f32; 3], direction: &mut [f32; 3]) -> bool {
    *direction = cross3(normal(plane0), normal(plane1));
    let determinant = magnitude_squared3(*direction);
    if determinant.abs() < EPSILON {
        return false;
    }
    let local = cross3(normal(plane1), *direction);
    let distance = plane0[3];
    let p = [local[0] * distance, local[1] * distance, local[2] * distance];
    let local = cross3(*direction, normal(plane0));
    let distance = plane1[3];
    let inverse_determinant = 1.0 / determinant;
    *point = [
        (local[0] * distance + p[0]) * inverse_determinant,
        (local[1] * distance + p[1]) * inverse_determinant,
        (local[2] * distance + p[2]) * inverse_determinant,
    ];
    true
}

/// `point_from_planes3d`: the point where three planes meet.
fn point_from_planes(plane0: [f32; 4], plane1: [f32; 4], plane2: [f32; 4], point: &mut [f32; 3]) -> bool {
    let cross = cross3(normal(plane0), normal(plane1));
    let determinant = dot3(cross, normal(plane2));
    if determinant.abs() < EPSILON {
        return false;
    }
    let cross = cross3(normal(plane1), normal(plane2));
    let distance = plane0[3];
    let p = [cross[0] * distance, cross[1] * distance, cross[2] * distance];
    let cross = cross3(normal(plane2), normal(plane0));
    let distance = plane1[3];
    let p = [cross[0] * distance + p[0], cross[1] * distance + p[1], cross[2] * distance + p[2]];
    let cross = cross3(normal(plane0), normal(plane1));
    let distance = plane2[3];
    let inverse = 1.0 / determinant;
    *point = [
        (cross[0] * distance + p[0]) * inverse,
        (cross[1] * distance + p[1]) * inverse,
        (cross[2] * distance + p[2]) * inverse,
    ];
    true
}

// ---------- the structure

/// A surface of the bsp (`collision_bsp_valid_surface_index`).
pub(crate) fn valid_surface(bsp: &CollisionBsp, surface: i32) -> Option<crate::collision::Surface> {
    (surface >= 0).then(|| bsp.surfaces.get(surface as usize).copied()).flatten()
}

/// `bsp3d_get_plane_from_designator`.
pub fn plane_from_designator(bsp: &CollisionBsp, designator: i32) -> [f32; 4] {
    let plane = bsp.planes.get((designator & i32::MAX) as usize).copied().unwrap_or_default();
    if designator & SIGN != 0 { [-plane[0], -plane[1], -plane[2], -plane[3]] } else { plane }
}

/// `collision_surface_polygon`: a surface's points, around its edge ring.
fn surface_polygon(bsp: &CollisionBsp, surface_index: i32) -> Vec<[f32; 3]> {
    let mut points = Vec::new();
    let Some(surface) = valid_surface(bsp, surface_index) else { return points };
    let first_edge = surface.first_edge;
    let mut edge_index = first_edge;
    if !bsp.ring_continues(edge_index, 0) {
        return points;
    }
    loop {
        let edge = bsp.edges[edge_index as usize];
        let reverse = edge.surfaces[1] == surface_index;
        points.push(bsp.vertex(edge.vertices[usize::from(reverse)]));
        edge_index = edge.edges[usize::from(reverse)];
        if edge_index == first_edge || !bsp.ring_continues(edge_index, points.len()) {
            break;
        }
    }
    points
}

// ---------- math, as the game's

/// `global_projection3d_mappings`.
const PROJECTIONS: [[[usize; 2]; 2]; 3] = [[[2, 1], [1, 2]], [[0, 2], [2, 0]], [[1, 0], [0, 1]]];

/// `projection_from_vector3d`.
fn projection_from_vector(n: [f32; 3]) -> usize {
    let (i, j, k) = (n[0].abs(), n[1].abs(), n[2].abs());
    if k >= j && k >= i {
        2
    } else if j >= i {
        1
    } else {
        0
    }
}

/// `project_point3d`.
fn project(p: [f32; 3], projection: usize, sign: bool) -> [f32; 2] {
    let m = PROJECTIONS[projection][usize::from(sign)];
    [p[m[0]], p[m[1]]]
}

fn normal(plane: [f32; 4]) -> [f32; 3] {
    [plane[0], plane[1], plane[2]]
}

/// `MIN`, as the macro compares.
fn min(a: f32, b: f32) -> f32 {
    if a > b { b } else { a }
}

pub(crate) fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `cross_product3d`.
pub(crate) fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `vector_from_points3d(b, a)`: a - b.
pub(crate) fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `scale_vector3d`.
pub(crate) fn scale3(v: [f32; 3], c: f32) -> [f32; 3] {
    [c * v[0], c * v[1], c * v[2]]
}

pub(crate) fn magnitude_squared3(v: [f32; 3]) -> f32 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

pub(crate) fn magnitude3(v: [f32; 3]) -> f32 {
    magnitude_squared3(v).sqrt()
}

/// `normalize3d`: the magnitude, 0 (leaving the vector) below epsilon.
pub(crate) fn normalize3(v: &mut [f32; 3]) -> f32 {
    let magnitude = magnitude3(*v);
    if !(EPSILON > magnitude.abs()) {
        *v = scale3(*v, 1.0 / magnitude);
        magnitude
    } else {
        0.0
    }
}
