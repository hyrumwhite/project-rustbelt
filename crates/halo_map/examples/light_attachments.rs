//! Lists the objects that carry a light, whether placed in the scenario or
//! not (a weapon an actor drops, a biped's own): each object's attached
//! light tags with their radius and colour bounds, and the attachment's
//! function sources: `light_attachments <map> [name part]` (the object's or
//! the light's).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    for tag in cache.tags.iter() {
        let Ok(root) = cache.tag_root(tag.datum, halo_map::groups::OBJECT) else { continue };
        let Ok(attachments) = root.block(halo_map::layout::_object_definition::ATTACHMENTS, 0x48) else { continue };
        for attachment in attachments.iter() {
            let Ok(reference) = attachment.reference(0) else { continue };
            let Some(attached) = reference.datum else { continue };
            if reference.group != halo_map::groups::LIGHT {
                continue;
            }
            let attached_name = cache.tag(attached).map(|t| t.name.clone()).unwrap_or_default();
            if args.get(2).is_some_and(|part| !tag.name.contains(part.as_str()) && !attached_name.contains(part.as_str())) {
                continue;
            }
            let Ok(light) = cache.tag_root(attached, halo_map::groups::LIGHT) else { continue };
            let f = |o: usize| light.f32(o).unwrap_or(0.0);
            // (the attachment: its marker's name at 0x10, the sources of its
            // primary scale, secondary scale and change colour at 0x30)
            let marker = attachment.bytes(0x10, 32).ok().map(|bytes| String::from_utf8_lossy(bytes.split(|&b| b == 0).next().unwrap_or(&[])).into_owned()).unwrap_or_default();
            let sources = [0x30, 0x32, 0x34].map(|o| attachment.i16(o).unwrap_or(-1));
            println!(
                "{} ({}): {attached_name} at marker {marker:?}, scale sources {sources:?}: flags {:#x} radius {} colour lower {:?} upper {:?}",
                tag.name,
                halo_map::fourcc_str(tag.group),
                light.u16(0x0).unwrap_or(0),
                f(0x4),
                [f(0x3c), f(0x40), f(0x44), f(0x48)],
                [f(0x4c), f(0x50), f(0x54), f(0x58)],
            );
        }
    }
}
