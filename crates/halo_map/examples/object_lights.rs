//! Prints an object definition's attached lights: each light tag's radius,
//! angles and colour, and its marker's place and forward on the model at
//! rest: `object_lights <map> <object name part>`.
fn main() {
    use halo_map::{collision::matrix_inverse, markers, pose::matrix_multiply};
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let mut seen = std::collections::HashSet::new();
    for placement in halo_map::objects::placements(&cache).unwrap() {
        let Some(datum) = placement.definition.datum else { continue };
        let Some(tag) = cache.tag(datum).ok() else { continue };
        if !tag.name.contains(&args[2]) || !seen.insert(datum) {
            continue;
        }
        println!("{}:", tag.name);
        let root = cache.tag_root(datum, halo_map::groups::OBJECT).unwrap();
        let attachments = root.block(halo_map::layout::_object_definition::ATTACHMENTS, 0x48).unwrap();
        let model = halo_map::objects::object_model(&cache, placement.definition).ok().flatten().and_then(|m| m.datum);
        let marker_set = model.and_then(|model| markers::ModelMarkers::load(&cache, model).ok());
        let nodes = model.and_then(|model| halo_map::model::Model::load(&cache, model).ok()).map(|m| m.nodes);
        for attachment in attachments.iter() {
            let Ok(reference) = attachment.reference(0) else { continue };
            let Some(attached) = reference.datum else { continue };
            let attached_name = cache.tag(attached).map(|t| t.name.clone()).unwrap_or_default();
            let marker = attachment.bytes(0x10, 32).ok().map(|bytes| String::from_utf8_lossy(bytes.split(|&b| b == 0).next().unwrap_or(&[])).into_owned()).unwrap_or_default();
            let light = (reference.group == halo_map::groups::LIGHT).then(|| cache.tag_root(attached, halo_map::groups::LIGHT).ok()).flatten();
            let light = light.map(|d| {
                let f = |o: usize| d.f32(o).unwrap_or(0.0);
                format!("radius {} falloff {:.0} cutoff {:.0} deg colour {:?}", f(0x4), f(0x10).to_degrees(), f(0x14).to_degrees(), [f(0x4c), f(0x50), f(0x54)])
            });
            println!("  {} at marker {marker:?}: {}", attached_name, light.unwrap_or_default());
            let Some(found) = marker_set.as_ref().and_then(|set| set.markers.iter().find(|m| m.name.eq_ignore_ascii_case(marker.as_bytes()))) else { continue };
            for instance in &found.instances {
                let Some(node) = nodes.as_ref().and_then(|n| n.get(instance.node as usize)) else { continue };
                let local = markers::matrix_from_point_and_quaternion(instance.translation, instance.rotation);
                let m = matrix_multiply(&matrix_inverse(&node.default_inverse), &local);
                println!("    instance: at {:.3?} forward {:.2?}", m.position, m.forward);
            }
        }
    }
}
