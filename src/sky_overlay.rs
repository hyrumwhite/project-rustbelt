//! The sky past DLSS-RR's denoiser: RR took the whole picture for noisy
//! light, and smoothed a10's opening starfield (the sky model's stars, a
//! pixel each) nearly away. The sky's pixels go to RR's transparency overlay
//! instead (`shaders/sky_overlay.wesl`), which it upscales without denoising
//! (the integration guide, 3.4.10; Bevy's DLSS node is patched for it, in
//! bevy_solarik's vendored bevy_anti_alias). `HALO_RT_NO_SKY_OVERLAY=1`
//! leaves the sky to the denoiser.

use bevy::{
    anti_alias::dlss::{node::dlss_super_resolution, Dlss, DlssRayReconstructionFeature, ViewDlssRayReconstructionOverlay},
    asset::{embedded_asset, load_embedded_asset},
    camera::MainPassResolutionOverride,
    core_pipeline::{prepass::ViewPrepassTextures, schedule::Core3d, Core3dSystems, FullscreenShader},
    prelude::*,
    render::{
        render_resource::{
            binding_types::{texture_2d, texture_depth_2d},
            BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId, ColorTargetState,
            ColorWrites, FragmentState, LoadOp, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
            RenderPipelineDescriptor, ShaderStages, StoreOp, TextureDescriptor, TextureDimension, TextureFormat,
            Extent3d, TextureSampleType, TextureUsages,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        texture::TextureCache,
        view::{ViewDepthStencilTexture, ViewTarget},
        Render, RenderApp, RenderStartup, RenderSystems,
    },
};

pub struct SkyOverlayPlugin;

impl Plugin for SkyOverlayPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("HALO_RT_NO_SKY_OVERLAY").is_ok_and(|v| v == "1") {
            return;
        }
        embedded_asset!(app, "shaders/sky_overlay.wesl");
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Render, prepare_overlay.in_set(RenderSystems::PrepareResources))
            .add_systems(Core3d, draw_overlay.in_set(Core3dSystems::EarlyPostProcess).before(dlss_super_resolution));
    }
}

const OVERLAY_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

#[derive(Resource)]
struct SkyOverlayPipeline {
    layout: BindGroupLayoutDescriptor,
    pipeline: CachedRenderPipelineId,
}

fn init_pipeline(mut commands: Commands, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>, cache: Res<PipelineCache>) {
    let layout = BindGroupLayoutDescriptor::new(
        "sky_overlay",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (texture_2d(TextureSampleType::Float { filterable: false }), texture_depth_2d(), texture_depth_2d()),
        ),
    );
    let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("sky_overlay".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: load_embedded_asset!(assets.as_ref(), "shaders/sky_overlay.wesl"),
            targets: vec![Some(ColorTargetState { format: OVERLAY_FORMAT, blend: None, write_mask: ColorWrites::ALL })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(SkyOverlayPipeline { layout, pipeline });
}

/// Each RR view's overlay, the size of the picture RR is given: its render
/// resolution, the corner of the view's textures the view is drawn in below
/// DLAA. (RR takes the overlay's size from its texture: one the size of the
/// view's textures it composited 1:1 over its output, the render corner as a
/// shrunken copy of the scene, and its edges black outlines.)
fn prepare_overlay(
    mut commands: Commands,
    views: Query<(Entity, &ViewTarget, Option<&MainPassResolutionOverride>), With<Dlss<DlssRayReconstructionFeature>>>,
    mut texture_cache: ResMut<TextureCache>,
    device: Res<RenderDevice>,
) {
    for (entity, target, resolution) in &views {
        let size = match resolution {
            Some(resolution) => Extent3d { width: resolution.0.x, height: resolution.0.y, depth_or_array_layers: 1 },
            None => target.main_texture().size(),
        };
        let texture = texture_cache.get(
            &device,
            TextureDescriptor {
                label: Some("sky_overlay"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: OVERLAY_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        );
        commands.entity(entity).insert(ViewDlssRayReconstructionOverlay(texture));
    }
}

fn draw_overlay(
    view: ViewQuery<(&ViewTarget, &ViewPrepassTextures, &ViewDepthStencilTexture, &ViewDlssRayReconstructionOverlay)>,
    overlay_pipeline: Res<SkyOverlayPipeline>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (target, prepass, main_depth, overlay) = view.into_inner();
    let (Some(depth), Some(main_depth), Some(pipeline)) = (
        prepass.depth_only_view(),
        main_depth.attachment.depth_stencil_views().depth_only_view(),
        cache.get_render_pipeline(overlay_pipeline.pipeline),
    ) else {
        return;
    };
    let bind_group = device.create_bind_group(
        "sky_overlay",
        &cache.get_bind_group_layout(&overlay_pipeline.layout),
        &BindGroupEntries::sequential((target.main_texture_view(), depth, main_depth)),
    );
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("sky_overlay"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: &overlay.0.default_view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    pass.draw(0..3, 0..1);
}
