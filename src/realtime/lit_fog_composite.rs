//! Lit fog's composite (`lit_fog.wesl`): the scattered light, accumulated at
//! the render resolution, added to the picture after the denoiser (DLSS-RR
//! would take it as the surfaces' light and texture it with their albedo)
//! and before tonemapping; with `SolarikRayFog::lit`'s z, in place of the
//! picture, to see it alone.

use super::prepare::SolarikLightingResources;
use crate::scene::SolarikRayFog;
use bevy_asset::{AssetServer, Handle, load_embedded_asset};
use bevy_core_pipeline::FullscreenShader;
use bevy_diagnostic::FrameCount;
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashMap;
use bevy_render::{
    render_resource::{
        binding_types::{sampler, texture_2d, uniform_buffer},
        *,
    },
    renderer::{RenderContext, RenderDevice, ViewQuery},
    view::{ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
};
use bevy_shader::Shader;

#[derive(Resource)]
pub(super) struct LitFogComposite {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    shader: Handle<Shader>,
}

pub(super) fn init_lit_fog_composite(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>) {
    let layout = BindGroupLayoutDescriptor::new(
        "solarik_lit_fog_composite",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<ViewUniform>(true),
            ),
        ),
    );
    commands.insert_resource(LitFogComposite {
        layout,
        sampler: device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        }),
        shader: load_embedded_asset!(assets.as_ref(), "lit_fog_composite.wesl"),
    });
}

pub(super) fn lit_fog_composite(
    view: ViewQuery<(&ViewTarget, &ViewUniformOffset, &SolarikLightingResources)>,
    ray_fog: Res<SolarikRayFog>,
    composite: Option<Res<LitFogComposite>>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    view_uniforms: Res<ViewUniforms>,
    frame_count: Res<FrameCount>,
    mut pipelines: Local<HashMap<(TextureFormat, bool), CachedRenderPipelineId>>,
    mut ctx: RenderContext,
) {
    if ray_fog.lit.x <= 0.0 {
        return;
    }
    let Some(composite) = composite else { return };
    let (target, view_offset, resources) = view.into_inner();
    let Some(view_binding) = view_uniforms.uniforms.binding() else { return };
    let format = target.main_texture_format();
    let only = ray_fog.lit.z > 0.5;
    let id = *pipelines.entry((format, only)).or_insert_with(|| {
        cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("solarik_lit_fog_composite".into()),
            layout: vec![composite.layout.clone()],
            vertex: fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: composite.shader.clone(),
                shader_defs: if only { vec!["LIT_FOG_ONLY".into()] } else { vec![] },
                targets: vec![Some(ColorTargetState {
                    format,
                    // added, or in place of the picture where alpha is 1
                    blend: Some(BlendState {
                        color: BlendComponent {
                            src_factor: BlendFactor::One,
                            dst_factor: BlendFactor::OneMinusSrcAlpha,
                            operation: BlendOperation::Add,
                        },
                        alpha: BlendComponent::OVER,
                    }),
                    write_mask: ColorWrites::ALL,
                })],
                ..Default::default()
            }),
            ..Default::default()
        })
    });
    let Some(pipeline) = cache.get_render_pipeline(id) else { return };
    // (the history the frame's accumulation wrote: its parity's)
    let history = &resources.lit_fog_history[(frame_count.0 & 1) as usize];
    let bind_group = device.create_bind_group(
        "solarik_lit_fog_composite",
        &cache.get_bind_group_layout(&composite.layout),
        &BindGroupEntries::sequential((history, &composite.sampler, view_binding)),
    );
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("solarik_lit_fog_composite"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target.main_texture_view(),
            depth_slice: None,
            resolve_target: None,
            ops: Operations { load: LoadOp::Load, store: StoreOp::Store },
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
