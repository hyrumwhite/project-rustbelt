//! Prints each collision model's shield effects (damaged, depleted,
//! recharging) and their effects' parts: `shield_effects <map> [name part]`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let filter = args.get(2).map(|s| s.to_lowercase());
    for tag in cache.tags.iter().filter(|t| t.group == halo_map::groups::COLLISION_MODEL) {
        if filter.as_ref().is_some_and(|f| !tag.name.to_lowercase().contains(f)) {
            continue;
        }
        let Ok(root) = cache.tag_root(tag.datum, halo_map::groups::COLLISION_MODEL) else { continue };
        let threshold = root.f32(halo_map::layout::damage_resistance::SHIELD_DAMAGED_EFFECT_THRESHOLD).unwrap_or(0.0);
        println!("{} (shield damaged effect below {threshold})", tag.name);
        for (what, offset) in [("damaged", 0x188), ("depleted", 0x198), ("recharging", 0x1a8)] {
            let Some(datum) = root.reference(offset).ok().and_then(|r| r.datum) else { continue };
            let name = cache.tag(datum).map(|t| t.name.clone()).unwrap_or_default();
            println!("  {what}: {name}");
            let Ok(effect) = halo_map::effects::EffectDefinition::load(&cache, datum) else { continue };
            for (e, event) in effect.events.iter().enumerate() {
                for part in &event.parts {
                    let part_name = (part.reference >= 0).then(|| cache.tag(part.reference as u32).ok()).flatten();
                    println!(
                        "    event {e} part {} {:?} radius {}..{}",
                        halo_map::fourcc_str(part_name.map_or(0, |t| t.group)),
                        part_name.map(|t| t.name.clone()),
                        part.radius_modifier_lower_bound,
                        part.radius_modifier_upper_bound
                    );
                }
                for particles in &event.particles {
                    let p = (particles.particle >= 0).then(|| cache.tag(particles.particle as u32).ok()).flatten();
                    println!("    event {e} particles {:?} x{}..{}", p.map(|t| t.name.clone()), particles.count_lower_bound, particles.count_upper_bound);
                }
            }
        }
    }
}
