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

// ---------- path_structure_bsp.c: the 2D tests over the structure's surfaces

/// path_structure_bsp.h's `struct path_collision_result`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PathCollisionResult {
    pub collision: i32,
    pub point: [f32; 3],
    pub surface_index: i32,
    pub edge_index: i32,
    pub t: f32,
}

/// collision_bsp.h's `struct collision_surface_test_line2d_result`.
#[derive(Debug, Clone, Copy)]
struct SurfaceLine2d {
    enter_t: f32,
    enter_edge_index: i32,
    enter_surface_index: i32,
    exit_t: f32,
    exit_edge_index: i32,
    exit_surface_index: i32,
}

/// path_structure_bsp.h's `struct structure_test_ray2d_result`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Ray2dResult {
    pub distance: f32,
    pub surface_index: i32,
    pub edge_index: i32,
}

const REAL_MIN: f32 = -3.4028235e38;
const TEST_PILL_ENDPOINT_NEAR_WALL_OK: u32 = 1 << 0;

fn cross2(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[1] - a[1] * b[0]
}

fn vector2(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [b[0] - a[0], b[1] - a[1]]
}

/// real_math.h's `point_from_line2d`.
fn point_from_line2(p: [f32; 2], v: [f32; 2], t: f32) -> [f32; 2] {
    [t * v[0] + p[0], t * v[1] + p[1]]
}

fn magnitude2(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

/// real_math.h's `normalize2d`.
fn normalize2(v: &mut [f32; 2]) -> f32 {
    let magnitude = magnitude2(*v);
    if !(REAL_EPSILON > (magnitude - 0.0).abs()) {
        let s = 1.0 / magnitude;
        *v = [s * v[0], s * v[1]];
        magnitude
    } else {
        0.0
    }
}

/// The 2D structure tests' world: a structure BSP and the breakable
/// surfaces' flags.
pub struct Structure<'a> {
    bsp: Bsp<'a>,
    breakable_surface_flags: u32,
}

impl<'a> Structure<'a> {
    pub fn new(m: &'a dyn AiMemory, structure: u32, breakable_surface_flags: u32) -> Self {
        Structure { bsp: Bsp::new(m, structure), breakable_surface_flags }
    }

    fn m(&self) -> &dyn AiMemory {
        self.bsp.m
    }

    /// `path_pathfinding_surface`: NONE reads the byte before the array, as
    /// the game does; any other index past it is no walkable surface.
    fn pathfinding_surface(&self, surface_index: i32) -> u8 {
        let block = self.bsp.structure + sb::PATHFINDING_SURFACES as u32;
        let count = self.m().i32(block + tb::COUNT as u32);
        if (surface_index == NONE && count > 0) || (surface_index >= 0 && surface_index < count) {
            self.m().r8(self.m().r32(block + tb::ADDRESS as u32).wrapping_add(surface_index as u32))
        } else {
            0
        }
    }

    /// Whether a breakable surface stands (`BIT_VECTOR_TEST_FLAG` of the
    /// flags).
    fn breakable_standing(&self, surface_index: i32) -> bool {
        let index = self.m().r8(self.bsp.surface(surface_index) + cs::BREAKABLE_SURFACE_INDEX as u32) as u32;
        self.m().r32(self.breakable_surface_flags + (index >> 5) * 4) & (1 << (index & 31)) != 0
    }

    fn project(&self, surface_index: i32, point: [f32; 2]) -> [f32; 3] {
        self.bsp.surface_project_point2d(surface_index, PROJECTION_Z, true, point)
    }

    /// collision_bsp.c's `collision_surface_test_line2d` (vertically): where
    /// the line through `point` along `direction` enters and leaves the
    /// surface.
    fn surface_test_line2d(&self, surface_index: i32, point: [f32; 2], direction: [f32; 2]) -> SurfaceLine2d {
        let m = self.m();
        let bsp = &self.bsp;
        let mut result = SurfaceLine2d {
            enter_t: REAL_MIN,
            enter_edge_index: NONE,
            enter_surface_index: NONE,
            exit_t: f32::MAX,
            exit_edge_index: NONE,
            exit_surface_index: NONE,
        };
        if !bsp.valid_surface_index(surface_index) {
            return result;
        }
        let first_edge_index = m.i32(bsp.surface(surface_index) + cs::FIRST_EDGE_INDEX as u32);
        let mut edge_index = first_edge_index;
        if !bsp.edge_ring_continues(edge_index, 0) {
            return result;
        }
        let mut edge_count: i16 = 0;
        loop {
            let edge = bsp.edge(edge_index);
            let reverse = bsp.edge_surface(edge, 1) == surface_index;
            let vertex0 = bsp.vertex(bsp.edge_vertex(edge, 0));
            let vertex1 = bsp.vertex(bsp.edge_vertex(edge, 1));
            let edge_vector = [vertex1[0] - vertex0[0], vertex1[1] - vertex0[1]];
            let point_vector = [point[0] - vertex0[0], point[1] - vertex0[1]];
            let edge_cross = cross2(direction, edge_vector);
            let mut point_cross = cross2(edge_vector, point_vector);
            let far_surface = bsp.edge_surface(edge, usize::from(!reverse));
            if edge_cross != 0.0 {
                point_cross /= edge_cross;
                if (edge_cross < 0.0) != reverse {
                    if point_cross > result.enter_t {
                        result.enter_t = point_cross;
                        result.enter_edge_index = edge_index;
                        result.enter_surface_index = far_surface;
                    }
                } else if point_cross < result.exit_t {
                    result.exit_t = point_cross;
                    result.exit_edge_index = edge_index;
                    result.exit_surface_index = far_surface;
                }
            } else if (point_cross < 0.0) != reverse {
                result.enter_t = f32::MAX;
                result.enter_edge_index = edge_index;
                result.enter_surface_index = far_surface;
                result.exit_t = REAL_MIN;
                result.exit_edge_index = edge_index;
                result.exit_surface_index = far_surface;
            }
            edge_index = bsp.edge_next(edge, usize::from(reverse));
            if edge_index == first_edge_index {
                break;
            }
            edge_count += 1;
            if !bsp.edge_ring_continues(edge_index, edge_count) {
                break;
            }
        }
        result
    }

    /// A surface the ray crosses into, if (unless they're ignored)
    /// unbroken.
    fn surface_unbroken(&self, surface_index: i32, ignore_broken_surfaces: bool) -> bool {
        if !ignore_broken_surfaces && self.pathfinding_surface(surface_index) & PATHFINDING_SURFACE_BREAKABLE != 0 {
            return self.breakable_standing(surface_index);
        }
        true
    }

    /// `structure_test_ray2d`: how far the ray goes over the pathfinding
    /// surfaces, up to `distance` (whether it stopped short, at an edge).
    pub fn test_ray2d(
        &self,
        ignore_broken_surfaces: bool,
        point: [f32; 2],
        mut surface_index: i32,
        direction: [f32; 2],
        distance: f32,
    ) -> (bool, Ray2dResult) {
        let mut surface_result = self.surface_test_line2d(surface_index, point, direction);
        let surface_count = self.bsp.block_count(cb::SURFACES);
        let mut surface_step_count = 0;
        loop {
            let mut next_surface_index = NONE;
            if distance < surface_result.enter_t
                && self.pathfinding_surface(surface_result.enter_surface_index) != 0
                && self.surface_unbroken(surface_result.enter_surface_index, ignore_broken_surfaces)
            {
                next_surface_index = surface_result.enter_surface_index;
            }
            if next_surface_index == NONE
                && distance > surface_result.exit_t
                && self.pathfinding_surface(surface_result.exit_surface_index) != 0
                && self.surface_unbroken(surface_result.exit_surface_index, ignore_broken_surfaces)
            {
                next_surface_index = surface_result.exit_surface_index;
            }
            if next_surface_index == NONE {
                break;
            }
            surface_step_count += 1;
            if surface_step_count > surface_count {
                break;
            }
            surface_index = next_surface_index;
            surface_result = self.surface_test_line2d(surface_index, point, direction);
        }
        if distance < surface_result.enter_t {
            return (
                true,
                Ray2dResult { distance: surface_result.enter_t, surface_index, edge_index: surface_result.enter_edge_index },
            );
        }
        if distance > surface_result.exit_t {
            return (
                true,
                Ray2dResult { distance: surface_result.exit_t, surface_index, edge_index: surface_result.exit_edge_index },
            );
        }
        (false, Ray2dResult { distance, surface_index, edge_index: NONE })
    }

    /// `structure_surfaces_are_equivalent`: whether the two surfaces are at
    /// the same height (within 0.05) over the point.
    pub fn surfaces_are_equivalent(&self, destination_point: [f32; 2], destination_surface_index: i32, test_surface_index: i32) -> bool {
        if destination_surface_index == NONE || test_surface_index == NONE {
            return false;
        }
        let destination = self.project(destination_surface_index, destination_point);
        let test = self.project(test_surface_index, destination_point);
        ((destination[2] - test[2]) as f64).abs() < 0.05f32 as f64
    }

    /// The blocked result: at p0, on no surface.
    fn blocked(&self, p0: [f32; 2], p0_surface_index: i32) -> PathCollisionResult {
        PathCollisionResult { collision: 1, point: self.project(p0_surface_index, p0), surface_index: NONE, edge_index: NONE, t: 0.0 }
    }

    /// `structure_test_line2d`: whether the line from p0 to p1 is blocked
    /// on the walkable surfaces (and where), or reaches p1's surface (or,
    /// with NONE, wherever it ends).
    pub fn test_line2d(
        &self,
        ignore_broken_surfaces: bool,
        p0: [f32; 2],
        p0_surface_index: i32,
        p1: [f32; 2],
        p1_surface_index: i32,
    ) -> (bool, PathCollisionResult) {
        let m = self.m();
        let bsp = &self.bsp;
        let surface_count = bsp.block_count(cb::SURFACES);
        let vertex_count = bsp.block_count(cb::VERTICES);
        let mut surface_index = p0_surface_index;
        let mut recursed = false;
        let p0p1 = vector2(p0, p1);
        let mut surface_step_count = 0;
        'surfaces: loop {
            surface_step_count += 1;
            if surface_step_count > 2 * surface_count || !bsp.valid_surface_index(surface_index) {
                return (true, self.blocked(p0, p0_surface_index));
            }
            let surface = bsp.surface(surface_index);
            let first_edge_index = m.i32(surface + cs::FIRST_EDGE_INDEX as u32);
            let mut edge_index = first_edge_index;
            let mut point_in_surface = [0.0f32; 3];
            let mut edge_count: i16 = 0;
            let mut crossed_any = false;
            let mut reached_target = false;
            loop {
                if !bsp.edge_ring_continues(edge_index, edge_count) {
                    return (true, self.blocked(p0, p0_surface_index));
                }
                let edge = bsp.edge(edge_index);
                let vertices = [bsp.edge_vertex(edge, 0), bsp.edge_vertex(edge, 1)];
                if !vertices.iter().all(|&v| v >= 0 && v < vertex_count) {
                    return (true, self.blocked(p0, p0_surface_index));
                }
                let on_right_side = surface_index == bsp.edge_surface(edge, 1);
                let vertex0 = bsp.vertex(vertices[usize::from(!on_right_side)]);
                let vertex1 = bsp.vertex(vertices[usize::from(on_right_side)]);
                let e0 = [vertex0[0], vertex0[1]];
                let e1 = [vertex1[0], vertex1[1]];
                let e0e1 = vector2(e0, e1);
                let e0p1 = vector2(e0, p1);
                let p0e0 = vector2(p0, e0);
                let p0e1 = vector2(p0, e1);
                let far_surface = bsp.edge_surface(edge, usize::from(!on_right_side));
                if far_surface == p1_surface_index {
                    reached_target = true;
                }
                point_in_surface[0] += vertex0[0];
                point_in_surface[1] += vertex0[1];
                point_in_surface[2] += vertex0[2];
                edge_count += 1;
                if cross2(e0e1, e0p1) > 0.0 {
                    crossed_any = true;
                    if cross2(p0p1, p0e0) > 0.0 && cross2(p0e1, p0p1) > 0.0 {
                        let next_pathfinding_surface = self.pathfinding_surface(far_surface);
                        let mut passable = next_pathfinding_surface & PATHFINDING_SURFACE_WALKABLE != 0;
                        if !ignore_broken_surfaces && passable && next_pathfinding_surface & PATHFINDING_SURFACE_BREAKABLE != 0 {
                            passable = self.breakable_standing(far_surface);
                        }
                        if passable {
                            surface_index = far_surface;
                            continue 'surfaces;
                        }
                        let t = (cross2(e0e1, p0e0) - magnitude2(e0e1) * (1.0 / 128.0)) / cross2(e0e1, p0p1);
                        let p2d = point_from_line2(p0, p0p1, t);
                        return (
                            true,
                            PathCollisionResult { collision: 1, point: self.project(surface_index, p2d), surface_index, edge_index, t },
                        );
                    }
                }
                edge_index = bsp.edge_next(edge, usize::from(on_right_side));
                if edge_index == first_edge_index {
                    if crossed_any {
                        point_in_surface[0] /= edge_count as f32;
                        point_in_surface[1] /= edge_count as f32;
                        if !recursed && self.pathfinding_surface(surface_index) != 0 {
                            let (blocked, p0_result) =
                                self.test_line2d(ignore_broken_surfaces, [point_in_surface[0], point_in_surface[1]], surface_index, p0, NONE);
                            if !blocked {
                                recursed = true;
                                surface_index = p0_result.surface_index;
                                continue 'surfaces;
                            }
                        }
                        return (true, self.blocked(p0, p0_surface_index));
                    }
                    if surface_index == p1_surface_index || reached_target || p1_surface_index == NONE {
                        return (
                            false,
                            PathCollisionResult { collision: 0, point: self.project(surface_index, p1), surface_index, edge_index: NONE, t: 1.0 },
                        );
                    }
                    return (true, self.blocked(p0, p0_surface_index));
                }
            }
        }
    }

    /// `structure_surface_index_from_point`: the surface under `point`, as
    /// the line from the known point reaches it (the point moved to where
    /// it stops).
    pub fn surface_index_from_point(&self, ignore_broken_surfaces: bool, known_point: [f32; 2], known_surface_index: i32, point: &mut [f32; 2]) -> i32 {
        if known_surface_index == NONE {
            return NONE;
        }
        let (_, result) = self.test_line2d(ignore_broken_surfaces, known_point, known_surface_index, *point, NONE);
        *point = [result.point[0], result.point[1]];
        if result.surface_index == NONE { known_surface_index } else { result.surface_index }
    }

    /// One side of `structure_test_pill2d`: its line, and whether its
    /// collision counts.
    #[allow(clippy::too_many_arguments)]
    fn pill_side(
        &self,
        ignore_broken_surfaces: bool,
        side_p0: [f32; 2],
        side_p0_surface_index: i32,
        side_p1: [f32; 2],
        side_p1_surface_index: i32,
        p1: [f32; 2],
        p1_surface_index: i32,
        flags: u32,
    ) -> PathCollisionResult {
        if side_p0_surface_index == NONE {
            // (the rest zero, as the C now starts it)
            return PathCollisionResult::default();
        }
        let (blocked, mut result) = self.test_line2d(ignore_broken_surfaces, side_p0, side_p0_surface_index, side_p1, side_p1_surface_index);
        if blocked
            && result.surface_index != NONE
            && flags & TEST_PILL_ENDPOINT_NEAR_WALL_OK == 0
            && !self.test_line2d(ignore_broken_surfaces, [result.point[0], result.point[1]], result.surface_index, p1, p1_surface_index).0
        {
            result.collision = 0;
        }
        result
    }

    /// `structure_test_pill2d`: whether a circle of `radius` swept from p0
    /// to p1 is blocked (its two sides' lines; a collision within the
    /// radius of p1 doesn't count). With no direction, `previous` is the
    /// result (the C leaves it).
    #[allow(clippy::too_many_arguments)]
    pub fn test_pill2d(
        &self,
        ignore_broken_surfaces: bool,
        p0: [f32; 2],
        p0_surface_index: i32,
        p1: [f32; 2],
        p1_surface_index: i32,
        radius: f32,
        flags: u32,
        previous: PathCollisionResult,
    ) -> (bool, PathCollisionResult) {
        let direction = vector2(p0, p1);
        let mut perpendicular = [-direction[1], direction[0]];
        if !(normalize2(&mut perpendicular) > 0.0) {
            return (false, previous);
        }
        let mut left_p0 = point_from_line2(p0, perpendicular, radius);
        let left_p0_surface_index = self.surface_index_from_point(ignore_broken_surfaces, p0, p0_surface_index, &mut left_p0);
        let mut left_p1 = point_from_line2(p1, perpendicular, radius);
        let left_p1_surface_index = self.surface_index_from_point(ignore_broken_surfaces, p1, p1_surface_index, &mut left_p1);
        let mut right_p0 = point_from_line2(p0, perpendicular, -radius);
        let right_p0_surface_index = self.surface_index_from_point(ignore_broken_surfaces, p0, p0_surface_index, &mut right_p0);
        let mut right_p1 = point_from_line2(p1, perpendicular, -radius);
        let right_p1_surface_index = self.surface_index_from_point(ignore_broken_surfaces, p1, p1_surface_index, &mut right_p1);
        let left = self.pill_side(ignore_broken_surfaces, left_p0, left_p0_surface_index, left_p1, left_p1_surface_index, p1, p1_surface_index, flags);
        let right =
            self.pill_side(ignore_broken_surfaces, right_p0, right_p0_surface_index, right_p1, right_p1_surface_index, p1, p1_surface_index, flags);
        let best = match (left.collision != 0, right.collision != 0) {
            (true, true) => Some(if left.t < right.t { left } else { right }),
            (true, false) => Some(left),
            (false, true) => Some(right),
            (false, false) => None,
        };
        match best {
            Some(best) if !(distance_squared2(&best.point, p1) < radius * radius) => (true, best),
            _ => (false, left),
        }
    }
}

fn distance_squared2(point: &[f32; 3], p1: [f32; 2]) -> f32 {
    let v = [p1[0] - point[0], p1[1] - point[1]];
    v[0] * v[0] + v[1] * v[1]
}

/// `path_state_approach_point`: on the search's path to the point's
/// surface, the furthest node back the point sees in a straight line (the
/// start, if it sees that), and whether it is the start.
pub fn path_state_approach_point(
    m: &dyn AiMemory,
    state: u32,
    breakable_surface_flags: u32,
    end_point: [f32; 2],
    end_surface_index: i32,
) -> Option<(bool, [f32; 3])> {
    let state = State { m, state };
    let mut node_index = state.node_from_hash_table(end_surface_index);
    if node_index == NONE as i16 {
        return None;
    }
    let structure = Structure::new(m, state.structure(), breakable_surface_flags);
    let ignore_broken_surfaces = m.bool(state.input(pi::IGNORE_BROKEN_SURFACES));
    let mut node = state.node(node_index);
    loop {
        let parent_node_index = m.i16(node + pn::PARENT_NODE_INDEX as u32);
        if parent_node_index == NONE as i16 {
            break;
        }
        let parent = state.node(parent_node_index);
        let parent_entry = m.vector(parent + pn::ENTRY_POINT as u32);
        if structure
            .test_line2d(ignore_broken_surfaces, end_point, end_surface_index, [parent_entry[0], parent_entry[1]], m.i32(parent + pn::SURFACE_INDEX as u32))
            .0
        {
            break;
        }
        node_index = parent_node_index;
        node = state.node(node_index);
    }
    if m.i16(node + pn::PARENT_NODE_INDEX as u32) == NONE as i16 {
        Some((true, m.vector(state.input(pi::START_POINT))))
    } else {
        Some((false, m.vector(node + pn::ENTRY_POINT as u32)))
    }
}

// ---------- path_smoothing.c: the raw path's corners cut

/// path.h's `struct path_step`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PathStep {
    pub surface_index: i32,
    pub point: [f32; 3],
}

const MAXIMUM_SMOOTHED_PATH_STEPS: usize = 4;
/// real_math.c's `global_left2d`.
const LEFT2D: [f32; 2] = [0.0, 1.0];

/// real_math.c's `signed_angle_between_vectors2d` (its arccosine the
/// double one).
fn signed_angle_between_vectors2d(a: [f32; 2], b: [f32; 2]) -> f32 {
    let dot = a[0] * b[0] + a[1] * b[1];
    let pinned = if dot < -1.0 {
        -1.0
    } else if dot > 1.0 {
        1.0
    } else {
        dot
    };
    let mut result = (pinned as f64).acos() as f32;
    if (a[0] * b[1]) - (a[1] * b[0]) < 0.0 {
        result = -result;
    }
    result
}

impl Structure<'_> {
    /// `path_smoothing_edge_valid`: an edge of the bsp, with vertices that
    /// are the bsp's.
    fn smoothing_edge_valid(&self, edge_index: i32) -> bool {
        let bsp = &self.bsp;
        if !(edge_index >= 0 && edge_index < bsp.block_count(cb::EDGES)) {
            return false;
        }
        let edge = bsp.edge(edge_index);
        let vertex_count = bsp.block_count(cb::VERTICES);
        (0..2).all(|which| {
            let vertex = bsp.edge_vertex(edge, which);
            vertex >= 0 && vertex < vertex_count
        })
    }

    /// path_smoothing.c's `surface_is_walkable`.
    fn surface_is_walkable(&self, surface_index: i32, ignore_broken_surfaces: bool) -> bool {
        let block = self.bsp.structure + sb::PATHFINDING_SURFACES as u32;
        let count = self.m().i32(block + tb::COUNT as u32);
        if surface_index == NONE && count <= 0 {
            return false;
        }
        if surface_index != NONE && !(surface_index >= 0 && surface_index < count) {
            return false;
        }
        let flags = self.m().r8(self.m().r32(block + tb::ADDRESS as u32).wrapping_add(surface_index as u32));
        let mut walkable = flags & PATHFINDING_SURFACE_WALKABLE != 0;
        if !ignore_broken_surfaces && walkable && flags & PATHFINDING_SURFACE_BREAKABLE != 0 {
            walkable = self.breakable_standing(surface_index);
        }
        walkable
    }

    /// `find_turning_point`: walking the unwalkable boundary from the edge
    /// (clockwise or not), the corner the circle around the point must turn
    /// about.
    fn find_turning_point(
        &self,
        point: [f32; 2],
        radius: f32,
        first_edge_index: i32,
        clockwise: bool,
        ignore_broken_surfaces: bool,
    ) -> Option<[f32; 2]> {
        let bsp = &self.bsp;
        let edge_total = bsp.block_count(cb::EDGES);
        let mut starting_vertex_index = NONE;
        let mut loop_reference_vertex_index = NONE;
        let mut edge_index = first_edge_index;
        let mut boundary_step_count = 0;
        loop {
            boundary_step_count += 1;
            if boundary_step_count > 2 * edge_total + 1 {
                return None;
            }
            if !self.smoothing_edge_valid(edge_index) {
                return None;
            }
            let mut collision_edge = bsp.edge(edge_index);
            let side_flag = self.surface_is_walkable(bsp.edge_surface(collision_edge, 0), ignore_broken_surfaces);
            let vertex_a = bsp.vertex(bsp.edge_vertex(collision_edge, usize::from(side_flag)));
            let vertex_b = bsp.vertex(bsp.edge_vertex(collision_edge, usize::from(!side_flag)));
            let edge_dx = vertex_b[0] - vertex_a[0];
            let edge_dy = vertex_b[1] - vertex_a[1];
            let mut edge_direction = [edge_dy, -edge_dx];
            let mut valid = false;
            normalize2(&mut edge_direction);
            let positive_point = point_from_line2(point, edge_direction, radius);
            let negative_point = point_from_line2(point, edge_direction, -radius);
            let vertex_to_positive = [vertex_a[0] - positive_point[0], vertex_a[1] - positive_point[1]];
            let vertex_to_negative = [vertex_a[0] - negative_point[0], vertex_a[1] - negative_point[1]];
            let side_test = vertex_to_positive[1] * edge_dy + vertex_to_positive[0] * edge_dx < 0.0;
            if side_test == clockwise && vertex_to_positive[0] * edge_dy - vertex_to_positive[1] * edge_dx < 0.0 {
                valid = true;
            }
            if vertex_to_negative[0] * edge_dy - vertex_to_negative[1] * edge_dx < 0.0 {
                valid = true;
            }
            if starting_vertex_index == NONE {
                valid = true;
            }
            let xor_flag = valid != side_flag;
            let next_vertex_index = bsp.edge_vertex(collision_edge, usize::from(xor_flag == clockwise));
            if next_vertex_index == loop_reference_vertex_index {
                let vertex = bsp.vertex(next_vertex_index);
                return Some([vertex[0], vertex[1]]);
            }
            if next_vertex_index == starting_vertex_index {
                return None;
            }
            if starting_vertex_index == NONE {
                starting_vertex_index = next_vertex_index;
            }
            let chain_start_edge_index = edge_index;
            let mut fan_step_count = 0;
            loop {
                fan_step_count += 1;
                if fan_step_count > edge_total {
                    return None;
                }
                let matches_end = next_vertex_index == bsp.edge_vertex(collision_edge, 1);
                let candidate_surface_index = bsp.edge_surface(collision_edge, usize::from(!matches_end));
                let refined_side = self.surface_is_walkable(candidate_surface_index, ignore_broken_surfaces);
                if refined_side == clockwise {
                    break;
                }
                edge_index = bsp.edge_next(collision_edge, usize::from(!matches_end));
                if !self.smoothing_edge_valid(edge_index) {
                    return None;
                }
                collision_edge = bsp.edge(edge_index);
                if edge_index == chain_start_edge_index {
                    return None;
                }
            }
            loop_reference_vertex_index = next_vertex_index;
        }
    }
}

/// `find_tangent_point`: where the line from the point touches the circle
/// (on the clockwise side or not), or the circle's point towards it.
fn find_tangent_point(point: [f32; 2], center: [f32; 2], radius: f32, clockwise: bool) -> [f32; 2] {
    let center_to_point = vector2(center, point);
    let distance_squared = center_to_point[0] * center_to_point[0] + center_to_point[1] * center_to_point[1];
    let inverse_distance_squared = radius / distance_squared;
    let mut tangent_length = distance_squared - radius * radius;
    if tangent_length > 0.0 {
        tangent_length = tangent_length.sqrt();
        let tangent_points = [
            [
                (center_to_point[0] * radius + center_to_point[1] * tangent_length) * inverse_distance_squared + center[0],
                (center_to_point[1] * radius - center_to_point[0] * tangent_length) * inverse_distance_squared + center[1],
            ],
            [
                (center_to_point[0] * radius - center_to_point[1] * tangent_length) * inverse_distance_squared + center[0],
                (center_to_point[1] * radius + center_to_point[0] * tangent_length) * inverse_distance_squared + center[1],
            ],
        ];
        let tangent_vectors = [vector2(point, tangent_points[0]), vector2(point, tangent_points[1])];
        tangent_points[usize::from((cross2(tangent_vectors[0], tangent_vectors[1]) > 0.0) != clockwise)]
    } else {
        let mut radius_vector = vector2(center, point);
        if normalize2(&mut radius_vector) == 0.0 {
            radius_vector = LEFT2D;
        }
        point_from_line2(center, radius_vector, radius)
    }
}

/// `find_avoidance_point`: where the two tangents meet (or, too far out, a
/// radius on from the first tangent point).
fn find_avoidance_point(tangent_points: &[[f32; 2]; 2], center: [f32; 2], start_point: [f32; 2], radius: f32) -> [f32; 2] {
    let center_to_tangent = [vector2(center, tangent_points[0]), vector2(center, tangent_points[1])];
    let cross = cross2(center_to_tangent[0], center_to_tangent[1]);
    if !((cross as f64).abs() < REAL_EPSILON as f64) {
        let scale = radius * radius / cross;
        let avoidance_j = (center_to_tangent[0][0] - center_to_tangent[1][0]) * scale + center[1];
        let avoidance_i = center[0] - (center_to_tangent[0][1] - center_to_tangent[1][1]) * scale;
        let direction = [avoidance_i - center[0], avoidance_j - center[1]];
        if !(direction[0] * direction[0] + direction[1] * direction[1] > radius * radius * 4.0) {
            return [avoidance_i, avoidance_j];
        }
    }
    let mut direction = [tangent_points[0][0] - start_point[0], tangent_points[0][1] - start_point[1]];
    let mut magnitude = ((direction[0] * direction[0] + direction[1] * direction[1]) as f64).sqrt() as f32;
    if !(REAL_EPSILON > (magnitude - 0.0).abs()) {
        let scale = 1.0 / magnitude;
        direction = [direction[0] * scale, direction[1] * scale];
    } else {
        magnitude = 0.0;
    }
    if magnitude == 0.0 {
        direction = LEFT2D;
    }
    [direction[0] * radius + tangent_points[0][0], direction[1] * radius + tangent_points[0][1]]
}

/// `choose_turning_point`: the turning point whose detour turns least
/// (whether it is the clockwise one).
fn choose_turning_point(
    start_point: [f32; 2],
    clockwise_turning_point: [f32; 2],
    counterclockwise_turning_point: [f32; 2],
    unobstructed_path_point: [f32; 2],
    obstructed_path_point: [f32; 2],
) -> (bool, [f32; 2]) {
    let normalized = |a: [f32; 2], b: [f32; 2]| {
        let mut v = vector2(a, b);
        normalize2(&mut v);
        v
    };
    let start_to_clockwise = normalized(clockwise_turning_point, start_point);
    let clockwise_to_unobstructed = normalized(clockwise_turning_point, unobstructed_path_point);
    let clockwise_to_obstructed = normalized(clockwise_turning_point, obstructed_path_point);
    let start_to_counterclockwise = normalized(counterclockwise_turning_point, start_point);
    let counterclockwise_to_unobstructed = normalized(counterclockwise_turning_point, unobstructed_path_point);
    let counterclockwise_to_obstructed = normalized(counterclockwise_turning_point, obstructed_path_point);
    let clockwise_turn = signed_angle_between_vectors2d(clockwise_to_unobstructed, clockwise_to_obstructed)
        + signed_angle_between_vectors2d(start_to_clockwise, clockwise_to_unobstructed);
    let counterclockwise_turn = signed_angle_between_vectors2d(counterclockwise_to_unobstructed, counterclockwise_to_obstructed);
    if clockwise_turn > -(counterclockwise_turn + signed_angle_between_vectors2d(start_to_counterclockwise, counterclockwise_to_unobstructed)) {
        (true, clockwise_turning_point)
    } else {
        (false, counterclockwise_turning_point)
    }
}

/// `path_smooth`: from the start, past the raw steps a 0.3 circle can
/// sweep to in a line, turning about the corners that block it (four
/// steps at most; whether the smoothed steps finish the path is cleared
/// when they don't).
pub fn path_smooth(
    m: &dyn AiMemory,
    state: u32,
    breakable_surface_flags: u32,
    raw_steps: &[PathStep],
    steps_finish_path: &mut bool,
) -> Vec<PathStep> {
    if raw_steps.len() <= 1 {
        return raw_steps.iter().take(1).copied().collect();
    }
    let state = State { m, state };
    let structure = Structure::new(m, state.structure(), breakable_surface_flags);
    let ignore_broken_surfaces = m.bool(state.input(pi::IGNORE_BROKEN_SURFACES));
    let start_point = m.vector(state.input(pi::START_POINT));
    let mut current_position = [start_point[0], start_point[1]];
    let mut current_surface_index = m.i32(state.input(pi::START_SURFACE_INDEX));
    let mut smoothed: Vec<PathStep> = Vec::new();
    let mut step_index = 1;
    let mut collision_result = PathCollisionResult::default();
    let finishes = 'smoothing: loop {
        let mut collision_step_index: Option<usize> = None;
        let mut collision_edge_index = NONE;
        let mut collision_active = false;
        for (i, raw_step) in raw_steps.iter().enumerate().skip(step_index) {
            let (blocked, result) = structure.test_pill2d(
                ignore_broken_surfaces,
                current_position,
                current_surface_index,
                [raw_step.point[0], raw_step.point[1]],
                raw_step.surface_index,
                0.3,
                TEST_PILL_ENDPOINT_NEAR_WALL_OK,
                collision_result,
            );
            collision_result = result;
            if blocked {
                if !collision_active {
                    collision_edge_index = collision_result.edge_index;
                    collision_step_index = Some(i);
                    collision_active = true;
                }
            } else if collision_active {
                collision_step_index = None;
                collision_edge_index = NONE;
                collision_active = false;
            }
        }
        if !collision_active || collision_edge_index == NONE {
            break 'smoothing true;
        }
        let collision_step_index = collision_step_index.unwrap_or(0);
        let clockwise_turning_point =
            structure.find_turning_point(current_position, 0.3, collision_edge_index, true, ignore_broken_surfaces);
        let counterclockwise_turning_point =
            structure.find_turning_point(current_position, 0.3, collision_edge_index, false, ignore_broken_surfaces);
        let (Some(clockwise_turning_point), Some(counterclockwise_turning_point)) =
            (clockwise_turning_point, counterclockwise_turning_point)
        else {
            break 'smoothing false;
        };
        let unobstructed = raw_steps[collision_step_index - 1].point;
        let obstructed = raw_steps[collision_step_index].point;
        let (chose_clockwise, chosen_center) = choose_turning_point(
            current_position,
            clockwise_turning_point,
            counterclockwise_turning_point,
            [unobstructed[0], unobstructed[1]],
            [obstructed[0], obstructed[1]],
        );
        let tangent_points = [
            find_tangent_point(current_position, chosen_center, 0.35, chose_clockwise),
            find_tangent_point([obstructed[0], obstructed[1]], chosen_center, 0.35, !chose_clockwise),
        ];
        let avoidance_point = find_avoidance_point(&tangent_points, chosen_center, current_position, 0.35);
        let known_point = current_position;
        current_position = avoidance_point;
        current_surface_index =
            structure.surface_index_from_point(ignore_broken_surfaces, known_point, current_surface_index, &mut current_position);
        smoothed.push(PathStep { surface_index: current_surface_index, point: structure.project(current_surface_index, current_position) });
        if smoothed.len() >= MAXIMUM_SMOOTHED_PATH_STEPS {
            break 'smoothing false;
        }
        step_index = collision_step_index;
    };
    if finishes {
        smoothed.push(raw_steps[raw_steps.len() - 1]);
    } else {
        *steps_finish_path = false;
    }
    smoothed
}

// ---------- path_obstacles.c and path_obstacle_avoidance.c: the way round
// the objects in the path

use crate::layout::{
    obstacle_disc as od, obstacle_path as op, obstacle_path_step as ops, obstacles as obs, path_result as pr, path_step as pst,
};

const MAXIMUM_DISC_COUNT: i16 = 128;
const MAXIMUM_OBSTACLE_AVOIDANCE_STEPS: i16 = 128;
const DISC_OPTIONAL: i16 = 1 << 0;

/// `struct obstacles` in the game's memory (or a buffer of Rust's).
#[derive(Clone, Copy)]
struct Obstacles<'a> {
    m: &'a dyn AiMemory,
    at: u32,
}

impl Obstacles<'_> {
    fn disc_count(&self) -> i16 {
        self.m.i16(self.at + obs::DISC_COUNT as u32)
    }
    fn disc(&self, index: i16) -> u32 {
        self.at + obs::DISCS as u32 + (index as i32 as u32).wrapping_mul(od::SIZEOF as u32)
    }
    fn center(&self, index: i16) -> [f32; 2] {
        let disc = self.disc(index);
        [self.m.f32(disc + od::CENTER as u32), self.m.f32(disc + od::CENTER as u32 + 4)]
    }
    fn radius(&self, index: i16) -> f32 {
        self.m.f32(self.disc(index) + od::RADIUS as u32)
    }
    fn obstacle_index(&self, index: i16) -> i16 {
        self.m.i16(self.disc(index) + od::OBSTACLE_INDEX as u32)
    }

    /// `obstacles_new`.
    fn new(&self) {
        self.m.set_i16(self.at + obs::OBSTACLE_COUNT as u32, 0);
        self.m.set_i16(self.at + obs::DISC_COUNT as u32, 0);
        self.m.set_i16(self.at + obs::DISC_OPTIONAL_COUNT as u32, 0);
    }

    /// `obstacles_add_disc`.
    fn add_disc(&self, object_index: i32, flags: i16, center: [f32; 3], radius: f32) -> bool {
        let m = self.m;
        let disc_count = self.disc_count();
        if disc_count == MAXIMUM_DISC_COUNT {
            return false;
        }
        m.set_i16(self.at + obs::DISC_COUNT as u32, disc_count + 1);
        let disc = self.disc(disc_count);
        if flags & DISC_OPTIONAL != 0 {
            let optional = self.at + obs::DISC_OPTIONAL_COUNT as u32;
            m.set_i16(optional, m.i16(optional) + 1);
        }
        m.set_i16(disc + od::FLAGS as u32, flags);
        m.w32(disc + od::OBJECT_INDEX as u32, object_index as u32);
        m.set_i16(disc + od::OBSTACLE_INDEX as u32, NONE as i16);
        m.set_f32(disc + od::CENTER as u32, center[0]);
        m.set_f32(disc + od::CENTER as u32 + 4, center[1]);
        m.set_f32(disc + od::RADIUS as u32, radius);
        m.set_f32(disc + od::HEIGHT as u32, center[2]);
        true
    }

    /// `obstacles_test_circle`.
    fn test_circle(&self, ignore_disc_index: i16, center: [f32; 2], radius: f32) -> i16 {
        for disc_index in 0..self.disc_count() {
            if disc_index != ignore_disc_index && point_in_circle(center, self.center(disc_index), self.radius(disc_index) + radius) {
                return disc_index;
            }
        }
        NONE as i16
    }

    /// `obstacles_test_pill`: the nearest disc the swept circle meets
    /// (distance, disc, obstacle).
    #[allow(clippy::too_many_arguments)]
    fn test_pill(
        &self,
        ignore_disc_index: i16,
        point: [f32; 2],
        direction: [f32; 2],
        radius: f32,
        distance: f32,
        ignore_optional: bool,
    ) -> (bool, f32, i16, i16) {
        let mut result = (distance, NONE as i16, NONE as i16);
        for disc_index in 0..self.disc_count() {
            if disc_index == ignore_disc_index {
                continue;
            }
            if ignore_optional && self.m.i16(self.disc(disc_index) + od::FLAGS as u32) & DISC_OPTIONAL != 0 {
                continue;
            }
            if let Some(disc_distance) = circle_intersect_ray(self.center(disc_index), self.radius(disc_index) + radius, point, direction)
                && result.0 > disc_distance
            {
                result = (disc_distance, disc_index, self.obstacle_index(disc_index));
            }
        }
        (result.1 != NONE as i16, result.0, result.1, result.2)
    }

    /// `obstacles_disc_tangents`: the right and left directions past the
    /// disc, and how far.
    fn disc_tangents(&self, disc_index: i16, point: [f32; 2], radius: f32) -> ([f32; 2], [f32; 2], f32) {
        let mut direction = vector2(point, self.center(disc_index));
        let distance = normalize2(&mut direction);
        circle_tangents(direction, distance, radius + self.radius(disc_index) + 0.00390625)
    }

    /// `obstacles_disc_neighborhood`: the discs that touch the seed's,
    /// through each other (a bit vector of 128).
    fn disc_neighborhood(&self, radius: f32, seed_disc_index: i16) -> [u32; 4] {
        let mut flags = [0u32; 4];
        if seed_disc_index == NONE as i16 {
            return flags;
        }
        let set = |flags: &mut [u32; 4], index: i16| flags[(index >> 5) as usize] |= 1 << (index & 31);
        let test = |flags: &[u32; 4], index: i16| flags[(index >> 5) as usize] & (1 << (index & 31)) != 0;
        set(&mut flags, seed_disc_index);
        let mut stack = vec![seed_disc_index];
        while let Some(current) = stack.pop() {
            for disc_index in 0..self.disc_count() {
                if !test(&flags, disc_index)
                    && point_in_circle(
                        self.center(current),
                        self.center(disc_index),
                        (self.radius(disc_index) + radius) + (self.radius(current) + radius),
                    )
                {
                    set(&mut flags, disc_index);
                    stack.push(disc_index);
                }
            }
        }
        flags
    }

    /// `obstacles_recompute`: the discs grouped into obstacles (those that
    /// touch, with the radius between).
    fn recompute(&self, radius: f32) {
        let m = self.m;
        m.set_i16(self.at + obs::OBSTACLE_COUNT as u32, 0);
        let disc_count = self.disc_count();
        for disc_index in 0..disc_count {
            m.set_i16(self.disc(disc_index) + od::OBSTACLE_INDEX as u32, NONE as i16);
        }
        for disc_index in 0..disc_count {
            if self.obstacle_index(disc_index) == NONE as i16 {
                let count_address = self.at + obs::OBSTACLE_COUNT as u32;
                let obstacle_index = m.i16(count_address);
                m.set_i16(count_address, obstacle_index + 1);
                let flags = self.disc_neighborhood(radius, disc_index);
                for neighbor in 0..disc_count {
                    if flags[(neighbor >> 5) as usize] & (1 << (neighbor & 31)) != 0 {
                        m.set_i16(self.disc(neighbor) + od::OBSTACLE_INDEX as u32, obstacle_index);
                    }
                }
            }
        }
    }

    /// `obstacle_from_disc`.
    fn obstacle_from_disc(&self, disc_index: i16) -> i16 {
        if disc_index != NONE as i16 { self.obstacle_index(disc_index) } else { NONE as i16 }
    }
}

/// real_math.h's `point_in_circle`.
fn point_in_circle(point: [f32; 2], center: [f32; 2], radius: f32) -> bool {
    let v = vector2(point, center);
    v[0] * v[0] + v[1] * v[1] <= radius * radius
}

/// path_obstacles.c's `circle_intersect_ray`.
fn circle_intersect_ray(center: [f32; 2], radius: f32, point: [f32; 2], direction: [f32; 2]) -> Option<f32> {
    let offset = vector2(point, center);
    let projection = direction[0] * offset[0] + direction[1] * offset[1];
    if projection > 0.0 {
        let distance_squared_minus_radius_squared = (offset[0] * offset[0] + offset[1] * offset[1]) - radius * radius;
        if distance_squared_minus_radius_squared <= 0.0 {
            return Some(0.0);
        }
        let discriminant = projection * projection - distance_squared_minus_radius_squared;
        if discriminant >= 0.0 {
            return Some(projection - discriminant.sqrt());
        }
    }
    None
}

/// real_math.h's `rotate_vector2d`.
fn rotate_vector2d(v: [f32; 2], sine: f32, cosine: f32) -> [f32; 2] {
    let j = sine * v[0] + cosine * v[1];
    let i = cosine * v[0] - sine * v[1];
    [i, j]
}

/// path_obstacles.c's `circle_tangents`.
fn circle_tangents(direction: [f32; 2], distance: f32, radius: f32) -> ([f32; 2], [f32; 2], f32) {
    let mut sine = radius / distance;
    sine = if sine > 1.0 { 1.0 } else { sine };
    let cosine = (1.0 - sine * sine).sqrt();
    (rotate_vector2d(direction, -sine, cosine), rotate_vector2d(direction, sine, cosine), cosine * distance)
}

/// path_obstacle_avoidance.c's `struct path_test_pill2d_result`.
#[derive(Debug, Clone, Copy)]
struct AvoidancePill {
    distance: f32,
    surface_index: i32,
    edge_index: i32,
    disc_index: i16,
    obstacle_index: i16,
}

/// `struct obstacle_path` in the game's memory (or a buffer of Rust's).
struct ObstaclePath<'a> {
    m: &'a dyn AiMemory,
    at: u32,
    obstacles: Obstacles<'a>,
    structure: &'a Structure<'a>,
}

impl ObstaclePath<'_> {
    fn field(&self, offset: usize) -> u32 {
        self.at + offset as u32
    }
    fn step(&self, index: i16) -> u32 {
        self.at + op::STEPS as u32 + (index as i32 as u32).wrapping_mul(ops::SIZEOF as u32)
    }
    fn step_count(&self) -> i16 {
        self.m.i16(self.field(op::STEP_COUNT))
    }
    fn heap_count(&self) -> i16 {
        self.m.i16(self.field(op::HEAP_COUNT))
    }
    fn heap(&self, index: i16) -> u32 {
        self.at + op::HEAP as u32 + (index as i32 as u32).wrapping_mul(2)
    }
    fn radius(&self) -> f32 {
        self.m.f32(self.field(op::RADIUS))
    }
    fn ignore_broken_surfaces(&self) -> bool {
        self.m.bool(self.field(op::IGNORE_BROKEN_SURFACES))
    }
    fn ignore_optional(&self) -> bool {
        self.m.bool(self.field(op::IGNORE_OPTIONAL))
    }
    fn goal_step_index(&self) -> i16 {
        self.m.i16(self.field(op::GOAL_STEP_INDEX))
    }
    fn step_point(&self, index: i16) -> [f32; 2] {
        let step = self.step(index);
        [self.m.f32(step + ops::POINT as u32), self.m.f32(step + ops::POINT as u32 + 4)]
    }
    fn step_direction(&self, index: i16) -> [f32; 2] {
        let step = self.step(index);
        [self.m.f32(step + ops::DIRECTION as u32), self.m.f32(step + ops::DIRECTION as u32 + 4)]
    }
    fn step_f32(&self, index: i16, offset: usize) -> f32 {
        self.m.f32(self.step(index) + offset as u32)
    }
    fn step_i16(&self, index: i16, offset: usize) -> i16 {
        self.m.i16(self.step(index) + offset as u32)
    }
    fn step_surface(&self, index: i16) -> i32 {
        self.m.i32(self.step(index) + ops::SURFACE_INDEX as u32)
    }

    /// `heap_cost`.
    fn heap_cost(&self, heap_index: i16) -> f32 {
        let step_index = self.m.i16(self.heap(heap_index));
        self.step_f32(step_index, ops::TOTAL_DISTANCE)
    }

    fn heap_swap(&self, a: i16, b: i16) {
        let step_index = self.m.i16(self.heap(a));
        self.m.set_i16(self.heap(a), self.m.i16(self.heap(b)));
        self.m.set_i16(self.heap(b), step_index);
    }

    /// `heap_up`.
    fn heap_up(&self, mut heap_index: i16) {
        while heap_index > 0 {
            let parent_index = (heap_index - 1) >> 1;
            if self.heap_cost(parent_index) > self.heap_cost(heap_index) {
                self.heap_swap(parent_index, heap_index);
                heap_index = parent_index;
            } else {
                break;
            }
        }
    }

    /// `heap_down`.
    fn heap_down(&self, mut heap_index: i16) {
        if heap_index >= self.heap_count() {
            return;
        }
        loop {
            let mut smallest_index = heap_index;
            let left_index = 2 * heap_index + 1;
            let right_index = 2 * (heap_index + 1);
            if left_index < self.heap_count() && self.heap_cost(heap_index) > self.heap_cost(left_index) {
                smallest_index = left_index;
            }
            if right_index < self.heap_count() && self.heap_cost(smallest_index) > self.heap_cost(right_index) {
                smallest_index = right_index;
            }
            if smallest_index == heap_index {
                break;
            }
            self.heap_swap(smallest_index, heap_index);
            heap_index = smallest_index;
        }
    }

    /// `heap_insert`.
    fn heap_insert(&self, step_index: i16) -> bool {
        let heap_index = self.heap_count();
        if heap_index >= MAXIMUM_OBSTACLE_AVOIDANCE_STEPS {
            return false;
        }
        self.m.set_i16(self.field(op::HEAP_COUNT), heap_index + 1);
        self.m.set_i16(self.heap(heap_index), step_index);
        self.heap_up(heap_index);
        true
    }

    /// `heap_remove`.
    fn heap_remove(&self) -> i16 {
        if self.heap_count() <= 0 {
            return NONE as i16;
        }
        let heap_count = self.heap_count() - 1;
        self.m.set_i16(self.field(op::HEAP_COUNT), heap_count);
        let step_index = self.m.i16(self.heap(0));
        self.m.set_i16(self.heap(0), self.m.i16(self.heap(heap_count)));
        self.heap_down(0);
        step_index
    }

    /// `path_add_step`: a step to the point (unless it doubles back round
    /// an obstacle it wraps), NONE if not.
    #[allow(clippy::too_many_arguments)]
    fn add_step(
        &self,
        point: [f32; 2],
        surface_index: i32,
        obstacle_index: i16,
        obstacle_direction_index: u8,
        previous_distance: f32,
        previous_step_index: i16,
    ) -> i16 {
        let m = self.m;
        if self.step_count() >= MAXIMUM_OBSTACLE_AVOIDANCE_STEPS {
            return NONE as i16;
        }
        let goal = [m.f32(self.field(op::GOAL)), m.f32(self.field(op::GOAL) + 4)];
        let goal_obstacle_index = m.i16(self.field(op::GOAL_OBSTACLE_INDEX));
        let mut blocks_goal = false;
        let direction = vector2(point, goal);
        let mut chain_step_index = previous_step_index;
        while chain_step_index != NONE as i16 {
            let chain_step = self.step(chain_step_index);
            if m.i16(chain_step + ops::OBSTACLE_INDEX as u32) == obstacle_index {
                if m.r8(chain_step + ops::OBSTACLE_DIRECTION_INDEX as u32) != obstacle_direction_index {
                    return NONE as i16;
                }
                chain_step_index = m.i16(chain_step + ops::PREVIOUS_STEP_INDEX as u32);
            } else {
                if obstacle_index == goal_obstacle_index && goal_obstacle_index != NONE as i16 {
                    let obstructed = |side: u8| chain_step + ops::OBSTRUCTED_GOAL_STEP_INDICES as u32 + 2 * side as u32;
                    let opposite_step_index = m.i16(obstructed(u8::from(obstacle_direction_index == 0)));
                    blocks_goal = true;
                    if opposite_step_index != NONE as i16 {
                        let opposite_direction = self.step_direction(opposite_step_index);
                        let previous_direction = self.step_direction(previous_step_index);
                        if opposite_direction[0] * direction[0] + opposite_direction[1] * direction[1] > 0.0
                            && cross2(opposite_direction, previous_direction) * cross2(opposite_direction, direction) < 0.0
                        {
                            return NONE as i16;
                        }
                    }
                    let same = m.i16(obstructed(obstacle_direction_index));
                    if same == previous_step_index || same == NONE as i16 {
                        m.set_i16(obstructed(obstacle_direction_index), self.step_count());
                    }
                }
                break;
            }
        }
        let step_index = self.step_count();
        m.set_i16(self.field(op::STEP_COUNT), step_index + 1);
        let step = self.step(step_index);
        m.set_f32(step + ops::POINT as u32, point[0]);
        m.set_f32(step + ops::POINT as u32 + 4, point[1]);
        m.w32(step + ops::SURFACE_INDEX as u32, surface_index as u32);
        let mut normalized = direction;
        let distance = normalize2(&mut normalized);
        m.set_f32(step + ops::DIRECTION as u32, normalized[0]);
        m.set_f32(step + ops::DIRECTION as u32 + 4, normalized[1]);
        m.set_f32(step + ops::DISTANCE as u32, distance);
        m.set_f32(step + ops::TOTAL_DISTANCE as u32, distance + previous_distance);
        m.set_i16(step + ops::OBSTACLE_INDEX as u32, obstacle_index);
        m.w8(step + ops::OBSTACLE_DIRECTION_INDEX as u32, obstacle_direction_index);
        m.set_i16(step + ops::PREVIOUS_STEP_INDEX as u32, previous_step_index);
        m.w32(step + ops::OBSTRUCTED_GOAL_STEP_INDICES as u32, 0xFFFF_FFFF);
        if blocks_goal && distance < m.f32(self.field(op::BEST_GOAL_BLOCKED_DISTANCE)) {
            m.set_f32(self.field(op::BEST_GOAL_BLOCKED_DISTANCE), distance);
            m.set_i16(self.field(op::BEST_GOAL_BLOCKED_STEP_INDEX), step_index);
        }
        self.heap_insert(step_index);
        step_index
    }

    /// `path_new`.
    #[allow(clippy::too_many_arguments)]
    fn new(
        &self,
        structure_address: u32,
        ignore_broken_surfaces: bool,
        radius: f32,
        start: [f32; 2],
        start_surface_index: i32,
        goal: [f32; 2],
        goal_surface_index: i32,
        finishing_path: bool,
        ignore_optional: bool,
    ) {
        let m = self.m;
        m.w32(self.field(op::OBSTACLES), self.obstacles.at);
        m.set_f32(self.field(op::RADIUS), radius);
        m.w32(self.field(op::STRUCTURE), structure_address);
        m.set_bool(self.field(op::IGNORE_BROKEN_SURFACES), ignore_broken_surfaces);
        m.set_bool(self.field(op::GOAL_FOUND_EXACTLY), false);
        m.set_f32(self.field(op::GOAL), goal[0]);
        m.set_f32(self.field(op::GOAL) + 4, goal[1]);
        m.w32(self.field(op::GOAL_SURFACE_INDEX), goal_surface_index as u32);
        let goal_obstacle_index = self.obstacles.obstacle_from_disc(self.obstacles.test_circle(NONE as i16, goal, radius));
        m.set_i16(self.field(op::GOAL_OBSTACLE_INDEX), goal_obstacle_index);
        m.set_bool(self.field(op::FINISHING), finishing_path);
        m.set_i16(self.field(op::GOAL_STEP_INDEX), NONE as i16);
        m.set_f32(self.field(op::BEST_GOAL_BLOCKED_DISTANCE), f32::MAX);
        m.set_i16(self.field(op::BEST_GOAL_BLOCKED_STEP_INDEX), NONE as i16);
        m.set_bool(self.field(op::IGNORE_OPTIONAL), ignore_optional);
        m.set_i16(self.field(op::STEP_COUNT), 0);
        m.set_i16(self.field(op::HEAP_COUNT), 0);
        self.add_step(start, start_surface_index, NONE as i16, 0, 0.0, NONE as i16);
    }

    /// path_obstacle_avoidance.c's `path_test_pill2d`: how far the circle
    /// sweeps over the structure and past the discs.
    #[allow(clippy::too_many_arguments)]
    fn test_pill2d(
        &self,
        ignore_disc_index: i16,
        point: [f32; 2],
        surface_index: i32,
        direction: [f32; 2],
        radius: f32,
        distance: f32,
        ignore_structure: bool,
        endpoint_is_goal: bool,
        ignore_optional: bool,
    ) -> (bool, AvoidancePill) {
        let structure = self.structure;
        let ignore_broken_surfaces = self.ignore_broken_surfaces();
        let mut result =
            AvoidancePill { distance, surface_index: NONE, edge_index: NONE, disc_index: NONE as i16, obstacle_index: NONE as i16 };
        if endpoint_is_goal {
            result.distance = distance - radius;
        }
        if !ignore_structure {
            let (hit, ray) = structure.test_ray2d(ignore_broken_surfaces, point, surface_index, direction, result.distance);
            if hit && result.distance > ray.distance {
                result.distance = ray.distance;
                result.edge_index = ray.edge_index;
            }
            let perpendicular = [-direction[1], direction[0]];
            let left_point = point_from_line2(point, perpendicular, radius);
            let (_, left) = structure.test_ray2d(ignore_broken_surfaces, point, surface_index, perpendicular, radius);
            let (hit, left) = structure.test_ray2d(ignore_broken_surfaces, left_point, left.surface_index, direction, result.distance);
            if hit && result.distance > left.distance {
                result.distance = left.distance;
                result.edge_index = left.edge_index;
            }
            let right_point = point_from_line2(point, perpendicular, -radius);
            // (the game's: towards the left again)
            let (_, right) = structure.test_ray2d(ignore_broken_surfaces, point, surface_index, perpendicular, radius);
            let (hit, right) = structure.test_ray2d(ignore_broken_surfaces, right_point, right.surface_index, direction, result.distance);
            if hit && result.distance > right.distance {
                result.distance = right.distance;
                result.edge_index = right.edge_index;
            }
        }
        let (hit, pill_distance, disc_index, obstacle_index) =
            self.obstacles.test_pill(ignore_disc_index, point, direction, radius, result.distance, ignore_optional);
        if hit && result.distance > pill_distance {
            result.distance = pill_distance;
            result.edge_index = NONE;
            result.disc_index = disc_index;
            result.obstacle_index = obstacle_index;
        }
        let hit = if result.edge_index == NONE && result.disc_index == NONE as i16 {
            result.distance = distance;
            false
        } else {
            true
        };
        let (_, ray) = structure.test_ray2d(ignore_broken_surfaces, point, surface_index, direction, result.distance);
        result.surface_index = ray.surface_index;
        (hit, result)
    }

    /// `path_add_steps`: round the disc the step met (and those it meets on
    /// the way), the steps past each side.
    fn add_steps(&self, step_index: i16, seed_disc_index: i16) {
        let radius = self.radius();
        let ignore_optional = self.ignore_optional();
        let mut visited = [0u32; 4];
        visited[(seed_disc_index >> 5) as usize] |= 1 << (seed_disc_index & 31);
        let mut stack = vec![seed_disc_index];
        while let Some(disc_index) = stack.pop() {
            let obstacle_index = self.obstacles.obstacle_from_disc(disc_index);
            let step_point = self.step_point(step_index);
            let (right, left, mut tangent_distance) = self.obstacles.disc_tangents(disc_index, step_point, radius);
            if tangent_distance < radius {
                tangent_distance = radius;
            }
            for (side, direction) in [right, left].into_iter().enumerate() {
                let step_point = self.step_point(step_index);
                let step_surface = self.step_surface(step_index);
                let (_, result) = self.test_pill2d(
                    disc_index,
                    step_point,
                    step_surface,
                    direction,
                    radius,
                    2.0 * radius + tangent_distance,
                    false,
                    false,
                    ignore_optional,
                );
                if result.disc_index != NONE as i16 {
                    let word = (result.disc_index >> 5) as usize;
                    let bit = 1u32 << (result.disc_index & 31);
                    if visited[word] & bit == 0 {
                        visited[word] |= bit;
                        stack.push(result.disc_index);
                    }
                }
                if result.distance > tangent_distance && result.obstacle_index != obstacle_index {
                    let midpoint_distance = (result.distance + tangent_distance) * 0.5;
                    let (_, ray) =
                        self.structure.test_ray2d(self.ignore_broken_surfaces(), step_point, step_surface, direction, midpoint_distance);
                    let midpoint = point_from_line2(step_point, direction, midpoint_distance);
                    self.add_step(
                        midpoint,
                        ray.surface_index,
                        obstacle_index,
                        side as u8,
                        self.step_f32(step_index, ops::TOTAL_DISTANCE) - self.step_f32(step_index, ops::DISTANCE) + midpoint_distance,
                        step_index,
                    );
                }
            }
        }
    }

    /// `path_iterate`: the cheapest step's sweep to the goal (or round what
    /// it meets); whether to go on.
    fn iterate(&self) -> bool {
        let m = self.m;
        let step_index = self.heap_remove();
        if step_index != NONE as i16 {
            let (_, result) = self.test_pill2d(
                NONE as i16,
                self.step_point(step_index),
                self.step_surface(step_index),
                self.step_direction(step_index),
                self.radius(),
                self.step_f32(step_index, ops::DISTANCE),
                self.step_i16(step_index, ops::PREVIOUS_STEP_INDEX) == NONE as i16,
                true,
                self.ignore_optional(),
            );
            if result.edge_index == NONE {
                let goal_surface_index = m.i32(self.field(op::GOAL_SURFACE_INDEX));
                if result.disc_index == NONE as i16 {
                    let goal = [m.f32(self.field(op::GOAL)), m.f32(self.field(op::GOAL) + 4)];
                    if result.surface_index == goal_surface_index
                        || self.structure.surfaces_are_equivalent(goal, goal_surface_index, result.surface_index)
                    {
                        let point = point_from_line2(self.step_point(step_index), self.step_direction(step_index), result.distance);
                        let goal_step_index = self.add_step(
                            point,
                            result.surface_index,
                            NONE as i16,
                            0,
                            self.step_f32(step_index, ops::TOTAL_DISTANCE) - self.step_f32(step_index, ops::DISTANCE) + result.distance,
                            step_index,
                        );
                        m.set_i16(self.field(op::GOAL_STEP_INDEX), goal_step_index);
                    }
                } else {
                    if result.obstacle_index == m.i16(self.field(op::GOAL_OBSTACLE_INDEX))
                        && self.step_f32(step_index, ops::DISTANCE) < m.f32(self.field(op::BEST_GOAL_BLOCKED_DISTANCE))
                    {
                        m.set_f32(self.field(op::BEST_GOAL_BLOCKED_DISTANCE), self.step_f32(step_index, ops::DISTANCE));
                        m.set_i16(self.field(op::BEST_GOAL_BLOCKED_STEP_INDEX), step_index);
                    }
                    self.add_steps(step_index, result.disc_index);
                }
            }
        }
        self.goal_step_index() == NONE as i16 && self.heap_count() > 0
    }

    /// `path_find`.
    #[allow(clippy::too_many_arguments)]
    fn find(
        &self,
        structure_address: u32,
        ignore_broken_surfaces: bool,
        radius: f32,
        start: [f32; 2],
        start_surface_index: i32,
        goal: [f32; 2],
        goal_surface_index: i32,
        finishing_path: bool,
        ignore_optional: bool,
    ) -> bool {
        let m = self.m;
        self.new(
            structure_address,
            ignore_broken_surfaces,
            radius,
            start,
            start_surface_index,
            goal,
            goal_surface_index,
            finishing_path,
            ignore_optional,
        );
        while self.iterate() {}
        if self.goal_step_index() != NONE as i16 {
            m.set_bool(self.field(op::GOAL_FOUND_EXACTLY), true);
        } else {
            let best = m.i16(self.field(op::BEST_GOAL_BLOCKED_STEP_INDEX));
            if best != NONE as i16 {
                m.set_i16(self.field(op::GOAL_STEP_INDEX), best);
            }
        }
        self.goal_step_index() != NONE as i16
    }
}

/// What the game gathers for the avoidance: the discs of the objects in a
/// sphere (`obstacles_get_discs_in_sphere`, into the obstacles at the
/// address).
pub type DiscsInSphere<'a> = dyn FnMut(u32, [f32; 3], f32, [f32; 3], i32, i32) + 'a;

/// `path_avoid_obstacles`: each smoothed step's way round the objects in
/// its way (into `avoided_steps`, four at most); false if one can't be
/// found.
#[allow(clippy::too_many_arguments)]
fn path_avoid_obstacles(
    m: &dyn AiMemory,
    state: &State,
    structure: &Structure,
    structure_address: u32,
    input_steps: &[PathStep],
    avoided_steps: &mut Vec<PathStep>,
    steps_finish_path: &mut bool,
    discs_in_sphere: &mut DiscsInSphere,
) -> bool {
    let radius = max_c(m.f32(state.input(pi::PATHFINDING_RADIUS)), 0.2);
    let debug = state.debug();
    let local_obstacles = vec![0u8; obs::SIZEOF];
    let local_path = vec![0u8; op::SIZEOF];
    if debug != 0 && !m.bool(debug + dbg::USE_STORED_OBSTACLES as u32) {
        m.set_i16(debug + dbg::AVOIDANCE_PATH_COUNT as u32, 0);
    }
    let start_point = m.vector(state.input(pi::START_POINT));
    let mut last_step_end_point = [0.0f32; 3];
    let mut previous_surface_index = NONE;
    let ignore_broken_surfaces = m.bool(state.input(pi::IGNORE_BROKEN_SURFACES));
    for (step_index, input_step) in input_steps.iter().enumerate() {
        let last_step = step_index == input_steps.len() - 1;
        let finishing = last_step && *steps_finish_path;
        let (start, start_surface_index) = if step_index > 0 {
            (last_step_end_point, previous_surface_index)
        } else {
            (start_point, m.i32(state.input(pi::START_SURFACE_INDEX)))
        };
        let goal_point = input_step.point;
        let goal_surface_index = input_step.surface_index;
        let mut movement_direction = vector_from_points(start, goal_point);
        crate::actor_transitions::normalize3(&mut movement_direction);
        let mut obstacles_address = local_obstacles.as_ptr() as u32;
        let mut path_address = local_path.as_ptr() as u32;
        let mut compute_obstacles = true;
        if debug != 0 {
            obstacles_address = debug + dbg::AVOIDANCE_OBSTACLES as u32 + (step_index * obs::SIZEOF) as u32;
            path_address = debug + dbg::AVOIDANCE_PATHS as u32 + (step_index * op::SIZEOF) as u32;
            if m.bool(debug + dbg::USE_STORED_OBSTACLES as u32)
                && (step_index as i16) < m.i16(debug + dbg::AVOIDANCE_PATH_COUNT as u32)
            {
                compute_obstacles = false;
            }
        }
        let obstacles = Obstacles { m, at: obstacles_address };
        if compute_obstacles {
            obstacles.new();
            discs_in_sphere(
                obstacles_address,
                start,
                4.0,
                movement_direction,
                m.i32(state.input(pi::IGNORE_SOURCE_OBJECT_INDEX)),
                m.i32(state.input(pi::IGNORE_TARGET_OBJECT_INDEX)),
            );
            if m.bool(state.input(pi::ATTRACTOR_VALID)) {
                obstacles.add_disc(
                    m.i32(state.input(pi::ATTRACTOR_OBJECT_INDEX)),
                    DISC_OPTIONAL,
                    m.vector(state.input(pi::ATTRACTOR_POINT)),
                    m.f32(state.input(pi::ATTRACTOR_RADIUS)),
                );
            }
            obstacles.recompute(radius);
            if debug != 0 && !m.bool(debug + dbg::USE_STORED_OBSTACLES as u32) {
                let count = debug + dbg::AVOIDANCE_PATH_COUNT as u32;
                m.set_i16(count, m.i16(count) + 1);
            }
        }
        let path = ObstaclePath { m, at: path_address, obstacles, structure };
        let start2 = [start[0], start[1]];
        let goal2 = [goal_point[0], goal_point[1]];
        let mut path_found = path.find(
            structure_address,
            ignore_broken_surfaces,
            radius,
            start2,
            start_surface_index,
            goal2,
            goal_surface_index,
            finishing,
            false,
        );
        if !path_found && m.i16(obstacles_address + obs::DISC_OPTIONAL_COUNT as u32) > 0 {
            path_found = path.find(
                structure_address,
                ignore_broken_surfaces,
                radius,
                start2,
                start_surface_index,
                goal2,
                goal_surface_index,
                finishing,
                true,
            );
        }
        if !path_found {
            return false;
        }
        let goal_step_index = path.goal_step_index();
        if m.bool(path.field(op::GOAL_FOUND_EXACTLY)) {
            last_step_end_point = goal_point;
            previous_surface_index = goal_surface_index;
        } else {
            previous_surface_index = path.step_surface(goal_step_index);
            last_step_end_point = structure.project(previous_surface_index, path.step_point(goal_step_index));
        }
        let mut overflow = false;
        let mut temporary_steps: Vec<PathStep> = Vec::new();
        let mut obstacle_step_index = goal_step_index;
        while obstacle_step_index != 0 {
            let surface_index = path.step_surface(obstacle_step_index);
            temporary_steps.push(PathStep { surface_index, point: structure.project(surface_index, path.step_point(obstacle_step_index)) });
            obstacle_step_index = path.step_i16(obstacle_step_index, ops::PREVIOUS_STEP_INDEX);
            if temporary_steps.len() >= MAXIMUM_OBSTACLE_AVOIDANCE_STEPS as usize {
                overflow = true;
                break;
            }
        }
        for step in temporary_steps.iter().rev() {
            if avoided_steps.len() >= MAXIMUM_SMOOTHED_PATH_STEPS {
                overflow = true;
                break;
            }
            avoided_steps.push(*step);
        }
        if overflow {
            *steps_finish_path = false;
            break;
        }
    }
    true
}

/// path.h's `_path_build_result_*`.
mod build_result {
    pub const NONE: i16 = 0;
    pub const NO_DESTINATION: i16 = 1;
    pub const CACHED_NODE_MISSING: i16 = 2;
    pub const OBSTACLE_AVOIDANCE_FAILED: i16 = 4;
    pub const SUCCESS: i16 = 5;
}

/// What the path build reads besides the state.
pub struct BuildContext<'a> {
    pub breakable_surface_flags: u32,
    /// `global_structure_bsp_get`.
    pub structure: u32,
    /// A local game's debug settings: no smoothing, no obstacle avoidance.
    pub disable_smoothing: bool,
    pub disable_obstacle_avoidance: bool,
    pub discs_in_sphere: &'a mut DiscsInSphere<'a>,
}

fn write_step(m: &dyn AiMemory, address: u32, step: &PathStep) {
    m.w32(address + pst::SURFACE_INDEX as u32, step.surface_index as u32);
    for (k, value) in step.point.iter().enumerate() {
        m.set_f32(address + pst::POINT as u32 + 4 * k as u32, *value);
    }
}

fn write_steps(m: &dyn AiMemory, address: u32, steps: &[PathStep]) {
    for (index, step) in steps.iter().enumerate() {
        write_step(m, address + (index * pst::SIZEOF) as u32, step);
    }
}

/// `path_state_build_path`: the steps (four at most) from the search's
/// start to its destination (or to the node nearest it, within its
/// radius): smoothed and round the objects in the way, into the result at
/// `path`; whether there is one.
pub fn path_state_build_path(m: &dyn AiMemory, state_address: u32, path: u32, context: &mut BuildContext) -> bool {
    let state = State { m, state: state_address };
    let debug = state.debug();
    let set_build_result = |result: i16| {
        if debug != 0 {
            m.set_i16(debug + dbg::PATH_BUILD_RESULT as u32, result);
        }
    };
    set_build_result(build_result::NONE);
    m.set_bool(path + pr::VALID as u32, false);
    let endpoint = path + pr::ENDPOINT as u32;
    if m.bool(state.at(ps::DESTINATION_VALID)) {
        let mut node_index = state.node_from_hash_table(m.i32(state.destination(pd::SURFACE_INDEX)));
        if node_index != NONE as i16 {
            for offset in (0..20).step_by(4) {
                m.w32(endpoint + offset, m.r32(state.at(ps::DESTINATION) + offset));
            }
            m.set_f32(endpoint + pd::TARGET_RADIUS as u32, 0.0);
        } else if m.f32(state.at(ps::CLOSEST_DISTANCE)) < m.f32(state.destination(pd::TARGET_RADIUS)) {
            node_index = m.i16(state.at(ps::CLOSEST_NODE_INDEX));
            state.set_vector(endpoint + pd::POINT as u32, m.vector(state.at(ps::CLOSEST_POINT)));
            m.w32(endpoint + pd::SURFACE_INDEX as u32, m.r32(state.node(node_index) + pn::SURFACE_INDEX as u32));
            m.set_f32(endpoint + pd::TARGET_RADIUS as u32, m.f32(state.at(ps::CLOSEST_DISTANCE)));
        }
        if node_index != NONE as i16 {
            let depth_plus_one = m.i16(state.node(node_index) + pn::DEPTH as u32) as i32 + 1;
            let mut steps_finish_path = true;
            let raw_step_count = depth_plus_one.min(64) as usize;
            let mut raw_steps = vec![PathStep::default(); raw_step_count];
            let mut child_node: Option<u32> = None;
            loop {
                let node = state.node(node_index);
                let depth = m.i16(node + pn::DEPTH as u32);
                if depth >= 64 {
                    steps_finish_path = false;
                } else {
                    let step = &mut raw_steps[depth as usize];
                    step.surface_index = m.i32(node + pn::SURFACE_INDEX as u32);
                    step.point = match child_node {
                        None => m.vector(endpoint + pd::POINT as u32),
                        Some(child) => m.vector(child + pn::ENTRY_POINT as u32),
                    };
                }
                child_node = Some(node);
                node_index = m.i16(node + pn::PARENT_NODE_INDEX as u32);
                if node_index == NONE as i16 {
                    break;
                }
            }
            let smoothed_steps = if context.disable_smoothing {
                raw_steps.iter().take(MAXIMUM_SMOOTHED_PATH_STEPS).copied().collect()
            } else {
                path_smooth(m, state_address, context.breakable_surface_flags, &raw_steps, &mut steps_finish_path)
            };
            let mut avoided_steps: Vec<PathStep> = Vec::new();
            let path_build_success = if context.disable_obstacle_avoidance {
                avoided_steps = smoothed_steps.iter().take(MAXIMUM_SMOOTHED_PATH_STEPS).copied().collect();
                true
            } else {
                let structure = Structure::new(m, context.structure, context.breakable_surface_flags);
                let success = path_avoid_obstacles(
                    m,
                    &state,
                    &structure,
                    context.structure,
                    &smoothed_steps,
                    &mut avoided_steps,
                    &mut steps_finish_path,
                    context.discs_in_sphere,
                );
                if !success {
                    set_build_result(build_result::OBSTACLE_AVOIDANCE_FAILED);
                }
                success
            };
            if path_build_success {
                m.w8(path + pr::STEP_COUNT as u32, avoided_steps.len() as u8);
                m.set_bool(path + pr::STEPS_FINISH_PATH as u32, steps_finish_path);
                m.set_bool(path + pr::VALID as u32, true);
                m.w8(path + pr::STEP_INDEX as u32, 0);
                write_steps(m, path + pr::STEPS as u32, &avoided_steps);
                if steps_finish_path {
                    // (the result's last step, as the game reads it)
                    let last = (path + pr::STEPS as u32)
                        .wrapping_add(((m.i8(path + pr::STEP_COUNT as u32) as i32 - 1) as u32).wrapping_mul(pst::SIZEOF as u32));
                    let last_point = m.vector(last + pst::POINT as u32);
                    state.set_vector(endpoint + pd::POINT as u32, last_point);
                    m.w32(endpoint + pd::SURFACE_INDEX as u32, m.r32(last + pst::SURFACE_INDEX as u32));
                    m.set_f32(endpoint + pd::TARGET_RADIUS as u32, distance(last_point, m.vector(state.destination(pd::POINT))));
                }
                set_build_result(build_result::SUCCESS);
            }
            if debug != 0 {
                m.set_i16(debug + dbg::RAW_STEP_COUNT as u32, raw_step_count as i16);
                write_steps(m, debug + dbg::RAW_STEPS as u32, &raw_steps);
                m.set_i16(debug + dbg::SMOOTHED_STEP_COUNT as u32, smoothed_steps.len() as i16);
                write_steps(m, debug + dbg::SMOOTHED_STEPS as u32, &smoothed_steps);
                m.set_i16(debug + dbg::AVOIDED_STEP_COUNT as u32, avoided_steps.len() as i16);
                write_steps(m, debug + dbg::AVOIDED_STEPS as u32, &avoided_steps);
            }
        } else if debug != 0 {
            let closest_valid = m.i16(state.at(ps::CLOSEST_NODE_INDEX)) != NONE as i16;
            set_build_result(closest_valid as i16 + build_result::CACHED_NODE_MISSING);
        }
    } else {
        set_build_result(build_result::NO_DESTINATION);
    }
    if debug != 0 {
        for offset in (0..pr::SIZEOF as u32).step_by(4) {
            m.w32(debug + dbg::RESULT as u32 + offset, m.r32(path + offset));
        }
        if m.i16(debug + dbg::PATH_BUILD_RESULT as u32) != build_result::SUCCESS {
            m.set_bool(debug + dbg::FAILURE as u32, true);
        }
    }
    m.bool(path + pr::VALID as u32)
}
