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

use crate::{bsp::Bsp, layout, model::Matrix4x3, CacheFile, Error, Ptr, Result};

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
pub(crate) struct Edge {
    pub(crate) vertices: [i32; 2],
    pub(crate) edges: [i32; 2],
    pub(crate) surfaces: [i32; 2],
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
    pub(crate) edges: Vec<Edge>,
    pub vertices: Vec<[f32; 3]>,
    /// Per vertex, its first edge.
    pub(crate) vertex_first_edges: Vec<i32>,
    /// Which breakable surfaces still stand (`breakable_surface_flags_get`:
    /// bit i of byte i / 8 for breakable surface i), or `None` to count every
    /// one as standing (a test the game makes with no breakable surfaces).
    pub breakable_surfaces: Option<[u8; 32]>,
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
        let blocks = bsp.root.block(layout::structure_bsp::COLLISION_BSP, layout::collision_bsp::SIZEOF)?;
        let root = blocks.get(0).ok_or_else(|| Error::Format("the bsp has no collision bsp".into()))?;
        Self::from_root(root)
    }

    /// A `collision_bsp` structure's geometry.
    pub fn from_root(root: Ptr<'_>) -> Result<Self> {
        use layout::{bsp2d, bsp2d_node, bsp3d, bsp3d_node, collision_bsp as c, collision_edge, collision_surface, collision_vertex};
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
            out.vertex_first_edges.push(vertex.i32(12)?);
        }
        Ok(out)
    }

    /// Halo's ray test: the first surface (by `flags`) the ray from `point`
    /// along `vector` hits, up to `maximum_t` (in units of `vector`). Broken
    /// breakable surfaces are passed through ([`Self::breakable_surfaces`]).
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
            leaves: None,
        };
        let t1 = maximum_t.clamp(0.0, 1.0);
        test.recurse(0, 0.0, t1, 0);
        test.hit
    }

    /// [`Self::test_vector`], and the leaves the ray passed through up to
    /// the hit, in order (at most 256, the last kept in the 256th place),
    /// as the game records them.
    pub fn test_vector_leaves(&self, flags: u32, point: [f32; 3], vector: [f32; 3], maximum_t: f32) -> (Option<Hit>, Vec<i32>) {
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
            leaves: Some(Vec::new()),
        };
        let t1 = maximum_t.clamp(0.0, 1.0);
        test.recurse(0, 0.0, t1, 0);
        (test.hit, test.leaves.unwrap_or_default())
    }

    /// The leaf a point is in (`bsp3d_test_point`), NONE outside the world.
    pub fn leaf_from_point(&self, point: [f32; 3]) -> i32 {
        let mut node_index = 0i32;
        let mut depth = 0u16;
        loop {
            if node_index < 0 || node_index as usize >= self.nodes.len() || depth >= MAXIMUM_DEPTH {
                return NONE;
            }
            depth += 1;
            let node = self.nodes[node_index as usize];
            let Some(plane) = self.plane(node.plane) else { return NONE };
            let distance = point[0] * plane[0] + point[1] * plane[1] + point[2] * plane[2] - plane[3];
            node_index = node.children[usize::from(distance >= 0.0)];
            if node_index & SIGN != 0 {
                break;
            }
        }
        if node_index != NONE { node_index & i32::MAX } else { NONE }
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
    /// (`collision_surface_test_point`; a broken one has none).
    fn surface_test_point(&self, surface_index: usize, projection: usize, sign: bool, point: [f32; 2]) -> bool {
        let surface = &self.surfaces[surface_index];
        if !self.ring_continues(surface.first_edge, 0) || !self.surface_extant(surface) {
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

    /// Whether its 3D BSP has nodes (a collision model's empty ones are
    /// never tested).
    pub fn has_nodes(&self) -> bool {
        !self.nodes.is_empty()
    }

    /// Whether a surface is there: not a breakable surface that has been
    /// broken.
    pub fn surface_extant(&self, surface: &Surface) -> bool {
        match &self.breakable_surfaces {
            Some(bits) if surface.flags & surface_flags::BREAKABLE != 0 => {
                let index = surface.breakable_surface as usize;
                bits[index / 8] & (1 << (index % 8)) != 0
            }
            _ => true,
        }
    }

    /// `collision_surface_edge_ring_continues`: an edge of the bsp, within
    /// the most edges a surface has.
    pub(crate) fn ring_continues(&self, edge_index: i32, edge_count: usize) -> bool {
        edge_index >= 0 && (edge_index as usize) < self.edges.len() && edge_count < MAXIMUM_EDGES_PER_SURFACE
    }

    /// A vertex (the origin for one that isn't the bsp's: the game reads
    /// whatever is there; the retail maps have none).
    pub(crate) fn vertex(&self, index: i32) -> [f32; 3] {
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

    /// Halo's swept sphere (`collision_bsp_test_pill`, what `collision_test_pill`
    /// and so biped movement uses): the first place a sphere of `radius`
    /// moving from `point` along `vector` touches the structure, up to
    /// `maximum_t`. Every breakable surface counts as intact.
    pub fn test_pill(&self, point: [f32; 3], vector: [f32; 3], radius: f32, maximum_t: f32) -> Option<PillHit> {
        let mut test = PillTest {
            bsp: self,
            point,
            vector,
            radius,
            hit: PillHit {
                t: if maximum_t < 0.0 { 0.0 } else { maximum_t },
                plane: [0.0; 4],
                surface: NONE,
                material: 0,
            },
            plane_stack: Vec::with_capacity(MAXIMUM_DEPTH as usize),
            projection: 0,
            sign: false,
            point2d: [0.0; 2],
            vector2d: [0.0; 2],
            nodes_left: self.nodes.len() as i64,
            nodes2d_left: 0,
        };
        test.bsp3d(0, 0).then_some(test.hit)
    }

    /// Halo's sphere test (`collision_bsp_test_sphere`): the surfaces, edges
    /// and vertices within `radius` of `center`, each list in the order the
    /// game finds them (at most 256 each). Broken breakable surfaces are
    /// passed over ([`Self::breakable_surfaces`]).
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

/// An object's collision geometry (a `coll` tag): per node, its region and
/// a collision BSP per permutation.
#[derive(Debug, Clone, Default)]
pub struct CollisionModel {
    pub nodes: Vec<CollisionNode>,
    /// Its damage materials' material types (`damage_resistance_material`).
    pub material_types: Vec<i16>,
}

#[derive(Debug, Clone, Default)]
pub struct CollisionNode {
    /// Its region (NONE: never tested).
    pub region: i16,
    pub bsps: Vec<CollisionBsp>,
}

/// What a ray hit of an object.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelHit {
    pub hit: Hit,
    pub node: usize,
    pub region: i16,
    pub bsp: usize,
}

/// The most regions an object has (`MAXIMUM_REGIONS_PER_OBJECT`).
const MAXIMUM_REGIONS_PER_OBJECT: i16 = 8;

impl CollisionModel {
    /// A collision model tag.
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{collision_model as m, collision_node as n};
        let root = cache.tag_root(datum, crate::groups::COLLISION_MODEL)?;
        let mut nodes = Vec::new();
        for node in root.block(m::NODES, n::SIZEOF)?.iter() {
            let bsps = node
                .block(n::BSPS, layout::collision_bsp::SIZEOF)?
                .iter()
                .map(CollisionBsp::from_root)
                .collect::<Result<Vec<_>>>()?;
            nodes.push(CollisionNode { region: node.i16(n::REGION_INDEX)?, bsps });
        }
        use layout::{damage_resistance as r, damage_resistance_material as d};
        let material_types = root
            .add(m::RESISTANCE)
            .block(r::MATERIALS, d::SIZEOF)?
            .iter()
            .map(|material| material.i16(d::MATERIAL_TYPE))
            .collect::<Result<_>>()?;
        Ok(CollisionModel { nodes, material_types })
    }

    /// `collision_model_get_material_type`: NONE for no material, one the
    /// model doesn't have, or a type that is none.
    pub fn material_type(&self, material_index: i16) -> i16 {
        if material_index < 0 {
            return NONE as i16;
        }
        match self.material_types.get(material_index as usize) {
            Some(&t) if t == NONE as i16 || (0..NUMBER_OF_MATERIAL_TYPES).contains(&t) => t,
            _ => NONE as i16,
        }
    }

    /// Halo's ray test against an object (`collision_model_test_vector`):
    /// each node's collision BSP for the object's permutation of its
    /// region, in the node's space (`matrices`: the object's node matrices),
    /// the nearest hit winning.
    pub fn test_vector(
        &self,
        region_permutations: &[u8],
        matrices: &[Matrix4x3],
        flags: u32,
        point: [f32; 3],
        vector: [f32; 3],
    ) -> Option<ModelHit> {
        let mut best: Option<ModelHit> = None;
        let mut best_t = f32::MAX;
        // (no more nodes than the object has matrices)
        for (node_index, node) in self.nodes.iter().enumerate().take(matrices.len()) {
            if node.region < 0 || node.region >= MAXIMUM_REGIONS_PER_OBJECT {
                continue;
            }
            let Some(&permutation) = region_permutations.get(node.region as usize) else { continue };
            if node.bsps.is_empty() {
                continue;
            }
            let permutation = (permutation as usize).min(node.bsps.len() - 1);
            let bsp = &node.bsps[permutation];
            if bsp.nodes.is_empty() {
                continue;
            }
            let inverse = matrix_inverse(&matrices[node_index]);
            let local_point = transform_point(&inverse, point);
            let local_vector = transform_vector(&inverse, vector);
            if let Some(hit) = bsp.test_vector(flags, local_point, local_vector, best_t) {
                best_t = hit.t;
                best = Some(ModelHit { hit, node: node_index, region: node.region, bsp: permutation });
            }
        }
        best
    }
}

/// `matrix4x3_inverse`, as the game computes it.
pub fn matrix_inverse(m: &Matrix4x3) -> Matrix4x3 {
    if m.scale == 0.0 {
        return Matrix4x3 { scale: 0.0, forward: [0.0; 3], left: [0.0; 3], up: [0.0; 3], position: [0.0; 3] };
    }
    let (mut x, mut y, mut z) = (-m.position[0], -m.position[1], -m.position[2]);
    let scale = if m.scale != 1.0 {
        let scale = 1.0 / m.scale;
        x *= scale;
        y *= scale;
        z *= scale;
        scale
    } else {
        1.0
    };
    // (the rotation transposed: n[0] forward, n[1] left, n[2] up)
    let forward = [m.forward[0], m.left[0], m.up[0]];
    let left = [m.forward[1], m.left[1], m.up[1]];
    let up = [m.forward[2], m.left[2], m.up[2]];
    let position = [
        (x * forward[0]) + y * left[0] + (z * up[0]),
        (x * forward[1]) + y * left[1] + (z * up[1]),
        (x * forward[2]) + y * left[2] + (z * up[2]),
    ];
    Matrix4x3 { scale, forward, left, up, position }
}

/// `matrix4x3_transform_point`.
pub fn transform_point(m: &Matrix4x3, p: [f32; 3]) -> [f32; 3] {
    let [mut x, mut y, mut z] = p;
    if m.scale != 1.0 {
        x *= m.scale;
        y *= m.scale;
        z *= m.scale;
    }
    [
        m.up[0] * z + m.left[0] * y + m.forward[0] * x + m.position[0],
        m.up[1] * z + m.left[1] * y + m.forward[1] * x + m.position[1],
        m.up[2] * z + m.left[2] * y + m.forward[2] * x + m.position[2],
    ]
}

/// `matrix4x3_transform_vector`.
pub fn transform_vector(m: &Matrix4x3, v: [f32; 3]) -> [f32; 3] {
    let [mut i, mut j, mut k] = v;
    if m.scale != 1.0 {
        i *= m.scale;
        j *= m.scale;
        k *= m.scale;
    }
    [
        i * m.forward[0] + j * m.left[0] + k * m.up[0],
        i * m.forward[1] + j * m.left[1] + k * m.up[1],
        i * m.forward[2] + j * m.left[2] + k * m.up[2],
    ]
}

/// A structure BSP's clusters: each leaf's cluster, and the portals between
/// clusters, which the game's broad phase walks.
#[derive(Debug, Clone, Default)]
pub struct StructureClusters {
    pub leaf_clusters: Vec<i16>,
    /// Per cluster, its portals.
    pub cluster_portals: Vec<Vec<i16>>,
    pub portals: Vec<Portal>,
    /// Per cluster, its water surface (`collision_cluster_fog_plane`), if it
    /// has one.
    pub cluster_water: Vec<Option<WaterPlane>>,
    /// Per cluster, how `scenario_location_water_depth` finds its water.
    pub cluster_water_depth: Vec<WaterDepth>,
    /// Per collision material, its material type
    /// (`runtime_physics_material_type`).
    pub collision_material_types: Vec<i16>,
}

/// `_material_water`.
pub const MATERIAL_WATER: i16 = 28;
/// `NUMBER_OF_MATERIAL_TYPES`.
const NUMBER_OF_MATERIAL_TYPES: i16 = 33;

/// A cluster's water surface: its fog plane, moved by the fog's plane
/// distance, and the material type of its top.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterPlane {
    pub plane: [f32; 4],
    pub material_type: i16,
}

/// How deep a point in a cluster is under water
/// (`scenario_location_water_depth`): no water, all water (a water fog
/// with no plane), or below a water fog's plane moved by its plane
/// distance.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum WaterDepth {
    #[default]
    None,
    Everywhere,
    Plane { plane: [f32; 4], plane_distance: f32 },
}

#[derive(Debug, Clone, Default)]
pub struct Portal {
    pub clusters: [i16; 2],
    pub plane: i32,
    pub centroid: [f32; 3],
    pub bounding_radius: f32,
    pub vertices: Vec<[f32; 3]>,
}

/// `MAXIMUM_VERTICES_PER_CLUSTER_PORTAL`.
const MAXIMUM_VERTICES_PER_CLUSTER_PORTAL: usize = 128;
/// `MAXIMUM_CLUSTERS_PER_STRUCTURE`.
const MAXIMUM_CLUSTERS_PER_STRUCTURE: usize = 512;

impl StructureClusters {
    pub fn load(cache: &CacheFile, bsp: &Bsp<'_>) -> Result<Self> {
        use layout::{structure_bsp as s, structure_cluster, structure_fog_palette_entry, structure_fog_plane, structure_fog_region, structure_leaf};
        let mut out = StructureClusters::default();
        // water: cluster -> fog plane -> region -> palette entry -> fog
        let fog_planes = bsp.root.block(s::FOG_PLANES, structure_fog_plane::SIZEOF)?;
        let fog_regions = bsp.root.block(s::FOG_REGIONS, structure_fog_region::SIZEOF)?;
        let fog_palette = bsp.root.block(s::FOG_PALETTE, structure_fog_palette_entry::SIZEOF)?;
        let water = |fog_reference: i16| -> Result<Option<WaterPlane>> {
            if fog_reference == -1 || fog_reference as u16 & 0x8000 == 0 {
                return Ok(None);
            }
            let Some(plane) = fog_planes.get((fog_reference as u16 & 0x7fff) as usize) else { return Ok(None) };
            let material_type = plane.i16(structure_fog_plane::RUNTIME_MATERIAL_TYPE)?;
            if material_type == -1 {
                return Ok(None);
            }
            let region = plane.i16(structure_fog_plane::REGION_INDEX)?;
            let Some(region) = fog_regions.get(region.max(0) as usize).filter(|_| region >= 0) else { return Ok(None) };
            let palette = region.i16(structure_fog_region::FOG_PALETTE_INDEX)?;
            let Some(entry) = fog_palette.get(palette.max(0) as usize).filter(|_| palette >= 0) else { return Ok(None) };
            let Some(fog) = entry.reference(structure_fog_palette_entry::FOG)?.datum else { return Ok(None) };
            // (fog_definition.plane_distance)
            let plane_distance = cache.tag_root(fog, crate::groups::FOG)?.f32(0x74)?;
            let p = structure_fog_plane::PLANE;
            let mut plane_value = [plane.f32(p)?, plane.f32(p + 4)?, plane.f32(p + 8)?, plane.f32(p + 12)?];
            plane_value[3] -= plane_distance;
            Ok(Some(WaterPlane { plane: plane_value, material_type }))
        };
        // (scenario_location_water_depth: cluster -> fog plane (or a region)
        // -> region -> palette entry -> a fog that is water)
        let water_depth = |fog_reference: i16| -> Result<WaterDepth> {
            if fog_reference == -1 {
                return Ok(WaterDepth::None);
            }
            let reference = fog_reference as u16;
            let (region, plane) = if reference & 0x8000 != 0 {
                let Some(plane) = fog_planes.get((reference & 0x7fff) as usize) else { return Ok(WaterDepth::None) };
                let p = structure_fog_plane::PLANE;
                (plane.i16(structure_fog_plane::REGION_INDEX)?, Some([plane.f32(p)?, plane.f32(p + 4)?, plane.f32(p + 8)?, plane.f32(p + 12)?]))
            } else {
                ((reference & 0x7fff) as i16, None)
            };
            // (scenario_fog_region_get_fog_index)
            let Some(region) = fog_regions.get(region.max(0) as usize).filter(|_| region >= 0) else { return Ok(WaterDepth::None) };
            let palette = region.i16(structure_fog_region::FOG_PALETTE_INDEX)?;
            let Some(entry) = fog_palette.get(palette.max(0) as usize).filter(|_| palette >= 0) else { return Ok(WaterDepth::None) };
            let Some(fog) = entry.reference(structure_fog_palette_entry::FOG)?.datum else { return Ok(WaterDepth::None) };
            let fog = cache.tag_root(fog, crate::groups::FOG)?;
            if fog.u8(layout::fog_definition::FLAGS)? & 1 == 0 {
                return Ok(WaterDepth::None);
            }
            Ok(match plane {
                Some(plane) => WaterDepth::Plane { plane, plane_distance: fog.f32(layout::fog_definition::PLANE_DISTANCE)? },
                None => WaterDepth::Everywhere,
            })
        };
        for leaf in bsp.root.block(s::LEAVES, structure_leaf::SIZEOF)?.iter() {
            out.leaf_clusters.push(leaf.i16(structure_leaf::CLUSTER_INDEX)?);
        }
        // (structure_cluster_graph: its portal indices at 0x5c)
        for cluster in bsp.root.block(s::CLUSTERS, structure_cluster::SIZEOF)?.iter() {
            let portals = cluster.block(0x5c, 2)?.iter().map(|p| p.i16(0)).collect::<Result<Vec<_>>>()?;
            out.cluster_portals.push(portals);
            out.cluster_water.push(water(cluster.i16(structure_cluster::FOG_REFERENCE)?)?);
            out.cluster_water_depth.push(water_depth(cluster.i16(structure_cluster::FOG_REFERENCE)?)?);
        }
        for material in bsp.root.block(s::COLLISION_MATERIALS, layout::structure_collision_material::SIZEOF)?.iter() {
            out.collision_material_types.push(material.i16(layout::structure_collision_material::RUNTIME_PHYSICS_MATERIAL_TYPE)?);
        }
        // (structure_cluster_portal, 0x40 bytes: its vertices at 0x34)
        for portal in bsp.root.block(s::CLUSTER_PORTALS, 0x40)?.iter() {
            out.portals.push(Portal {
                clusters: [portal.i16(0)?, portal.i16(2)?],
                plane: portal.i32(4)?,
                centroid: portal.vec3(8)?,
                bounding_radius: portal.f32(0x14)?,
                vertices: portal.block(0x34, 12)?.iter().map(|v| v.vec3(0)).collect::<Result<Vec<_>>>()?,
            });
        }
        Ok(out)
    }

    /// `collision_structure_material_type`: NONE for no material, one the
    /// structure doesn't have, or a type that is none.
    pub fn structure_material_type(&self, material_index: i16) -> i16 {
        if material_index < 0 {
            return NONE as i16;
        }
        match self.collision_material_types.get(material_index as usize) {
            Some(&t) if t == NONE as i16 || (0..NUMBER_OF_MATERIAL_TYPES).contains(&t) => t,
            _ => NONE as i16,
        }
    }

    /// `scenario_location_water_depth`: how deep a point in a cluster is
    /// under water (the most negative float for no water).
    pub fn water_depth(&self, cluster: i16, point: [f32; 3]) -> f32 {
        if cluster == NONE as i16 {
            return f32::MIN;
        }
        match self.cluster_water_depth.get(cluster as usize).copied().unwrap_or_default() {
            WaterDepth::None => f32::MIN,
            WaterDepth::Everywhere => f32::MAX,
            WaterDepth::Plane { plane, plane_distance } => {
                // (plane3d_distance_to_point)
                let distance = point[0] * plane[0] + point[1] * plane[1] + point[2] * plane[2] - plane[3];
                -(distance + plane_distance)
            }
        }
    }

    /// `scenario_location_from_point`: the leaf and cluster a point is in.
    pub fn location_from_point(&self, collision: &CollisionBsp, point: [f32; 3]) -> (i32, i16) {
        let leaf = collision.leaf_from_point(point);
        if leaf == NONE {
            return (NONE, NONE as i16);
        }
        match self.leaf_clusters.get((leaf & i32::MAX) as usize) {
            Some(&cluster) => (leaf, cluster),
            None => (NONE, NONE as i16),
        }
    }

    /// `collision_cluster_index_from_leaf`.
    pub fn cluster_from_leaf(&self, leaf: i32) -> i16 {
        if leaf == NONE {
            return NONE as i16;
        }
        self.leaf_clusters.get((leaf & i32::MAX) as usize).copied().unwrap_or(NONE as i16)
    }

    /// `structure_clusters_in_sphere`: the clusters a sphere reaches from
    /// `cluster` through portals, in the order the game finds them (at most
    /// `maximum`).
    pub fn clusters_in_sphere(&self, collision: &CollisionBsp, cluster: i16, center: [f32; 3], radius: f32, maximum: usize) -> Vec<i16> {
        let mut out = Vec::new();
        if cluster < 0 || cluster as usize >= self.cluster_portals.len() || cluster as usize >= MAXIMUM_CLUSTERS_PER_STRUCTURE {
            return out;
        }
        if radius > 0.0 {
            let mut marked = vec![false; self.cluster_portals.len()];
            self.flood(collision, cluster, center, radius, maximum, &mut marked, &mut out);
        } else if maximum > 0 {
            out.push(cluster);
        }
        out
    }

    fn flood(&self, collision: &CollisionBsp, cluster: i16, center: [f32; 3], radius: f32, maximum: usize, marked: &mut [bool], out: &mut Vec<i16>) {
        if out.len() < maximum {
            out.push(cluster);
        }
        marked[cluster as usize] = true;
        for &portal_index in &self.cluster_portals[cluster as usize] {
            if portal_index < 0 || portal_index as usize >= self.portals.len() {
                continue;
            }
            let portal = &self.portals[portal_index as usize];
            let adjacent = if portal.clusters[0] == cluster { portal.clusters[1] } else { portal.clusters[0] };
            if (adjacent as usize) < self.cluster_portals.len()
                && adjacent >= 0
                && !marked[adjacent as usize]
                && self.sphere_intersects_portal(collision, portal, center, radius)
            {
                self.flood(collision, adjacent, center, radius, maximum, marked, out);
            }
        }
    }

    /// `sphere_intersects_cluster_portal`.
    fn sphere_intersects_portal(&self, collision: &CollisionBsp, portal: &Portal, point: [f32; 3], radius: f32) -> bool {
        if portal.plane < 0 || portal.plane as usize >= collision.planes.len() {
            return false;
        }
        if portal.vertices.len() > MAXIMUM_VERTICES_PER_CLUSTER_PORTAL {
            return false;
        }
        let plane = collision.planes[portal.plane as usize];
        let plane_distance = point[0] * plane[0] + point[1] * plane[1] + point[2] * plane[2] - plane[3];
        let d = sub3(point, portal.centroid);
        let reach = portal.bounding_radius + radius;
        if !(plane_distance.abs() < radius && dot3(d, d) < reach * reach) {
            return false;
        }
        let normal = [plane[0], plane[1], plane[2]];
        let (ai, aj, ak) = (normal[0].abs(), normal[1].abs(), normal[2].abs());
        let projection = if ak >= aj && ak >= ai {
            2
        } else if aj >= ai {
            1
        } else {
            0
        };
        let sign = normal[projection] > 0.0;
        let t = -plane_distance;
        let projected = [normal[0] * t + point[0], normal[1] * t + point[1], normal[2] * t + point[2]];
        let center2d = project(projected, projection, sign);
        let points: Vec<[f32; 2]> = portal.vertices.iter().map(|&v| project(v, projection, sign)).collect();
        convex_hull2d_test_circle(&points, center2d, (radius * radius - plane_distance * plane_distance).sqrt())
    }
}

/// `convex_hull2d_test_circle`: whether a circle is inside or touches every
/// edge's inner side of a convex polygon.
fn convex_hull2d_test_circle(points: &[[f32; 2]], center: [f32; 2], radius: f32) -> bool {
    let radius_squared = radius * radius;
    let count = points.len();
    for index in 0..count {
        let next = if index + 1 < count { index + 1 } else { 0 };
        let edge = [points[next][0] - points[index][0], points[next][1] - points[index][1]];
        let offset = [center[0] - points[index][0], center[1] - points[index][1]];
        let mut edge_length_squared = edge[0] * edge[0];
        edge_length_squared += edge[1] * edge[1];
        if edge_length_squared != 0.0 {
            let cross = offset[0] * edge[1] - offset[1] * edge[0];
            if cross > 0.0 && cross * cross > edge_length_squared * radius_squared {
                return false;
            }
        }
    }
    true
}

/// An object as the broad phase sees it (the game's runtime state).
#[derive(Debug, Clone)]
pub struct WorldObject {
    pub index: u32,
    /// `object_type` (0 biped ... 11 placeholder).
    pub kind: i16,
    /// The object's `flags` (bit 0 invisible, 11 connected to the map, 25
    /// has a collision model).
    pub flags: u32,
    pub bounding_center: [f32; 3],
    pub bounding_radius: f32,
    /// Its parent (`u32::MAX` for none).
    pub parent: u32,
    /// Its location's cluster (NONE outside the map).
    pub location_cluster: i16,
    pub region_permutations: [u8; 8],
    pub matrices: Vec<Matrix4x3>,
    /// Its collision model, if it has one.
    pub model: Option<std::sync::Arc<CollisionModel>>,
    /// A vehicle's physics shape where it is, if it has one (tested
    /// instead of its collision model with `test_flags::USE_VEHICLE_PHYSICS`).
    pub physics: Option<crate::vehicle_physics::PhysicsInstance>,
}

/// What a full ray test hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionKind {
    Structure,
    Media,
    Object,
}

/// `collision_result`: what the game's full ray test reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollisionResult {
    /// What it hit, if anything.
    pub kind: Option<CollisionKind>,
    /// How far along the vector (1 for no hit).
    pub t: f32,
    /// Where (the vector's end for no hit).
    pub point: [f32; 3],
    /// The plane hit, facing the ray's start.
    pub plane: [f32; 4],
    pub object: Option<u32>,
    pub node: i16,
    pub bsp: i16,
    pub surface: i32,
    /// The leaf and cluster the ray ended in (NONE outside the world).
    pub location_leaf: i32,
    pub location_cluster: i16,
    /// The material type hit (NONE for none).
    pub material_type: i16,
    /// The surface's flags (`collision_surface_flags`).
    pub surface_flags: u8,
}

/// `matrix4x3_transform_plane`.
pub fn transform_plane(m: &Matrix4x3, plane: [f32; 4]) -> [f32; 4] {
    let [i, j, k] = [plane[0], plane[1], plane[2]];
    let n = [
        i * m.forward[0] + j * m.left[0] + k * m.up[0],
        i * m.forward[1] + j * m.left[1] + k * m.up[1],
        i * m.forward[2] + j * m.left[2] + k * m.up[2],
    ];
    let d = plane[3] * m.scale + dot3(m.position, n);
    [n[0], n[1], n[2], d]
}

/// `plane3d_negate`.
fn negate_plane(p: [f32; 4]) -> [f32; 4] {
    [-p[0], -p[1], -p[2], -p[3]]
}

/// What the game's full ray test found (`collision_test_vector`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldHit {
    pub t: f32,
    /// The object hit, or `None` for the structure.
    pub object: Option<u32>,
    pub node: i16,
    pub bsp: i16,
    pub surface: i32,
}

const OBJECT_INVISIBLE: u32 = 1 << 0;
const OBJECT_CONNECTED_TO_MAP: u32 = 1 << 11;
const OBJECT_HAS_COLLISION_MODEL: u32 = 1 << 25;
/// `collision_test_flags`: the structure and objects, and each object type.
pub mod test_flags {
    pub const STRUCTURE: u32 = 1 << 5;
    pub const MEDIA: u32 = 1 << 6;
    pub const OBJECTS: u32 = 1 << 7;
    pub const TRY_TO_KEEP_LOCATION_VALID: u32 = 1 << 20;
    /// Vehicles by their physics' mass points, not their collision models.
    pub const USE_VEHICLE_PHYSICS: u32 = 1 << 22;
    pub const FIRST_OBJECT_TYPE: u32 = 8;
    pub const ALL_OBJECT_TYPES: u32 = ((1 << 12) - 1) << FIRST_OBJECT_TYPE;
}

/// The structure and the objects in it, for the game's full ray test.
pub struct World<'a> {
    pub collision: &'a CollisionBsp,
    pub clusters: &'a StructureClusters,
    pub objects: &'a [WorldObject],
}

impl World<'_> {
    /// Halo's ray test against the structure, objects and water
    /// (`collision_test_vector`): the nearest hit as the game finds it, or
    /// `None`. Broken breakable surfaces are passed through
    /// ([`CollisionBsp::breakable_surfaces`]).
    pub fn test_vector(&self, flags: u32, point: [f32; 3], vector: [f32; 3], ignore_object: u32) -> Option<WorldHit> {
        let result = self.test(flags, point, vector, ignore_object);
        result.kind.map(|_| WorldHit { t: result.t, object: result.object, node: result.node, bsp: result.bsp, surface: result.surface })
    }

    /// The whole of `collision_test_vector`'s result (no
    /// `_collision_test_try_to_keep_location_valid_bit`).
    pub fn test(&self, mut flags: u32, point: [f32; 3], vector: [f32; 3], ignore_object: u32) -> CollisionResult {
        if flags & (flags::FRONT_FACING | flags::BACK_FACING) == 0 {
            flags |= flags::FRONT_FACING | flags::BACK_FACING;
        }
        let bsp_flags =
            flags & (flags::FRONT_FACING | flags::BACK_FACING | flags::IGNORE_TWO_SIDED | flags::IGNORE_INVISIBLE | flags::IGNORE_BREAKABLE);
        let mut result = CollisionResult {
            kind: None,
            t: 1.0,
            point: [0.0; 3],
            plane: [0.0; 4],
            object: None,
            node: NONE as i16,
            bsp: NONE as i16,
            surface: NONE,
            location_leaf: NONE,
            location_cluster: NONE as i16,
            material_type: NONE as i16,
            surface_flags: 0,
        };
        let mut best: Option<WorldHit> = None;
        let mut t = 1.0f32;
        let (structure_hit, leaves) = self.collision.test_vector_leaves(bsp_flags, point, vector, f32::MAX);
        if let Some(hit) = structure_hit
            && flags & test_flags::STRUCTURE != 0
        {
            t = hit.t;
            best = Some(WorldHit { t, object: None, node: NONE as i16, bsp: NONE as i16, surface: hit.surface as i32 });
            result.kind = Some(CollisionKind::Structure);
            result.plane = if hit.plane_designator & SIGN != 0 { negate_plane(hit.plane) } else { hit.plane };
            result.material_type = self.clusters.structure_material_type(hit.material);
            result.surface_flags = hit.flags;
        }
        if let (Some(&first), Some(&last)) = (leaves.first(), leaves.last()) {
            let _ = first;
            result.location_leaf = last;
            result.location_cluster = self.clusters.cluster_from_leaf(last);
        }
        // water, in the cluster the ray ended in
        if flags & test_flags::MEDIA != 0
            && result.location_cluster >= 0
            && let Some(Some(water)) = self.clusters.cluster_water.get(result.location_cluster as usize)
        {
            let plane = water.plane;
            let point_distance = point[0] * plane[0] + point[1] * plane[1] + point[2] * plane[2] - plane[3];
            let vector_dot = plane[0] * vector[0] + plane[1] * vector[1] + plane[2] * vector[2];
            if (point_distance > 0.0) != (vector_dot > 0.0)
                && point_distance.abs() < vector_dot.abs()
                && vector_dot.abs() >= REAL_EPSILON
            {
                let water_t = -(point_distance / vector_dot);
                if water_t < t {
                    t = water_t;
                    result.kind = Some(CollisionKind::Media);
                    result.plane = if point_distance < 0.0 { negate_plane(plane) } else { plane };
                    result.material_type = if point_distance < 0.0 {
                        MATERIAL_WATER
                    } else if (0..NUMBER_OF_MATERIAL_TYPES).contains(&water.material_type) {
                        water.material_type
                    } else {
                        NONE as i16
                    };
                    best = Some(WorldHit { t, object: None, node: NONE as i16, bsp: NONE as i16, surface: NONE });
                }
            }
        }
        if flags & test_flags::OBJECTS != 0 && !leaves.is_empty() {
            if flags & test_flags::ALL_OBJECT_TYPES == 0 {
                flags |= test_flags::ALL_OBJECT_TYPES;
            }
            // (who is in which cluster: the game registers each connected
            // object with a collision model in the clusters its bounding
            // sphere reaches)
            let mut members: std::collections::HashMap<i16, Vec<usize>> = Default::default();
            for (index, object) in self.objects.iter().enumerate() {
                if object.parent != u32::MAX
                    || object.flags & OBJECT_CONNECTED_TO_MAP == 0
                    || object.flags & OBJECT_HAS_COLLISION_MODEL == 0
                {
                    continue;
                }
                for cluster in self.clusters.clusters_in_sphere(
                    self.collision,
                    object.location_cluster,
                    object.bounding_center,
                    object.bounding_radius,
                    64,
                ) {
                    members.entry(cluster).or_default().push(index);
                }
            }
            let mut marked_clusters = std::collections::HashSet::new();
            let mut marked_objects = std::collections::HashSet::new();
            for &leaf in &leaves {
                let cluster = self.clusters.cluster_from_leaf(leaf);
                if cluster < 0 || !marked_clusters.insert(cluster) {
                    continue;
                }
                for &index in members.get(&cluster).map(Vec::as_slice).unwrap_or(&[]) {
                    if marked_objects.insert(index) {
                        self.object_test_vector(index, flags, bsp_flags, point, vector, ignore_object, &mut t, &mut best, &mut result);
                    }
                }
            }
        }
        match best {
            Some(hit) => {
                result.t = hit.t;
                result.object = hit.object;
                result.node = hit.node;
                result.bsp = hit.bsp;
                result.surface = hit.surface;
            }
            None => {
                result.kind = None;
                result.t = 1.0;
            }
        }
        // (COLLISION_POINT_FROM_LINE3D)
        let point_at = |t: f32| [vector[0] * t + point[0], vector[1] * t + point[1], vector[2] * t + point[2]];
        result.point = point_at(result.t);
        // (the hit point in the leaf the ray found, if it can be)
        if flags & test_flags::TRY_TO_KEEP_LOCATION_VALID != 0
            && result.kind.is_some()
            && result.location_leaf != NONE
            && self.collision.leaf_from_point(result.point) != result.location_leaf
        {
            const NUDGE: f32 = 0.00024414062;
            let n = [result.plane[0], result.plane[1], result.plane[2]];
            result.point = [n[0] * NUDGE + result.point[0], n[1] * NUDGE + result.point[1], n[2] * NUDGE + result.point[2]];
            (result.location_leaf, result.location_cluster) = self.clusters.location_from_point(self.collision, result.point);
            if result.location_leaf == NONE {
                let vector_dot = dot3(vector, n);
                // (C's fabs is the double one)
                let step = if vector_dot != 0.0 { (NUDGE as f64 / (vector_dot as f64).abs()) as f32 } else { 0.03125 };
                loop {
                    let t = result.t - step;
                    result.t = if t < 0.0 { 0.0 } else { t };
                    result.point = point_at(result.t);
                    (result.location_leaf, result.location_cluster) = self.clusters.location_from_point(self.collision, result.point);
                    if result.t <= 0.0 || result.location_leaf != NONE {
                        break;
                    }
                }
            }
        }
        result
    }

    /// `object_test_vector`: an object, then (if its bounding sphere is
    /// reached) its children.
    #[allow(clippy::too_many_arguments)]
    fn object_test_vector(
        &self,
        index: usize,
        flags: u32,
        bsp_flags: u32,
        point: [f32; 3],
        vector: [f32; 3],
        ignore_object: u32,
        t: &mut f32,
        best: &mut Option<WorldHit>,
        result: &mut CollisionResult,
    ) {
        let object = &self.objects[index];
        if object.index == ignore_object
            || object.flags & OBJECT_INVISIBLE != 0
            || object.kind < 0
            || flags & (1 << (object.kind as u32 + test_flags::FIRST_OBJECT_TYPE)) == 0
            || !vector_intersects_sphere(point, vector, object.bounding_center, object.bounding_radius)
        {
            return;
        }
        const OBJECT_TYPE_VEHICLE: i16 = 1;
        if object.kind == OBJECT_TYPE_VEHICLE && flags & test_flags::USE_VEHICLE_PHYSICS != 0 {
            if let Some(physics) = &object.physics
                && let Some((hit_t, plane)) = physics.test_vector(point, vector)
                && *t > hit_t
            {
                *t = hit_t;
                *best = Some(WorldHit { t: hit_t, object: Some(object.index), node: NONE as i16, bsp: NONE as i16, surface: NONE });
                result.kind = Some(CollisionKind::Object);
                result.plane = plane;
                result.material_type = NONE as i16;
                result.surface_flags = 0;
            }
        } else if let Some(model) = &object.model
            && let Some(hit) = model.test_vector(&object.region_permutations, &object.matrices, bsp_flags, point, vector)
            && *t > hit.hit.t
        {
            *t = hit.hit.t;
            *best = Some(WorldHit {
                t: hit.hit.t,
                object: Some(object.index),
                node: hit.node as i16,
                bsp: hit.bsp as i16,
                surface: hit.hit.surface as i32,
            });
            result.kind = Some(CollisionKind::Object);
            result.material_type = model.material_type(hit.hit.material);
            result.surface_flags = hit.hit.flags;
            let plane = transform_plane(&object.matrices[hit.node], hit.hit.plane);
            result.plane = if hit.hit.plane_designator & SIGN != 0 { negate_plane(plane) } else { plane };
        }
        for (child, candidate) in self.objects.iter().enumerate() {
            if candidate.parent == object.index {
                self.object_test_vector(child, flags, bsp_flags, point, vector, ignore_object, t, best, result);
            }
        }
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

/// Where a swept sphere first touched the structure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PillHit {
    /// How far along the sweep.
    pub t: f32,
    /// The plane touched; for an edge, the normal from the edge to the
    /// sphere's centre, with `d` the largest f32.
    pub plane: [f32; 4],
    pub surface: i32,
    pub material: i16,
}

const BSP3D_TEST_PILL_EPSILON: f32 = 1.0 / 4096.0;
const BSP2D_TEST_PILL_EPSILON: f32 = 1.0 / 8192.0;
const REAL_EPSILON: f32 = 0.0001;

/// `PIN(n, 0, 1)`, as the macro compares (a NaN stays a NaN).
fn pin01(n: f32) -> f32 {
    if n < 0.0 {
        0.0
    } else if n > 1.0 {
        1.0
    } else {
        n
    }
}

fn sub3(b: [f32; 3], a: [f32; 3]) -> [f32; 3] {
    [b[0] - a[0], b[1] - a[1], b[2] - a[2]]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `sphere_test_vector`: when a point moving along `vector` comes within
/// `radius` of `center`.
fn sphere_test_vector(center: [f32; 3], radius: f32, point: [f32; 3], vector: [f32; 3]) -> Option<f32> {
    let delta = sub3(center, point);
    let outside = dot3(delta, delta) - radius * radius;
    if outside <= 0.0 {
        return Some(0.0);
    }
    let projection = dot3(delta, vector);
    if projection > 0.0 {
        let length_squared = dot3(vector, vector);
        let discriminant = projection * projection - length_squared * outside;
        if discriminant >= 0.0 {
            let hit_t = (projection - discriminant.sqrt()) / length_squared;
            if hit_t <= 1.0 {
                return Some(hit_t);
            }
        }
    }
    None
}

/// `pill_test_vector`: when a point moving along `vector` comes within
/// `radius` of the segment from `base` along `edge`: (t, where along the
/// edge).
fn pill_test_vector(base: [f32; 3], edge: [f32; 3], radius: f32, point: [f32; 3], vector: [f32; 3]) -> Option<(f32, f32)> {
    let delta = sub3(point, base);
    let edge_length_squared = dot3(edge, edge);
    let edge_dot_vector = dot3(edge, vector);
    let vector_length_squared = dot3(vector, vector);
    let denominator = vector_length_squared * edge_length_squared - edge_dot_vector * edge_dot_vector;
    if denominator == 0.0 {
        return None;
    }
    let delta_dot_edge = dot3(delta, edge);
    let delta_dot_vector = dot3(delta, vector);
    let linear = delta_dot_edge * edge_dot_vector - delta_dot_vector * edge_length_squared;
    let constant = (dot3(delta, delta) - radius * radius) * edge_length_squared - delta_dot_edge * delta_dot_edge;
    let discriminant = linear * linear - constant * denominator;
    if discriminant < 0.0 {
        return None;
    }
    let root = discriminant.sqrt();
    let inverse = 1.0 / denominator;
    let t0 = -((root + linear) * inverse);
    if t0 > 1.0 {
        return None;
    }
    let t1 = -((linear - root) * inverse);
    if t1 < 0.0 {
        return None;
    }
    // (FLOOR(t0, 0))
    let hit_t = if t0 < 0.0 { 0.0 } else { t0 };
    let edge_projection = edge_dot_vector * hit_t + delta_dot_edge;
    if edge_projection < 0.0 {
        sphere_test_vector(base, radius, point, vector).map(|t| (t, 0.0))
    } else if edge_projection > edge_length_squared {
        let end = [base[0] + edge[0], base[1] + edge[1], base[2] + edge[2]];
        sphere_test_vector(end, radius, point, vector).map(|t| (t, 1.0))
    } else {
        Some((hit_t, edge_projection / edge_length_squared))
    }
}

struct PillTest<'a> {
    bsp: &'a CollisionBsp,
    point: [f32; 3],
    vector: [f32; 3],
    radius: f32,
    hit: PillHit,
    plane_stack: Vec<i32>,
    projection: usize,
    sign: bool,
    point2d: [f32; 2],
    vector2d: [f32; 2],
    nodes_left: i64,
    nodes2d_left: i64,
}

impl PillTest<'_> {
    fn bsp3d(&mut self, child_index: i32, depth: u16) -> bool {
        let bsp = self.bsp;
        let mut hit = false;
        if child_index & SIGN == 0 {
            // (collision_bsp3d_enter_node)
            if child_index as usize >= bsp.nodes.len() || depth >= MAXIMUM_DEPTH {
                return false;
            }
            self.nodes_left -= 1;
            if self.nodes_left < 0 {
                return false;
            }
            let node = bsp.nodes[child_index as usize];
            let Some(plane) = bsp.plane(node.plane).copied() else { return false };
            let normal = [plane[0], plane[1], plane[2]];
            let distance0 = dot3(self.point, normal) - plane[3];
            let dot = dot3(self.vector, normal);
            let distance1 = dot + distance0;
            let r = self.radius;
            let reaches_back = distance0 <= r + BSP3D_TEST_PILL_EPSILON || distance1 <= r + BSP3D_TEST_PILL_EPSILON;
            let reaches_front = distance0 >= -r - BSP3D_TEST_PILL_EPSILON || distance1 >= -r - BSP3D_TEST_PILL_EPSILON;
            if reaches_back && reaches_front {
                let front = dot > 0.0;
                self.plane_stack.push(if front { node.plane | SIGN } else { node.plane & i32::MAX });
                if self.bsp3d(node.children[usize::from(!front)], depth + 1) {
                    hit = true;
                }
                self.plane_stack.pop();
                if self.bsp3d(node.children[usize::from(front)], depth + 1) {
                    hit = true;
                }
            } else if self.bsp3d(node.children[usize::from(reaches_front)], depth + 1) {
                hit = true;
            }
            return hit;
        }
        if child_index == NONE || !bsp.valid_leaf((child_index & i32::MAX) as usize) {
            return false;
        }
        let leaf = bsp.leaves[(child_index & i32::MAX) as usize];
        let first = leaf.first_reference.max(0) as usize;
        for reference_index in first..first + leaf.reference_count.max(0) as usize {
            let reference = bsp.references[reference_index];
            if !self.plane_stack.contains(&reference.plane_designator) {
                continue;
            }
            let Some(plane) = bsp.planes.get((reference.plane_designator & i32::MAX) as usize).copied() else { continue };
            let normal = [plane[0], plane[1], plane[2]];
            let distance0 = dot3(self.point, normal) - plane[3];
            let dot = dot3(self.vector, normal);
            let mut t = 0.0f32;
            if dot != 0.0 {
                let inverse = 1.0 / dot;
                // (C's fabs is the double one: the subtraction and the
                // product with the radius happen in double, rounded once)
                t = ((-(inverse * distance0)) as f64 - (inverse as f64).abs() * self.radius as f64) as f32;
                t = pin01(t);
            }
            if !(self.hit.t > t) {
                continue;
            }
            let (ai, aj, ak) = (plane[0].abs(), plane[1].abs(), plane[2].abs());
            self.projection = if ak >= aj && ak >= ai {
                2
            } else if aj >= ai {
                1
            } else {
                0
            };
            self.sign = (plane[self.projection] > 0.0) != (reference.plane_designator & SIGN != 0);
            let mut pill_point = [
                self.vector[0] * t + self.point[0],
                self.vector[1] * t + self.point[1],
                self.vector[2] * t + self.point[2],
            ];
            let plane_distance = -(dot3(pill_point, normal) - plane[3]);
            pill_point = [
                plane[0] * plane_distance + pill_point[0],
                plane[1] * plane_distance + pill_point[1],
                plane[2] * plane_distance + pill_point[2],
            ];
            let p2d = project(pill_point, self.projection, self.sign);
            let surface_index = bsp.bsp2d_test_point(p2d, reference.root);
            if surface_index >= 0
                && (surface_index as usize) < bsp.surfaces.len()
                && bsp.surface_test_point(surface_index as usize, self.projection, self.sign, p2d)
            {
                let surface = bsp.surfaces[surface_index as usize];
                self.hit.t = t;
                self.hit.plane = if reference.plane_designator & SIGN != 0 {
                    [-plane[0], -plane[1], -plane[2], -plane[3]]
                } else {
                    plane
                };
                self.hit.surface = surface_index;
                self.hit.material = surface.material;
                hit = true;
            }
            let negative_distance = -distance0;
            let start_point = [
                plane[0] * negative_distance + self.point[0],
                plane[1] * negative_distance + self.point[1],
                plane[2] * negative_distance + self.point[2],
            ];
            self.point2d = project(start_point, self.projection, self.sign);
            let negative_dot = -dot;
            let sweep = [
                plane[0] * negative_dot + self.vector[0],
                plane[1] * negative_dot + self.vector[1],
                plane[2] * negative_dot + self.vector[2],
            ];
            self.vector2d = project(sweep, self.projection, self.sign);
            self.nodes2d_left = bsp.nodes2d.len() as i64;
            if self.bsp2d(reference.root, 0) {
                hit = true;
            }
        }
        hit
    }

    fn bsp2d(&mut self, child_index: i32, depth: u16) -> bool {
        let bsp = self.bsp;
        if child_index & SIGN == 0 {
            // (collision_bsp2d_enter_node)
            if child_index as usize >= bsp.nodes2d.len() || depth >= MAXIMUM_BSP2D_DEPTH {
                return false;
            }
            self.nodes2d_left -= 1;
            if self.nodes2d_left < 0 {
                return false;
            }
            let node = bsp.nodes2d[child_index as usize];
            let distance0 = (node.plane[0] * self.point2d[0] + node.plane[1] * self.point2d[1]) - node.plane[2];
            let distance1 = node.plane[0] * self.vector2d[0] + node.plane[1] * self.vector2d[1] + distance0;
            let r = self.radius;
            let reaches_back = distance0 <= r + BSP2D_TEST_PILL_EPSILON || distance1 <= r + BSP2D_TEST_PILL_EPSILON;
            let reaches_front = distance0 >= -r - BSP2D_TEST_PILL_EPSILON || distance1 >= -r - BSP2D_TEST_PILL_EPSILON;
            // (short-circuits as the game's does)
            return (reaches_back && self.bsp2d(node.children[0], depth + 1))
                || (reaches_front && self.bsp2d(node.children[1], depth + 1));
        }
        self.surface(child_index & i32::MAX)
    }

    /// `collision_surface_test_pill`: the sphere against a surface's edges.
    fn surface(&mut self, surface_index: i32) -> bool {
        let bsp = self.bsp;
        if surface_index < 0 || surface_index as usize >= bsp.surfaces.len() {
            return false;
        }
        let surface = bsp.surfaces[surface_index as usize];
        let mut edge_index = surface.first_edge;
        if !bsp.ring_continues(edge_index, 0) {
            return false;
        }
        let mut hit = false;
        let mut edge_count = 0;
        loop {
            let edge = &bsp.edges[edge_index as usize];
            let reverse = edge.surfaces[1] == surface_index;
            let v0 = bsp.vertex(edge.vertices[usize::from(reverse)]);
            let v1 = bsp.vertex(edge.vertices[usize::from(!reverse)]);
            let edge_vector = sub3(v1, v0);
            if let Some((t, edge_t)) = pill_test_vector(v0, edge_vector, self.radius, self.point, self.vector)
                && self.hit.t > t
            {
                self.hit.t = t;
                let edge_point = [
                    edge_vector[0] * edge_t + v0[0],
                    edge_vector[1] * edge_t + v0[1],
                    edge_vector[2] * edge_t + v0[2],
                ];
                let pill_point = [
                    self.vector[0] * t + self.point[0],
                    self.vector[1] * t + self.point[1],
                    self.vector[2] * t + self.point[2],
                ];
                let mut normal = sub3(pill_point, edge_point);
                let magnitude = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
                if !(REAL_EPSILON > magnitude.abs()) {
                    let inverse = 1.0 / magnitude;
                    normal = [inverse * normal[0], inverse * normal[1], inverse * normal[2]];
                }
                self.hit.plane = [normal[0], normal[1], normal[2], f32::MAX];
                self.hit.surface = surface_index;
                self.hit.material = surface.material;
                hit = true;
            }
            edge_index = edge.edges[usize::from(reverse)];
            edge_count += 1;
            if edge_index == surface.first_edge || !bsp.ring_continues(edge_index, edge_count) {
                return hit;
            }
        }
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
        if !bsp.ring_continues(surface.first_edge, 0) || !bsp.surface_extant(&surface) {
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
    /// The leaves passed, when wanted.
    leaves: Option<Vec<i32>>,
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
        if leaf_index != NONE
            && let Some(leaves) = &mut self.leaves
        {
            if leaves.len() < 256 {
                leaves.push(leaf_index);
            } else {
                leaves[255] = leaf_index;
            }
        }
        self.last_leaf = leaf_index;
        self.last_contents = contents;
        false
    }
}
