//! Prints a scenario's scripts as source, from their compiled syntax (each
//! token's text is in the string constants): `script_source <map> [filter]`,
//! the filter keeping the scripts whose source contains it.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let scenario = cache.scenario().unwrap();
    // (struct scenario: hs_syntax_data, hs_string_constants then hs_scripts
    // follow ai_conversations)
    let at = halo_map::layout::scenario::AI_CONVERSATIONS + 12;
    let syntax = scenario.data_bytes(at).unwrap();
    let strings = scenario.data_bytes(at + 20).unwrap();
    let scripts = scenario.block(at + 40, 0x5c).unwrap();
    // (the syntax is a struct data_array: its 0x38-byte header, then struct
    // hs_syntax_node, 20 bytes each)
    let node = |index: i32| -> Option<(i16, i32, i32, i32)> {
        let at = 0x38 + (index & 0xffff) as usize * 20;
        let b = syntax.get(at..at + 20)?;
        let i32_at = |o: usize| i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        Some((i16::from_le_bytes([b[6], b[7]]), i32_at(8), i32_at(12), i32_at(16)))
    };
    let text = |offset: i32| -> String {
        let rest = strings.get(offset.max(0) as usize..).unwrap_or(&[]);
        String::from_utf8_lossy(&rest[..rest.iter().position(|&c| c == 0).unwrap_or(rest.len())]).into_owned()
    };
    fn print(index: i32, depth: usize, out: &mut String, node: &dyn Fn(i32) -> Option<(i16, i32, i32, i32)>, text: &dyn Fn(i32) -> String) {
        let Some((flags, _, string, data)) = node(index) else { return };
        if flags & 1 != 0 {
            out.push_str(&text(string));
            return;
        }
        out.push_str(&format!("\n{}(", "  ".repeat(depth)));
        let mut child = data;
        let mut first = true;
        while child != -1 {
            if !first {
                out.push(' ');
            }
            first = false;
            print(child, depth + 1, out, node, text);
            child = node(child).map_or(-1, |n| n.1);
        }
        out.push(')');
    }
    const TYPES: [&str; 5] = ["startup", "dormant", "continuous", "static", "stub"];
    for script in scripts.iter() {
        let name = String::from_utf8_lossy(script.bytes(0, 32).unwrap()).trim_end_matches('\0').to_string();
        let kind = script.bytes(32, 2).unwrap();
        let kind = i16::from_le_bytes([kind[0], kind[1]]);
        let root = script.u32(36).unwrap() as i32;
        let mut body = String::new();
        print(root, 1, &mut body, &node, &text);
        if args.get(2).is_some_and(|filter| !body.contains(filter.as_str())) {
            continue;
        }
        println!("(script {} {name}{body})\n", TYPES.get(kind as usize).unwrap_or(&"?"));
    }
}
