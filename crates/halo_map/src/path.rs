//! The AI's path search (`path.c`'s `path_state_find`): an A* search over
//! the structure BSP's collision surfaces from a start point, through the
//! edges into walkable (and, unless told otherwise, unbroken) neighbours,
//! with costs in tenths of a unit, an attractor that makes a stretch near
//! it dearer (or cheaper), and a bound on how far it goes; towards a
//! destination it keeps the node nearest it. Also the estimated distance
//! and direction of a point on a surface the search reached
//! (`path_state_estimated_distance`).
//!
//! The search state (`struct path_state`, with its node list, heap and hash
//! table) and its debug storage are the game's memory, worked on in place,
//! as are the structure BSP's surfaces, edges, vertices and pathfinding
//! flags (read in place, as [`crate::ai_actor`]'s steps read theirs).

use crate::ai_actor::{AiMemory, AiMemoryExt, NONE};
use crate::layout::{
    collision_bsp as cb, collision_edge as ce, collision_surface as cs, collision_vertex as cv, path_debug_storage as dbg,
    path_destination as pd, path_heap_element as ph, path_input as pi, path_node as pn, path_state as ps, structure_bsp as sb,
    tag_block as tb,
};

const NODE_LIST_SIZE: i16 = 0x400;
const HASH_TABLE_SIZE: u32 = 0x1000;
const HASH_KEY_MASK: i32 = 511;
const HASH_TABLE_MASK: i16 = 4095;
const MAXIMUM_PATH_EDGES_PER_COLLISION_SURFACE: usize = 64;
const MAXIMUM_EDGES_PER_COLLISION_SURFACE: i16 = 8;
const COST_ESTIMATE_GRANULARITY: f32 = 0.1;
const SHORT_MAX: i32 = 32767;
const COLLISION_SURFACE_BREAKABLE: u8 = 1 << 3;
const PATHFINDING_SURFACE_WALKABLE: u8 = 1 << 6;
const PATHFINDING_SURFACE_BREAKABLE: u8 = 1 << 7;
const PROJECTION_Z: usize = 2;
const REAL_EPSILON: f32 = 0.0001;

/// path.h's `_path_traverse_result_*`.
mod traverse_result {
    pub const NONE: i16 = 0;
    pub const INITIAL_NOT_PATHFINDABLE: i16 = 1;
    pub const NEVER_CLOSE_ENOUGH: i16 = 2;
    pub const EXHAUSTED_SEARCH: i16 = 3;
    pub const OVERFLOWED_NODES: i16 = 4;
    pub const SUCCESS: i16 = 5;
}

/// What a search reads besides its state: the breakable surfaces' flags
/// (`breakable_surface_flags_get`'s bit vector) and the structure BSP's
/// index (for the debug storage).
#[derive(Debug, Clone, Copy)]
pub struct PathContext {
    pub breakable_surface_flags: u32,
    pub structure_bsp_index: i16,
}

fn vector_from_points(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [b[0] - a[0], b[1] - a[1], b[2] - a[2]]
}

fn magnitude_squared(v: [f32; 3]) -> f32 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    magnitude_squared(vector_from_points(a, b))
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    distance_squared(a, b).sqrt()
}

/// real_math.h's `point_from_line3d(p, v, t)`.
fn point_from_line(p: [f32; 3], v: [f32; 3], t: f32) -> [f32; 3] {
    [v[0] * t + p[0], v[1] * t + p[1], v[2] * t + p[2]]
}

/// cseries.h's `MAX(a, b)`.
fn max_c(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

/// cseries.h's `MIN(a, b)`.
fn min_c(a: f32, b: f32) -> f32 {
    if a > b { b } else { a }
}

/// `global_projection3d_mappings`.
const PROJECTION_MAPPINGS: [[[usize; 2]; 2]; 3] = [[[2, 1], [1, 2]], [[0, 2], [2, 0]], [[1, 0], [0, 1]]];

fn project_point3d(p: [f32; 3], projection: usize, sign: bool) -> [f32; 2] {
    let [x, y] = PROJECTION_MAPPINGS[projection][usize::from(sign)];
    [p[x], p[y]]
}

/// real_math.h's `project_point2d`: the point of the plane over `p`.
fn project_point2d(p: [f32; 2], plane: [f32; 4], projection: usize, sign: bool) -> [f32; 3] {
    let [x, y] = PROJECTION_MAPPINGS[projection][usize::from(sign)];
    let mut result = [0.0f32; 3];
    result[x] = p[0];
    result[y] = p[1];
    result[projection] = if (plane[projection] as f64).abs() < REAL_EPSILON as f64 {
        0.0
    } else {
        ((plane[3] - p[0] * plane[x]) - p[1] * plane[y]) / plane[projection]
    };
    result
}

/// A structure BSP's collision BSP (its first), in the game's memory.
struct Bsp<'a> {
    m: &'a dyn AiMemory,
    structure: u32,
    bsp: u32,
}

impl<'a> Bsp<'a> {
    fn new(m: &'a dyn AiMemory, structure: u32) -> Self {
        let bsp = m.r32(structure + sb::COLLISION_BSP as u32 + tb::ADDRESS as u32);
        Bsp { m, structure, bsp }
    }

    fn block_count(&self, block: usize) -> i32 {
        self.m.i32(self.bsp + block as u32 + tb::COUNT as u32)
    }

    fn element(&self, block: usize, index: i32, size: usize) -> u32 {
        self.m
            .r32(self.bsp + block as u32 + tb::ADDRESS as u32)
            .wrapping_add((index as u32).wrapping_mul(size as u32))
    }

    fn surface(&self, index: i32) -> u32 {
        self.element(cb::SURFACES, index, cs::SIZEOF)
    }

    fn edge(&self, index: i32) -> u32 {
        self.element(cb::EDGES, index, ce::SIZEOF)
    }

    fn vertex(&self, index: i32) -> [f32; 3] {
        self.m.vector(self.element(cb::VERTICES, index, cv::SIZEOF))
    }

    fn plane(&self, index: i32) -> [f32; 4] {
        let address = self
            .m
            .r32(self.bsp + cb::BSP3D as u32 + crate::layout::bsp3d::PLANES as u32 + tb::ADDRESS as u32)
            .wrapping_add((index as u32).wrapping_mul(16));
        [self.m.f32(address), self.m.f32(address + 4), self.m.f32(address + 8), self.m.f32(address + 12)]
    }

    /// `collision_bsp_valid_surface_index`.
    fn valid_surface_index(&self, surface_index: i32) -> bool {
        surface_index >= 0 && surface_index < self.block_count(cb::SURFACES)
    }

    /// `collision_surface_edge_ring_continues`.
    fn edge_ring_continues(&self, edge_index: i32, edge_count: i16) -> bool {
        edge_index >= 0 && edge_index < self.block_count(cb::EDGES) && edge_count < MAXIMUM_EDGES_PER_COLLISION_SURFACE
    }

    fn edge_vertex(&self, edge: u32, which: usize) -> i32 {
        self.m.i32(edge + ce::VERTEX_INDICES as u32 + 4 * which as u32)
    }

    fn edge_next(&self, edge: u32, which: usize) -> i32 {
        self.m.i32(edge + ce::EDGE_INDICES as u32 + 4 * which as u32)
    }

    fn edge_surface(&self, edge: u32, which: usize) -> i32 {
        self.m.i32(edge + ce::SURFACE_INDICES as u32 + 4 * which as u32)
    }

    /// collision_bsp.c's `collision_surface_find_closest_point2d`: the
    /// closest point of the surface's projected polygon (whether the point
    /// was inside).
    fn surface_find_closest_point2d(&self, surface_index: i32, projection: usize, sign: bool, point: [f32; 2]) -> (bool, [f32; 2]) {
        if !self.valid_surface_index(surface_index) {
            return (true, point);
        }
        let surface = self.surface(surface_index);
        let first_edge_index = self.m.i32(surface + cs::FIRST_EDGE_INDEX as u32);
        let mut edge_index = first_edge_index;
        if !self.edge_ring_continues(edge_index, 0) {
            return (true, point);
        }
        let mut previous_before = false;
        let mut previous_after = false;
        let mut first_before = false;
        let mut first_after = false;
        let mut before;
        let mut after;
        let mut edge_count: i16 = 0;
        loop {
            let edge = self.edge(edge_index);
            let reverse = self.edge_surface(edge, 1) == surface_index;
            let origin = self.vertex(self.edge_vertex(edge, usize::from(reverse)));
            let target = self.vertex(self.edge_vertex(edge, usize::from(!reverse)));
            before = false;
            after = false;
            let origin2d = project_point3d(origin, projection, sign);
            let target2d = project_point3d(target, projection, sign);
            let point_vector = [point[0] - origin2d[0], point[1] - origin2d[1]];
            let edge_vector = [target2d[0] - origin2d[0], target2d[1] - origin2d[1]];
            if point_vector[0] * edge_vector[1] - point_vector[1] * edge_vector[0] > 0.0 {
                let dot = point_vector[0] * edge_vector[0] + point_vector[1] * edge_vector[1];
                if dot < 0.0 {
                    before = true;
                } else {
                    let length_squared = edge_vector[0] * edge_vector[0] + edge_vector[1] * edge_vector[1];
                    if dot > length_squared {
                        after = true;
                    } else {
                        let t = dot / length_squared;
                        return (false, [edge_vector[0] * t + origin2d[0], edge_vector[1] * t + origin2d[1]]);
                    }
                }
            }
            if edge_index != first_edge_index {
                if (previous_after && (before || !after)) || (before && (previous_after || !previous_before)) {
                    return (false, origin2d);
                }
            } else {
                first_before = before;
                first_after = after;
            }
            previous_before = before;
            previous_after = after;
            edge_index = self.edge_next(edge, usize::from(reverse));
            if edge_index == first_edge_index {
                break;
            }
            edge_count += 1;
            if !self.edge_ring_continues(edge_index, edge_count) {
                break;
            }
        }
        if (after && (first_before || !first_after)) || (first_before && (after || !before)) {
            let edge = self.edge(first_edge_index);
            let reverse = self.edge_surface(edge, 1) == surface_index;
            let origin = self.vertex(self.edge_vertex(edge, usize::from(reverse)));
            return (false, project_point3d(origin, projection, sign));
        }
        (true, point)
    }

    /// collision_bsp.c's `collision_surface_project_point2d`.
    fn surface_project_point2d(&self, surface_index: i32, projection: usize, sign: bool, point: [f32; 2]) -> [f32; 3] {
        let surface = self.surface(surface_index);
        let plane = self.plane(self.m.i32(surface + cs::PLANE_DESIGNATOR as u32) & i32::MAX);
        project_point2d(point, plane, projection, sign)
    }
}

/// path.c's `struct path_edge`.
#[derive(Debug, Clone, Copy, Default)]
struct PathEdge {
    adjacent_surface_index: i32,
    adjacent_pathfinding_surface: u8,
    base_point: [f32; 3],
    edge_vector: [f32; 3],
}

/// A search state in the game's memory.
struct State<'a> {
    m: &'a dyn AiMemory,
    state: u32,
}

impl State<'_> {
    fn at(&self, offset: usize) -> u32 {
        self.state + offset as u32
    }
    fn node(&self, index: i16) -> u32 {
        self.state + ps::NODE_LIST as u32 + (index as i32 as u32).wrapping_mul(pn::SIZEOF as u32)
    }
    fn heap(&self, location: i16) -> u32 {
        self.state + ps::HEAP as u32 + (location as i32 as u32).wrapping_mul(ph::SIZEOF as u32)
    }
    fn hash(&self, slot: i16) -> u32 {
        self.state + ps::HASH_TABLE as u32 + (slot as i32 as u32).wrapping_mul(2)
    }
    fn heap_count(&self) -> i16 {
        self.m.i16(self.at(ps::HEAP_COUNT))
    }
    fn node_count(&self) -> i16 {
        self.m.i16(self.at(ps::NODE_COUNT))
    }
    fn input(&self, offset: usize) -> u32 {
        self.at(ps::INPUT + offset)
    }
    fn destination(&self, offset: usize) -> u32 {
        self.at(ps::DESTINATION + offset)
    }
    fn structure(&self) -> u32 {
        self.m.r32(self.at(ps::STRUCTURE))
    }
    fn debug(&self) -> u32 {
        self.m.r32(self.at(ps::DEBUG))
    }
    fn set_vector(&self, address: u32, v: [f32; 3]) {
        for (k, value) in v.iter().enumerate() {
            self.m.set_f32(address + 4 * k as u32, *value);
        }
    }
    fn heap_element(&self, location: i16) -> (i16, i16) {
        let address = self.heap(location);
        (self.m.i16(address + ph::NODE_INDEX as u32), self.m.i16(address + ph::QUANTIZED_COST_ESTIMATE as u32))
    }
    fn set_heap_element(&self, location: i16, node_index: i16, cost: i16) {
        let address = self.heap(location);
        self.m.set_i16(address + ph::NODE_INDEX as u32, node_index);
        self.m.set_i16(address + ph::QUANTIZED_COST_ESTIMATE as u32, cost);
    }
    fn set_heap_location(&self, node_index: i16, location: i16) {
        self.m.set_i16(self.node(node_index) + pn::HEAP_LOCATION as u32, location);
    }

    /// The debug storage's traverse result, if it has storage and none is
    /// set yet.
    fn debug_traverse_result(&self, result: i16) {
        let debug = self.debug();
        if debug != 0 && self.m.i16(debug + dbg::PATH_TRAVERSE_RESULT as u32) == traverse_result::NONE {
            self.m.set_i16(debug + dbg::PATH_TRAVERSE_RESULT as u32, result);
        }
    }

    /// `path_state_reset`.
    fn reset(&self) {
        self.m.set_i16(self.at(ps::NODE_COUNT), 0);
        self.m.set_i16(self.at(ps::HEAP_COUNT), 1);
        for slot in 0..HASH_TABLE_SIZE {
            self.m.w16(self.at(ps::HASH_TABLE) + 2 * slot, 0xFFFF);
        }
        self.m.set_i16(self.at(ps::CLOSEST_NODE_INDEX), NONE as i16);
        self.m.set_f32(self.at(ps::CLOSEST_DISTANCE), f32::MAX);
        self.m.set_f32(self.at(ps::CLOSEST_COST_ESTIMATE), f32::MAX);
    }

    /// `path_heap_bubble_up`.
    fn heap_bubble_up(&self, mut heap_location: i16) {
        let (node_index, node_cost) = self.heap_element(heap_location);
        if heap_location > 1 {
            loop {
                let parent_location = heap_location >> 1;
                let (parent_node_index, parent_cost_estimate) = self.heap_element(parent_location);
                if node_cost >= parent_cost_estimate {
                    break;
                }
                self.set_heap_element(heap_location, parent_node_index, parent_cost_estimate);
                self.set_heap_location(parent_node_index, heap_location);
                heap_location = parent_location;
                if parent_location <= 1 {
                    break;
                }
            }
        }
        self.set_heap_element(heap_location, node_index, node_cost);
        self.set_heap_location(node_index, heap_location);
    }

    /// `path_heap_bubble_down`.
    fn heap_bubble_down(&self, mut heap_location: i16) {
        let (node_index, node_cost) = self.heap_element(heap_location);
        loop {
            let mut new_node_index = node_index;
            let mut new_heap_location = heap_location;
            let mut new_cost = node_cost;
            let mut child_number = 0;
            let mut child_heap_location = (heap_location as i32 * 2) as i16;
            while child_number < 2 && child_heap_location < self.heap_count() {
                let (child_node_index, child_cost) = self.heap_element(child_heap_location);
                if child_cost < new_cost {
                    new_heap_location = child_heap_location;
                    new_node_index = child_node_index;
                    new_cost = child_cost;
                }
                child_number += 1;
                child_heap_location += 1;
            }
            if new_heap_location == heap_location {
                break;
            }
            self.set_heap_element(heap_location, new_node_index, new_cost);
            self.set_heap_location(new_node_index, heap_location);
            heap_location = new_heap_location;
        }
        self.set_heap_element(heap_location, node_index, node_cost);
        self.set_heap_location(node_index, heap_location);
    }

    /// `path_heap_pop_cheapest_node`.
    fn heap_pop_cheapest_node(&self) -> i16 {
        let mut node_index = NONE as i16;
        if self.heap_count() > 1 {
            node_index = self.heap_element(1).0;
            self.set_heap_location(node_index, NONE as i16);
            let heap_count = self.heap_count() - 1;
            self.m.set_i16(self.at(ps::HEAP_COUNT), heap_count);
            if heap_count > 1 {
                let (last_node, last_cost) = self.heap_element(heap_count);
                self.set_heap_element(1, last_node, last_cost);
                self.heap_bubble_down(1);
            }
        }
        node_index
    }

    /// `path_heap_insert`.
    fn heap_insert(&self, node_index: i16, quantized_cost_estimate: i16) {
        let heap_location = self.heap_count();
        if heap_location < NODE_LIST_SIZE {
            self.m.set_i16(self.at(ps::HEAP_COUNT), heap_location + 1);
            self.set_heap_element(heap_location, node_index, quantized_cost_estimate);
            self.heap_bubble_up(heap_location);
        }
    }

    /// `path_node_from_hash_table`.
    fn node_from_hash_table(&self, surface_index: i32) -> i16 {
        let mut slot = ((surface_index & 0x1FF) << 3) as i16;
        loop {
            let node_index = self.m.i16(self.hash(slot));
            slot = (slot + 1) & 0xFFF;
            if node_index == NONE as i16 || self.m.i32(self.node(node_index) + pn::SURFACE_INDEX as u32) == surface_index {
                return node_index;
            }
        }
    }

    /// `path_state_begin`: the start's node.
    fn begin(&self) -> bool {
        let m = self.m;
        let start_surface_index = m.i32(self.input(pi::START_SURFACE_INDEX));
        let start_point = m.vector(self.input(pi::START_POINT));
        if start_surface_index == NONE || !(start_point[2] > -1000.0) {
            return false;
        }
        let destination_valid = m.bool(self.at(ps::DESTINATION_VALID));
        let distance_to_destination;
        let quantized_cost_estimate: i32;
        if destination_valid {
            let distance_squared = distance_squared(start_point, m.vector(self.destination(pd::POINT)));
            distance_to_destination = distance_squared.sqrt();
            quantized_cost_estimate = (distance_to_destination / COST_ESTIMATE_GRANULARITY) as i32;
            if quantized_cost_estimate >= SHORT_MAX {
                return false;
            }
        } else {
            distance_to_destination = 0.0;
            quantized_cost_estimate = 0;
        }
        let node_index = self.node_count();
        m.set_i16(self.at(ps::NODE_COUNT), node_index + 1);
        let node = self.node(node_index);
        m.set_i16(node + pn::PARENT_NODE_INDEX as u32, NONE as i16);
        m.w32(node + pn::PARENT_NODE_SURFACE_INDEX as u32, NONE as u32);
        m.w32(node + pn::SURFACE_INDEX as u32, start_surface_index as u32);
        self.set_vector(node + pn::ENTRY_POINT as u32, start_point);
        m.set_f32(node + pn::LINEAR_DISTANCE_TO_ENTRY_POINT as u32, 0.0);
        m.set_f32(node + pn::CLOSEST_APPROACH_TO_ATTRACTOR as u32, f32::MAX);
        m.set_f32(node + pn::PATH_DISTANCE_FROM_ORIGIN as u32, 0.0);
        m.set_f32(node + pn::CUMULATIVE_COST as u32, 0.0);
        m.set_f32(node + pn::TOTAL_COST_ESTIMATE as u32, distance_to_destination);
        m.set_i16(node + pn::QUANTIZED_COST_ESTIMATE as u32, quantized_cost_estimate as i16);
        m.set_i16(node + pn::DEPTH as u32, 0);
        m.set_i16(node + pn::LAST_RENDER_ID as u32, NONE as i16);
        if destination_valid {
            m.set_f32(self.at(ps::CLOSEST_DISTANCE), distance_to_destination);
            m.set_i16(self.at(ps::CLOSEST_NODE_INDEX), node_index);
            self.set_vector(self.at(ps::CLOSEST_POINT), start_point);
            m.set_f32(self.at(ps::CLOSEST_COST_ESTIMATE), distance_to_destination);
        }
        let slot = ((start_surface_index & HASH_KEY_MASK) << 3) as i16;
        m.set_i16(self.hash(slot), node_index);
        self.heap_insert(node_index, quantized_cost_estimate as i16);
        true
    }

    /// `surface_is_broken`.
    fn surface_is_broken(&self, bsp: &Bsp, context: &PathContext, surface_index: i32) -> bool {
        let surface = bsp.surface(surface_index);
        if self.m.r8(surface + cs::FLAGS as u32) & COLLISION_SURFACE_BREAKABLE == 0 {
            return false;
        }
        let breakable_surface_index = self.m.r8(surface + cs::BREAKABLE_SURFACE_INDEX as u32) as u32;
        let word = self.m.r32(context.breakable_surface_flags + (breakable_surface_index >> 5) * 4);
        word & (1 << (breakable_surface_index & 31)) == 0
    }

    /// `build_path_edges_for_surface`: the surface's edges, each with its
    /// neighbour and the neighbour's pathfinding flags.
    fn build_path_edges_for_surface(&self, bsp: &Bsp, surface_index: i32) -> Vec<PathEdge> {
        let m = self.m;
        let mut edges = Vec::new();
        let pathfinding_surfaces = bsp.structure + sb::PATHFINDING_SURFACES as u32;
        let pathfinding_count = m.i32(pathfinding_surfaces + tb::COUNT as u32);
        let pathfinding_address = m.r32(pathfinding_surfaces + tb::ADDRESS as u32);
        if !bsp.valid_surface_index(surface_index) {
            return edges;
        }
        let surface = bsp.surface(surface_index);
        let first_edge_index = m.i32(surface + cs::FIRST_EDGE_INDEX as u32);
        let vertex_count = bsp.block_count(cb::VERTICES);
        let mut edge_index = first_edge_index;
        loop {
            if !bsp.edge_ring_continues(edge_index, edges.len() as i16) {
                break;
            }
            let collision_edge = bsp.edge(edge_index);
            let vertex0 = bsp.edge_vertex(collision_edge, 0);
            let vertex1 = bsp.edge_vertex(collision_edge, 1);
            if !(vertex0 >= 0 && vertex0 < vertex_count) || !(vertex1 >= 0 && vertex1 < vertex_count) {
                break;
            }
            let right_surface = surface_index == bsp.edge_surface(collision_edge, 1);
            let adjacent_surface_index = bsp.edge_surface(collision_edge, usize::from(!right_surface));
            let adjacent_pathfinding_surface = if (adjacent_surface_index != NONE
                && (!bsp.valid_surface_index(adjacent_surface_index)
                    || !(adjacent_surface_index >= 0 && adjacent_surface_index < pathfinding_count)))
                || (adjacent_surface_index == NONE && pathfinding_count <= 0)
            {
                0
            } else {
                m.r8(pathfinding_address.wrapping_add(adjacent_surface_index as u32))
            };
            let start = bsp.vertex(vertex0);
            let end = bsp.vertex(vertex1);
            edges.push(PathEdge {
                adjacent_surface_index,
                adjacent_pathfinding_surface,
                base_point: start,
                edge_vector: vector_from_points(start, end),
            });
            if edges.len() == MAXIMUM_PATH_EDGES_PER_COLLISION_SURFACE {
                break;
            }
            edge_index = bsp.edge_next(collision_edge, usize::from(right_surface));
            if edge_index == first_edge_index {
                break;
            }
        }
        edges
    }

    /// `closest_available_point_on_surface`: the point's nearest on the
    /// surface (through its vertical projection), and its distance.
    fn closest_available_point_on_surface(&self, bsp: &Bsp, surface_index: i32, point: [f32; 3]) -> (f32, [f32; 3]) {
        let (_, closest2d) = bsp.surface_find_closest_point2d(surface_index, PROJECTION_Z, true, [point[0], point[1]]);
        let closest = bsp.surface_project_point2d(surface_index, PROJECTION_Z, true, closest2d);
        (distance(point, closest), closest)
    }

    /// `path_attractor_weight`: the extra cost of the stretch to `point`
    /// from `previous_point` near the attractor, and how near it comes.
    fn attractor_weight(&self, point: [f32; 3], previous_point: [f32; 3]) -> (f32, f32) {
        let m = self.m;
        let attractor_point = m.vector(self.input(pi::ATTRACTOR_POINT));
        let attractor_radius = m.f32(self.input(pi::ATTRACTOR_RADIUS));
        let closest_point = closest_point_to_attractor(point, previous_point, attractor_point);
        let distance_squared = distance_squared(attractor_point, closest_point);
        let mut weight = 0.0;
        let mut distance = f32::MAX;
        if distance_squared < attractor_radius * attractor_radius {
            distance = distance_squared.sqrt();
            weight = (1.0 - distance / attractor_radius) * m.f32(self.input(pi::ATTRACTOR_WEIGHT));
        }
        (weight, distance)
    }

    /// `path_state_traverse`.
    fn traverse(&self, context: &PathContext) -> bool {
        let m = self.m;
        let pathfinding_radius = max_c(0.2, m.f32(self.input(pi::PATHFINDING_RADIUS)));
        let bsp = Bsp::new(m, self.structure());
        let destination_valid = m.bool(self.at(ps::DESTINATION_VALID));
        let destination_point = m.vector(self.destination(pd::POINT));
        let destination_surface_index = m.i32(self.destination(pd::SURFACE_INDEX));
        let ignore_broken_surfaces = m.bool(self.input(pi::IGNORE_BROKEN_SURFACES));
        let attractor_valid = m.bool(self.input(pi::ATTRACTOR_VALID));
        let search_bounded = m.bool(self.input(pi::SEARCH_BOUNDED));
        let search_maximum_distance = m.f32(self.input(pi::SEARCH_MAXIMUM_DISTANCE));
        loop {
            let cheapest_node_index = self.heap_pop_cheapest_node();
            if cheapest_node_index == NONE as i16 {
                self.debug_traverse_result(traverse_result::EXHAUSTED_SEARCH);
                break;
            }
            let cheapest = self.node(cheapest_node_index);
            let cheapest_field = |offset: usize| cheapest + offset as u32;
            if destination_valid {
                if m.i32(cheapest_field(pn::SURFACE_INDEX)) == destination_surface_index {
                    m.set_i16(self.at(ps::CLOSEST_NODE_INDEX), cheapest_node_index);
                    m.set_f32(self.at(ps::CLOSEST_DISTANCE), 0.0);
                    self.set_vector(self.at(ps::CLOSEST_POINT), destination_point);
                    break;
                }
                if m.f32(cheapest_field(pn::TOTAL_COST_ESTIMATE))
                    > max_c(5.0, m.f32(self.at(ps::CLOSEST_DISTANCE))) * 10.0 + m.f32(self.at(ps::CLOSEST_COST_ESTIMATE))
                {
                    self.debug_traverse_result(traverse_result::NEVER_CLOSE_ENOUGH);
                    break;
                }
            }
            let edges = self.build_path_edges_for_surface(&bsp, m.i32(cheapest_field(pn::SURFACE_INDEX)));
            for edge in &edges {
                let adjacent_surface_index = edge.adjacent_surface_index;
                let mut passable = true;
                if adjacent_surface_index == m.i32(cheapest_field(pn::PARENT_NODE_SURFACE_INDEX)) {
                    passable = false;
                }
                if edge.adjacent_pathfinding_surface & PATHFINDING_SURFACE_WALKABLE == 0 {
                    passable = false;
                }
                if !ignore_broken_surfaces
                    && edge.adjacent_pathfinding_surface & PATHFINDING_SURFACE_BREAKABLE != 0
                    && self.surface_is_broken(&bsp, context, adjacent_surface_index)
                {
                    passable = false;
                }
                if !passable {
                    continue;
                }
                let mut entry_point = point_from_line(edge.base_point, edge.edge_vector, 0.5);
                if destination_valid {
                    let edge_length_squared = magnitude_squared(edge.edge_vector);
                    if edge_length_squared > 16.0
                        && edge_length_squared > (2.0 * pathfinding_radius) * (2.0 * pathfinding_radius)
                    {
                        let edge_length = edge_length_squared.sqrt();
                        let edge_to_destination = vector_from_points(edge.base_point, destination_point);
                        let mut t = dot(edge_to_destination, edge.edge_vector) / magnitude_squared(edge.edge_vector);
                        let floor = pathfinding_radius / edge_length;
                        let ceiling = 1.0 - pathfinding_radius / edge_length;
                        t = if t < floor {
                            floor
                        } else if t > ceiling {
                            ceiling
                        } else {
                            t
                        };
                        entry_point = point_from_line(edge.base_point, edge.edge_vector, t);
                    }
                }
                let cheapest_entry = m.vector(cheapest_field(pn::ENTRY_POINT));
                let linear_distance = distance(cheapest_entry, entry_point);
                let path_distance_from_origin = m.f32(cheapest_field(pn::PATH_DISTANCE_FROM_ORIGIN)) + linear_distance;
                let cost;
                let closest_approach_to_attractor;
                if attractor_valid {
                    let (weight, approach) = self.attractor_weight(cheapest_entry, entry_point);
                    cost = (weight + 1.0) * linear_distance;
                    closest_approach_to_attractor = min_c(m.f32(cheapest_field(pn::CLOSEST_APPROACH_TO_ATTRACTOR)), approach);
                } else {
                    cost = linear_distance;
                    closest_approach_to_attractor = 0.0;
                }
                let cumulative_cost = m.f32(cheapest_field(pn::CUMULATIVE_COST)) + cost;
                let mut total_cost_estimate = cumulative_cost;
                let mut distance_to_destination = 0.0;
                if destination_valid {
                    distance_to_destination = distance(entry_point, destination_point);
                    total_cost_estimate += distance_to_destination;
                }
                let quantized_cost_estimate = (total_cost_estimate / COST_ESTIMATE_GRANULARITY) as i32;
                if quantized_cost_estimate >= SHORT_MAX {
                    continue;
                }
                if search_bounded && path_distance_from_origin > search_maximum_distance {
                    continue;
                }

                let mut new_node_index = NONE as i16;
                let mut hash_slot = ((adjacent_surface_index & HASH_KEY_MASK) << 3) as i16;
                loop {
                    let node_index = m.i16(self.hash(hash_slot));
                    if node_index != NONE as i16 {
                        let node = self.node(node_index);
                        if m.i32(node + pn::SURFACE_INDEX as u32) == adjacent_surface_index {
                            if quantized_cost_estimate < m.i16(node + pn::QUANTIZED_COST_ESTIMATE as u32) as i32
                                && m.i16(node + pn::HEAP_LOCATION as u32) != NONE as i16
                            {
                                new_node_index = node_index;
                            }
                            break;
                        }
                        hash_slot = (hash_slot + 1) & HASH_TABLE_MASK;
                    } else {
                        let node_count = self.node_count();
                        if node_count < NODE_LIST_SIZE {
                            new_node_index = node_count;
                            m.set_i16(self.at(ps::NODE_COUNT), node_count + 1);
                            m.set_i16(self.hash(hash_slot), new_node_index);
                            self.set_heap_location(new_node_index, NONE as i16);
                        } else {
                            self.debug_traverse_result(traverse_result::OVERFLOWED_NODES);
                        }
                        break;
                    }
                }
                if new_node_index == NONE as i16 {
                    continue;
                }

                let new_node = self.node(new_node_index);
                let field = |offset: usize| new_node + offset as u32;
                m.set_i16(field(pn::PARENT_NODE_INDEX), cheapest_node_index);
                m.w32(field(pn::PARENT_NODE_SURFACE_INDEX), m.r32(cheapest_field(pn::SURFACE_INDEX)));
                m.w32(field(pn::SURFACE_INDEX), adjacent_surface_index as u32);
                self.set_vector(field(pn::ENTRY_POINT), entry_point);
                m.set_f32(field(pn::LINEAR_DISTANCE_TO_ENTRY_POINT), linear_distance);
                m.set_f32(field(pn::CLOSEST_APPROACH_TO_ATTRACTOR), closest_approach_to_attractor);
                m.set_f32(field(pn::PATH_DISTANCE_FROM_ORIGIN), path_distance_from_origin);
                m.set_f32(field(pn::CUMULATIVE_COST), cumulative_cost);
                m.set_f32(field(pn::TOTAL_COST_ESTIMATE), total_cost_estimate);
                m.set_i16(field(pn::QUANTIZED_COST_ESTIMATE), quantized_cost_estimate as i16);
                m.set_i16(field(pn::DEPTH), m.i16(cheapest_field(pn::DEPTH)).wrapping_add(1));
                m.set_i16(field(pn::LAST_RENDER_ID), NONE as i16);
                m.set_f32(field(pn::CLOSEST_DISTANCE_TO_ATTRACTOR), f32::MAX);

                let heap_location = m.i16(field(pn::HEAP_LOCATION));
                if heap_location == NONE as i16 {
                    self.heap_insert(new_node_index, quantized_cost_estimate as i16);
                } else {
                    m.set_i16(self.heap(heap_location) + ph::QUANTIZED_COST_ESTIMATE as u32, quantized_cost_estimate as i16);
                    self.heap_bubble_up(heap_location);
                }

                if destination_valid {
                    let mut closest_distance = distance_to_destination;
                    let mut closest_point = entry_point;
                    if closest_distance < 4.0 {
                        (closest_distance, closest_point) =
                            self.closest_available_point_on_surface(&bsp, adjacent_surface_index, destination_point);
                    }
                    m.set_f32(field(pn::CLOSEST_DISTANCE_TO_ATTRACTOR), closest_distance);
                    self.set_vector(field(pn::CLOSEST_POINT_TO_ATTRACTOR), closest_point);
                    if closest_distance < m.f32(self.at(ps::CLOSEST_DISTANCE)) {
                        m.set_f32(self.at(ps::CLOSEST_DISTANCE), closest_distance);
                        m.set_i16(self.at(ps::CLOSEST_NODE_INDEX), new_node_index);
                        self.set_vector(self.at(ps::CLOSEST_POINT), closest_point);
                        m.set_f32(self.at(ps::CLOSEST_COST_ESTIMATE), total_cost_estimate);
                    }
                }
            }
        }
        let mut result = true;
        if destination_valid {
            result = m.f32(self.at(ps::CLOSEST_DISTANCE)) <= m.f32(self.destination(pd::TARGET_RADIUS));
        }
        let debug = self.debug();
        if debug != 0 && result {
            m.set_i16(debug + dbg::PATH_TRAVERSE_RESULT as u32, traverse_result::SUCCESS);
        }
        result
    }
}

/// path.c's `closest_point_to_attractor` (as the game has it: the
/// segment's parameter from the offset of its start from the attractor).
fn closest_point_to_attractor(p0: [f32; 3], p1: [f32; 3], q: [f32; 3]) -> [f32; 3] {
    let segment = vector_from_points(p0, p1);
    let offset = vector_from_points(q, p0);
    let t = dot(offset, segment) / magnitude_squared(segment);
    if t < 0.0 || t > 1.0 { p1 } else { point_from_line(p0, segment, t) }
}

/// `path_state_find` (after the profile's count): the search from the
/// state's start, and whether it reached its destination (or, without
/// one, true); with debug storage, the result, a copy of the state and the
/// structure BSP.
pub fn path_state_find(m: &dyn AiMemory, state: u32, context: &PathContext) -> bool {
    let state = State { m, state };
    let mut result = false;
    state.reset();
    let debug = state.debug();
    if debug != 0 {
        m.set_i16(debug + dbg::PATH_TRAVERSE_RESULT as u32, traverse_result::NONE);
    }
    if state.begin() {
        result = state.traverse(context);
    } else if debug != 0 {
        m.set_i16(debug + dbg::PATH_TRAVERSE_RESULT as u32, traverse_result::INITIAL_NOT_PATHFINDABLE);
    }
    if debug != 0 {
        let source = state.state;
        let target = debug + dbg::PATH_STATE as u32;
        for offset in (0..ps::SIZEOF as u32).step_by(4) {
            m.w32(target + offset, m.r32(source + offset));
        }
        m.set_i16(debug + dbg::STRUCTURE_BSP_INDEX as u32, context.structure_bsp_index);
        if m.i16(debug + dbg::PATH_TRAVERSE_RESULT as u32) != traverse_result::SUCCESS {
            m.set_bool(debug + dbg::FAILURE as u32, true);
        }
    }
    result
}

/// `path_state_estimated_distance`: the path distance to the point on the
/// surface the search reached (and the closest approach to the attractor,
/// and the direction it sets off in: the node list's child links rewritten
/// on the way), or REAL_MAX.
pub fn path_state_estimated_distance(
    m: &dyn AiMemory,
    state: u32,
    end_point: [f32; 3],
    end_surface_index: i32,
    want_closest_approach: bool,
    want_direction: bool,
) -> (bool, f32, f32, [f32; 3]) {
    let state = State { m, state };
    let node_index = state.node_from_hash_table(end_surface_index);
    if node_index == NONE as i16 {
        return (false, f32::MAX, f32::MAX, [0.0; 3]);
    }
    let node = state.node(node_index);
    let entry_point = m.vector(node + pn::ENTRY_POINT as u32);
    let distance = distance(entry_point, end_point) + m.f32(node + pn::PATH_DISTANCE_FROM_ORIGIN as u32);
    let closest_approach_to_attractor = if m.bool(state.input(pi::ATTRACTOR_VALID)) {
        let attractor_point = m.vector(state.input(pi::ATTRACTOR_POINT));
        let closest_point = closest_point_to_attractor(entry_point, end_point, attractor_point);
        min_c(distance_squared(attractor_point, closest_point).sqrt(), m.f32(node + pn::CLOSEST_APPROACH_TO_ATTRACTOR as u32))
    } else {
        0.0
    };
    let _ = want_closest_approach;
    let mut direction = [0.0f32; 3];
    if want_direction {
        let mut current_node_index = node_index;
        let mut child_node_index = NONE as i16;
        loop {
            let node = state.node(current_node_index);
            m.set_i16(node + pn::CHILD_NODE_INDEX as u32, child_node_index);
            child_node_index = current_node_index;
            current_node_index = m.i16(node + pn::PARENT_NODE_INDEX as u32);
            if current_node_index == NONE as i16 {
                break;
            }
        }
        current_node_index = child_node_index;
        let mut path_distance = 0.0f32;
        let mut node = state.node(current_node_index);
        while current_node_index != NONE as i16 && path_distance < 0.8 {
            node = state.node(current_node_index);
            path_distance += m.f32(node + pn::LINEAR_DISTANCE_TO_ENTRY_POINT as u32);
            current_node_index = m.i16(node + pn::CHILD_NODE_INDEX as u32);
        }
        let direction_point = if current_node_index == NONE as i16 { end_point } else { m.vector(node + pn::ENTRY_POINT as u32) };
        direction = vector_from_points(m.vector(state.input(pi::START_POINT)), direction_point);
        crate::actor_transitions::normalize3(&mut direction);
    }
    (true, distance, closest_approach_to_attractor, direction)
}
