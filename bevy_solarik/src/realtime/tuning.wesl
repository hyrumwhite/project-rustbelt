// The lighting's tuning knobs that the renderer passes as shader definitions
// with values (node.rs reads them from the environment, defaults as they were,
// while Phase 3 of docs/boiling-plan.md tunes them in the harness without a
// build a variant): the DI pass's candidates and confidence cap, the GI pass's
// confidence cap, the world cache update's tile candidates, the history
// below which the GI spatial pass takes a second neighbour.
const DI_TILE_CANDIDATES: u32 = constants::DI_TILE_CANDIDATES;
const DI_CELL_CANDIDATES: u32 = constants::DI_CELL_CANDIDATES;
const DI_CONFIDENCE_CAP: f32 = f32(constants::DI_CONFIDENCE_CAP);
const GI_CONFIDENCE_CAP: f32 = f32(constants::GI_CONFIDENCE_CAP);
const CACHE_TILE_CANDIDATES: u32 = constants::CACHE_TILE_CANDIDATES;
const GI_ADAPTIVE_CONFIDENCE: f32 = f32(constants::GI_ADAPTIVE_CONFIDENCE);
