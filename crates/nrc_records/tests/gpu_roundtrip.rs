//! The WGSL port against the Rust reference on a Vulkan device.
//!
//! `cargo test -p nrc_records --release -- --ignored --nocapture`
//!
//! A few thousand deterministic random cases (plus hand-picked edge cases) go
//! through every function in `nrc_records.wgsl` and the record accessors in
//! `nrc_buffers.wgsl` (tests/gpu_roundtrip.wgsl); the host reads the results
//! back in the same order and compares:
//!
//! - **bit-exact** (`exact`): unorm16, snorm16, f32tof16, R11G11B10 (pack and
//!   unpack), packedData, the indices, the NaN/Inf tests, sanitize, the
//!   termination heuristic, the record layouts (the buffers' words, read back
//!   as u32, against the Rust structs' bytes), the constants uniform;
//! - **bit-exact given the GPU's float intermediate** (`exact*`): the LogLuv
//!   and octahedral-normal quantizers applied by the Rust reference to the
//!   GPU's own `logY`/`uv` and octahedral point give the GPU's word. Against
//!   the Rust reference end to end (`quantized`), those two can be one step
//!   apart in a field when the float lands within the GPU's `log2`/`rcp`
//!   error of a step; the test bounds that to one step and a small rate;
//! - **within a tolerance** (`float`): decoded floats, spherical directions,
//!   color-space transforms (the GPU's `dot`, `normalize`, `rsqrt`, `rcp`,
//!   `acos`, `atan2`, `pow` are not IEEE correctly rounded and may fuse
//!   multiply-adds).
//!
//! Round-trip errors (encode then decode) are printed for information.

use bytemuck::cast_slice;
use nrc_records::*;
use std::collections::BTreeMap;
use wgpu::util::DeviceExt;

const IN_WORDS: usize = 40;
const OUT_WORDS: usize = 160;
const RANDOM_CASES: usize = 8192;

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        // splitmix64
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        (((self.next_u64() >> 32) * n as u64) >> 32) as u32
    }
    /// [0, 1)
    fn unit(&mut self) -> f32 {
        (self.u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
    /// 10^[lo, hi)
    fn log_uniform(&mut self, lo: f32, hi: f32) -> f32 {
        10f32.powf(self.range(lo, hi))
    }
}

#[derive(Clone, Copy, Default)]
struct Case {
    color: Float3,
    s: f32,
    n: Float3,
    v: Float3,
    world: Float3,
    rough: f32,
    a: u32,
    b: u32,
    albedo: Float3,
    specular: Float3,
    t: f32,
    s2: f32,
    dirty: Float3,
    primary: f32,
    cumul: f32,
    threshold: f32,
    c: u32,
    d: u32,
    p: Float2,
    local: Float3,
    e: u32,
}

impl Case {
    fn words(&self) -> [u32; IN_WORDS] {
        let mut w = [0u32; IN_WORDS];
        let mut put3 = |at: usize, v: Float3| {
            for k in 0..3 {
                w[at + k] = v[k].to_bits();
            }
        };
        put3(0, self.color);
        put3(4, self.n);
        put3(7, self.v);
        put3(10, self.world);
        put3(16, self.albedo);
        put3(19, self.specular);
        put3(24, self.dirty);
        put3(34, self.local);
        w[3] = self.s.to_bits();
        w[13] = self.rough.to_bits();
        w[14] = self.a;
        w[15] = self.b;
        w[22] = self.t.to_bits();
        w[23] = self.s2.to_bits();
        w[27] = self.primary.to_bits();
        w[28] = self.cumul.to_bits();
        w[29] = self.threshold.to_bits();
        w[30] = self.c;
        w[31] = self.d;
        w[32] = self.p[0].to_bits();
        w[33] = self.p[1].to_bits();
        w[37] = self.e;
        w
    }
}

fn direction(rng: &mut Rng) -> Float3 {
    match rng.below(10) {
        // axis-aligned, both signs, sometimes not unit length
        0 => {
            let mut v = [0.0; 3];
            v[rng.below(3) as usize] = if rng.below(2) == 0 { 1.0 } else { -2.5 };
            v
        }
        // on an octahedron edge or face diagonal
        1 => {
            let sx = if rng.below(2) == 0 { 1.0 } else { -1.0 };
            let sz = if rng.below(2) == 0 { 1.0 } else { -1.0 };
            [sx, 0.0, sz]
        }
        // unnormalized
        2 => [
            rng.range(-50.0, 50.0),
            rng.range(-50.0, 50.0),
            rng.range(-50.0, 50.0),
        ],
        _ => loop {
            let v = [
                rng.range(-1.0, 1.0),
                rng.range(-1.0, 1.0),
                rng.range(-1.0, 1.0),
            ];
            let l2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
            if l2 > 1e-4 && l2 <= 1.0 {
                let inv = 1.0 / l2.sqrt();
                break [v[0] * inv, v[1] * inv, v[2] * inv];
            }
        },
    }
}

fn color(rng: &mut Rng) -> Float3 {
    match rng.below(8) {
        0 => [0.0; 3],
        1 => {
            let g = rng.log_uniform(-7.0, 4.0);
            [g; 3]
        }
        2 => [rng.unit(), rng.unit(), rng.unit()],
        3 => [
            rng.log_uniform(-1.0, 4.0),
            rng.log_uniform(-1.0, 4.0),
            rng.log_uniform(-1.0, 4.0),
        ],
        4 => [
            rng.log_uniform(-7.0, -5.0),
            rng.log_uniform(-7.0, -5.0),
            rng.log_uniform(-7.0, -5.0),
        ],
        5 => {
            let mut c = [
                rng.log_uniform(-6.0, 4.0),
                rng.log_uniform(-6.0, 4.0),
                rng.log_uniform(-6.0, 4.0),
            ];
            c[rng.below(3) as usize] = 0.0;
            c
        }
        6 => [1.0, 1.0, 1.0],
        _ => [
            rng.log_uniform(-6.0, 4.0),
            rng.log_uniform(-6.0, 4.0),
            rng.log_uniform(-6.0, 4.0),
        ],
    }
}

/// Values on and around the unorm/snorm rounding points.
fn scalar(rng: &mut Rng) -> f32 {
    match rng.below(12) {
        0 => f32::NAN,
        1 => [f32::INFINITY, f32::NEG_INFINITY, 0.0, -0.0, 1.0, -1.0][rng.below(6) as usize],
        2 => (rng.below(65536) as f32 + 0.5) / 65535.0,
        3 => rng.below(65536) as f32 / 65535.0,
        4 => (rng.below(65535) as f32 - 32767.0 + 0.5) / 32767.0,
        5 => (rng.below(65535) as f32 - 32767.0) / 32767.0,
        6 => f32::from_bits(rng.u32()), // anything, NaNs and denormals included
        _ => rng.range(-1.5, 1.5),
    }
}

fn unit_scalar(rng: &mut Rng) -> f32 {
    match rng.below(4) {
        0 => (rng.below(65536) as f32 + 0.5) / 65535.0,
        1 => rng.below(65536) as f32 / 65535.0,
        _ => rng.unit(),
    }
}

/// Albedo-like: [0, 1], HDR, tiny, and f16 rounding midpoints (exact in f32).
fn albedo(rng: &mut Rng) -> Float3 {
    let channel = |rng: &mut Rng| match rng.below(8) {
        0 => 0.0,
        1 => rng.log_uniform(-1.0, 4.0),
        2 => rng.log_uniform(-8.0, -4.0),
        3 => {
            // the midpoint between a normal half and the next one
            let h = 0x0400 + rng.below(0x7bff - 0x0400);
            let lo = f16_to_f32(h);
            let hi = f16_to_f32(h + 1);
            0.5 * (lo + hi)
        }
        4 => {
            // R11G11B10 drops 4 (5 for blue) more bits: its rounding points
            let h = (0x0400 + rng.below(0x7800 - 0x0400)) & !0xf | 0x8;
            f16_to_f32(h)
        }
        5 => 1e5,
        _ => rng.unit(),
    };
    [channel(rng), channel(rng), channel(rng)]
}

fn dirty(rng: &mut Rng) -> Float3 {
    let mut c = direction(rng);
    match rng.below(6) {
        0 => c[rng.below(3) as usize] = f32::NAN,
        1 => {
            c[rng.below(3) as usize] = if rng.below(2) == 0 {
                f32::INFINITY
            } else {
                f32::NEG_INFINITY
            }
        }
        2 => c = color(rng),
        _ => {}
    }
    if c == [0.0; 3] {
        c = [0.0, 0.0, 1.0];
    }
    c
}

fn cases() -> Vec<Case> {
    let mut rng = Rng(0x4e52_435f_7265_636f);
    let special_a = [
        0u32,
        u32::MAX,
        0x7fff_7fff,
        0x8000_8000,
        0x8001_4580,
        0x0000_03c0,
        0x7800_0000,
        0x001e_0000,
        0xffc0_0000,
    ];
    let mut cases = Vec::new();
    for k in 0..RANDOM_CASES + special_a.len() {
        let width = 1 + rng.below(2048);
        let height = 1 + rng.below(2048);
        let mut c = Case {
            color: color(&mut rng),
            s: scalar(&mut rng),
            n: direction(&mut rng),
            v: direction(&mut rng),
            world: [
                rng.range(-5000.0, 5000.0),
                rng.range(-5000.0, 5000.0),
                rng.range(-500.0, 500.0),
            ],
            rough: rng.unit(),
            a: rng.u32(),
            b: rng.u32(),
            albedo: albedo(&mut rng),
            specular: albedo(&mut rng),
            t: unit_scalar(&mut rng),
            s2: scalar(&mut rng),
            dirty: dirty(&mut rng),
            primary: if rng.below(4) == 0 {
                0.0
            } else {
                rng.log_uniform(-3.0, 2.0)
            },
            cumul: rng.log_uniform(-3.0, 2.0),
            threshold: rng.range(0.0, 1.0),
            c: width | (height << 16),
            d: rng.below(width) | (rng.below(height) << 16),
            p: [rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)],
            local: [
                rng.range(-10.0, 10.0),
                rng.range(-10.0, 10.0),
                rng.range(-10.0, 10.0),
            ],
            e: rng.u32(),
        };
        if k >= RANDOM_CASES {
            c.a = special_a[k - RANDOM_CASES];
        }
        if k % 16 == 0 {
            c.p = [[0.0, 0.0], [1.0, 1.0], [-1.0, 0.0], [0.5, -0.5]][(k / 16) % 4];
        }
        cases.push(c);
    }
    cases
}

fn constants() -> NrcConstants {
    NrcConstants {
        frame_dimensions: [1280, 720],
        training_dimensions: [197, 111],
        scene_pos_scale: [1.0 / 10_000.0, 1.0 / 10_000.0, 1.0 / 1_000.0],
        samples_per_pixel: 1,
        scene_pos_bias: [0.5, 0.5, 0.5],
        max_path_vertices: 8,
        learn_irradiance: 1,
        radiance_cache_direct: 0,
        radiance_unpack_multiplier: 37.5,
        resolve_mode: 10,
        enable_termination_heuristic: 1,
        skip_delta_vertices: 0,
        termination_heuristic_threshold: 0.01,
        training_termination_heuristic_threshold: 0.1,
        proportion_unbiased: 0.0625,
        pad0: 0xdead_0001,
        pad1: 0xdead_0002,
        pad2: 0xdead_0003,
    }
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

struct GpuResults {
    outputs: Vec<u32>,
    query_path_info: Vec<u32>,
    training_path_info: Vec<u32>,
    training_path_vertices: Vec<u32>,
    query_radiance_params: Vec<u32>,
    counters: Vec<u32>,
    adapter: String,
}

fn run_gpu(cases: &[Case], constants: &NrcConstants, query_radiance: &[f32]) -> GpuResults {
    futures_lite::future::block_on(async {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = instance
            .request_adapter(&Default::default())
            .await
            .expect("a Vulkan adapter");
        let info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .unwrap();

        let source = format!(
            "{}\n{}\n{}",
            WGSL,
            WGSL_BUFFERS,
            include_str!("gpu_roundtrip.wgsl")
        );
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nrc_records roundtrip"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("nrc_records roundtrip"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        let count = cases.len();
        let input_words: Vec<u32> = cases.iter().flat_map(|c| c.words()).collect();
        let storage = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST;
        let init = |label: &str, contents: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage,
            })
        };
        // Records start with a garbage pattern so a word the shader fails to
        // write shows up.
        let garbage = |words: usize| vec![0xa5a5_a5a5u32; words];
        let constants_buffer = init(
            "constants",
            bytemuck::bytes_of(constants),
            wgpu::BufferUsages::UNIFORM,
        );
        let inputs = init("inputs", cast_slice(&input_words), storage);
        let outputs = init(
            "outputs",
            cast_slice(&garbage(count * OUT_WORDS + 32)),
            storage,
        );
        let query_path_info = init("query path info", cast_slice(&garbage(count * 2)), storage);
        let training_path_info = init(
            "training path info",
            cast_slice(&garbage(count * 2)),
            storage,
        );
        let training_path_vertices = init(
            "training path vertices",
            cast_slice(&garbage(count * 12)),
            storage,
        );
        let query_radiance_params = init(
            "query radiance params",
            cast_slice(&garbage(count * 14)),
            storage,
        );
        let counters = init("counters", cast_slice(&[0u32; 2]), storage);
        let query_radiance_buffer = init("query radiance", cast_slice(query_radiance), storage);

        let buffers = [
            &constants_buffer,
            &inputs,
            &outputs,
            &query_path_info,
            &training_path_info,
            &training_path_vertices,
            &query_radiance_params,
            &counters,
            &query_radiance_buffer,
        ];
        let entries: Vec<_> = buffers
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });

        let readable = [
            &outputs,
            &query_path_info,
            &training_path_info,
            &training_path_vertices,
            &query_radiance_params,
            &counters,
        ];
        let readbacks: Vec<wgpu::Buffer> = readable
            .iter()
            .map(|b| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("readback"),
                    size: b.size(),
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                })
            })
            .collect();

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(count.div_ceil(64) as u32, 1, 1);
        }
        for (src, dst) in readable.iter().zip(&readbacks) {
            encoder.copy_buffer_to_buffer(src, 0, dst, 0, src.size());
        }
        queue.submit([encoder.finish()]);

        for r in &readbacks {
            r.slice(..)
                .map_async(wgpu::MapMode::Read, |result| result.unwrap());
        }
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let mut read: Vec<Vec<u32>> = readbacks
            .iter()
            .map(|r| cast_slice::<u8, u32>(&r.slice(..).get_mapped_range().unwrap()).to_vec())
            .collect();

        GpuResults {
            counters: read.pop().unwrap(),
            query_radiance_params: read.pop().unwrap(),
            training_path_vertices: read.pop().unwrap(),
            training_path_info: read.pop().unwrap(),
            query_path_info: read.pop().unwrap(),
            outputs: read.pop().unwrap(),
            adapter: format!(
                "{} ({:?}, driver {} {})",
                info.name, info.backend, info.driver, info.driver_info
            ),
        }
    })
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Stat {
    kind: &'static str,
    count: usize,
    exact: usize,
    failures: usize,
    max_ulps: u32,
    max_abs: f32,
    first_failure: Option<String>,
}

#[derive(Default)]
struct Checker {
    stats: BTreeMap<&'static str, Stat>,
    /// (input, GPU decode of the GPU encoding)
    luv_round_trip: Vec<(Float3, Float3)>,
    oct_round_trip: Vec<(Float3, Float3)>,
}

/// Distance in units in the last place, 0 for two NaNs or equal values
/// (+0 and -0 too).
fn ulps(a: f32, b: f32) -> u32 {
    if a.is_nan() && b.is_nan() || a == b {
        return 0;
    }
    if a.is_nan() || b.is_nan() {
        return u32::MAX;
    }
    let ordered = |x: f32| {
        let i = x.to_bits() as i32;
        if i < 0 { i32::MIN.wrapping_sub(i) } else { i }
    };
    (ordered(a) as i64 - ordered(b) as i64)
        .unsigned_abs()
        .min(u32::MAX as u64) as u32
}

impl Checker {
    fn stat(&mut self, name: &'static str, kind: &'static str) -> &mut Stat {
        let s = self.stats.entry(name).or_default();
        s.kind = kind;
        s.count += 1;
        s
    }

    fn exact(&mut self, name: &'static str, case: usize, gpu: u32, cpu: u32) {
        let s = self.stat(name, "exact");
        if gpu == cpu {
            s.exact += 1;
        } else {
            s.failures += 1;
            s.first_failure
                .get_or_insert(format!("case {case}: gpu {gpu:#010x} cpu {cpu:#010x}"));
        }
    }

    /// Bit-exact given the GPU's own intermediate.
    fn exact_derived(&mut self, name: &'static str, case: usize, gpu: u32, cpu: u32) {
        self.exact(name, case, gpu, cpu);
        self.stats.get_mut(name).unwrap().kind = "exact*";
    }

    /// An integer encoding against the Rust reference end to end: fields may
    /// be one step apart. `fields` are (shift, bits).
    fn quantized(
        &mut self,
        name: &'static str,
        case: usize,
        gpu: u32,
        cpu: u32,
        fields: &[(u32, u32)],
    ) {
        let s = self.stat(name, "quantized");
        if gpu == cpu {
            s.exact += 1;
            return;
        }
        let one_step = fields.iter().all(|&(shift, bits)| {
            let mask = (1u32 << bits) - 1;
            let (g, c) = ((gpu >> shift) & mask, (cpu >> shift) & mask);
            g.abs_diff(c) <= 1
        });
        let fields_off = fields
            .iter()
            .filter(|&&(shift, bits)| ((gpu ^ cpu) >> shift) & ((1u32 << bits) - 1) != 0)
            .count();
        s.max_ulps = s.max_ulps.max(fields_off as u32);
        if !one_step {
            s.failures += 1;
            s.first_failure
                .get_or_insert(format!("case {case}: gpu {gpu:#010x} cpu {cpu:#010x}"));
        }
    }

    fn float(
        &mut self,
        name: &'static str,
        case: usize,
        gpu: f32,
        cpu: f32,
        max_ulps: u32,
        max_abs: f32,
    ) {
        let s = self.stat(name, "float");
        let d = ulps(gpu, cpu);
        if d == 0 {
            s.exact += 1;
            return;
        }
        let abs = (gpu - cpu).abs();
        if d != u32::MAX {
            s.max_ulps = s.max_ulps.max(d);
            s.max_abs = s.max_abs.max(abs);
        }
        if !(d <= max_ulps || abs <= max_abs) {
            s.failures += 1;
            s.first_failure.get_or_insert(format!(
                "case {case}: gpu {gpu:e} ({:#010x}) cpu {cpu:e} ({:#010x}), {d} ulps",
                gpu.to_bits(),
                cpu.to_bits()
            ));
        }
    }

    fn float2(
        &mut self,
        name: &'static str,
        case: usize,
        gpu: Float2,
        cpu: Float2,
        max_ulps: u32,
        max_abs: f32,
    ) {
        for k in 0..2 {
            self.float(name, case, gpu[k], cpu[k], max_ulps, max_abs);
        }
    }

    fn float3(
        &mut self,
        name: &'static str,
        case: usize,
        gpu: Float3,
        cpu: Float3,
        max_ulps: u32,
        max_abs: f32,
    ) {
        for k in 0..3 {
            self.float(name, case, gpu[k], cpu[k], max_ulps, max_abs);
        }
    }

    /// Bit-exact, except that any NaN equals any NaN: the GPU's f16 to f32
    /// conversion returns its canonical NaN (0x7fffffff) instead of carrying
    /// the half's payload over.
    fn exact_f3_nan(&mut self, name: &'static str, case: usize, gpu: Float3, cpu: Float3) {
        for k in 0..3 {
            let both_nan = gpu[k].is_nan() && cpu[k].is_nan();
            self.exact(
                name,
                case,
                gpu[k].to_bits(),
                if both_nan {
                    gpu[k].to_bits()
                } else {
                    cpu[k].to_bits()
                },
            );
        }
    }

    /// NrcCartesianToSphericalUnorm of `p`. theta = acos(z) / pi is badly
    /// conditioned near the poles: d theta = dz / (pi sin theta), and near
    /// sin theta = 0, acos(1 - dz) = sqrt(2 dz). The GPU's normalize (an
    /// approximate rsqrt) puts z a few ulps from the CPU's, so the tolerance
    /// follows the conditioning, capped at sqrt(2 * 4 ulps) / pi = 2.2e-4.
    /// phi = atan2(-y, -x) / 2pi + 0.5 wraps between 0 and 1 across its branch
    /// cut, and is undefined (atan2(0, 0)) exactly on the poles.
    fn spherical(&mut self, name: &'static str, case: usize, gpu: Float2, cpu: Float2, p: Float3) {
        let (x, y, z) = (p[0] as f64, p[1] as f64, p[2] as f64);
        let sin_theta = x.hypot(y) / x.hypot(y).hypot(z);
        let tol_theta = (8.0 * f32::EPSILON as f64 / (core::f64::consts::PI * sin_theta))
            .clamp(2e-6, 3e-4) as f32;
        if gpu[0].is_nan() && (cpu[0] < tol_theta || cpu[0] > 1.0 - tol_theta) {
            // Within an ulp or two of a pole the GPU's normalized |z| can
            // round past 1, and acos gives NaN; the HLSL has no clamp either
            // (the same hardware rsqrt does the same under DX). Counted, not
            // failed.
            self.stat(
                "spherical: NaN theta at a pole (acos(|z| > 1), as in the HLSL)",
                "faithful",
            )
            .exact += 1;
        } else {
            self.float(name, case, gpu[0], cpu[0], 16, tol_theta);
        }
        if sin_theta > 0.0 {
            let mut g = gpu[1];
            if (g - cpu[1]).abs() > 0.5 {
                g -= (g - cpu[1]).signum();
            }
            self.float(name, case, g, cpu[1], 16, 2e-6);
        }
    }

    fn exact_f3(&mut self, name: &'static str, case: usize, gpu: Float3, cpu: Float3) {
        for k in 0..3 {
            self.exact(name, case, gpu[k].to_bits(), cpu[k].to_bits());
        }
    }
}

/// Reads one case's outputs in the order the shader wrote them.
struct Reader<'a> {
    words: &'a [u32],
    at: usize,
}

impl Reader<'_> {
    fn u(&mut self) -> u32 {
        self.at += 1;
        self.words[self.at - 1]
    }
    fn f(&mut self) -> f32 {
        f32::from_bits(self.u())
    }
    fn v2(&mut self) -> Float2 {
        [self.f(), self.f()]
    }
    fn v3(&mut self) -> Float3 {
        [self.f(), self.f(), self.f()]
    }
}

const LOG_LUV_FIELDS: &[(u32, u32)] = &[(18, 14), (9, 9), (0, 9)];
const SNORM2X16_FIELDS: &[(u32, u32)] = &[(16, 16), (0, 16)];

/// A tolerance for a value that results from cancellation: a few ulps of the
/// largest term.
fn scale(v: Float3) -> f32 {
    v.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

#[allow(clippy::too_many_lines)]
fn check_case(
    k: &mut Checker,
    i: usize,
    c: &Case,
    gpu: &GpuResults,
    consts: &NrcConstants,
    query_radiance: &[f32],
) -> u32 {
    let mut r = Reader {
        words: &gpu.outputs[i * OUT_WORDS..(i + 1) * OUT_WORDS],
        at: 0,
    };
    let eps = f32::EPSILON;

    // unorm / snorm
    k.exact("pack_unorm16", i, r.u(), nrc_pack_unorm16(c.s));
    k.exact(
        "pack_unorm16_unsafe",
        i,
        r.u(),
        nrc_pack_unorm16_unsafe(c.t),
    );
    k.exact("pack_unorm2x16", i, r.u(), nrc_pack_unorm2x16([c.s, c.s2]));
    k.float2(
        "unpack_unorm2x16",
        i,
        r.v2(),
        nrc_unpack_unorm2x16(c.a),
        1,
        0.0,
    );
    k.exact(
        "float_to_snorm16",
        i,
        r.u(),
        nrc_float_to_snorm16(c.s) as u32,
    );
    k.exact("pack_snorm2x16", i, r.u(), nrc_pack_snorm2x16([c.s, c.s2]));
    k.float2(
        "unpack_snorm2x16",
        i,
        r.v2(),
        nrc_unpack_snorm2x16(c.a),
        1,
        0.0,
    );

    // octahedral
    k.float2("oct_wrap", i, r.v2(), nrc_oct_wrap(c.p), 0, 0.0);
    k.float3(
        "oct_to_direction",
        i,
        r.v3(),
        nrc_oct_to_direction(c.p),
        4,
        4.0 * eps,
    );
    let gpu_oct = r.v2();
    k.float2(
        "direction_to_oct",
        i,
        gpu_oct,
        nrc_direction_to_oct(c.n),
        2,
        2.0 * eps,
    );
    let gpu_normal = r.u();
    k.quantized(
        "encode_normal2x16",
        i,
        gpu_normal,
        nrc_encode_normal2x16(c.n),
        SNORM2X16_FIELDS,
    );
    k.exact_derived(
        "encode_normal2x16 | gpu oct",
        i,
        gpu_normal,
        nrc_pack_snorm2x16(gpu_oct),
    );
    k.float3(
        "decode_normal2x16",
        i,
        r.v3(),
        nrc_decode_normal2x16(c.a),
        4,
        4.0 * eps,
    );

    // f16 and R11G11B10
    for ch in 0..3 {
        k.exact(
            "f32_to_f16",
            i,
            r.u(),
            f32_to_f16(c.albedo[ch].min(65504.0)),
        );
    }
    k.exact("pack_r11g11b10", i, r.u(), nrc_pack_r11g11b10(c.albedo));
    k.exact_f3_nan(
        "unpack_r11g11b10 (NaN = NaN)",
        i,
        r.v3(),
        nrc_unpack_r11g11b10(c.a),
    );

    // color spaces and LogLuv
    k.float3(
        "rgb_to_xyz_rec709",
        i,
        r.v3(),
        nrc_rgb_to_xyz_rec709(c.color),
        2,
        2.0 * eps * scale(c.color),
    );
    k.float3(
        "xyz_to_rgb_rec709",
        i,
        r.v3(),
        nrc_xyz_to_rgb_rec709(c.color),
        2,
        4.0 * eps * scale(c.color),
    );
    let gpu_luv = r.u();
    k.quantized(
        "encode_log_luv_hdr",
        i,
        gpu_luv,
        nrc_encode_log_luv_hdr(c.color),
        LOG_LUV_FIELDS,
    );
    let gpu_log_y = r.f();
    let gpu_uv = r.v2();
    k.exact_derived(
        "encode_log_luv_hdr | gpu logY,uv",
        i,
        gpu_luv,
        nrc_log_luv_quantize(gpu_log_y, gpu_uv),
    );
    let xyz = nrc_rgb_to_xyz_rec709(c.color);
    let cpu_log_y = 409.6 * (xyz[1].log2() + 20.0);
    if cpu_log_y.is_finite() {
        k.float("log_luv logY (log2)", i, gpu_log_y, cpu_log_y, 4, 1e-3);
    }
    let decoded = nrc_decode_log_luv_hdr(c.a);
    k.float3(
        "decode_log_luv_hdr",
        i,
        r.v3(),
        decoded,
        64,
        2e-6 * scale(decoded),
    );

    // spherical
    k.spherical(
        "cartesian_to_spherical_unorm",
        i,
        r.v2(),
        nrc_cartesian_to_spherical_unorm(c.n),
        c.n,
    );
    let safe = nrc_safe_cartesian_to_spherical_unorm(c.dirty);
    if nrc_is_nan3(c.dirty) || nrc_is_inf3(c.dirty) {
        k.exact(
            "safe_cartesian_to_spherical_unorm (not finite)",
            i,
            r.u() | r.u(),
            0,
        );
    } else {
        k.spherical(
            "safe_cartesian_to_spherical_unorm",
            i,
            r.v2(),
            safe,
            c.dirty,
        );
    }

    // NaN / Inf
    let bits = nrc_is_nan(c.s) as u32
        | (nrc_is_inf(c.s) as u32) << 1
        | (nrc_is_nan3(c.dirty) as u32) << 2
        | (nrc_is_inf3(c.dirty) as u32) << 3;
    k.exact("is_nan / is_inf", i, r.u(), bits);
    k.exact_f3(
        "sanitize_nans_infs",
        i,
        r.v3(),
        nrc_sanitize_nans_infs(c.dirty),
    );

    // indices
    let dims = [c.c & 0xffff, c.c >> 16];
    let pixel = [c.d & 0xffff, c.d >> 16];
    k.exact(
        "calculate_query_path_index",
        i,
        r.u(),
        nrc_calculate_query_path_index(dims, pixel, c.e & 3, (c.e >> 2) & 7),
    );
    k.exact(
        "calculate_training_path_index",
        i,
        r.u(),
        nrc_calculate_training_path_index(dims, pixel),
    );
    k.exact(
        "calculate_training_path_vertex_index",
        i,
        r.u(),
        nrc_calculate_training_path_vertex_index(dims, pixel, (c.e >> 5) & 7, (c.e >> 8) & 15),
    );

    // positions
    let encoded = nrc_encode_position(c.world, consts);
    let gpu_encoded = r.v3();
    k.float3("encode_position (fma)", i, gpu_encoded, encoded, 1, 0.0);
    k.float3(
        "encode_position_local (fma)",
        i,
        r.v3(),
        nrc_encode_position_local(c.local, c.world, consts),
        1,
        0.0,
    );

    // packedData
    let flag = NRC_PATH_FLAG_HAS_EXITED_SCENE << ((c.e >> 12) & 3);
    let p1 = nrc_set_vertex_count(c.a & !NRC_VERTEX_COUNT_MASK, (c.e >> 16) & 0x7f);
    let p2 = nrc_set_debug_path_termination_reason(p1, ((c.e >> 23) & 0xff) % 7);
    let p3 = nrc_set_flag(p2, flag);
    let p4 = nrc_clear_flag(p3, NRC_PATH_FLAG_HAS_EXITED_SCENE << ((c.e >> 14) & 3));
    let p5 = nrc_set_flag_value(p4, flag, (c.b & 1) == 1);
    k.exact("set_vertex_count", i, r.u(), p1);
    k.exact("set_debug_path_termination_reason", i, r.u(), p2);
    k.exact("set_flag", i, r.u(), p3);
    k.exact("clear_flag", i, r.u(), p4);
    k.exact("set_flag_value", i, r.u(), p5);
    let gets = nrc_get_vertex_count(c.a)
        | nrc_get_debug_path_termination_reason(c.a) << 8
        | (nrc_get_flag(c.a, NRC_PATH_FLAG_HAS_EXITED_SCENE) as u32) << 16
        | (nrc_get_flag(c.a, NRC_PATH_FLAG_IS_UNBIASED) as u32) << 17
        | (nrc_get_flag(c.a, NRC_PATH_FLAG_PREVIOUS_HIT_WAS_DELTA_LOBE) as u32) << 18
        | (nrc_get_flag(c.a, NRC_PATH_FLAG_HEURISTIC_RESET) as u32) << 19;
    k.exact("get_vertex_count / reason / flag", i, r.u(), gets);
    let state = NrcPathState {
        primary_spread_radius: c.primary,
        cumul_spread_radius: c.cumul,
        ..Default::default()
    };
    k.exact(
        "evaluate_termination_heuristic",
        i,
        r.u(),
        nrc_evaluate_termination_heuristic(&state, c.threshold) as u32,
    );

    // ---- records ----
    let counter = r.u(); // checked as a set by the caller

    // The training path vertex: the buffer's 12 words, the words the read
    // helper returned, the Rust struct's bytes.
    let record: [u32; 12] = gpu.training_path_vertices[i * 12..(i + 1) * 12]
        .try_into()
        .unwrap();
    let read_back: [u32; 12] = core::array::from_fn(|_| r.u());
    let gpu_luv_dirty = r.u();
    let gpu_normal_v = r.u();
    let gpu_oct_v = r.v2();
    k.exact_derived(
        "encode_normal2x16 | gpu oct",
        i,
        gpu_normal_v,
        nrc_pack_snorm2x16(gpu_oct_v),
    );
    for w in 0..12 {
        k.exact(
            "path vertex: read helper = buffer words",
            i,
            read_back[w],
            record[w],
        );
    }
    let mut cpu_vertex =
        nrc_initialize_packed_path_vertex(c.rough, c.n, c.v, c.albedo, c.specular, encoded);
    if i & 1 == 1 {
        cpu_vertex = nrc_update_training_path_vertex(cpu_vertex, c.color, c.dirty);
    }
    let cpu_words: &[u32] = cast_slice(bytemuck::bytes_of(&cpu_vertex));
    // The words that hold an encoding through log2/rcp: exact against the
    // GPU's own encoder output, within a step against the Rust reference.
    let (gpu_data0, gpu_data1) = if i & 1 == 1 {
        (gpu_luv, gpu_luv_dirty)
    } else {
        (0, 0x8001_4580)
    };
    let direct = [gpu_data0, gpu_data1, 0, gpu_normal, gpu_normal_v];
    for w in [0, 1, 3, 4] {
        k.exact(
            "path vertex: record word = gpu encoder",
            i,
            record[w],
            direct[w],
        );
        let fields = if w < 2 {
            LOG_LUV_FIELDS
        } else {
            SNORM2X16_FIELDS
        };
        k.quantized(
            "path vertex: LogLuv / oct words",
            i,
            record[w],
            cpu_words[w],
            fields,
        );
    }
    for w in [2, 5, 6, 7, 11] {
        k.exact(
            "path vertex: roughness, R11G11B10, pads",
            i,
            record[w],
            cpu_words[w],
        );
    }
    for w in 8..11 {
        k.exact(
            "path vertex: encodedPosition = gpu encode_position",
            i,
            record[w],
            gpu_encoded[w - 8].to_bits(),
        );
        k.float(
            "path vertex: encodedPosition",
            i,
            f32::from_bits(record[w]),
            f32::from_bits(cpu_words[w]),
            1,
            0.0,
        );
    }

    // Unpacking the GPU's record
    let gpu_record: NrcPackedPathVertex = bytemuck::cast(record);
    let cpu_unpacked = if (i >> 1) & 1 == 0 {
        nrc_unpack_path_vertex(gpu_record)
    } else {
        nrc_unpack_path_vertex_with(gpu_record, c.color, c.dirty)
    };
    let radiance = r.v3();
    let throughput = r.v3();
    k.float3(
        "unpack_path_vertex radiance/throughput",
        i,
        radiance,
        cpu_unpacked.radiance,
        64,
        2e-6 * scale(cpu_unpacked.radiance),
    );
    k.float3(
        "unpack_path_vertex radiance/throughput",
        i,
        throughput,
        cpu_unpacked.throughput,
        64,
        2e-6 * scale(cpu_unpacked.throughput),
    );
    k.exact_f3(
        "unpack_path_vertex position",
        i,
        r.v3(),
        cpu_unpacked.encoded_position,
    );
    k.exact(
        "unpack_path_vertex roughness",
        i,
        r.u(),
        cpu_unpacked.linear_roughness.to_bits(),
    );
    let gpu_n = r.v3();
    let gpu_v = r.v3();
    k.oct_round_trip.push((c.n, gpu_n));
    k.oct_round_trip.push((c.v, gpu_v));
    k.float3(
        "unpack_path_vertex normal/view",
        i,
        gpu_n,
        cpu_unpacked.normal,
        4,
        4.0 * eps,
    );
    k.float3(
        "unpack_path_vertex normal/view",
        i,
        gpu_v,
        cpu_unpacked.view_direction,
        4,
        4.0 * eps,
    );
    k.exact_f3(
        "unpack_path_vertex albedo/specular",
        i,
        r.v3(),
        cpu_unpacked.albedo,
    );
    k.exact_f3(
        "unpack_path_vertex albedo/specular",
        i,
        r.v3(),
        cpu_unpacked.specular,
    );

    // NrcRadianceParams: the 14 buffer words, from the GPU's unpacked vertex
    let params_words: [u32; 14] = gpu.query_radiance_params[i * 14..(i + 1) * 14]
        .try_into()
        .unwrap();
    let read_back: [u32; 14] = core::array::from_fn(|_| r.u());
    for w in 0..14 {
        k.exact(
            "radiance params: read helper = buffer words",
            i,
            read_back[w],
            params_words[w],
        );
    }
    let gpu_vertex = NrcPathVertex {
        normal: gpu_n,
        view_direction: gpu_v,
        ..cpu_unpacked
    };
    let cpu_params = nrc_create_radiance_params(&gpu_vertex);
    let gpu_params: NrcRadianceParams = bytemuck::cast(params_words);
    k.exact_f3(
        "radiance params: position",
        i,
        gpu_params.encoded_position,
        cpu_params.encoded_position,
    );
    k.exact(
        "radiance params: roughness",
        i,
        gpu_params.roughness.to_bits(),
        cpu_params.roughness.to_bits(),
    );
    k.spherical(
        "radiance params: normal/view (spherical)",
        i,
        gpu_params.normal,
        cpu_params.normal,
        gpu_n,
    );
    k.spherical(
        "radiance params: normal/view (spherical)",
        i,
        gpu_params.view_direction,
        cpu_params.view_direction,
        gpu_v,
    );
    k.exact_f3(
        "radiance params: albedo/specular",
        i,
        gpu_params.albedo,
        cpu_params.albedo,
    );
    k.exact_f3(
        "radiance params: albedo/specular",
        i,
        gpu_params.specular,
        cpu_params.specular,
    );

    // NrcPackedQueryPathInfo
    let q = &gpu.query_path_info[i * 2..i * 2 + 2];
    k.exact("query path info: buffer words", i, q[0], gpu_luv);
    k.exact("query path info: buffer words", i, q[1], c.b);
    k.exact("query path info: read helper", i, r.u(), q[0]);
    k.exact("query path info: read helper", i, r.u(), q[1]);
    let unpacked = nrc_unpack_query_path_info(NrcPackedQueryPathInfo {
        prefix_throughput: q[0],
        query_buffer_index: q[1],
    });
    let gpu_prefix = r.v3();
    k.luv_round_trip.push((c.color, gpu_prefix));
    k.float3(
        "unpack_query_path_info",
        i,
        gpu_prefix,
        unpacked.prefix_throughput,
        64,
        2e-6 * scale(unpacked.prefix_throughput),
    );
    k.exact(
        "unpack_query_path_info index",
        i,
        r.u(),
        unpacked.query_buffer_index,
    );

    // NrcPackedTrainingPathInfo
    let t = &gpu.training_path_info[i * 2..i * 2 + 2];
    let cpu_t = nrc_pack_training_path_info(NrcTrainingPathInfo {
        packed_data: p5,
        query_buffer_index: c.a,
    });
    k.exact(
        "training path info: buffer words",
        i,
        t[0],
        cpu_t.packed_data,
    );
    k.exact(
        "training path info: buffer words",
        i,
        t[1],
        cpu_t.query_buffer_index,
    );
    let cpu_u = nrc_unpack_training_path_info(cpu_t);
    k.exact("unpack_training_path_info", i, r.u(), cpu_u.packed_data);
    k.exact(
        "unpack_training_path_info",
        i,
        r.u(),
        cpu_u.query_buffer_index,
    );

    // Query radiance
    let qr = [
        query_radiance[i * 3],
        query_radiance[i * 3 + 1],
        query_radiance[i * 3 + 2],
    ];
    k.exact_f3(
        "read_query_radiance + unpack_query_radiance",
        i,
        r.v3(),
        nrc_unpack_query_radiance(consts, qr),
    );
    assert!(r.at <= OUT_WORDS);
    counter
}

// ---------------------------------------------------------------------------
// The test
// ---------------------------------------------------------------------------

#[test]
#[ignore = "needs a Vulkan GPU"]
#[allow(clippy::print_stdout)]
fn wgsl_matches_rust_reference() {
    let cases = cases();
    let consts = constants();
    let mut rng = Rng(7);
    let query_radiance: Vec<f32> = (0..cases.len() * 3)
        .map(|_| rng.log_uniform(-4.0, 1.0))
        .collect();
    let gpu = run_gpu(&cases, &consts, &query_radiance);
    println!("adapter: {}", gpu.adapter);
    println!("cases: {}", cases.len());

    let mut k = Checker::default();
    let mut indices: Vec<u32> = cases
        .iter()
        .enumerate()
        .map(|(i, c)| check_case(&mut k, i, c, &gpu, &consts, &query_radiance))
        .collect();

    // The constants uniform, field by field, against the Rust struct's bytes
    let echo = &gpu.outputs[cases.len() * OUT_WORDS..cases.len() * OUT_WORDS + 24];
    let cpu: &[u32] = cast_slice(bytemuck::bytes_of(&consts));
    for w in 0..24 {
        k.exact("NrcConstants uniform layout", w, echo[w], cpu[w]);
    }

    // Counters: every invocation got a distinct Queries index, and the totals
    indices.sort_unstable();
    let permutation = indices.iter().enumerate().all(|(i, &x)| x == i as u32);
    k.exact(
        "increment_counter: Queries indices are 0..N",
        0,
        permutation as u32,
        1,
    );
    k.exact(
        "increment_counter: Queries total",
        0,
        gpu.counters[0],
        cases.len() as u32,
    );
    k.exact(
        "increment_counter: TrainingRecords total",
        0,
        gpu.counters[1],
        cases.len().div_ceil(2) as u32,
    );

    println!();
    println!(
        "{:<52} {:>9} {:>8} {:>8} {:>8} {:>8} {:>10}",
        "check", "kind", "values", "exact", "fail", "max ulp", "max abs"
    );
    let mut failed = false;
    for (name, s) in &k.stats {
        let ulp = if s.kind == "quantized" {
            format!("{} fld", s.max_ulps)
        } else {
            s.max_ulps.to_string()
        };
        println!(
            "{name:<52} {:>9} {:>8} {:>8} {:>8} {:>8} {:>10.3e}",
            s.kind, s.count, s.exact, s.failures, ulp, s.max_abs
        );
        if let Some(f) = &s.first_failure {
            println!("    first failure: {f}");
            failed = true;
        }
    }

    // Quantized checks: one step at most (above) and rare
    for (name, s) in &k.stats {
        if s.kind == "quantized" {
            let off = s.count - s.exact;
            println!(
                "{name}: {off} of {} words one step from the Rust reference ({:.3}%)",
                s.count,
                100.0 * off as f64 / s.count as f64
            );
            assert!(off * 100 <= s.count, "{name}: more than 1% one step off");
        }
    }

    round_trip_report(&k);
    assert!(!failed, "some checks failed, see above");
}

/// LogLuv: relative luminance error and per-channel error relative to the
/// largest channel, over (color, decoded) pairs; in gamut means every channel
/// is at least 5% of the largest (saturated colors lose their small channels
/// to the decoder's clamp of out-of-gamut values).
#[allow(clippy::print_stdout)]
fn report_log_luv(label: &str, pairs: &[(Float3, Float3)]) {
    for (group, in_gamut_only) in [("in gamut", true), ("all colors", false)] {
        let (mut y_max, mut c_max, mut c_sum, mut n) = (0.0f64, 0.0f64, 0.0f64, 0usize);
        for &(c, d) in pairs {
            let y = nrc_rgb_to_xyz_rec709(c)[1] as f64;
            let m = scale(c) as f64;
            if !(1e-5..=1e5).contains(&y)
                || in_gamut_only && c.iter().any(|&x| (x as f64) < 0.05 * m)
            {
                continue;
            }
            let dy = nrc_rgb_to_xyz_rec709(d)[1] as f64;
            y_max = y_max.max(((dy - y) / y).abs());
            for k in 0..3 {
                let e = (d[k] as f64 - c[k] as f64).abs() / m;
                c_max = c_max.max(e);
                c_sum += e;
            }
            n += 1;
        }
        println!(
            "  {label} LogLuv, {group} ({n} colors): luminance rel. error max {:.4}%; channel error / max channel: max {:.4}%, mean {:.4}%",
            y_max * 100.0,
            c_max * 100.0,
            c_sum / (3 * n.max(1)) as f64 * 100.0
        );
    }
}

#[allow(clippy::print_stdout)]
fn report_oct(label: &str, pairs: &[(Float3, Float3)]) {
    let (mut max, mut sum) = (0.0f64, 0.0f64);
    for &(v, d) in pairs {
        let l = (v[0] as f64).hypot(v[1] as f64).hypot(v[2] as f64);
        let dot =
            (v[0] as f64 * d[0] as f64 + v[1] as f64 * d[1] as f64 + v[2] as f64 * d[2] as f64) / l;
        let angle = dot.clamp(-1.0, 1.0).acos().to_degrees();
        max = max.max(angle);
        sum += angle;
    }
    println!(
        "  {label} octahedral 2x16 normal ({} directions): angle error max {max:.5} deg, mean {:.5} deg",
        pairs.len(),
        sum / pairs.len() as f64
    );
}

/// Encode then decode: the GPU's pairs from the test cases, and 200,000 more
/// on the Rust reference.
#[allow(clippy::print_stdout)]
fn round_trip_report(k: &Checker) {
    let mut rng = Rng(11);
    let cpu_luv: Vec<(Float3, Float3)> = (0..200_000)
        .map(|_| {
            let c = color(&mut rng);
            (c, nrc_decode_log_luv_hdr(nrc_encode_log_luv_hdr(c)))
        })
        .collect();
    let cpu_oct: Vec<(Float3, Float3)> = (0..200_000)
        .map(|_| {
            let v = direction(&mut rng);
            (v, nrc_decode_normal2x16(nrc_encode_normal2x16(v)))
        })
        .collect();
    println!();
    println!("round trip, encode then decode:");
    report_log_luv("GPU", &k.luv_round_trip);
    report_log_luv("CPU", &cpu_luv);
    report_oct("GPU", &k.oct_round_trip);
    report_oct("CPU", &cpu_oct);
}
