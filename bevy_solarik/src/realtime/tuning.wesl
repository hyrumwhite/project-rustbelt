// The lighting's tuning knobs that the renderer passes as shader definitions
// with values (node.rs reads them from the environment, defaults as they were,
// while Phase 3 of docs/boiling-plan.md tunes them in the harness without a
// build a variant): the DI pass's candidates and confidence cap, the GI pass's
// confidence cap, the world cache update's tile candidates, the history
// below which the GI spatial pass takes a second neighbour, the roughness
// above which specular GI reuses the diffuse GI instead of tracing.
const DI_TILE_CANDIDATES: u32 = constants::DI_TILE_CANDIDATES;
const DI_CELL_CANDIDATES: u32 = constants::DI_CELL_CANDIDATES;
const DI_CONFIDENCE_CAP: f32 = f32(constants::DI_CONFIDENCE_CAP);
const GI_CONFIDENCE_CAP: f32 = f32(constants::GI_CONFIDENCE_CAP);
const CACHE_TILE_CANDIDATES: u32 = constants::CACHE_TILE_CANDIDATES;
const GI_ADAPTIVE_CONFIDENCE: f32 = f32(constants::GI_ADAPTIVE_CONFIDENCE);
// (in hundredths from the environment: the roughness above which specular GI
// reuses the diffuse GI's reservoir instead of tracing a glossy path)
const SPECULAR_REUSE_ROUGHNESS: f32 = f32(constants::SPECULAR_REUSE_ROUGHNESS) / 100.0;
// (1: a cache cell's stored sample count is the history its blend used, so
// a cell that just followed a change holds a short history and lets go of a
// flash as fast as it took it; 0: the count climbs regardless, as before)
const CACHE_HISTORY_CLAMP: u32 = constants::CACHE_HISTORY_CLAMP;
// (1: a cell's light dropping is seen the update it drops, not when the
// smoothed delta catches up: a flash let go in a few updates; 0: as before)
const CACHE_DROP_DETECT: u32 = constants::CACHE_DROP_DETECT;
// (the drop, in hundredths of the cell's light, below which a drop is not
// one; and the fewest samples a drop, or the clamp, leaves a cell)
const CACHE_DROP_THRESHOLD: u32 = constants::CACHE_DROP_THRESHOLD;
const CACHE_DROP_FLOOR: u32 = constants::CACHE_DROP_FLOOR;
// (ReSTIR GI's guided initial sampling, GI_GUIDE in restir_gi.wesl: the most
// of the candidates aimed at the temporal reservoir's sample, in hundredths,
// held under 0.9 where it is used; the cone's half-angle in degrees, held to
// 1 to 80 there; and in hundredths of a steradian the effective solid angle
// of the pixel's light at which the guide fades out)
const GI_GUIDE_SHARE: f32 = f32(constants::GI_GUIDE_SHARE) / 100.0;
const GI_GUIDE_CONE: f32 = f32(constants::GI_GUIDE_CONE);
const GI_GUIDE_WIDE: f32 = f32(constants::GI_GUIDE_WIDE) / 100.0;
// (ReSTIR GI's spatial reuse radius in pixels, restir_gi.wesl: the disk its
// spatial pass draws a neighbour from; 30 by default, as the constant was,
// and 30 for a pipeline built without the definition)
@if((GI_SPATIAL_RADIUS))
const GI_SPATIAL_RADIUS: f32 = f32(constants::GI_SPATIAL_RADIUS);
@if((!(GI_SPATIAL_RADIUS)))
const GI_SPATIAL_RADIUS: f32 = 30.0;
// (ReSTIR GI's spatial neighbours a pixel merges, restir_gi.wesl, held to
// at most 16 there; 1 by default, as before, and 1 for a pipeline built
// without the definition)
@if((GI_SPATIAL_COUNT))
const GI_SPATIAL_COUNT: u32 = constants::GI_SPATIAL_COUNT;
@if((!(GI_SPATIAL_COUNT)))
const GI_SPATIAL_COUNT: u32 = 1u;
// (ReSTIR GI's guide taken from a neighbour's history where the pixel's own
// gives none, restir_gi.wesl, GI_GUIDE: how many of last frame's reservoirs
// near the pixel it offers, held to at most 16 there, 8 by node.rs's
// default, 0 the old (and 0 for a pipeline built without the definition);
// and within how many pixels of the pixel's reprojected place, 30)
@if((GI_GUIDE_NEIGHBOURS))
const GI_GUIDE_NEIGHBOURS: u32 = constants::GI_GUIDE_NEIGHBOURS;
@if((!(GI_GUIDE_NEIGHBOURS)))
const GI_GUIDE_NEIGHBOURS: u32 = 0u;
@if((GI_GUIDE_RADIUS))
const GI_GUIDE_RADIUS: f32 = f32(constants::GI_GUIDE_RADIUS);
@if((!(GI_GUIDE_RADIUS)))
const GI_GUIDE_RADIUS: f32 = 30.0;
