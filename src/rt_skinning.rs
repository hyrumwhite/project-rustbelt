//! The skinned characters' ray-traced copies skinned on the GPU.
//!
//! Solarik traces no skinned meshes, so each bending part of an animated
//! model has a copy for the rays, a plain mesh in the pose the camera draws.
//! The bridge skinned those copies on the CPU every frame a character moved
//! (`TracedSkin::mesh`) and replaced each mesh asset: the main thread's
//! skinning, the render thread's extraction of a hundred changed meshes a
//! frame and their upload (`mesh/copies`, a millisecond of GPU copies a
//! frame in a fight) before Solarik refit their acceleration structures.
//!
//! Here the copy's mesh asset is made once, and each frame a character moves
//! the bridge queues a [`SkinJob`] with the object's node matrices
//! ([`RtSkinJobs`]). In the render world, between the mesh allocator's
//! uploads and the BLAS builds, a compute pass (`shaders/rt_skin.wgsl`)
//! reads the camera's skinned mesh from its vertex slab (the rest pose with
//! its joints and weights) and writes the copy's positions, normals and
//! tangents into the copy's slab; the copy is then refit as a deformable
//! mesh changed in place (`SolarikSkinnedRefits`). `HALO_RT_CPU_SKINNING=1`
//! keeps the CPU path, for an A/B.

use bevy::{
    asset::embedded_asset,
    prelude::*,
    render::{
        diagnostic::RecordDiagnostics,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        mesh::{allocator::{allocate_and_free_meshes, MeshAllocator}, RenderMesh},
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{storage_buffer_read_only_sized, storage_buffer_sized},
            BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Buffer, BufferBinding, BufferDescriptor,
            BufferUsages, CachedComputePipelineId, ComputePassDescriptor, ComputePipelineDescriptor, PipelineCache,
            ShaderStages, VertexFormat,
        },
        renderer::{RenderDevice, RenderQueue},
        Render, RenderApp, RenderStartup, RenderSystems,
    },
};
use bevy_solarik::scene::{prepare_raytracing_blas, SolarikSkinnedRefits};

/// A part to skin this frame: the camera's skinned mesh read, the rays'
/// copy written, by the node matrices at `first_joint` in [`RtSkinJobs`].
#[derive(Clone, Debug)]
pub struct SkinJob {
    pub source: AssetId<Mesh>,
    pub target: AssetId<Mesh>,
    pub first_joint: u32,
    pub joint_count: u32,
}

/// The frame's skinning jobs and their node matrices (relative to the root
/// node, each node's default inverse included: what `TracedSkin::mesh`
/// took), extracted to the render world and emptied for the next frame.
#[derive(Resource, Default, Clone, ExtractResource)]
#[extract_app(RenderApp)]
pub struct RtSkinJobs {
    pub jobs: Vec<SkinJob>,
    pub joints: Vec<Mat4>,
}

impl RtSkinJobs {
    /// A part skinned to `nodes` this frame.
    pub fn push(&mut self, source: AssetId<Mesh>, target: AssetId<Mesh>, nodes: &[Mat4]) {
        let first_joint = self.joints.len() as u32;
        self.joints.extend_from_slice(nodes);
        self.jobs.push(SkinJob { source, target, first_joint, joint_count: nodes.len() as u32 });
    }
}

/// Whether the copies are skinned on the GPU (`HALO_RT_CPU_SKINNING` unset).
pub fn gpu_skinning() -> bool {
    static GPU: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *GPU.get_or_init(|| std::env::var_os("HALO_RT_CPU_SKINNING").is_none())
}

pub struct RtSkinningPlugin;

impl Plugin for RtSkinningPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/rt_skin.wgsl");
        app.init_resource::<RtSkinJobs>()
            .add_plugins(ExtractResourcePlugin::<RtSkinJobs>::default())
            // (the frame's jobs, extracted after the main schedule, are
            // emptied at the next frame's start)
            .add_systems(First, |mut jobs: ResMut<RtSkinJobs>| {
                jobs.jobs.clear();
                jobs.joints.clear();
            });
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.init_resource::<SkinBuffers>().add_systems(RenderStartup, init_pipeline).add_systems(
            Render,
            skin_raytraced_copies
                .in_set(RenderSystems::PrepareAssets)
                .after(allocate_and_free_meshes)
                .before(prepare_raytracing_blas),
        );
    }
}

#[derive(Resource)]
struct SkinPipeline {
    layout: BindGroupLayoutDescriptor,
    pipeline: CachedComputePipelineId,
}

/// A job (`Job` in the shader): 16 words.
const JOB_WORDS: usize = 16;

fn init_pipeline(mut commands: Commands, assets: Res<AssetServer>, cache: Res<PipelineCache>) {
    let layout = BindGroupLayoutDescriptor::new(
        "rt skinning",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
            ),
        ),
    );
    let pipeline = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("rt skinning".into()),
        layout: vec![layout.clone()],
        shader: bevy::asset::load_embedded_asset!(assets.as_ref(), "shaders/rt_skin.wgsl"),
        entry_point: Some("skin".into()),
        ..Default::default()
    });
    commands.insert_resource(SkinPipeline { layout, pipeline });
}

/// The frame's node matrices and job uniforms, kept and grown.
#[derive(Resource, Default)]
struct SkinBuffers {
    joints: Option<Buffer>,
    jobs: Option<Buffer>,
}

fn grown(buffer: &mut Option<Buffer>, device: &RenderDevice, size: u64, usage: BufferUsages, label: &str) -> Buffer {
    let size = size.max(256).next_power_of_two();
    if buffer.as_ref().is_none_or(|b| b.size() < size) {
        *buffer = Some(device.create_buffer(&BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false }));
    }
    buffer.clone().unwrap()
}

/// A mesh's vertex attributes in its slab, in words: the stride and each
/// attribute's offset, or none where the layout is not what the shader reads.
fn words(mesh: &RenderMesh, wanted: &[(bevy::mesh::MeshVertexAttribute, VertexFormat)]) -> Option<(u32, Vec<u32>)> {
    let layout = mesh.layout.0.layout();
    let stride = layout.array_stride;
    if stride % 4 != 0 {
        return None;
    }
    let mut offsets = Vec::with_capacity(wanted.len());
    for (attribute, format) in wanted {
        let index = mesh.layout.0.attribute_ids().iter().position(|id| *id == attribute.id)?;
        let found = layout.attributes.get(index)?;
        if found.format != *format || found.offset % 4 != 0 {
            return None;
        }
        offsets.push((found.offset / 4) as u32);
    }
    Some(((stride / 4) as u32, offsets))
}

/// The frame's jobs dispatched: the jobs on each pair of slabs together, a
/// bind group and one dispatch a pair (a job a workgroup row), one submit;
/// the copies written noted for Solarik's refits.
#[allow(clippy::too_many_arguments)]
fn skin_raytraced_copies(
    jobs: Res<RtSkinJobs>,
    pipeline: Option<Res<SkinPipeline>>,
    cache: Res<PipelineCache>,
    allocator: Res<MeshAllocator>,
    meshes: Res<RenderAssets<RenderMesh>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut buffers: ResMut<SkinBuffers>,
    mut refits: ResMut<SolarikSkinnedRefits>,
    diagnostics: Option<Res<bevy::render::diagnostic::DiagnosticsRecorder>>,
    mut warned: Local<bool>,
) {
    refits.0.clear();
    if jobs.jobs.is_empty() {
        return;
    }
    let _profile = bevy::render::diagnostic::profile_scope("skin.system");
    let Some(pipeline) = pipeline.as_deref() else { return };
    let Some(compute) = cache.get_compute_pipeline(pipeline.pipeline) else { return };

    // the jobs that can run: both meshes resident, in different slabs, the
    // layouts as the shader reads them, the same vertex count
    struct Ready {
        source: Buffer,
        target: Buffer,
        words: [u32; JOB_WORDS],
        count: u32,
        mesh: AssetId<Mesh>,
    }
    let mut ready: Vec<Ready> = Vec::with_capacity(jobs.jobs.len());
    for job in &jobs.jobs {
        let (Some(source), Some(target)) = (meshes.get(job.source), meshes.get(job.target)) else { continue };
        let (Some(source_slice), Some(target_slice)) =
            (allocator.mesh_vertex_slice(&job.source), allocator.mesh_vertex_slice(&job.target))
        else {
            continue;
        };
        if source_slice.buffer.id() == target_slice.buffer.id() || source_slice.range.len() != target_slice.range.len() {
            continue;
        }
        let Some((src_stride, src)) = words(
            source,
            &[
                (Mesh::ATTRIBUTE_POSITION, VertexFormat::Float32x3),
                (Mesh::ATTRIBUTE_NORMAL, VertexFormat::Float32x3),
                (Mesh::ATTRIBUTE_TANGENT, VertexFormat::Float32x4),
                (Mesh::ATTRIBUTE_JOINT_INDEX, VertexFormat::Uint16x4),
                (Mesh::ATTRIBUTE_JOINT_WEIGHT, VertexFormat::Float32x4),
            ],
        ) else {
            if !*warned {
                warn!("rt skinning: a skinned mesh's layout isn't what the shader reads; its copies stay");
                *warned = true;
            }
            continue;
        };
        let Some((dst_stride, dst)) = words(
            target,
            &[
                (Mesh::ATTRIBUTE_POSITION, VertexFormat::Float32x3),
                (Mesh::ATTRIBUTE_NORMAL, VertexFormat::Float32x3),
                (Mesh::ATTRIBUTE_TANGENT, VertexFormat::Float32x4),
            ],
        ) else {
            continue;
        };
        let count = source_slice.range.len() as u32;
        ready.push(Ready {
            source: source_slice.buffer.clone(),
            target: target_slice.buffer.clone(),
            words: [
                source_slice.range.start * src_stride,
                target_slice.range.start * dst_stride,
                count,
                job.first_joint,
                job.joint_count,
                src_stride,
                src[0],
                src[1],
                src[2],
                src[3],
                src[4],
                dst_stride,
                dst[0],
                dst[1],
                dst[2],
                0,
            ],
            count,
            mesh: job.target,
        });
    }
    bevy::render::diagnostic::profile_value("skin.jobs", ready.len() as f64, "count");
    if ready.is_empty() {
        return;
    }

    // the jobs by their pair of slabs, each pair's together in the table
    ready.sort_by_key(|job| (job.source.id(), job.target.id()));
    let mut pairs: Vec<(Buffer, Buffer, std::ops::Range<usize>, u32)> = Vec::new();
    for (index, job) in ready.iter().enumerate() {
        match pairs.last_mut() {
            Some((source, target, range, widest)) if source.id() == job.source.id() && target.id() == job.target.id() => {
                range.end = index + 1;
                *widest = (*widest).max(job.count);
            }
            _ => pairs.push((job.source.clone(), job.target.clone(), index..index + 1, job.count)),
        }
        refits.0.insert(job.mesh);
    }

    // the matrices and the job table
    let columns: Vec<[f32; 16]> = jobs.joints.iter().map(Mat4::to_cols_array).collect();
    let joints_bytes: &[u8] = bytemuck::cast_slice(&columns);
    let joints = grown(&mut buffers.joints, &device, joints_bytes.len() as u64, BufferUsages::STORAGE | BufferUsages::COPY_DST, "rt skinning joints");
    queue.write_buffer(&joints, 0, joints_bytes);
    let table: Vec<[u32; JOB_WORDS]> = ready.iter().map(|job| job.words).collect();
    let table_bytes: &[u8] = bytemuck::cast_slice(&table);
    let job_buffer = grown(&mut buffers.jobs, &device, table_bytes.len() as u64, BufferUsages::STORAGE | BufferUsages::COPY_DST, "rt skinning jobs");
    queue.write_buffer(&job_buffer, 0, table_bytes);

    // a bind group and a dispatch a pair: x the vertices of the widest job,
    // y the pair's jobs
    let layout = cache.get_bind_group_layout(&pipeline.layout);
    let mut encoder = device.create_command_encoder(&Default::default());
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(&mut encoder, "scene/skin");
    {
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor { label: Some("rt skinning"), timestamp_writes: None });
        pass.set_pipeline(compute);
        let groups: Vec<_> = pairs
            .iter()
            .map(|(source, target, range, _)| {
                device.create_bind_group(
                    "rt skinning",
                    &layout,
                    &BindGroupEntries::sequential((
                        source.as_entire_binding(),
                        target.as_entire_binding(),
                        joints.as_entire_binding(),
                        bevy::render::render_resource::BindingResource::Buffer(BufferBinding {
                            buffer: &job_buffer,
                            offset: (range.start * JOB_WORDS * 4) as u64,
                            size: std::num::NonZero::new((range.len() * JOB_WORDS * 4) as u64),
                        }),
                    )),
                )
            })
            .collect();
        for ((_, _, range, widest), group) in pairs.iter().zip(&groups) {
            pass.set_bind_group(0, group, &[]);
            pass.dispatch_workgroups(widest.div_ceil(64), range.len() as u32, 1);
        }
    }
    span.end(&mut encoder);
    queue.submit([encoder.finish()]);
}
