//! A structure BSP's collision geometry and Halo's ray test against it
//! (ROADMAP phase 7, objects and physics): `collision_bsp_test_vector` in
//! OpenCE's `source/physics/collision_bsp.c`, ported as it is, in the same
//! single-precision arithmetic, so that a ray finds what the game's finds.
//!
//! A 3D BSP of planes leads to leaves; a ray through the tree crosses from
//! leaf to leaf, and where it passes from empty space into solid (or the
//! other way, or between two-sided leaves) the plane it crosses holds the
//! surface it hits: the leaf's 2D BSP references for that plane say which
//! surface polygon the crossing point is in, and a two-sided surface's edge
//! ring checks that the point is inside it.
//!
//! Points and vectors are the game's: world units, Z up.

use crate::{bsp::Bsp, layout, Error, Result};

/// `collision_test_flags` (collisions.h): which surfaces a test hits.
pub mod flags {
    pub const FRONT_FACING: u32 = 1 << 0;
    pub const BACK_FACING: u32 = 1 << 1;
    pub const IGNORE_TWO_SIDED: u32 = 1 << 2;
    pub const IGNORE_INVISIBLE: u32 = 1 << 3;
    pub const IGNORE_BREAKABLE: u32 = 1 << 4;
}

/// `collision_surface_flags`.
pub mod surface_flags {
    pub const TWO_SIDED: u8 = 1 << 0;
    pub const INVISIBLE: u8 = 1 << 1;
    pub const CLIMBABLE: u8 = 1 << 2;
    pub const BREAKABLE: u8 = 1 << 3;
}

const LEAF_CONTAINS_TWO_SIDED: u16 = 1 << 0;
/// How deep a walk of a 3D BSP goes (the engine's plane stacks).
const MAXIMUM_DEPTH: u16 = 128;
const MAXIMUM_BSP2D_DEPTH: u16 = 128;
const MAXIMUM_EDGES_PER_SURFACE: usize = 8;
const NONE: i32 = -1;
const SIGN: i32 = i32::MIN;

#[derive(Debug, Clone, Copy)]
struct Node3d {
    plane: i32,
    children: [i32; 2],
}

#[derive(Debug, Clone, Copy)]
struct Leaf {
    flags: u16,
    reference_count: i16,
    first_reference: i32,
}

#[derive(Debug, Clone, Copy)]
struct Reference2d {
    plane_designator: i32,
    root: i32,
}

#[derive(Debug, Clone, Copy)]
struct Node2d {
    /// n.x, n.y, d.
    plane: [f32; 3],
    children: [i32; 2],
}

#[derive(Debug, Clone, Copy)]
pub struct Surface {
    pub plane_designator: i32,
    pub first_edge: i32,
    pub flags: u8,
    pub breakable_surface: u8,
    pub material: i16,
}

#[derive(Debug, Clone, Copy)]
struct Edge {
    vertices: [i32; 2],
    edges: [i32; 2],
    surfaces: [i32; 2],
}

/// A structure BSP's collision geometry, copied out of the map.
#[derive(Debug, Clone, Default)]
pub struct CollisionBsp {
    nodes: Vec<Node3d>,
    /// n.x, n.y, n.z, d: a point p is in front where n.p - d >= 0.
    pub planes: Vec<[f32; 4]>,
    leaves: Vec<Leaf>,
    references: Vec<Reference2d>,
    nodes2d: Vec<Node2d>,
    pub surfaces: Vec<Surface>,
    edges: Vec<Edge>,
    pub vertices: Vec<[f32; 3]>,
}

/// What a ray hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// How far along the ray (0 at its start, 1 at its end).
    pub t: f32,
    pub surface: usize,
    /// The plane crossed (as stored: the surface faces along it or away).
    pub plane: [f32; 4],
    pub plane_designator: i32,
    pub flags: u8,
    pub material: i16,
}

impl CollisionBsp {
    /// The collision geometry of a loaded structure BSP.
    pub fn load(bsp: &Bsp<'_>) -> Result<Self> {
        use layout::{bsp2d, bsp2d_node, bsp3d, bsp3d_node, collision_bsp as c, collision_edge, collision_surface, collision_vertex};
        let blocks = bsp.root.block(layout::structure_bsp::COLLISION_BSP, c::SIZEOF)?;
        let root = blocks.get(0).ok_or_else(|| Error::Format("the bsp has no collision bsp".into()))?;
        let mut out = CollisionBsp::default();
        for node in root.block(c::BSP3D + bsp3d::NODES, bsp3d_node::SIZEOF)?.iter() {
            out.nodes.push(Node3d { plane: node.i32(0)?, children: [node.i32(4)?, node.i32(8)?] });
        }
        for plane in root.block(c::BSP3D + bsp3d::PLANES, 16)?.iter() {
            out.planes.push([plane.f32(0)?, plane.f32(4)?, plane.f32(8)?, plane.f32(12)?]);
        }
        for leaf in root.block(c::LEAVES, 8)?.iter() {
            out.leaves.push(Leaf { flags: leaf.i16(0)? as u16, reference_count: leaf.i16(2)?, first_reference: leaf.i32(4)? });
        }
        for reference in root.block(c::BSP2D_REFERENCES, 8)?.iter() {
            out.references.push(Reference2d { plane_designator: reference.i32(0)?, root: reference.i32(4)? });
        }
        for node in root.block(c::BSP2D + bsp2d::NODES, bsp2d_node::SIZEOF)?.iter() {
            let p = bsp2d_node::PLANE;
            let ch = bsp2d_node::CHILD_INDICES;
            out.nodes2d.push(Node2d {
                plane: [node.f32(p)?, node.f32(p + 4)?, node.f32(p + 8)?],
                children: [node.i32(ch)?, node.i32(ch + 4)?],
            });
        }
        for surface in root.block(c::SURFACES, collision_surface::SIZEOF)?.iter() {
            use collision_surface as s;
            out.surfaces.push(Surface {
                plane_designator: surface.i32(s::PLANE_DESIGNATOR)?,
                first_edge: surface.i32(s::FIRST_EDGE_INDEX)?,
                flags: surface.u8(s::FLAGS)?,
                breakable_surface: surface.u8(s::FLAGS + 1)?,
                material: surface.i16(s::MATERIAL_INDEX)?,
            });
        }
        for edge in root.block(c::EDGES, collision_edge::SIZEOF)?.iter() {
            use collision_edge as e;
            out.edges.push(Edge {
                vertices: [edge.i32(e::VERTEX_INDICES)?, edge.i32(e::VERTEX_INDICES + 4)?],
                edges: [edge.i32(e::EDGE_INDICES)?, edge.i32(e::EDGE_INDICES + 4)?],
                surfaces: [edge.i32(e::SURFACE_INDICES)?, edge.i32(e::SURFACE_INDICES + 4)?],
            });
        }
        for vertex in root.block(c::VERTICES, collision_vertex::SIZEOF)?.iter() {
            out.vertices.push(vertex.vec3(0)?);
        }
        Ok(out)
    }

    /// Halo's ray test: the first surface (by `flags`) the ray from `point`
    /// along `vector` hits, up to `maximum_t` (in units of `vector`). Every
    /// breakable surface counts as intact.
    pub fn test_vector(&self, flags: u32, point: [f32; 3], vector: [f32; 3], maximum_t: f32) -> Option<Hit> {
        let mut test = VectorTest {
            bsp: self,
            flags,
            point,
            vector,
            best_t: maximum_t.max(0.0),
            hit: None,
            last_leaf: NONE,
            last_contents: Contents::Unknown,
            last_plane: NONE,
            nodes_left: self.nodes.len() as i64,
        };
        let t1 = maximum_t.clamp(0.0, 1.0);
        test.recurse(0, 0.0, t1, 0);
        test.hit
    }

    fn plane(&self, index: i32) -> Option<&[f32; 4]> {
        self.planes.get((index & i32::MAX) as usize)
    }

    /// The surface (bsp2d leaf) a projected point is in, from a 2D root.
    fn bsp2d_test_point(&self, point: [f32; 2], mut node: i32) -> i32 {
        let mut depth = 0;
        while node & SIGN == 0 {
            let Some(n) = self.nodes2d.get(node as usize) else { return NONE };
            depth += 1;
            if depth > MAXIMUM_BSP2D_DEPTH {
                return NONE;
            }
            let distance = (n.plane[0] * point[0] + n.plane[1] * point[1]) - n.plane[2];
            node = n.children[usize::from(distance >= 0.0)];
        }
        if node != NONE { node & i32::MAX } else { NONE }
    }

    fn leaf_test_vector(&self, point: [f32; 3], vector: [f32; 3], leaf: usize, plane_index: i32, t: f32, test_surface: bool) -> i32 {
        let Some(leaf) = self.leaves.get(leaf) else { return NONE };
        let first = leaf.first_reference.max(0) as usize;
        for reference in self.references.iter().skip(first).take(leaf.reference_count.max(0) as usize) {
            if (reference.plane_designator & i32::MAX) != plane_index {
                continue;
            }
            let Some(plane) = self.planes.get(plane_index as usize) else { continue };
            let (ai, aj, ak) = (plane[0].abs(), plane[1].abs(), plane[2].abs());
            let projection = if ak >= aj && ak >= ai {
                2
            } else if aj >= ai {
                1
            } else {
                0
            };
            let sign = (plane[projection] > 0.0) != (reference.plane_designator & SIGN != 0);
            let hit_point = [vector[0] * t + point[0], vector[1] * t + point[1], vector[2] * t + point[2]];
            let point2d = project(hit_point, projection, sign);
            let surface = self.bsp2d_test_point(point2d, reference.root);
            if surface >= 0
                && (surface as usize) < self.surfaces.len()
                && (!test_surface || self.surface_test_point(surface as usize, projection, sign, point2d))
            {
                return surface;
            }
        }
        NONE
    }

    /// Whether a projected point is inside a surface's edge ring
    /// (`collision_surface_test_point`; every breakable surface intact).
    fn surface_test_point(&self, surface_index: usize, projection: usize, sign: bool, point: [f32; 2]) -> bool {
        let surface = &self.surfaces[surface_index];
        if !self.ring_continues(surface.first_edge, 0) {
            return false;
        }
        let mut edge_index = surface.first_edge;
        let mut edge_count = 0;
        loop {
            let edge = &self.edges[edge_index as usize];
            let reverse = edge.surfaces[1] == surface_index as i32;
            let p0 = project(self.vertex(edge.vertices[usize::from(reverse)]), projection, sign);
            let p1 = project(self.vertex(edge.vertices[usize::from(!reverse)]), projection, sign);
            let to_point = [point[0] - p0[0], point[1] - p0[1]];
            let along = [p1[0] - p0[0], p1[1] - p0[1]];
            if to_point[0] * along[1] - to_point[1] * along[0] > 0.0 {
                return false;
            }
            edge_index = edge.edges[usize::from(reverse)];
            edge_count += 1;
            if edge_index == surface.first_edge || !self.ring_continues(edge_index, edge_count) {
                return true;
            }
        }
    }

    /// `collision_surface_edge_ring_continues`: an edge of the bsp, within
    /// the most edges a surface has.
    fn ring_continues(&self, edge_index: i32, edge_count: usize) -> bool {
        edge_index >= 0 && (edge_index as usize) < self.edges.len() && edge_count < MAXIMUM_EDGES_PER_SURFACE
    }

    /// A vertex (the origin for one that isn't the bsp's: the game reads
    /// whatever is there; the retail maps have none).
    fn vertex(&self, index: i32) -> [f32; 3] {
        self.vertices.get(index as usize).copied().unwrap_or_default()
    }

    /// `collision_bsp_valid_leaf`: a leaf of the bsp whose references are
    /// all the bsp's.
    fn valid_leaf(&self, index: usize) -> bool {
        let Some(leaf) = self.leaves.get(index) else { return false };
        leaf.reference_count == 0
            || (leaf.first_reference >= 0
                && leaf.reference_count > 0
                && leaf.first_reference as i64 <= self.references.len() as i64 - leaf.reference_count as i64)
    }

    /// Halo's sphere test (`collision_bsp_test_sphere`): the surfaces, edges
    /// and vertices within `radius` of `center`, each list in the order the
    /// game finds them (at most 256 each). Every breakable surface counts as
    /// intact.
    pub fn test_sphere(&self, center: [f32; 3], radius: f32) -> SphereFeatures {
        let mut test = SphereTest {
            bsp: self,
            center,
            radius,
            result: SphereFeatures::default(),
            plane_stack: Vec::with_capacity(MAXIMUM_DEPTH as usize),
            projection: 0,
            sign: false,
            center2d: [0.0; 2],
            nodes_left: self.nodes.len() as i64,
            nodes2d_left: 0,
        };
        test.bsp3d(0, 0);
        test.result
    }
}

/// What a sphere test found (`collision_bsp_test_sphere_result`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SphereFeatures {
    pub surfaces: Vec<i32>,
    pub edges: Vec<i32>,
    pub vertices: Vec<i32>,
    pub leaves: Vec<i32>,
}

impl SphereFeatures {
    /// Whether the sphere touches a surface or an edge (the test's result).
    pub fn touches(&self) -> bool {
        !self.surfaces.is_empty() || !self.edges.is_empty()
    }
}

/// `add_feature`: once each, at most 256.
fn add_feature(list: &mut Vec<i32>, index: i32) {
    if !list.contains(&index) && list.len() < 256 {
        list.push(index);
    }
}

/// `fast_vector_intersects_sphere`: whether the segment from `point` along
/// `vector` passes within `radius` of `center`.
fn vector_intersects_sphere(point: [f32; 3], vector: [f32; 3], center: [f32; 3], radius: f32) -> bool {
    let p = [point[0] - center[0], point[1] - center[1], point[2] - center[2]];
    let c = (p[0] * p[0]) + (p[1] * p[1]) + (p[2] * p[2]) - (radius * radius);
    if c < 0.0 {
        return true;
    }
    let b = vector[0] * p[0] + vector[1] * p[1] + vector[2] * p[2];
    if b >= 0.0 {
        return false;
    }
    let a = vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2];
    let disc = b * b - a * c;
    if disc <= 0.0 {
        return false;
    }
    let neg_a_minus_b = -a - b;
    if neg_a_minus_b < 0.0 {
        return true;
    }
    neg_a_minus_b * neg_a_minus_b < disc
}

struct SphereTest<'a> {
    bsp: &'a CollisionBsp,
    center: [f32; 3],
    radius: f32,
    result: SphereFeatures,
    plane_stack: Vec<i32>,
    projection: usize,
    sign: bool,
    center2d: [f32; 2],
    nodes_left: i64,
    nodes2d_left: i64,
}

impl SphereTest<'_> {
    fn bsp3d(&mut self, mut node_index: i32, mut depth: u16) {
        while node_index & SIGN == 0 {
            // (collision_bsp3d_enter_node)
            self.nodes_left -= 1;
            if node_index as usize >= self.bsp.nodes.len() || depth >= MAXIMUM_DEPTH || self.nodes_left < 0 {
                return;
            }
            depth += 1;
            let node = self.bsp.nodes[node_index as usize];
            let Some(plane) = self.bsp.plane(node.plane).copied() else { return };
            let c = self.center;
            let distance = plane[0] * c[0] + plane[1] * c[1] + plane[2] * c[2] - plane[3];
            let reaches_second_child = distance < self.radius;
            let child_index = distance > -self.radius;
            if child_index && reaches_second_child {
                self.plane_stack.push(node.plane | SIGN);
                self.bsp3d(node.children[0], depth);
                self.plane_stack.pop();
                self.plane_stack.push(node.plane & i32::MAX);
                self.bsp3d(node.children[1], depth);
                self.plane_stack.pop();
                return;
            }
            node_index = node.children[usize::from(child_index)];
        }
        if node_index == NONE || !self.bsp.valid_leaf((node_index & i32::MAX) as usize) {
            return;
        }
        let leaf_index = node_index & i32::MAX;
        let leaf = self.bsp.leaves[leaf_index as usize];
        if self.result.leaves.len() < 256 {
            self.result.leaves.push(leaf_index);
        }
        let first = leaf.first_reference.max(0) as usize;
        for reference_index in first..first + leaf.reference_count.max(0) as usize {
            let reference = self.bsp.references[reference_index];
            if !self.plane_stack.contains(&reference.plane_designator) {
                continue;
            }
            let Some(plane) = self.bsp.planes.get((reference.plane_designator & i32::MAX) as usize).copied() else { continue };
            let c = self.center;
            let plane_distance = -(plane[0] * c[0] + plane[1] * c[1] + plane[2] * c[2] - plane[3]);
            let projected = [
                plane[0] * plane_distance + c[0],
                plane[1] * plane_distance + c[1],
                plane[2] * plane_distance + c[2],
            ];
            let (ai, aj, ak) = (plane[0].abs(), plane[1].abs(), plane[2].abs());
            self.projection = if ak >= aj && ak >= ai {
                2
            } else if aj >= ai {
                1
            } else {
                0
            };
            self.sign = (plane[self.projection] > 0.0) != (reference.plane_designator & SIGN != 0);
            self.center2d = project(projected, self.projection, self.sign);
            self.nodes2d_left = self.bsp.nodes2d.len() as i64;
            self.bsp2d(reference.root, 0);
        }
    }

    fn bsp2d(&mut self, mut child_index: i32, mut depth: u16) {
        while child_index & SIGN == 0 {
            // (collision_bsp2d_enter_node)
            self.nodes2d_left -= 1;
            if child_index as usize >= self.bsp.nodes2d.len() || depth >= MAXIMUM_BSP2D_DEPTH || self.nodes2d_left < 0 {
                return;
            }
            depth += 1;
            let node = self.bsp.nodes2d[child_index as usize];
            let distance = (node.plane[0] * self.center2d[0] + node.plane[1] * self.center2d[1]) - node.plane[2];
            let reaches_first_child = distance <= self.radius;
            let reaches_second_child = distance >= -self.radius;
            if reaches_first_child {
                self.bsp2d(node.children[0], depth);
            }
            if !reaches_second_child {
                return;
            }
            child_index = node.children[1];
        }
        self.surface(child_index & i32::MAX);
    }

    /// `collision_surface_test_sphere`.
    fn surface(&mut self, surface_index: i32) {
        let bsp = self.bsp;
        if surface_index < 0 || surface_index as usize >= bsp.surfaces.len() {
            return;
        }
        let surface = bsp.surfaces[surface_index as usize];
        if !bsp.ring_continues(surface.first_edge, 0) {
            return;
        }
        let ring = || {
            let mut edges = Vec::new();
            let mut edge_index = surface.first_edge;
            let mut edge_count = 0;
            loop {
                edges.push(edge_index);
                let edge = &bsp.edges[edge_index as usize];
                let reverse = edge.surfaces[1] == surface_index;
                edge_index = edge.edges[usize::from(reverse)];
                edge_count += 1;
                if edge_index == surface.first_edge || !bsp.ring_continues(edge_index, edge_count) {
                    return edges;
                }
            }
        };
        let edges = ring();
        let radius_squared = self.radius * self.radius;
        let mut hit_feature = false;
        for &edge_index in &edges {
            let edge = &bsp.edges[edge_index as usize];
            let reverse = edge.surfaces[1] == surface_index;
            let vertex_index = edge.vertices[usize::from(reverse)];
            let v = bsp.vertex(vertex_index);
            let (dx, dy, dz) = (v[0] - self.center[0], v[1] - self.center[1], v[2] - self.center[2]);
            if dx * dx + dy * dy + dz * dz <= radius_squared {
                add_feature(&mut self.result.vertices, vertex_index);
                hit_feature = true;
            }
        }
        for &edge_index in &edges {
            let edge = &bsp.edges[edge_index as usize];
            let reverse = edge.surfaces[1] == surface_index;
            let v0 = bsp.vertex(edge.vertices[usize::from(reverse)]);
            let v1 = bsp.vertex(edge.vertices[usize::from(!reverse)]);
            let vector = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
            if vector_intersects_sphere(v0, vector, self.center, self.radius) {
                add_feature(&mut self.result.edges, edge_index);
                hit_feature = true;
            }
        }
        if !hit_feature {
            for &edge_index in &edges {
                let edge = &bsp.edges[edge_index as usize];
                let reverse = edge.surfaces[1] == surface_index;
                let p0 = project(bsp.vertex(edge.vertices[usize::from(reverse)]), self.projection, self.sign);
                let p1 = project(bsp.vertex(edge.vertices[usize::from(!reverse)]), self.projection, self.sign);
                let v0 = [p0[0] - self.center2d[0], p0[1] - self.center2d[1]];
                let v1 = [p1[0] - self.center2d[0], p1[1] - self.center2d[1]];
                if v0[0] * v1[1] - v0[1] * v1[0] < 0.0 {
                    return;
                }
            }
        }
        add_feature(&mut self.result.surfaces, surface_index);
    }
}

/// `global_projection3d_mappings`: the two axes a point keeps when projected
/// along `projection`, ordered by `sign`.
fn project(p: [f32; 3], projection: usize, sign: bool) -> [f32; 2] {
    const MAPPINGS: [[[usize; 2]; 2]; 3] = [[[2, 1], [1, 2]], [[0, 2], [2, 0]], [[1, 0], [0, 1]]];
    let [x, y] = MAPPINGS[projection][usize::from(sign)];
    [p[x], p[y]]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Contents {
    Unknown,
    Empty,
    SemiEmpty,
    Solid,
}

struct VectorTest<'a> {
    bsp: &'a CollisionBsp,
    flags: u32,
    point: [f32; 3],
    vector: [f32; 3],
    best_t: f32,
    hit: Option<Hit>,
    last_leaf: i32,
    last_contents: Contents,
    last_plane: i32,
    nodes_left: i64,
}

impl VectorTest<'_> {
    fn recurse(&mut self, node_index: i32, t0: f32, t1: f32, depth: u16) -> bool {
        if node_index & SIGN == 0 {
            // (collision_bsp3d_enter_node: a bad node ends the test)
            self.nodes_left -= 1;
            if self.nodes_left < 0 || depth >= MAXIMUM_DEPTH {
                return false;
            }
            let Some(node) = self.bsp.nodes.get(node_index as usize).copied() else { return false };
            let Some(plane) = self.bsp.plane(node.plane).copied() else { return false };
            let distance = plane[0] * self.point[0] + plane[1] * self.point[1] + plane[2] * self.point[2] - plane[3];
            let dot = self.vector[0] * plane[0] + self.vector[1] * plane[1] + self.vector[2] * plane[2];
            let distance0 = dot * t0 + distance;
            let distance1 = dot * t1 + distance;
            let reaches_back = distance0 < 0.0 || distance1 < 0.0;
            let reaches_front = distance0 >= 0.0 || distance1 >= 0.0;
            if reaches_back && reaches_front {
                let front = dot > 0.0;
                let t = -(distance / dot);
                if self.recurse(node.children[usize::from(!front)], t0, t, depth + 1) {
                    return true;
                }
                if self.best_t <= t {
                    return false;
                }
                self.last_plane = node.plane;
                if self.recurse(node.children[usize::from(front)], t, t1, depth + 1) {
                    return true;
                }
            } else if self.recurse(node.children[usize::from(reaches_front)], t0, t1, depth + 1) {
                return true;
            }
            return false;
        }

        // a leaf (NONE, or a bad one, is solid)
        let mut leaf_index = NONE;
        let mut contents = Contents::Solid;
        if node_index != NONE
            && self.bsp.valid_leaf((node_index & i32::MAX) as usize)
            && let Some(leaf) = self.bsp.leaves.get((node_index & i32::MAX) as usize)
        {
            leaf_index = node_index & i32::MAX;
            contents = if leaf.flags & LEAF_CONTAINS_TWO_SIDED != 0 { Contents::SemiEmpty } else { Contents::Empty };
        }
        let open = |c: Contents| c == Contents::Empty || c == Contents::SemiEmpty;
        let mut test_surface = false;
        let test_leaf = if self.flags & flags::FRONT_FACING != 0 && open(self.last_contents) && contents == Contents::Solid {
            self.last_leaf
        } else if self.flags & flags::BACK_FACING != 0 && self.last_contents == Contents::Solid && open(contents) {
            leaf_index
        } else if self.flags & flags::IGNORE_TWO_SIDED == 0
            && self.last_contents == Contents::SemiEmpty
            && contents == Contents::SemiEmpty
        {
            test_surface = true;
            if self.flags & flags::FRONT_FACING != 0 { self.last_leaf } else { leaf_index }
        } else {
            NONE
        };
        if test_leaf != NONE {
            let surface_index =
                self.bsp.leaf_test_vector(self.point, self.vector, test_leaf as usize, self.last_plane, t0, test_surface);
            if surface_index != NONE {
                let surface = self.bsp.surfaces[surface_index as usize];
                let invisible = surface.flags & surface_flags::INVISIBLE != 0;
                let breakable = surface.flags & surface_flags::BREAKABLE != 0;
                if (!invisible || self.flags & flags::IGNORE_INVISIBLE == 0)
                    && (!breakable || self.flags & flags::IGNORE_BREAKABLE == 0)
                {
                    self.best_t = t0;
                    self.hit = Some(Hit {
                        t: t0,
                        surface: surface_index as usize,
                        plane: self.bsp.plane(self.last_plane).copied().unwrap_or_default(),
                        plane_designator: surface.plane_designator,
                        flags: surface.flags,
                        material: surface.material,
                    });
                    return true;
                }
            }
        }
        self.last_leaf = leaf_index;
        self.last_contents = contents;
        false
    }
}
