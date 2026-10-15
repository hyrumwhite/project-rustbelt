//! The game's fog, drawn here (ROADMAP: effects in the viewer, step 1). With
//! `game.bevy_bridge_viewer_fog`, the game leaves its environment fog pass
//! out of the overlay and shares the fog it would have drawn with each frame
//! ([`halo_bridge::Fog`]); this fogs the traced picture with it, after
//! tonemapping, in the display's encoding, as the game blends it
//! (`shaders/halo_fog.wesl` has the equations). The fog now covers objects
//! too, which in play the game's own model shaders fog and the overlay
//! never could.

use crate::bridge::BridgeLink;
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    core_pipeline::{
        prepass::ViewPrepassTextures, schedule::Core3d, tonemapping::tonemapping, Core3dSystems, FullscreenShader,
    },
    platform::collections::HashMap,
    prelude::*,
    render::{
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_resource::{
            binding_types::{sampler, texture_2d, texture_depth_2d, uniform_buffer},
            encase::UniformBuffer,
            BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferInitDescriptor, BufferUsages,
            CachedRenderPipelineId, ColorTargetState, ColorWrites, FragmentState, Operations, PipelineCache,
            RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler, SamplerBindingType,
            SamplerDescriptor, ShaderStages, ShaderType, TextureFormat, TextureSampleType,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        view::{ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
        RenderApp, RenderStartup,
    },
};
use halo_map::math::{halo_to_bevy, METERS_PER_WORLD_UNIT};

pub struct FogPlugin;

impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/halo_fog.wesl");
        app.add_plugins(ExtractComponentPlugin::<HaloFog>::default())
            .add_systems(Update, follow_fog);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Core3d, draw_fog.in_set(Core3dSystems::PostProcess).after(tonemapping));
    }
}

/// The fog the camera's picture is drawn with: the game's, in this world's
/// terms (metres, Y up). On the camera while the game shares a fog with any
/// density.
#[derive(Component, Clone, Copy, Default, ShaderType, ExtractComponent)]
#[extract_app(RenderApp)]
pub struct HaloFog {
    /// rgb, maximum density
    atmospheric_color: Vec4,
    /// minimum and maximum distance, the camera's density
    atmospheric: Vec4,
    /// normal (out of the fog) and d
    plane: Vec4,
    /// rgb, maximum density
    planar_color: Vec4,
    /// maximum distance and depth, the camera's density
    planar: Vec4,
}

impl HaloFog {
    /// The game's fog as seen from its camera (in the game's coordinates):
    /// the camera's densities as `_rasterizer_environment_fog_begin` finds
    /// them. None if it has no density.
    fn new(fog: &halo_bridge::Fog, camera: [f32; 3]) -> Option<Self> {
        let atmospheric_density = fog.atmospheric_maximum_density.clamp(0.0, 1.0);
        let planar_density = fog.planar_maximum_density.clamp(0.0, 1.0);
        if atmospheric_density <= 0.0 && planar_density <= 0.0 {
            return None;
        }
        let [i, j, k, d] = fog.plane;
        let normal = Vec3::new(i, j, k);
        let above = normal.dot(Vec3::from(camera)) - d;
        let mut atmospheric_eye = (above / fog.atmospheric_maximum_distance.max(1e-6)).clamp(0.0, 1.0);
        // (atmosphere dominant)
        if fog.flags & 2 != 0 {
            atmospheric_eye = 1.0;
        }
        let planar_eye = (-above / fog.planar_maximum_depth.max(1e-6)).clamp(0.0, 1.0);
        let metres = METERS_PER_WORLD_UNIT;
        Some(Self {
            atmospheric_color: Vec3::from(fog.atmospheric_color).extend(atmospheric_density),
            atmospheric: Vec4::new(
                fog.atmospheric_minimum_distance * metres,
                fog.atmospheric_maximum_distance * metres,
                atmospheric_eye,
                0.0,
            ),
            plane: Vec3::from(halo_to_bevy([i, j, k], 1.0)).extend(d * metres),
            planar_color: Vec3::from(fog.planar_color).extend(planar_density),
            planar: Vec4::new(fog.planar_maximum_distance * metres, fog.planar_maximum_depth * metres, planar_eye, 0.0),
        })
    }
}

/// The latest frame's fog onto the camera.
fn follow_fog(
    mut commands: Commands,
    link: Res<BridgeLink>,
    camera: Single<(Entity, Option<&HaloFog>), With<Camera3d>>,
    mut logged: Local<Option<halo_bridge::Fog>>,
) {
    let (entity, current) = *camera;
    let Some(snapshot) = link.latest.as_ref() else { return };
    // (each fog the game changes to, in the log)
    if let Some(fog) = snapshot.fog.filter(|fog| {
        logged.is_none_or(|old| {
            (old.flags, old.planar_mode, old.atmospheric_color, old.planar_color, old.atmospheric_maximum_distance)
                != (fog.flags, fog.planar_mode, fog.atmospheric_color, fog.planar_color, fog.atmospheric_maximum_distance)
        })
    }) {
        info!("the game's fog: {fog:?}");
        *logged = Some(fog);
    }
    let fog = snapshot.fog.as_ref().zip(snapshot.camera.as_ref()).and_then(|(fog, camera)| HaloFog::new(fog, camera.position));
    match (fog, current) {
        (Some(fog), _) => {
            commands.entity(entity).insert(fog);
        }
        (None, Some(_)) => {
            commands.entity(entity).remove::<HaloFog>();
        }
        (None, None) => {}
    }
}

#[derive(Resource)]
struct FogPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    shader: Handle<Shader>,
}

fn init_pipeline(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>) {
    let layout = BindGroupLayoutDescriptor::new(
        "halo_fog",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_depth_2d(),
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer::<HaloFog>(false),
            ),
        ),
    );
    commands.insert_resource(FogPipeline {
        layout,
        sampler: device.create_sampler(&SamplerDescriptor::default()),
        shader: load_embedded_asset!(assets.as_ref(), "shaders/halo_fog.wesl"),
    });
}

fn draw_fog(
    view: ViewQuery<(&ViewTarget, &ViewPrepassTextures, &ViewUniformOffset, &ExtractedView, &HaloFog)>,
    fog_pipeline: Res<FogPipeline>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    view_uniforms: Res<ViewUniforms>,
    mut pipelines: Local<HashMap<TextureFormat, CachedRenderPipelineId>>,
    mut ctx: RenderContext,
) {
    let (target, prepass, view_offset, extracted, fog) = view.into_inner();
    let (Some(depth), Some(view_binding)) = (prepass.depth_only_view(), view_uniforms.uniforms.binding()) else { return };
    let id = *pipelines.entry(extracted.target_format).or_insert_with(|| {
        cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("halo_fog".into()),
            layout: vec![fog_pipeline.layout.clone()],
            vertex: fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: fog_pipeline.shader.clone(),
                targets: vec![Some(ColorTargetState {
                    format: extracted.target_format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        })
    });
    let Some(pipeline) = cache.get_render_pipeline(id) else { return };

    let mut uniform = UniformBuffer::new(Vec::new());
    if uniform.write(fog).is_err() {
        return;
    }
    let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("halo_fog"),
        contents: uniform.as_ref(),
        usage: BufferUsages::UNIFORM,
    });
    let post_process = target.post_process_write();
    let bind_group = device.create_bind_group(
        "halo_fog",
        &cache.get_bind_group_layout(&fog_pipeline.layout),
        &BindGroupEntries::sequential((
            post_process.source,
            &fog_pipeline.sampler,
            depth,
            view_binding,
            buffer.as_entire_binding(),
        )),
    );
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("halo_fog"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: post_process.destination,
            depth_slice: None,
            resolve_target: None,
            ops: Operations::default(),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[view_offset.offset]);
    pass.draw(0..3, 0..1);
}
