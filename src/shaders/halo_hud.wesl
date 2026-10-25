// The game's HUD and menu bitmaps (src/hud.rs): each screen quad the game
// would have drawn, with the NV2A pixel shader it built for it (general
// combiner stages over t0-t2 and v0, then the final combiner), into a texture
// in the display's encoding that is composited over the picture.

struct HudDraw {
    // the viewport the vertices' pixels are of (xy)
    viewport: vec4<f32>,
    // stage count, PSCombinerCount flags, final combiner inputs abcd, efg
    program: vec4<u32>,
    rgb_inputs: array<vec4<u32>, 2>,
    rgb_outputs: array<vec4<u32>, 2>,
    alpha_inputs: array<vec4<u32>, 2>,
    alpha_outputs: array<vec4<u32>, 2>,
    constant0: array<vec4<f32>, 8>,
    constant1: array<vec4<f32>, 8>,
    // x: the maps present (bit per map), y: 1 t0's alpha kill
    modes: vec4<u32>,
}

@group(0) @binding(0) var<storage, read> draws: array<HudDraw>;
@group(1) @binding(0) var map0: texture_2d<f32>;
@group(1) @binding(1) var map0_sampler: sampler;
@group(1) @binding(2) var map1: texture_2d<f32>;
@group(1) @binding(3) var map1_sampler: sampler;
@group(1) @binding(4) var map2: texture_2d<f32>;
@group(1) @binding(5) var map2_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv0: vec2<f32>,
    @location(1) uv1: vec2<f32>,
    @location(2) uv2: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) @interpolate(flat) draw: u32,
}

@vertex
fn vertex(
    @location(0) position: vec2<f32>,
    @location(1) uv0: vec2<f32>,
    @location(2) uv1: vec2<f32>,
    @location(3) uv2: vec2<f32>,
    @location(4) color: vec4<f32>,
    @builtin(instance_index) draw: u32,
) -> VertexOutput {
    let viewport = draws[draw].viewport.xy;
    var out: VertexOutput;
    out.position = vec4(2.0 * position.x / viewport.x - 1.0, 1.0 - 2.0 * position.y / viewport.y, 0.0, 1.0);
    out.uv0 = uv0;
    out.uv1 = uv1;
    out.uv2 = uv2;
    out.color = color;
    out.draw = draw;
    return out;
}

fn word(words: array<vec4<u32>, 2>, index: u32) -> u32 {
    return words[index / 4u][index % 4u];
}

fn read_register(d: HudDraw, registers: ptr<function, array<vec4<f32>, 16>>, index: u32, stage: u32) -> vec4<f32> {
    let unique_c0 = (d.program.y & 0x1000u) != 0u;
    let unique_c1 = (d.program.y & 0x10000u) != 0u;
    switch index {
        case 1u: { return d.constant0[select(0u, stage, unique_c0)]; }
        case 2u: { return d.constant1[select(0u, stage, unique_c1)]; }
        default: { return (*registers)[index]; }
    }
}

fn map_input(value: vec4<f32>, mapping: u32) -> vec4<f32> {
    switch mapping {
        case 0x00u: { return max(value, vec4<f32>(0.0)); }
        case 0x20u: { return 1.0 - saturate(value); }
        case 0x40u: { return 2.0 * max(value, vec4<f32>(0.0)) - 1.0; }
        case 0x60u: { return 1.0 - 2.0 * max(value, vec4<f32>(0.0)); }
        case 0x80u: { return max(value, vec4<f32>(0.0)) - 0.5; }
        case 0xa0u: { return 0.5 - max(value, vec4<f32>(0.0)); }
        case 0xc0u: { return value; }
        default: { return -value; }
    }
}

// an input byte as rgb (xyz) or, for the alpha portion, a scalar in all lanes
fn combiner_input(d: HudDraw, registers: ptr<function, array<vec4<f32>, 16>>, input: u32, alpha_portion: bool, stage: u32) -> vec4<f32> {
    let source = read_register(d, registers, input & 0x0fu, stage);
    let alpha_channel = (input & 0x10u) != 0u;
    var value: vec4<f32>;
    if alpha_portion {
        value = vec4<f32>(select(source.b, source.a, alpha_channel));
    } else {
        value = select(vec4<f32>(source.rgb, 0.0), vec4<f32>(source.a), alpha_channel);
    }
    return map_input(value, input & 0xe0u);
}

fn output_mapping(value: vec4<f32>, flags: u32) -> vec4<f32> {
    var v = value;
    switch flags & 0x38u {
        case 0x08u: { v = v - 0.5; }
        case 0x10u: { v = v * 2.0; }
        case 0x18u: { v = (v - 0.5) * 2.0; }
        case 0x20u: { v = v * 4.0; }
        case 0x30u: { v = v * 0.5; }
        default: {}
    }
    return clamp(v, vec4<f32>(-1.0), vec4<f32>(1.0));
}

fn final_input(d: HudDraw, registers: ptr<function, array<vec4<f32>, 16>>, input: u32, alpha_portion: bool) -> vec4<f32> {
    let index = input & 0x0fu;
    var source = (*registers)[index];
    if index == 1u { source = d.constant0[0]; }
    if index == 2u { source = d.constant1[0]; }
    let alpha_channel = (input & 0x10u) != 0u;
    var value: vec4<f32>;
    if alpha_portion {
        value = vec4<f32>(select(source.b, source.a, alpha_channel));
    } else {
        value = select(vec4<f32>(source.rgb, 0.0), vec4<f32>(source.a), alpha_channel);
    }
    value = saturate(value);
    return select(value, 1.0 - value, (input & 0x20u) != 0u);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let d = draws[in.draw];
    var registers: array<vec4<f32>, 16>;
    for (var i = 0u; i < 16u; i++) {
        registers[i] = vec4<f32>(0.0);
    }
    // the bitmaps' stored values (the combiners' t0-t2)
    if (d.modes.x & 1u) != 0u { registers[8] = textureSample(map0, map0_sampler, in.uv0); }
    if (d.modes.x & 2u) != 0u { registers[9] = textureSample(map1, map1_sampler, in.uv1); }
    if (d.modes.x & 4u) != 0u { registers[10] = textureSample(map2, map2_sampler, in.uv2); }
    // v0 the vertex's colour; v1 the screen geometry shader leaves black
    registers[4] = in.color;
    registers[5] = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    // r0's alpha starts as t0's
    registers[12] = vec4<f32>(0.0, 0.0, 0.0, registers[8].a);

    let mux_msb = (d.program.y & 0x100u) != 0u;
    for (var stage = 0u; stage < min(d.program.x, 8u); stage++) {
        var results: array<vec4<f32>, 6>; // rgb AB, CD, SUM; alpha AB, CD, SUM
        for (var portion = 0u; portion < 2u; portion++) {
            let alpha = portion == 1u;
            let inputs = select(word(d.rgb_inputs, stage), word(d.alpha_inputs, stage), alpha);
            let outputs = select(word(d.rgb_outputs, stage), word(d.alpha_outputs, stage), alpha);
            let flags = outputs >> 12u;
            let a = combiner_input(d, &registers, (inputs >> 24u) & 0xffu, alpha, stage);
            let b = combiner_input(d, &registers, (inputs >> 16u) & 0xffu, alpha, stage);
            let c = combiner_input(d, &registers, (inputs >> 8u) & 0xffu, alpha, stage);
            let e = combiner_input(d, &registers, inputs & 0xffu, alpha, stage);
            var ab = a * b;
            var cd = c * e;
            if !alpha && (flags & 0x02u) != 0u { ab = vec4<f32>(dot(a.rgb, b.rgb)); }
            if !alpha && (flags & 0x01u) != 0u { cd = vec4<f32>(dot(c.rgb, e.rgb)); }
            var sum = ab + cd;
            if (flags & 0x04u) != 0u {
                let r0a = registers[12].a;
                let pick_cd = select((u32(r0a * 255.0 + 0.5) & 1u) != 0u, r0a >= 0.5, mux_msb);
                sum = select(ab, cd, pick_cd);
            }
            results[portion * 3u + 0u] = output_mapping(ab, flags);
            results[portion * 3u + 1u] = output_mapping(cd, flags);
            results[portion * 3u + 2u] = output_mapping(sum, flags);
        }
        // write back once both portions have read their inputs
        for (var portion = 0u; portion < 2u; portion++) {
            let alpha = portion == 1u;
            let outputs = select(word(d.rgb_outputs, stage), word(d.alpha_outputs, stage), alpha);
            let destinations = array<u32, 3>((outputs >> 4u) & 0x0fu, outputs & 0x0fu, (outputs >> 8u) & 0x0fu);
            for (var k = 0u; k < 3u; k++) {
                let destination = destinations[k];
                // only v0, v1, t0-t3, r0, r1 are writable
                if destination != 4u && destination != 5u && !(destination >= 8u && destination <= 13u) {
                    continue;
                }
                let value = results[portion * 3u + k];
                if alpha {
                    registers[destination].a = value.a;
                } else {
                    registers[destination] = vec4<f32>(value.rgb, registers[destination].a);
                }
            }
        }
    }

    // final combiner: rgb = A*B + (1-A)*C + D, alpha = G
    let abcd = d.program.z;
    let efg = d.program.w;
    let fe = final_input(d, &registers, (efg >> 24u) & 0xffu, false);
    let ff = final_input(d, &registers, (efg >> 16u) & 0xffu, false);
    registers[15] = vec4<f32>(fe.rgb * ff.rgb, 0.0);
    registers[14] = vec4<f32>(saturate(registers[5].rgb + registers[12].rgb), 0.0);
    let fa = final_input(d, &registers, (abcd >> 24u) & 0xffu, false);
    let fb = final_input(d, &registers, (abcd >> 16u) & 0xffu, false);
    let fc = final_input(d, &registers, (abcd >> 8u) & 0xffu, false);
    let fd = final_input(d, &registers, abcd & 0xffu, false);
    let rgb = saturate(fa.rgb * fb.rgb + (1.0 - fa.rgb) * fc.rgb + fd.rgb);
    let alpha = final_input(d, &registers, (efg >> 8u) & 0xffu, true).a;

    // a meter's alpha kill: its map's zero alpha
    if d.modes.y != 0u && registers[8].a <= 0.0 {
        discard;
    }
    return vec4(rgb, alpha);
}
