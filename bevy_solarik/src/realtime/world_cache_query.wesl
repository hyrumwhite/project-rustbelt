import bevy_pbr::render::utils::rand_f;
import bevy_pbr::render::utils::rand_vec2f;
import bevy_render::maths::orthonormalize;
import bevy_solarik::realtime::realtime_bindings::world_cache_life;
import bevy_solarik::realtime::realtime_bindings::world_cache_checksums;
import bevy_solarik::realtime::realtime_bindings::world_cache_radiance;
import bevy_solarik::realtime::realtime_bindings::world_cache_geometry_data;
import bevy_solarik::realtime::realtime_bindings::world_cache_light_slots;
import bevy_solarik::realtime::realtime_bindings::world_cache_luminance_deltas;
import bevy_solarik::realtime::realtime_bindings::world_cache_a;
import bevy_solarik::realtime::realtime_bindings::world_cache_b;
import bevy_solarik::realtime::realtime_bindings::world_cache_active_cell_indices;
import bevy_solarik::realtime::realtime_bindings::world_cache_active_cells_count;
import bevy_solarik::realtime::realtime_bindings::WorldCacheGeometryData;

enable wgpu_ray_query;



/// How responsive the world cache is to changes in lighting (higher is less responsive but more stable, lower is more responsive but less stable)
const WORLD_CACHE_MAX_TEMPORAL_SAMPLES: f32 = 32.0;
/// How many direct light samples each cell takes when updating each frame
const WORLD_CACHE_DIRECT_LIGHT_SAMPLE_COUNT: u32 = 128u;
// Shadow-tested light samples a cell takes each update.
const WORLD_CACHE_DIRECT_LIGHT_SAMPLES_PER_UPDATE: u32 = 4u;
// How many rendered frames a cell's history may span, in updates.
const WORLD_CACHE_HISTORY_FRAMES: f32 = 128.0;
/// Maximum amount of distance to trace GI rays between two cache cells
const WORLD_CACHE_MAX_GI_RAY_DISTANCE: f32 = 50.0;
/// Soft upper limit on the amount of cache cells to update each frame
const WORLD_CACHE_CELL_UPDATES_SOFT_CAP: u32 = 40000u;

/// Maximum amount of frames a cell can live for without being queried
const WORLD_CACHE_CELL_LIFETIME: u32 = 10u;
/// Maximum amount of attempts to find a cache entry after a hash collision
const WORLD_CACHE_MAX_SEARCH_STEPS: u32 = 3u;

/// Size of a cache cell at the lowest LOD in meters
const WORLD_CACHE_POSITION_BASE_CELL_SIZE: f32 = 0.15;
/// How fast the world cache transitions between LODs as a function of distance to the camera
const WORLD_CACHE_POSITION_LOD_SCALE: f32 = 15.0;

/// Marker value for an empty cell
const WORLD_CACHE_EMPTY_CELL: u32 = 0u;

// (CACHE_WARM_START) A new cell starts from its parent's radiance (the cell
// one level of detail coarser at the same point and normal) when the parent
// has had at least WORLD_CACHE_WARM_START_MIN_SAMPLES updates, as worth
// WORLD_CACHE_WARM_START_SAMPLES samples: its first real update then blends
// at about 1/5 instead of replacing a zero with one GI ray and four light
// samples. The sample count stored is WORLD_CACHE_WARM_START_MARK, just
// under four, a value no ordinary cell holds (their counts are whole
// numbers, min(a + 1, 32) from 0): world_cache_priority_bucket reads it as
// still unlit, so the cell keeps its first-light update, and that update's
// a + 1 (4.999) clears the mark and blends at 1/4.999.
@if(CACHE_WARM_START)
const WORLD_CACHE_WARM_START_SAMPLES: f32 = 4.0;
@if(CACHE_WARM_START)
const WORLD_CACHE_WARM_START_MARK: f32 = WORLD_CACHE_WARM_START_SAMPLES - 0.001;
@if(CACHE_WARM_START)
const WORLD_CACHE_WARM_START_MIN_SAMPLES: f32 = 4.0;

// Cache entries remain incident irradiance for one oriented hemisphere.
// Apply the authored thin-leaf split only at consumption, never to stored
// entries. Opposite normals select independent geometry, visibility and history.
// The coefficient is already clamped/quantized by material resolution.
@if((!WORLD_CACHE_NON_ATOMIC_LIFE_BUFFER))
fn query_two_sided_world_cache(world_position: vec3<f32>, world_normal: vec3<f32>, diffuse_transmission: f32, view_position: vec3<f32>, ray_t: f32, cell_lifetime: u32, rng: ptr<function, u32>) -> vec3<f32> {
    // Preserve both the opaque result and its random-number consumption.
    if diffuse_transmission == 0.0 {
        return query_world_cache(world_position, world_normal, view_position, ray_t, cell_lifetime, rng);
    }
    var irradiance = vec3(0.0);
    if diffuse_transmission < 1.0 {
        irradiance = (1.0 - diffuse_transmission) * query_world_cache(world_position, world_normal, view_position, ray_t, cell_lifetime, rng);
    }
    return irradiance + diffuse_transmission * query_world_cache(world_position, -world_normal, view_position, ray_t, cell_lifetime, rng);
}

// Radius is relative to the requested endpoint, including cache quantization
// and jitter. Transitive irradiance carries its complete previous support.
@if((!WORLD_CACHE_NON_ATOMIC_LIFE_BUFFER))
fn query_two_sided_world_cache_with_support(world_position: vec3<f32>, world_normal: vec3<f32>, diffuse_transmission: f32, view_position: vec3<f32>, ray_t: f32, cell_lifetime: u32, rng: ptr<function, u32>) -> vec4<f32> {
    if diffuse_transmission == 0.0 {
        return query_world_cache_with_support(world_position, world_normal, view_position, ray_t, cell_lifetime, rng);
    }
    var front = vec4(0.0);
    if diffuse_transmission < 1.0 {
        front = query_world_cache_with_support(world_position, world_normal, view_position, ray_t, cell_lifetime, rng);
        front = vec4((1.0 - diffuse_transmission) * front.rgb, front.w);
    }
    let back = query_world_cache_with_support(world_position, -world_normal, view_position, ray_t, cell_lifetime, rng);
    return vec4(front.rgb + diffuse_transmission * back.rgb, max(front.w, back.w));
}

@if((!WORLD_CACHE_NON_ATOMIC_LIFE_BUFFER))
fn query_world_cache(world_position_in: vec3<f32>, world_normal: vec3<f32>, view_position: vec3<f32>, ray_t: f32, cell_lifetime: u32, rng: ptr<function, u32>) -> vec3<f32> {
    return query_world_cache_with_support(world_position_in, world_normal, view_position, ray_t, cell_lifetime, rng).rgb;
}

// (VISUALIZE_CACHE_AGE: the samples in the cell the last query found, 0 for
// one it made, -1 for none)
@if(VISUALIZE_CACHE_AGE) {
var<private> world_cache_debug_samples: f32;
}

@if((!WORLD_CACHE_NON_ATOMIC_LIFE_BUFFER))
fn query_world_cache_with_support(world_position_in: vec3<f32>, world_normal: vec3<f32>, view_position: vec3<f32>, ray_t: f32, cell_lifetime: u32, rng: ptr<function, u32>) -> vec4<f32> {
@if(VISUALIZE_CACHE_AGE) {
    world_cache_debug_samples = -1.0;
}
    var world_position = world_position_in;
    var cell_size = get_cell_size(world_position, view_position, rng);

@if(WORLD_CACHE_FIRST_BOUNCE_LIGHT_LEAK_PREVENTION) {
    if ray_t < cell_size {
        // Prevent light leaks
        cell_size = WORLD_CACHE_POSITION_BASE_CELL_SIZE;
    }
}

@if(!NO_JITTER_WORLD_CACHE) {
    // Jitter query point, which essentially blurs the cache a bit so it's not so grid-like
    // https://tomclabault.github.io/blog/2025/regir, jitter_world_position_tangent_plane
    let TBN = orthonormalize(world_normal);
    let offset = (rand_vec2f(rng) * 2.0 - 1.0) * cell_size * 0.5;
    world_position += offset.x * TBN[0] + offset.y * TBN[1];
    cell_size = get_cell_size(world_position, view_position, rng);
}

    let world_position_quantized = bitcast<vec3<u32>>(quantize_position(world_position, cell_size));
    let world_normal_quantized = bitcast<vec3<u32>>(quantize_normal(world_normal));
    var key = compute_key(world_position_quantized, world_normal_quantized);
    let checksum = compute_checksum(world_position_quantized, world_normal_quantized);

    for (var i = 0u; i < WORLD_CACHE_MAX_SEARCH_STEPS; i++) {
        let existing_checksum = atomicCompareExchangeWeak(&world_cache_checksums[key], WORLD_CACHE_EMPTY_CELL, checksum).old_value;

        // Cell already exists or is empty - reset lifetime
        if existing_checksum == checksum || existing_checksum == WORLD_CACHE_EMPTY_CELL {
@if(!WORLD_CACHE_QUERY_ATOMIC_MAX_LIFETIME) {
            atomicStore(&world_cache_life[key], cell_lifetime);
}
@if(!(!WORLD_CACHE_QUERY_ATOMIC_MAX_LIFETIME)) {
            atomicMax(&world_cache_life[key], cell_lifetime);
}
        }

        if existing_checksum == checksum {
            // Cache entry already exists - get radiance
            let geometry = world_cache_geometry_data[key];
@if(VISUALIZE_CACHE_AGE) {
            world_cache_debug_samples = world_cache_radiance[key].a;
}
            return vec4(world_cache_radiance[key].rgb,
                geometry.support_radius + distance(world_position_in, geometry.world_position));
        } else if existing_checksum == WORLD_CACHE_EMPTY_CELL {
            // Cell is empty - initialize it
@if(VISUALIZE_CACHE_AGE) {
            world_cache_debug_samples = 0.0;
}
            // (CACHE_CELL_UNJITTERED: the cell keeps the query's own point,
            // on its surface; the jittered point, half a cell off in the
            // tangent plane, lay inside a rock or behind a wall for 38% of
            // b40's canyon wall and the cell's rays started there)
@if(CACHE_CELL_UNJITTERED) {
            world_cache_geometry_data[key].world_position = world_position_in;
}
@if(!CACHE_CELL_UNJITTERED) {
            world_cache_geometry_data[key].world_position = world_position;
}
            world_cache_geometry_data[key].world_normal = world_normal;
            world_cache_geometry_data[key].last_traced_frame = 0u;
            world_cache_geometry_data[key].support_radius = 0.0;
            world_cache_light_slots[key] = vec4(WORLD_CACHE_NO_LIGHT);
@if(CACHE_WARM_START) {
            // (the parent of the jittered point the key was made from: the
            // cell grids are powers of two of the base size, so this is the
            // next level's cell that holds the whole of this one)
            let parent = find_world_cache_parent(world_position, cell_size, world_normal_quantized);
            if parent >= 0 {
                let parent_radiance = world_cache_radiance[parent];
                let parent_geometry = world_cache_geometry_data[parent];
                let cell_position = world_cache_geometry_data[key].world_position;
                let parent_distance = distance(parent_geometry.world_position, cell_position);
                // (a level's quantised coordinates hash alike at every level:
                // a parent stored more than a few of its cells away is some
                // other place's cell, not this one's)
                if parent_radiance.a >= WORLD_CACHE_WARM_START_MIN_SAMPLES && parent_distance < cell_size * 6.0 {
                    let support = parent_geometry.support_radius + parent_distance;
                    world_cache_radiance[key] = vec4(parent_radiance.rgb, WORLD_CACHE_WARM_START_MARK);
                    world_cache_geometry_data[key].support_radius = support;
@if(VISUALIZE_CACHE_AGE) {
                    world_cache_debug_samples = WORLD_CACHE_WARM_START_MARK;
}
                    return vec4(parent_radiance.rgb, support + distance(world_position_in, cell_position));
                }
            }
}
            return vec4(0.0);
        } else {
            // Collision - linear probe to next entry
            key = wrap_key(key + 1u);
        }
    }

    return vec4(0.0);
}

// (CACHE_WARM_START) The cell one level of detail coarser than `cell_size`
// at a point, under the same quantised normal, -1 for none: read only, as
// find_world_cache_cell_once, the parent neither made nor kept alive.
@if(CACHE_WARM_START)
fn find_world_cache_parent(world_position: vec3<f32>, cell_size: f32, world_normal_quantized: vec3<u32>) -> i32 {
    let parent_position_quantized = bitcast<vec3<u32>>(quantize_position(world_position, cell_size * 2.0));
    var key = compute_key(parent_position_quantized, world_normal_quantized);
    let checksum = compute_checksum(parent_position_quantized, world_normal_quantized);
    for (var i = 0u; i < WORLD_CACHE_MAX_SEARCH_STEPS; i++) {
        let existing_checksum = atomicLoad(&world_cache_checksums[key]);
        if existing_checksum == checksum { return i32(key); }
        if existing_checksum == WORLD_CACHE_EMPTY_CELL { return -1; }
        key = wrap_key(key + 1u);
    }
    return -1;
}

// The normal a ray hit's cache cell is keyed by: its smoothed vertex normal
// (upstream), or with CELL_KEY_PLANE its triangle's plane normal, turned to
// the hit's side. The camera's pixel looks its cell up by its G-buffer
// normal, bump-mapped by the raster material, which on c20's slab walls
// fell in another floor(2n) bucket than the slab's vertex normal for three
// pixels in four (no light candidates by place there); keyed by the plane
// on both sides (the pixel's from its depths, pixel_geometric_normal), a
// flat wall is one key.
fn cell_key_normal(vertex_normal: vec3<f32>, plane_normal: vec3<f32>) -> vec3<f32> {
@if(CELL_KEY_PLANE) {
    return select(-plane_normal, plane_normal, dot(plane_normal, vertex_normal) >= 0.0);
}
@if(!CELL_KEY_PLANE) {
    return vertex_normal;
}
}

// An empty light slot.
const WORLD_CACHE_NO_LIGHT: u32 = 0xFFFFFFFFu;

// A cell's light slot: the light's id in the low 16 bits (0xFFFF for none)
// and above, as an f16, how much it lights the cell (its shadow-tested
// irradiance there, exposed, smoothed over the updates that found it). The
// slots are drawn by it: drawn uniformly, a narrow corridor's dim recessed
// strip, beside the brighter fixtures down the hall, got its quarter of the
// cell's candidates whatever it lit, and got into the slots by luck.
fn world_cache_slot_light(slot: u32) -> u32 {
    return select(slot & 0xFFFFu, WORLD_CACHE_NO_LIGHT, (slot & 0xFFFFu) == 0xFFFFu);
}

fn world_cache_slot_weight(slot: u32) -> f32 {
    let weight = unpack2x16float(slot).y;
    return select(0.0, weight, (slot & 0xFFFFu) != 0xFFFFu && weight > 0.0 && weight < 65504.0);
}

fn world_cache_slot(light_id: u32, weight: f32) -> u32 {
    return (light_id & 0xFFFFu) | (pack2x16float(vec2(0.0, clamp(weight, 0.0, 60000.0))) & 0xFFFF0000u);
}

// Each slot's probability of being drawn: by its weight, with a quarter of
// the mean beside (each light in the cell drawn now and then: a weight is a
// noisy estimate), none for an empty slot.
fn world_cache_slot_probabilities(slots: vec4<u32>) -> vec4<f32> {
    let present = vec4<f32>((slots & vec4(0xFFFFu)) != vec4(0xFFFFu));
    let count = dot(present, vec4(1.0));
    if count == 0.0 { return vec4(0.0); }
    let weights = vec4(world_cache_slot_weight(slots.x), world_cache_slot_weight(slots.y), world_cache_slot_weight(slots.z), world_cache_slot_weight(slots.w));
    let total = dot(weights, present);
    let floor = select(1.0, 0.25 * total / count, total > 0.0);
    let p = (weights + floor) * present;
    return p / dot(p, vec4(1.0));
}

// The probability that a draw from the slots picks `light_id`.
fn world_cache_slot_light_probability(slots: vec4<u32>, probabilities: vec4<f32>, light_id: u32) -> f32 {
    return dot(probabilities, vec4<f32>((slots & vec4(0xFFFFu)) == vec4(light_id & 0xFFFFu)));
}

// A slot drawn by `probabilities`, `u` in [0, 1).
fn world_cache_draw_slot(probabilities: vec4<f32>, u: f32) -> u32 {
    let c = vec3(probabilities.x, probabilities.x + probabilities.y, probabilities.x + probabilities.y + probabilities.z);
    return u32(u >= c.x) + u32(u >= c.y) + u32(u >= c.z);
}

// Smoothing of a slot's weight by each estimate found, and its fading each
// update (a light no longer found, or no longer in sight, gives way).
const WORLD_CACHE_SLOT_SMOOTHING: f32 = 0.25;
const WORLD_CACHE_SLOT_FADE: f32 = 0.95;
// Candidates a cell's light search draws from its slots, beside the tiles'.
const WORLD_CACHE_SLOT_LIGHT_SAMPLE_COUNT: u32 = 16u;

// `slots` with a light found in sight, `weight` its estimate: its slot's
// weight smoothed toward it, or else the weakest slot taken if the light
// would outweigh it.
fn world_cache_slots_found(slots: vec4<u32>, light_id: u32, weight: f32) -> vec4<u32> {
    var s = slots;
    var weakest = 0u;
    var weakest_weight = 3.4e38;
    for (var j = 0u; j < 4u; j++) {
        if world_cache_slot_light(s[j]) == light_id {
            s[j] = world_cache_slot(light_id, mix(world_cache_slot_weight(s[j]), weight, WORLD_CACHE_SLOT_SMOOTHING));
            return s;
        }
        let w = select(world_cache_slot_weight(s[j]), -1.0, world_cache_slot_light(s[j]) == WORLD_CACHE_NO_LIGHT);
        if w < weakest_weight {
            weakest = j;
            weakest_weight = w;
        }
    }
    let entering = weight * WORLD_CACHE_SLOT_SMOOTHING;
    if entering > weakest_weight {
        s[weakest] = world_cache_slot(light_id, entering);
    }
    return s;
}

// The cell a point's query lands in (with the query's jitter and level of
// detail), -1 for none: read only, the cell neither made nor kept alive.
// A camera pixel's shading normal (normal mapped) keys another cell than the
// geometric normal the cells were made with (the GI rays' hits) where a
// component lies near a step of floor(2n), a wall's zero above all: such a
// pixel tries those components on either side of the step, each with a
// jitter of its own, before it gives up (in a10's Jefferies tubes half the
// screen found no cell, and so no light candidates by place).
fn find_world_cache_cell(world_position: vec3<f32>, world_normal: vec3<f32>, view_position: vec3<f32>, rng: ptr<function, u32>) -> i32 {
    let scaled = world_normal * 2.0;
    // (no -0: the key hashes the bits)
    let step = select(round(scaled), vec3(0.0), round(scaled) == vec3(0.0));
    let near_step = abs(scaled - step) < vec3(0.2);
    let quantized = quantize_normal(world_normal);
    var tries = array<vec3<f32>, 4>(select(quantized, step, near_step), quantized, select(quantized, step - 1.0, near_step), select(quantized, step, near_step));
    for (var attempt = 0u; attempt < 4u; attempt++) {
        let cell = find_world_cache_cell_once(world_position, world_normal, tries[attempt], view_position, rng);
        if cell >= 0 { return cell; }
    }
    return -1;
}

fn find_world_cache_cell_once(world_position_in: vec3<f32>, world_normal: vec3<f32>, normal_quantized: vec3<f32>, view_position: vec3<f32>, rng: ptr<function, u32>) -> i32 {
    var world_position = world_position_in;
    var cell_size = get_cell_size(world_position, view_position, rng);
@if(!NO_JITTER_WORLD_CACHE) {
    let TBN = orthonormalize(world_normal);
    let offset = (rand_vec2f(rng) * 2.0 - 1.0) * cell_size * 0.5;
    world_position += offset.x * TBN[0] + offset.y * TBN[1];
    cell_size = get_cell_size(world_position, view_position, rng);
}
    let world_position_quantized = bitcast<vec3<u32>>(quantize_position(world_position, cell_size));
    let world_normal_quantized = bitcast<vec3<u32>>(normal_quantized);
    var key = compute_key(world_position_quantized, world_normal_quantized);
    let checksum = compute_checksum(world_position_quantized, world_normal_quantized);
    for (var i = 0u; i < WORLD_CACHE_MAX_SEARCH_STEPS; i++) {
        let existing_checksum = atomicLoad(&world_cache_checksums[key]);
        if existing_checksum == checksum { return i32(key); }
        if existing_checksum == WORLD_CACHE_EMPTY_CELL { return -1; }
        key = wrap_key(key + 1u);
    }
    return -1;
}

fn get_cell_size(world_position: vec3<f32>, view_position: vec3<f32>, rng: ptr<function, u32>) -> f32 {
    let camera_distance = distance(view_position, world_position) / WORLD_CACHE_POSITION_LOD_SCALE;
    let lod_f = log2(1.0 + camera_distance);
    let lod_fract = fract(lod_f);
    let lod = floor(lod_f) + select(0.0, 1.0, rand_f(rng) < lod_fract * lod_fract * lod_fract);
    return WORLD_CACHE_POSITION_BASE_CELL_SIZE * exp2(lod);
}

fn quantize_position(world_position: vec3<f32>, quantization_factor: f32) -> vec3<f32> {
    return floor(world_position / quantization_factor + 0.0001);
}

fn quantize_normal(world_normal: vec3<f32>) -> vec3<f32> {
    return floor(world_normal * 2.0 + 0.0001);
}

// TODO: Clustering
fn compute_key(world_position: vec3<u32>, world_normal: vec3<u32>) -> u32 {
    var key = pcg_hash(world_position.x);
    key = pcg_hash(key + world_position.y);
    key = pcg_hash(key + world_position.z);
    key = pcg_hash(key + world_normal.x);
    key = pcg_hash(key + world_normal.y);
    key = pcg_hash(key + world_normal.z);
    return wrap_key(key);
}

fn compute_checksum(world_position: vec3<u32>, world_normal: vec3<u32>) -> u32 {
    // Domain-separated PCG preserves all input bits. The former odd-polynomial
    // chain erased the sign bit of subsequent float coordinates/normals, so
    // opposite leaf hemispheres could alias when their table probes collided.
    var key = pcg_hash(world_position.x ^ 0x9E3779B9u);
    key = pcg_hash(key + world_position.y);
    key = pcg_hash(key + world_position.z);
    key = pcg_hash(key + world_normal.x);
    key = pcg_hash(key + world_normal.y);
    key = pcg_hash(key + world_normal.z);
    return max(key, 1u); // 0u is reserved for WORLD_CACHE_EMPTY_CELL
}

fn pcg_hash(input: u32) -> u32 {
    let state = input * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn wrap_key(key: u32) -> u32 {
    return key & (constants::WORLD_CACHE_SIZE - 1u);
}
