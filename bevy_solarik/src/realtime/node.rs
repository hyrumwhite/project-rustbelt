use super::{
    SolarikLighting,
    prepare::{LIGHT_TILE_BLOCKS, SolarikLightingResources, WORLD_CACHE_SIZE},
};
use crate::scene::RaytracingSceneBindings;
#[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
use bevy_anti_alias::dlss::ViewDlssRayReconstructionTextures;
use bevy_asset::{AssetServer, Handle, load_embedded_asset};
use bevy_core_pipeline::core_3d::Transparent3d;
use bevy_core_pipeline::prepass::{
    PreviousViewData, PreviousViewUniformOffset, PreviousViewUniforms, ViewPrepassTextures,
};
use bevy_diagnostic::FrameCount;
use bevy_ecs::{prelude::*, resource::Resource, system::Commands};
use bevy_platform::collections::HashSet;
use bevy_render::{
    diagnostic::RecordDiagnostics as _,
    render_resource::{
        BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
        CachedComputePipelineId, ComputePassDescriptor, ComputePipelineDescriptor, LoadOp,
        PipelineCache, RenderPassDescriptor, ShaderStages, StorageTextureAccess, TextureFormat,
        TextureSampleType,
        binding_types::{
            storage_buffer_sized, texture_2d, texture_depth_2d, texture_storage_2d, uniform_buffer,
        },
    },
    renderer::{RenderAdapter, RenderContext, RenderDevice, RenderQueue, ViewQuery},
    view::{ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
};
use bevy_render::{
    render_phase::ViewSortedRenderPhases,
    view::{ExtractedView, RetainedViewEntity},
};
use bevy_shader::{Shader, ShaderDefVal};
use bevy_utils::default;

/// Resource holding the Solarik lighting pipeline configuration.
#[derive(Resource)]
pub struct SolarikLightingPipelines {
    bind_group_layout: BindGroupLayoutDescriptor,
    bind_group_layout_world_cache_active_cells_dispatch: BindGroupLayoutDescriptor,
    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    bind_group_layout_resolve_dlss_rr_textures: BindGroupLayoutDescriptor,
    build_sky_rows_pipeline: CachedComputePipelineId,
    build_sky_marginal_pipeline: CachedComputePipelineId,
    decay_world_cache_pipeline: CachedComputePipelineId,
    compact_world_cache_single_block_pipeline: CachedComputePipelineId,
    compact_world_cache_blocks_pipeline: CachedComputePipelineId,
    compact_world_cache_write_active_cells_pipeline: CachedComputePipelineId,
    clear_world_cache_priority_pipeline: CachedComputePipelineId,
    histogram_world_cache_priority_pipeline: CachedComputePipelineId,
    budget_world_cache_priority_pipeline: CachedComputePipelineId,
    select_world_cache_priority_pipeline: CachedComputePipelineId,
    sample_di_for_world_cache_pipeline: CachedComputePipelineId,
    sample_gi_for_world_cache_pipeline: CachedComputePipelineId,
    blend_new_world_cache_samples_pipeline: CachedComputePipelineId,
    presample_light_tiles_pipeline: CachedComputePipelineId,
    build_light_triangles_pipeline: CachedComputePipelineId,
    di_initial_and_temporal_pipeline: CachedComputePipelineId,
    di_spatial_and_shade_pipeline: CachedComputePipelineId,
    gi_initial_and_temporal_pipeline: CachedComputePipelineId,
    gi_spatial_and_shade_pipeline: CachedComputePipelineId,
    specular_gi_pipeline: CachedComputePipelineId,
    lit_fog_pipeline: CachedComputePipelineId,
    /// One level of lit fog's a-trous prefilter (`lit_fog_filter`),
    /// dispatched `lit_fog_levels` times between the two others.
    lit_fog_filter_pipeline: CachedComputePipelineId,
    lit_fog_accumulate_pipeline: CachedComputePipelineId,
    /// The prefilter's level count, as its shader definition gave it
    /// (`lit_fog_levels`).
    lit_fog_levels: u32,
    primary_glass_pipeline: CachedComputePipelineId,
    primary_foliage_pipeline: CachedComputePipelineId,
    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    primary_foliage_rr_pipeline: CachedComputePipelineId,
    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    primary_glass_rr_pipeline: CachedComputePipelineId,
    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    specular_gi_with_psr_pipeline: CachedComputePipelineId,
    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    resolve_dlss_rr_textures_pipeline: CachedComputePipelineId,
    /// The NRC frame's pipelines, when NRC initialized.
    #[cfg(feature = "nrc")]
    nrc: Option<super::nrc::NrcPipelineIds>,
}

/// Without the `nrc` feature there is never an NRC frame; this stands in for
/// `nrc::NrcFrame` so the node reads the same either way.
#[cfg(not(feature = "nrc"))]
mod no_nrc {
    use bevy_render::{
        render_resource::{BindGroup, ComputePipeline},
        renderer::{RenderContext, RenderDevice, RenderQueue},
    };

    pub(super) enum NrcFrame {}

    impl NrcFrame {
        pub(super) fn begin_frame(
            &mut self,
            _: &mut RenderContext,
            _: &RenderDevice,
            _: &RenderQueue,
            _: u32,
        ) -> bool {
            match *self {}
        }
        pub(super) fn bind_group(&self) -> &BindGroup {
            match *self {}
        }
        pub(super) fn gi_initial_pipeline(&self) -> &ComputePipeline {
            match *self {}
        }
        pub(super) fn training_pipeline(&self) -> &ComputePipeline {
            match *self {}
        }
        pub(super) fn resolve_pipeline(&self) -> &ComputePipeline {
            match *self {}
        }
        pub(super) fn specular_pipeline(&self, _: bool) -> Option<&'static ComputePipeline> {
            match *self {}
        }
        pub(super) fn specular_reuse_pipeline(&self, _: bool) -> Option<&'static ComputePipeline> {
            match *self {}
        }
        pub(super) fn drops_cache_gi(&self, _: u32) -> bool {
            match *self {}
        }
        pub(super) fn training_workgroups(&self) -> (u32, u32) {
            match *self {}
        }
        pub(super) fn query_and_train(
            &mut self,
            _: &mut RenderContext,
            _: &RenderDevice,
            _: &impl bevy_render::diagnostic::RecordDiagnostics,
        ) -> bool {
            match *self {}
        }
    }
}

#[cfg(any(not(feature = "dlss"), feature = "force_disable_dlss"))]
type SolarikLightingViewQuery = (
    &'static SolarikLighting,
    &'static ExtractedView,
    &'static SolarikLightingResources,
    &'static ViewTarget,
    &'static ViewPrepassTextures,
    &'static ViewUniformOffset,
    &'static PreviousViewUniformOffset,
);

#[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
type SolarikLightingViewQuery = (
    &'static SolarikLighting,
    &'static ExtractedView,
    &'static SolarikLightingResources,
    &'static ViewTarget,
    &'static ViewPrepassTextures,
    &'static ViewUniformOffset,
    &'static PreviousViewUniformOffset,
    Option<&'static ViewDlssRayReconstructionTextures>,
);

/// Views whose raster panes are owned by the primary compositor this frame.
/// (A pane is queued in the transmissive phase instead of the transparent
/// one when its view has `ScreenSpaceTransmission`: an application that
/// draws in that phase, before this compositor, what lies behind the panes.)
#[derive(Resource, Default)]
pub struct PrimaryGlassViews(
    HashSet<RetainedViewEntity>,
    Vec<(RetainedViewEntity, Transparent3d)>,
    Vec<(RetainedViewEntity, bevy_pbr::Transmissive3d)>,
);

/// Restore retained entries before Bevy updates visibility and materials.
/// Pipeline reloads and disabled cameras must regain their raster fallback.
pub fn restore_primary_glass(
    mut state: ResMut<PrimaryGlassViews>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut transmissive_phases: ResMut<ViewSortedRenderPhases<bevy_pbr::Transmissive3d>>,
) {
    for (view, item) in state.1.drain(..) {
        if let Some(phase) = phases.get_mut(&view) {
            phase.add_retained(item);
        }
    }
    for (view, item) in state.2.drain(..) {
        if let Some(phase) = transmissive_phases.get_mut(&view) {
            phase.add_retained(item);
        }
    }
}

/// Runs after resource preparation and before phase batching, so a glass item
/// cannot share a raster batch with an instance that remains raster-owned.
pub fn prepare_primary_glass(
    views: Query<SolarikLightingViewQuery>,
    main_entities: Query<&bevy_render::sync_world::MainEntity>,
    pipelines: Option<Res<SolarikLightingPipelines>>,
    cache: Res<PipelineCache>,
    scene: Res<RaytracingSceneBindings>,
    view_uniforms: Res<ViewUniforms>,
    previous_uniforms: Res<PreviousViewUniforms>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut transmissive_phases: ResMut<ViewSortedRenderPhases<bevy_pbr::Transmissive3d>>,
    mut ready_views: ResMut<PrimaryGlassViews>,
    #[cfg(feature = "nrc")] nrc_state: Option<Res<super::nrc::NrcState>>,
) {
    ready_views.0.clear();
    let Some(p) = pipelines else {
        return;
    };
    if scene.bind_group.is_none()
        || scene.glass_entities.is_empty()
        || view_uniforms.uniforms.binding().is_none()
        || previous_uniforms.uniforms.binding().is_none()
    {
        return;
    }
    let ids = [
        p.build_sky_rows_pipeline,
        p.build_sky_marginal_pipeline,
        p.decay_world_cache_pipeline,
        p.compact_world_cache_single_block_pipeline,
        p.compact_world_cache_blocks_pipeline,
        p.compact_world_cache_write_active_cells_pipeline,
        p.clear_world_cache_priority_pipeline,
        p.histogram_world_cache_priority_pipeline,
        p.budget_world_cache_priority_pipeline,
        p.select_world_cache_priority_pipeline,
        p.sample_di_for_world_cache_pipeline,
        p.sample_gi_for_world_cache_pipeline,
        p.blend_new_world_cache_samples_pipeline,
        p.presample_light_tiles_pipeline,
        p.build_light_triangles_pipeline,
        p.di_initial_and_temporal_pipeline,
        p.di_spatial_and_shade_pipeline,
        p.gi_initial_and_temporal_pipeline,
        p.gi_spatial_and_shade_pipeline,
        p.specular_gi_pipeline,
        p.primary_glass_pipeline,
    ];
    if ids
        .into_iter()
        .any(|id| cache.get_compute_pipeline(id).is_none())
    {
        return;
    }
    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    if [
        p.primary_glass_rr_pipeline,
        p.specular_gi_with_psr_pipeline,
        p.resolve_dlss_rr_textures_pipeline,
    ]
    .into_iter()
    .any(|id| cache.get_compute_pipeline(id).is_none())
    {
        return;
    }
    // (the lighting node waits for NRC's pipelines too)
    #[cfg(feature = "nrc")]
    if super::nrc::pipelines_pending(p.nrc.as_ref(), nrc_state.as_deref(), &cache) {
        return;
    }
    let glass_main_entities: HashSet<_> = scene
        .glass_entities
        .iter()
        .filter_map(|entity| main_entities.get(*entity).ok().copied())
        .collect();
    for view in &views {
        #[cfg(any(not(feature = "dlss"), feature = "force_disable_dlss"))]
        let (_, extracted, _, _, textures, _, _) = view;
        #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
        let (_, extracted, _, _, textures, _, _, _) = view;
        if textures.deferred_view().is_none()
            || textures.depth_only_view().is_none()
            || textures.motion_vectors_view().is_none()
            || textures.previous_deferred_view().is_none()
            || textures.previous_depth_only_view().is_none()
        {
            continue;
        }
        let Some(phase) = phases.get_mut(&extracted.retained_view_entity) else {
            continue;
        };
        let keys: Vec<_> = phase
            .items
            .keys()
            .filter(|(_, main_entity)| glass_main_entities.contains(main_entity))
            .copied()
            .collect();
        for key in keys {
            if let Some(item) = phase.items.shift_remove(&key) {
                ready_views.1.push((extracted.retained_view_entity, item));
            }
        }
        if let Some(phase) = transmissive_phases.get_mut(&extracted.retained_view_entity) {
            let keys: Vec<_> = phase
                .items
                .keys()
                .filter(|(_, main_entity)| glass_main_entities.contains(main_entity))
                .copied()
                .collect();
            for key in keys {
                if let Some(item) = phase.items.shift_remove(&key) {
                    ready_views.2.push((extracted.retained_view_entity, item));
                }
            }
        }
        ready_views.0.insert(extracted.retained_view_entity);
    }
}
pub fn solarik_lighting<const PRIMARY: bool>(
    view: ViewQuery<SolarikLightingViewQuery>,
    solarik_pipelines: Option<Res<SolarikLightingPipelines>>,
    primary_views: Res<PrimaryGlassViews>,
    pipeline_cache: Res<PipelineCache>,
    scene_bindings: Res<RaytracingSceneBindings>,
    ray_fog: Res<crate::scene::SolarikRayFog>,
    view_uniforms: Res<ViewUniforms>,
    previous_view_uniforms: Res<PreviousViewUniforms>,
    frame_count: Res<FrameCount>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    #[cfg(feature = "nrc")] nrc_state: Option<ResMut<super::nrc::NrcState>>,
    #[cfg(feature = "graphics_debug")] query_capture: Option<
        Res<super::query_capture::PrimaryQueryCapture>,
    >,
    mut ctx: RenderContext,
) {
    let view_entity = view.entity();
    #[cfg(any(not(feature = "dlss"), feature = "force_disable_dlss"))]
    let (
        solarik_lighting,
        extracted_view,
        solarik_lighting_resources,
        view_target,
        view_prepass_textures,
        view_uniform_offset,
        previous_view_uniform_offset,
    ) = view.into_inner();

    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    let (
        solarik_lighting,
        extracted_view,
        solarik_lighting_resources,
        view_target,
        view_prepass_textures,
        view_uniform_offset,
        previous_view_uniform_offset,
        view_dlss_rr_textures,
    ) = view.into_inner();

    let owns_glass = primary_views
        .0
        .contains(&extracted_view.retained_view_entity);
    if PRIMARY
        && !scene_bindings.has_foliage
        && !scene_bindings.has_canopy
        && !primary_views
            .0
            .contains(&extracted_view.retained_view_entity)
    {
        return;
    }

    let Some(pipelines) = solarik_pipelines else {
        return;
    };

    #[cfg(not(all(feature = "dlss", not(feature = "force_disable_dlss"))))]
    let specular_gi_pipeline = pipelines.specular_gi_pipeline;
    // (whether the specular pass is the ray reconstruction variant)
    #[cfg(not(all(feature = "dlss", not(feature = "force_disable_dlss"))))]
    let specular_psr = false;
    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    let specular_psr = view_dlss_rr_textures.is_some();
    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    let specular_gi_pipeline = if view_dlss_rr_textures.is_some() {
        pipelines.specular_gi_with_psr_pipeline
    } else {
        pipelines.specular_gi_pipeline
    };

    let (
        Some(build_sky_rows_pipeline),
        Some(build_sky_marginal_pipeline),
        Some(decay_world_cache_pipeline),
        Some(compact_world_cache_single_block_pipeline),
        Some(compact_world_cache_blocks_pipeline),
        Some(compact_world_cache_write_active_cells_pipeline),
        Some(clear_world_cache_priority_pipeline),
        Some(histogram_world_cache_priority_pipeline),
        Some(budget_world_cache_priority_pipeline),
        Some(select_world_cache_priority_pipeline),
        Some(sample_di_for_world_cache_pipeline),
        Some(sample_gi_for_world_cache_pipeline),
        Some(blend_new_world_cache_samples_pipeline),
        Some(presample_light_tiles_pipeline),
        Some(build_light_triangles_pipeline),
        Some(di_initial_and_temporal_pipeline),
        Some(di_spatial_and_shade_pipeline),
        Some(gi_initial_and_temporal_pipeline),
        Some(gi_spatial_and_shade_pipeline),
        Some(specular_gi_pipeline),
        Some(scene_bind_group),
        Some(gbuffer),
        Some(depth_buffer),
        Some(motion_vectors),
        Some(previous_gbuffer),
        Some(previous_depth_buffer),
        Some(view_uniforms_binding),
        Some(previous_view_uniforms_binding),
    ) = (
        pipeline_cache.get_compute_pipeline(pipelines.build_sky_rows_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.build_sky_marginal_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.decay_world_cache_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.compact_world_cache_single_block_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.compact_world_cache_blocks_pipeline),
        pipeline_cache
            .get_compute_pipeline(pipelines.compact_world_cache_write_active_cells_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.clear_world_cache_priority_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.histogram_world_cache_priority_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.budget_world_cache_priority_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.select_world_cache_priority_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.sample_di_for_world_cache_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.sample_gi_for_world_cache_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.blend_new_world_cache_samples_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.presample_light_tiles_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.build_light_triangles_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.di_initial_and_temporal_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.di_spatial_and_shade_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.gi_initial_and_temporal_pipeline),
        pipeline_cache.get_compute_pipeline(pipelines.gi_spatial_and_shade_pipeline),
        pipeline_cache.get_compute_pipeline(specular_gi_pipeline),
        &scene_bindings.bind_group,
        view_prepass_textures.deferred_view(),
        view_prepass_textures.depth_only_view(),
        view_prepass_textures.motion_vectors_view(),
        view_prepass_textures.previous_deferred_view(),
        view_prepass_textures.previous_depth_only_view(),
        view_uniforms.uniforms.binding(),
        previous_view_uniforms.uniforms.binding(),
    )
    else {
        return;
    };

    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    let Some(resolve_dlss_rr_textures_pipeline) =
        pipeline_cache.get_compute_pipeline(pipelines.resolve_dlss_rr_textures_pipeline)
    else {
        return;
    };

    let primary_pipeline = pipelines.primary_glass_pipeline;
    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    let primary_pipeline = if view_dlss_rr_textures.is_some() {
        pipelines.primary_glass_rr_pipeline
    } else {
        primary_pipeline
    };
    let primary_pipeline = pipeline_cache.get_compute_pipeline(primary_pipeline);
    if PRIMARY && owns_glass && primary_pipeline.is_none() {
        return;
    }

    let foliage_pipeline = pipelines.primary_foliage_pipeline;
    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    let foliage_pipeline = if view_dlss_rr_textures.is_some() {
        pipelines.primary_foliage_rr_pipeline
    } else {
        foliage_pipeline
    };
    let foliage_pipeline = pipeline_cache.get_compute_pipeline(foliage_pipeline);

    // NRC answers the diffuse GI's query for the view it is configured for
    // (never in the primary compositor); otherwise the world cache does.
    #[cfg(feature = "nrc")]
    let nrc_frame = match super::nrc::NrcFrame::prepare(
        nrc_state.filter(|_| !PRIMARY),
        pipelines.nrc.as_ref(),
        &pipeline_cache,
        view_entity,
        solarik_lighting_resources.view_size,
    ) {
        super::nrc::NrcReadiness::Ready(frame) => Some(frame),
        super::nrc::NrcReadiness::Pending => return,
        super::nrc::NrcReadiness::Off => None,
    };
    #[cfg(not(feature = "nrc"))]
    let nrc_frame: Option<no_nrc::NrcFrame> = {
        let _ = view_entity;
        None
    };
    let mut nrc_frame = nrc_frame;

    let view_target_attachment = view_target.get_unsampled_color_attachment();

    let s = solarik_lighting_resources;
    let bind_group = render_device.create_bind_group(
        "solarik_lighting_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipelines.bind_group_layout),
        &BindGroupEntries::sequential((
            view_target_attachment.view,
            s.light_tile_samples.as_entire_binding(),
            s.light_tile_resolved_samples.as_entire_binding(),
            &s.di_reservoirs_a,
            &s.di_reservoirs_b,
            s.gi_reservoirs_a.as_entire_binding(),
            s.gi_reservoirs_b.as_entire_binding(),
            gbuffer,
            depth_buffer,
            motion_vectors,
            previous_gbuffer,
            previous_depth_buffer,
            view_uniforms_binding,
            previous_view_uniforms_binding,
            s.world_cache_checksums.as_entire_binding(),
            s.world_cache_life.as_entire_binding(),
            s.world_cache_radiance.as_entire_binding(),
            s.world_cache_geometry_data.as_entire_binding(),
            s.world_cache_luminance_deltas.as_entire_binding(),
            s.world_cache_active_cells_new_radiance.as_entire_binding(),
            s.world_cache_a.as_entire_binding(),
            s.world_cache_b.as_entire_binding(),
            s.world_cache_active_cell_indices.as_entire_binding(),
            s.world_cache_active_cells_count.as_entire_binding(),
            s.sky_distribution.as_entire_binding(),
            &s.primary_background_view,
            s.world_cache_light_slots.as_entire_binding(),
            &s.lit_fog_raw,
            &s.lit_fog_history[0],
            &s.lit_fog_history[1],
            &s.lit_fog_scratch,
            s.light_triangles.as_entire_binding(),
        )),
    );
    let bind_group_world_cache_active_cells_dispatch = render_device.create_bind_group(
        "solarik_lighting_bind_group_world_cache_active_cells_dispatch",
        &pipeline_cache
            .get_bind_group_layout(&pipelines.bind_group_layout_world_cache_active_cells_dispatch),
        &BindGroupEntries::single(s.world_cache_active_cells_dispatch.as_entire_binding()),
    );

    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    let bind_group_resolve_dlss_rr_textures = view_dlss_rr_textures.map(|d| {
        render_device.create_bind_group(
            "solarik_lighting_bind_group_resolve_dlss_rr_textures",
            &pipeline_cache
                .get_bind_group_layout(&pipelines.bind_group_layout_resolve_dlss_rr_textures),
            &BindGroupEntries::sequential((
                &d.diffuse_albedo.default_view,
                &d.specular_albedo.default_view,
                &d.normal_roughness.default_view,
                &d.depth.default_view,
                &d.specular_motion_vectors.default_view,
            )),
        )
    });

    // Preserve the stochastic seed; cache scheduling needs a separate clock.
    let frame_index = frame_count.0.wrapping_mul(5782582);
    let render_frame = frame_count.0;
    let stable_radius = s.history.stable_radius(
        scene_bindings.history_generation,
        scene_bindings.history_stable_radius,
    );
    let regional = std::env::var("SOLARIK_REGIONAL_HISTORY").is_ok_and(|v| v == "1")
        && scene_bindings.history_regional
        && s.history
            .has_one_pending_event(scene_bindings.history_generation);
    let full_reset = solarik_lighting.reset || s.history.full_reset_pending();
    let reset_flags = if full_reset {
        1u32
    } else if solarik_lighting.dynamic_scene {
        // (scene changes keep the history: SolarikLighting::dynamic_scene)
        0u32
    } else if stable_radius == 0.0 && !regional {
        1u32
    } else if regional {
        6u32
    } else if stable_radius.is_finite() {
        2u32
    } else {
        0u32
    };

    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();

    let command_encoder = ctx.command_encoder();

    // Clear the view target if we're the first node to write to it
    if !PRIMARY && matches!(view_target_attachment.ops.load, LoadOp::Clear(_)) {
        command_encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("solarik_lighting_clear"),
            color_attachments: &[Some(view_target_attachment)],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }

    let mut pass = command_encoder.begin_compute_pass(&ComputePassDescriptor {
        label: Some("solarik_lighting"),
        timestamp_writes: None,
    });

    let dx = solarik_lighting_resources.view_size.x.div_ceil(8);
    let dy = solarik_lighting_resources.view_size.y.div_ceil(8);

    pass.set_bind_group(0, scene_bind_group, &[]);
    pass.set_bind_group(
        1,
        &bind_group,
        &[
            view_uniform_offset.offset,
            previous_view_uniform_offset.offset,
        ],
    );

    if PRIMARY {
        #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
        if let Some(group) = &bind_group_resolve_dlss_rr_textures {
            pass.set_bind_group(2, group, &[]);
        }
        if let Some(pipeline) =
            foliage_pipeline.filter(|_| scene_bindings.has_foliage || scene_bindings.has_canopy)
        {
            let span = diagnostics.time_span(&mut pass, "solarik_lighting/primary_foliage");
            pass.set_pipeline(pipeline);
            let capture_frame = 0u32;
            #[cfg(feature = "graphics_debug")]
            let capture_frame = if let Some(capture) = &query_capture {
                #[cfg(any(not(feature = "dlss"), feature = "force_disable_dlss"))]
                let rr = false;
                #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
                let rr = view_dlss_rr_textures.is_some();
                if !rr {
                    pass.set_bind_group(2, &capture.empty_group, &[]);
                }
                pass.set_bind_group(3, &capture.group, &[]);
                u32::from(capture.frame == Some(render_frame))
            } else {
                capture_frame
            };
            pass.set_immediates(
                0,
                bytemuck::cast_slice(&[
                    frame_index,
                    capture_frame,
                    render_frame,
                    stable_radius.to_bits(),
                ]),
            );
            pass.dispatch_workgroups(dx, dy, 1);
            span.end(&mut pass);
        }
        if let Some(pipeline) = primary_pipeline.filter(|_| owns_glass) {
            // (refracting panes read the picture behind them from a copy:
            // their neighbours write theirs over it meanwhile)
            if scene_bindings.has_refraction {
                drop(pass);
                command_encoder.copy_texture_to_texture(
                    view_target.main_texture().as_image_copy(),
                    s.primary_background.as_image_copy(),
                    bevy_image::ToExtents::to_extents(s.view_size),
                );
                pass = command_encoder.begin_compute_pass(&ComputePassDescriptor {
                    label: Some("solarik_lighting_primary_glass"),
                    timestamp_writes: None,
                });
                pass.set_bind_group(0, scene_bind_group, &[]);
                pass.set_bind_group(
                    1,
                    &bind_group,
                    &[
                        view_uniform_offset.offset,
                        previous_view_uniform_offset.offset,
                    ],
                );
                #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
                if let Some(group) = &bind_group_resolve_dlss_rr_textures {
                    pass.set_bind_group(2, group, &[]);
                }
            }
            let span = diagnostics.time_span(&mut pass, "solarik_lighting/primary_glass");
            pass.set_pipeline(pipeline);
            pass.set_immediates(
                0,
                bytemuck::cast_slice(&[frame_index, 0u32, render_frame, stable_radius.to_bits()]),
            );
            pass.dispatch_workgroups(dx, dy, 1);
            span.end(&mut pass);
        }
        return;
    }

    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    if let Some(bind_group_resolve_dlss_rr_textures) = &bind_group_resolve_dlss_rr_textures {
        pass.set_bind_group(2, bind_group_resolve_dlss_rr_textures, &[]);
        pass.set_pipeline(resolve_dlss_rr_textures_pipeline);
        pass.dispatch_workgroups(dx, dy, 1);
    }

    // (only here, never in the primary compositor: a static sky's
    // distribution is kept until its inputs change, SolarikStaticSky)
    let d = diagnostics.time_span(&mut pass, "solarik_lighting/sky_distribution");
    if crate::scene::sky_distribution_stale(
        &mut s
            .sky_distribution_built
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        scene_bindings.sky_distribution_key(),
        full_reset,
    ) {
        pass.set_pipeline(build_sky_rows_pipeline);
        pass.dispatch_workgroups(6 * 128, 1, 1);
        pass.set_pipeline(build_sky_marginal_pipeline);
        pass.dispatch_workgroups(1, 1, 1);
    }
    d.end(&mut pass);

    let d = diagnostics.time_span(&mut pass, "solarik_lighting/presample_light_tiles");
    // (the emissive triangles in world space first, a workgroup a light: the
    // tiles and every light sample after them resolve from the table)
    pass.set_pipeline(build_light_triangles_pipeline);
    pass.dispatch_workgroups(scene_bindings.light_source_count.max(1), 1, 1);
    pass.set_pipeline(presample_light_tiles_pipeline);
    pass.set_immediates(
        0,
        bytemuck::cast_slice(&[
            frame_index,
            reset_flags,
            render_frame,
            stable_radius.to_bits(),
        ]),
    );
    pass.dispatch_workgroups(LIGHT_TILE_BLOCKS as u32, 1, 1);
    d.end(&mut pass);

    let d = diagnostics.time_span(&mut pass, "solarik_lighting/world_cache");

    pass.set_bind_group(2, &bind_group_world_cache_active_cells_dispatch, &[]);

    pass.set_pipeline(decay_world_cache_pipeline);
    pass.set_immediates(
        0,
        bytemuck::cast_slice(&[
            frame_index,
            reset_flags,
            render_frame,
            stable_radius.to_bits(),
        ]),
    );
    pass.dispatch_workgroups((WORLD_CACHE_SIZE / 1024) as u32, 1, 1);

    pass.set_pipeline(compact_world_cache_single_block_pipeline);
    pass.dispatch_workgroups((WORLD_CACHE_SIZE / 1024) as u32, 1, 1);

    pass.set_pipeline(compact_world_cache_blocks_pipeline);
    pass.dispatch_workgroups(1, 1, 1);

    pass.set_pipeline(compact_world_cache_write_active_cells_pipeline);
    pass.dispatch_workgroups((WORLD_CACHE_SIZE / 1024) as u32, 1, 1);

    pass.set_pipeline(clear_world_cache_priority_pipeline);
    pass.dispatch_workgroups(1, 1, 1);
    pass.set_pipeline(histogram_world_cache_priority_pipeline);
    pass.dispatch_workgroups((WORLD_CACHE_SIZE / 256) as u32, 1, 1);
    pass.set_pipeline(budget_world_cache_priority_pipeline);
    pass.dispatch_workgroups(1, 1, 1);
    pass.set_pipeline(select_world_cache_priority_pipeline);
    pass.dispatch_workgroups((WORLD_CACHE_SIZE / 256) as u32, 1, 1);

    pass.set_bind_group(2, None, &[]);

    pass.set_pipeline(sample_di_for_world_cache_pipeline);
    pass.set_immediates(
        0,
        bytemuck::cast_slice(&[
            frame_index,
            reset_flags,
            render_frame,
            stable_radius.to_bits(),
        ]),
    );
    pass.dispatch_workgroups_indirect(
        &solarik_lighting_resources.world_cache_active_cells_dispatch,
        0,
    );

    // With NRC answering both the diffuse and the specular GI's queries
    // after its warm-up, nothing reads the cells' bounce light: their GI
    // rays are left out (sample_gi only adds to what sample_di wrote for
    // the cell, so the blend takes the direct light alone; the active cells
    // and their indirect dispatch are the same either way).
    let drop_cache_gi = nrc_frame
        .as_ref()
        .is_some_and(|nrc| nrc.drops_cache_gi(render_frame));
    if !drop_cache_gi {
        pass.set_pipeline(sample_gi_for_world_cache_pipeline);
        pass.set_immediates(
            0,
            bytemuck::cast_slice(&[
                frame_index,
                reset_flags,
                render_frame,
                stable_radius.to_bits(),
            ]),
        );
        pass.dispatch_workgroups_indirect(
            &solarik_lighting_resources.world_cache_active_cells_dispatch,
            0,
        );
    }

    pass.set_pipeline(blend_new_world_cache_samples_pipeline);
    pass.dispatch_workgroups_indirect(
        &solarik_lighting_resources.world_cache_active_cells_dispatch,
        0,
    );

    d.end(&mut pass);

    // NRC's BeginFrame goes between two of the lighting's encoders (wgpu
    // panics when the library's raw commands share one with its own), then
    // the training pass writes its paths before the GI's queries. Group 2
    // stays NRC's from here through the GI's initial pass: direct
    // lighting's layouts do not reach it.
    if nrc_frame.is_some() {
        drop(pass);
        let began = nrc_frame
            .as_mut()
            .is_some_and(|nrc| nrc.begin_frame(&mut ctx, &render_device, &render_queue, render_frame));
        if !began {
            nrc_frame = None;
        }
        pass = ctx.command_encoder().begin_compute_pass(&ComputePassDescriptor {
            label: Some("solarik_lighting_nrc_training"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, scene_bind_group, &[]);
        pass.set_bind_group(
            1,
            &bind_group,
            &[
                view_uniform_offset.offset,
                previous_view_uniform_offset.offset,
            ],
        );
        if let Some(nrc) = &nrc_frame {
            pass.set_bind_group(2, nrc.bind_group(), &[]);
            let d = diagnostics.time_span(&mut pass, "solarik_lighting/nrc_training");
            pass.set_pipeline(nrc.training_pipeline());
            pass.set_immediates(
                0,
                bytemuck::cast_slice(&[
                    frame_index,
                    reset_flags,
                    render_frame,
                    stable_radius.to_bits(),
                ]),
            );
            let (x, y) = nrc.training_workgroups();
            pass.dispatch_workgroups(x, y, 1);
            d.end(&mut pass);
        }
    }

    let d = diagnostics.time_span(&mut pass, "solarik_lighting/direct_lighting");

    // (the pass's two halves timed within it: `di_initial_temporal` the
    // candidates, their shadow tests and the temporal reuse;
    // `di_spatial_shade` the spatial reuse, the final shadow ray and the
    // shading)
    let half = diagnostics.time_span(&mut pass, "solarik_lighting/di_initial_temporal");
    pass.set_pipeline(di_initial_and_temporal_pipeline);
    pass.set_immediates(
        0,
        bytemuck::cast_slice(&[
            frame_index,
            reset_flags,
            render_frame,
            stable_radius.to_bits(),
        ]),
    );
    pass.dispatch_workgroups(dx, dy, 1);
    half.end(&mut pass);

    let half = diagnostics.time_span(&mut pass, "solarik_lighting/di_spatial_shade");
    pass.set_pipeline(di_spatial_and_shade_pipeline);
    pass.set_immediates(
        0,
        bytemuck::cast_slice(&[
            frame_index,
            reset_flags,
            render_frame,
            stable_radius.to_bits(),
        ]),
    );
    pass.dispatch_workgroups(dx, dy, 1);
    half.end(&mut pass);

    d.end(&mut pass);

    let mut d = diagnostics.time_span(&mut pass, "solarik_lighting/diffuse_indirect_lighting");

    // (with NRC the initial samples only, their queries written; the
    // temporal step follows NRC's answers in the resolve pass)
    pass.set_pipeline(
        nrc_frame
            .as_ref()
            .map_or(gi_initial_and_temporal_pipeline, |nrc| nrc.gi_initial_pipeline()),
    );
    pass.set_immediates(
        0,
        bytemuck::cast_slice(&[
            frame_index,
            reset_flags,
            render_frame,
            stable_radius.to_bits(),
        ]),
    );
    pass.dispatch_workgroups(dx, dy, 1);

    // With the specular query on, the specular pass goes here, its queries
    // written before QueryAndTrain (group 2 is still NRC's; the guide
    // textures at group 3 in the ray reconstruction variant). Its own
    // radiance goes into the view as later; the resolve adds the answers.
    // (Ahead of the resolve is safe for the GI: the specular pass writes the
    // view, which both add to, and the guide textures, which no GI pass
    // reads.)
    // (its surfaces that reuse the ReSTIR GI reservoir wait for the spatial
    // pass, in a second stage at the usual place: nrc_specular_reuse)
    let nrc_specular = nrc_frame
        .as_ref()
        .and_then(|nrc| nrc.specular_pipeline(specular_psr));
    let nrc_specular_reuse = nrc_frame
        .as_ref()
        .and_then(|nrc| nrc.specular_reuse_pipeline(specular_psr));

    // QueryAndTrain in an encoder of its own, after the frame's queries and
    // training paths are written, then the answers resolved into the
    // reservoirs with the temporal step. (Should QueryAndTrain fail, this
    // frame's spatial pass sees the bare initial samples, and the next
    // frame takes the world cache path.)
    if nrc_frame.is_some() {
        d.end(&mut pass);
        if let Some(pipeline) = nrc_specular {
            let d = diagnostics.time_span(&mut pass, "solarik_lighting/specular_indirect_lighting");
            #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
            if let Some(bind_group_resolve_dlss_rr_textures) = &bind_group_resolve_dlss_rr_textures {
                pass.set_bind_group(3, bind_group_resolve_dlss_rr_textures, &[]);
            }
            pass.set_pipeline(pipeline);
            pass.set_immediates(
                0,
                bytemuck::cast_slice(&[
                    frame_index,
                    reset_flags,
                    render_frame,
                    stable_radius.to_bits(),
                ]),
            );
            pass.dispatch_workgroups(dx, dy, 1);
            d.end(&mut pass);
        }
        drop(pass);
        let answered = nrc_frame
            .as_mut()
            .is_some_and(|nrc| nrc.query_and_train(&mut ctx, &render_device, &diagnostics));
        pass = ctx.command_encoder().begin_compute_pass(&ComputePassDescriptor {
            label: Some("solarik_lighting_nrc_resolve"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, scene_bind_group, &[]);
        pass.set_bind_group(
            1,
            &bind_group,
            &[
                view_uniform_offset.offset,
                previous_view_uniform_offset.offset,
            ],
        );
        if let Some(nrc) = nrc_frame.as_ref().filter(|_| answered) {
            pass.set_bind_group(2, nrc.bind_group(), &[]);
            let d = diagnostics.time_span(&mut pass, "solarik_lighting/nrc_resolve");
            pass.set_pipeline(nrc.resolve_pipeline());
            pass.set_immediates(
                0,
                bytemuck::cast_slice(&[
                    frame_index,
                    reset_flags,
                    render_frame,
                    stable_radius.to_bits(),
                ]),
            );
            pass.dispatch_workgroups(dx, dy, 1);
            d.end(&mut pass);
        }
        d = diagnostics.time_span(&mut pass, "solarik_lighting/diffuse_indirect_spatial");
    }

    pass.set_pipeline(gi_spatial_and_shade_pipeline);
    pass.set_immediates(
        0,
        bytemuck::cast_slice(&[
            frame_index,
            reset_flags,
            render_frame,
            stable_radius.to_bits(),
        ]),
    );
    pass.dispatch_workgroups(dx, dy, 1);

    d.end(&mut pass);

    // (here unless it ran ahead of QueryAndTrain with NRC; then its second
    // stage, the surfaces reusing the reservoir spatial_and_shade just wrote)
    let specular_here = match nrc_specular {
        None => Some((specular_gi_pipeline, "solarik_lighting/specular_indirect_lighting")),
        Some(_) => nrc_specular_reuse.map(|p| (p, "solarik_lighting/specular_indirect_reuse")),
    };
    if let Some((pipeline, span)) = specular_here {
        let d = diagnostics.time_span(&mut pass, span);
        #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
        if let Some(bind_group_resolve_dlss_rr_textures) = &bind_group_resolve_dlss_rr_textures {
            pass.set_bind_group(2, bind_group_resolve_dlss_rr_textures, &[]);
        }
        pass.set_pipeline(pipeline);
        pass.set_immediates(
            0,
            bytemuck::cast_slice(&[
                frame_index,
                reset_flags,
                render_frame,
                stable_radius.to_bits(),
            ]),
        );
        pass.dispatch_workgroups(dx, dy, 1);
        d.end(&mut pass);
    }

    // lit fog, when asked for (and compiled: it doesn't hold the rest back;
    // the prefilter's pipeline waited for only when it has levels to run)
    let lit_fog_levels = pipelines.lit_fog_levels;
    let lit_fog_pipelines = (ray_fog.lit.x > 0.0)
        .then(|| {
            let filter = if lit_fog_levels > 0 {
                Some(pipeline_cache.get_compute_pipeline(pipelines.lit_fog_filter_pipeline)?)
            } else {
                None
            };
            Some((
                pipeline_cache.get_compute_pipeline(pipelines.lit_fog_pipeline)?,
                filter,
                pipeline_cache.get_compute_pipeline(pipelines.lit_fog_accumulate_pipeline)?,
            ))
        })
        .flatten();
    // (the first frame it runs after one it didn't starts without the
    // history, which is from whenever it last ran: bit 0, which only
    // lit_fog_accumulate reads)
    let lit_fog_ran = s
        .lit_fog_ran
        .swap(lit_fog_pipelines.is_some(), core::sync::atomic::Ordering::Relaxed);
    if let Some((lit_fog_pipeline, filter_pipeline, accumulate_pipeline)) = lit_fog_pipelines {
        let lit_fog_reset = if lit_fog_ran { reset_flags } else { reset_flags | 1 };
        let d = diagnostics.time_span(&mut pass, "solarik_lighting/lit_fog");
        // (the estimate, the prefilter's levels, the accumulation; each
        // level's index in the fourth immediate, which lit_fog_filter reads
        // as its bits, the others stable_radius, which they don't read)
        let levels = filter_pipeline.map_or(0, |_| lit_fog_levels);
        let steps = core::iter::once((lit_fog_pipeline, stable_radius.to_bits()))
            .chain((0..levels).filter_map(|level| filter_pipeline.map(|p| (p, level))))
            .chain(core::iter::once((accumulate_pipeline, stable_radius.to_bits())));
        for (pipeline, fourth) in steps {
            pass.set_pipeline(pipeline);
            pass.set_immediates(
                0,
                bytemuck::cast_slice(&[frame_index, lit_fog_reset, render_frame, fourth]),
            );
            pass.dispatch_workgroups(dx, dy, 1);
        }
        d.end(&mut pass);
    }

    drop(pass);
    // Earlier returns (missing scene/pipelines and the primary compositor)
    // leave both scene events and explicit full resets pending for this view.
    s.history.rendered(scene_bindings.history_generation);

    diagnostics.record_u32(
        ctx.command_encoder(),
        &s.world_cache_active_cells_count.slice(..),
        "solarik_lighting/world_cache_active_cells_count",
    );
    for (word, name) in [
        (96, "world_cache_selected_cells"),
        (97, "world_cache_unlit_cells"),
        (98, "world_cache_selected_unlit"),
        (99, "world_cache_selected_refresh"),
        (100, "world_cache_oldest_age"),
        (101, "world_cache_updated_cells"),
        (102, "world_cache_reset"),
        (103, "world_cache_local_reset"),
        (104, "world_cache_adaptive_allocator"),
        (105, "world_cache_starved_candidates"),
        (106, "world_cache_high_priority_candidates"),
    ] {
        diagnostics.record_u32(
            ctx.command_encoder(),
            &s.world_cache_b.slice(word * 4..(word + 1) * 4),
            format!("solarik_lighting/{name}"),
        );
    }
}

/// Initializes the Solarik lighting pipelines at render startup.
pub fn init_solari_lighting_pipelines(
    mut commands: Commands,
    adapter: Res<RenderAdapter>,
    #[cfg(feature = "graphics_debug")] device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    scene_bindings: Res<RaytracingSceneBindings>,
    asset_server: Res<AssetServer>,
    #[cfg(feature = "nrc")] nrc_state: Option<Res<super::nrc::NrcState>>,
) {
    let raster_subpixel_bits = adapter
        .raster_subpixel_precision_bits()
        .unwrap_or(4)
        .min(24);
    tracing::info!(
        raster_subpixel_bits,
        "Solarik raster reconstruction precision"
    );
    let bind_group_layout = BindGroupLayoutDescriptor::new(
        "solarik_lighting_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_storage_2d(TextureFormat::Rgba16Float, StorageTextureAccess::ReadWrite),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                texture_storage_2d(TextureFormat::Rgba32Uint, StorageTextureAccess::ReadWrite),
                texture_storage_2d(TextureFormat::Rgba32Uint, StorageTextureAccess::ReadWrite),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                texture_2d(TextureSampleType::Uint),
                texture_depth_2d(),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Uint),
                texture_depth_2d(),
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer::<PreviousViewData>(true),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                texture_2d(TextureSampleType::Float { filterable: false }),
                storage_buffer_sized(false, None),
                texture_storage_2d(TextureFormat::Rgba16Float, StorageTextureAccess::ReadWrite),
                texture_storage_2d(TextureFormat::Rgba16Float, StorageTextureAccess::ReadWrite),
                texture_storage_2d(TextureFormat::Rgba16Float, StorageTextureAccess::ReadWrite),
                // (lit fog's scratch texture, the prefilter's)
                texture_storage_2d(TextureFormat::Rgba16Float, StorageTextureAccess::ReadWrite),
                // (the emissive triangles' table)
                storage_buffer_sized(false, None),
            ),
        ),
    );

    let bind_group_layout_world_cache_active_cells_dispatch = BindGroupLayoutDescriptor::new(
        "solarik_lighting_bind_group_layout_world_cache_active_cells_dispatch",
        &BindGroupLayoutEntries::single(ShaderStages::COMPUTE, storage_buffer_sized(false, None)),
    );

    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    let bind_group_layout_resolve_dlss_rr_textures = BindGroupLayoutDescriptor::new(
        "solarik_lighting_bind_group_layout_resolve_dlss_rr_textures",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_storage_2d(TextureFormat::Rgba8Unorm, StorageTextureAccess::WriteOnly),
                texture_storage_2d(TextureFormat::Rgba8Unorm, StorageTextureAccess::WriteOnly),
                texture_storage_2d(TextureFormat::Rgba16Float, StorageTextureAccess::WriteOnly),
                texture_storage_2d(TextureFormat::R32Float, StorageTextureAccess::WriteOnly),
                texture_storage_2d(TextureFormat::Rg16Float, StorageTextureAccess::WriteOnly),
            ),
        ),
    );

    #[cfg(feature = "graphics_debug")]
    let direct_capture = std::env::var("SOLARIK_PRIMARY_DIRECT_ONLY").as_deref() == Ok("1");
    #[cfg(feature = "graphics_debug")]
    let query_capture = (direct_capture
        || std::env::var("SOLARIK_PRIMARY_QUERY_ONLY").as_deref() == Ok("1"))
    .then(|| {
        super::query_capture::PrimaryQueryCapture::new(&device, &pipeline_cache, direct_capture)
    });
    // (groups 2 and on from `extra_bind_group_layouts`, in order)
    let create_pipeline_with_layouts = |label: &'static str,
                                        entry_point: &'static str,
                                        shader: Handle<Shader>,
                                        extra_bind_group_layouts: &[&BindGroupLayoutDescriptor],
                                        extra_shader_defs: Vec<ShaderDefVal>| {
        let mut layout = vec![
            scene_bindings.bind_group_layout.clone(),
            bind_group_layout.clone(),
        ];
        layout.extend(extra_bind_group_layouts.iter().map(|&l| l.clone()));

        let mut shader_defs = vec![ShaderDefVal::UInt(
            "WORLD_CACHE_SIZE".into(),
            WORLD_CACHE_SIZE as u32,
        )];
        shader_defs.extend_from_slice(&extra_shader_defs);
        shader_defs.push(ShaderDefVal::UInt(
            "RASTER_SUBPIXEL_BITS".into(),
            raster_subpixel_bits,
        ));
        shader_defs.push("BINDLESS_SURFACE_DETAIL".into());
        // (the tuning knobs of tuning.wesl, from the environment while
        // docs/boiling-plan.md's Phase 3 tunes them; the DI candidates are
        // Phase 3's result, 16 and 16 from 32 and 8, the rest as the constants
        // were)
        for (name, env, default) in [
            ("DI_TILE_CANDIDATES", "SOLARIK_DI_TILE_CANDIDATES", 16u32),
            ("DI_CELL_CANDIDATES", "SOLARIK_DI_CELL_CANDIDATES", 16),
            ("DI_CONFIDENCE_CAP", "SOLARIK_DI_CONFIDENCE_CAP", 8),
            ("GI_CONFIDENCE_CAP", "SOLARIK_GI_CONFIDENCE_CAP", 8),
            // (64 since 8 November, Phase 5's first payback: 0.34 ms off
            // the cache pass at every harness spot with no boiling column
            // moved, and the user's A/B in b40's shaft and corridor saw no
            // difference; 128 the old)
            ("CACHE_TILE_CANDIDATES", "SOLARIK_CACHE_TILE_CANDIDATES", 64),
            ("GI_ADAPTIVE_CONFIDENCE", "SOLARIK_GI_ADAPTIVE_CONFIDENCE", 4),
            // (the roughness in hundredths above which specular GI reuses the
            // diffuse GI reservoir instead of tracing, and a glossy path ends;
            // 40 until 6 November: at 30 the harness pictures are the same and
            // a smooth-floored b40 corridor 0.8 ms cheaper, docs/boiling-plan.md)
            ("SPECULAR_REUSE_ROUGHNESS", "SOLARIK_SPECULAR_REUSE_ROUGHNESS", 30),
            // (the roughness in thousandths of alpha under which a surface's
            // own glossy path ends in the world cache at its first opaque
            // hit that is not a mirror, as a pane's reflection does: Chief's
            // visor, 0.0144, was dull; 25 since 14 November, 0 the old, no
            // surface sharp; specular_gi.wesl glossy_lobe)
            ("SPECULAR_SHARP_ROUGHNESS", "SOLARIK_SPECULAR_SHARP_ROUGHNESS", 25),
            // (1: a clear coat's lobe under SPECULAR_SHARP_ROUGHNESS takes the
            // sharp rule too; every coat in Halo is under it, Chief's and the
            // Elites' armour, the eyes, the Hunters' worms, the Covenant
            // vehicles, and with it Chief's armour reads as a mirror in a lit
            // room; 0 the default since 14 November, the coats' paths as they
            // were; a coat marked sharp in the G-buffer, both coat bits, takes
            // the rule whatever this says, since 18 November)
            ("SPECULAR_SHARP_COATS", "SOLARIK_SPECULAR_SHARP_COATS", 0),
            // (the near distance in hundredths of the square root of an
            // emissive triangle's area, within which a receiver takes its
            // point by solid angle: sampling.wesl, near_triangle_warp)
            ("LIGHT_NEAR_DISTANCE", "SOLARIK_LIGHT_NEAR_DISTANCE", 100),
            // (ReSTIR GI's guided initial sampling, GI_GUIDE below: the most
            // of the candidates aimed at the temporal reservoir's sample, in
            // percent, held under 90 in the shader; the cone's half-angle in
            // degrees, held to 1 to 80; and in hundredths of a steradian the
            // effective solid angle of the pixel's light, its W, at which the
            // guide fades out; tuning.wesl)
            ("GI_GUIDE_SHARE", "SOLARIK_GI_GUIDE_SHARE", 50),
            ("GI_GUIDE_CONE", "SOLARIK_GI_GUIDE_CONE", 10),
            ("GI_GUIDE_WIDE", "SOLARIK_GI_GUIDE_WIDE", 100),
            // (in units of 2^-24, the share of a position's magnitude by
            // which the rays leave a surface and stop short of one, never
            // under the millimetre: raytracing_scene_bindings.wesl's
            // ray_epsilon; SOLARIK_SHADER_DEFS=RAY_OFFSET_FIXED the old
            // millimetre everywhere)
            ("RAY_OFFSET_SCALE", "SOLARIK_RAY_OFFSET_SCALE", 16),
            // (the radius in pixels of ReSTIR GI's spatial reuse, the disk
            // its spatial pass draws a neighbour from; tuning.wesl. DI's
            // radius is its own constant)
            ("GI_SPATIAL_RADIUS", "SOLARIK_GI_SPATIAL_RADIUS", 30),
            // (the spatial neighbours ReSTIR GI merges a pixel, in sequence,
            // at most 16; tuning.wesl)
            ("GI_SPATIAL_COUNT", "SOLARIK_GI_SPATIAL_COUNT", 1),
            // (the radius in pixels of the pixel's reprojected place within
            // which ReSTIR GI's guide takes its neighbours,
            // GI_GUIDE_NEIGHBOURS below; tuning.wesl)
            ("GI_GUIDE_RADIUS", "SOLARIK_GI_GUIDE_RADIUS", 30),
        ] {
            let value = std::env::var(env).ok().and_then(|v| v.trim().parse().ok()).filter(|&v: &u32| v > 0).unwrap_or(default);
            shader_defs.push(ShaderDefVal::UInt(name.into(), value));
        }
        // (the switches, 0 allowed: the world cache's history clamp and
        // drop detector, tuning.wesl; on since 9 November: a flash in a
        // closed room sat in the cache for ten seconds, and with these it
        // leaves in a few updates; the harness tables base64 and cachefast
        // hold every boiling column, docs/boiling-plan.md)
        for (name, env, default) in [
            ("CACHE_HISTORY_CLAMP", "SOLARIK_CACHE_HISTORY_CLAMP", 1u32),
            ("CACHE_DROP_DETECT", "SOLARIK_CACHE_DROP_DETECT", 1u32),
            // (a drop past 60 hundredths of the cell's light counts; it, and
            // the clamp, leave a cell 4 samples at the fewest)
            ("CACHE_DROP_THRESHOLD", "SOLARIK_CACHE_DROP_THRESHOLD", 60u32),
            ("CACHE_DROP_FLOOR", "SOLARIK_CACHE_DROP_FLOOR", 4u32),
            // (ReSTIR GI's guide offered by up to this many neighbours'
            // histories where the pixel's own gives none, at most 16 in the
            // shader; 8 since 14 November, about 60% off the boiling of
            // d40's doorway wall for about 0.23 ms;
            // SOLARIK_GI_GUIDE_NEIGHBOURS=0 none, the old; tuning.wesl)
            ("GI_GUIDE_NEIGHBOURS", "SOLARIK_GI_GUIDE_NEIGHBOURS", 8u32),
            // (lit fog, lit_fog.wesl: the most frames of its history, held
            // to 1 to 1024 in the shader, 24 the old; the history's clamp,
            // its band in hundredths of the prefiltered light, 0 none, the
            // old, and off whatever this says with no prefilter; the depth
            // weights' tolerance in thousandths of the view depth, held to
            // at least 1; the estimate's candidates from the light tiles,
            // held to 1 to 64. The prefilter's levels, LIT_FOG_LEVELS, are
            // pushed below from lit_fog_levels, which the dispatch reads too)
            ("LIT_FOG_FRAMES", "SOLARIK_LIT_FOG_FRAMES", 96u32),
            ("LIT_FOG_CLAMP", "SOLARIK_LIT_FOG_CLAMP", 25u32),
            // (the power of the share of its luminance a clamped history
            // kept by which its frame count is scaled, held to 1 to 64 in
            // the shader; 2, the plain square: 8 and 24 followed a muzzle
            // flash no sooner and 8 read a little grainier on stills)
            ("LIT_FOG_CLAMP_POWER", "SOLARIK_LIT_FOG_CLAMP_POWER", 2u32),
            // (the clamp's band widened by this many tenths of the
            // prefiltered light's own noise at the pixel, the difference of
            // the last a-trous level from the one before it averaged over
            // the history's 3x3, held to 200 in the shader and nothing with
            // fewer than two levels; 40, four times the noise (20 left a quarter of
            // the blobs, 60 and 80 no fewer than 40): one frame's light
            // in dark far fog is off by more than the band, the clamp chased
            // it and soft blobs crawled; SOLARIK_LIT_FOG_CLAMP_NOISE=0 the
            // old band)
            ("LIT_FOG_CLAMP_NOISE", "SOLARIK_LIT_FOG_CLAMP_NOISE", 40u32),
            // (a surface past an opaque fog's reach, where its coverage is
            // whole, taken as the sky: its ray's end and its depth for the
            // filter and the history the sky's; c10's far trees among the
            // fogged sky were each a few pixels of their own to the depth
            // weights, blocky streaks through the lit fog;
            // SOLARIK_LIT_FOG_REACH=0 the old)
            ("LIT_FOG_REACH", "SOLARIK_LIT_FOG_REACH", 1u32),
            // (lit fog's history refused where the previous frame's surface
            // at the reprojected place is not at the pixel's surface's depth:
            // the fog's light belongs to the ray in front of a surface, and a
            // rock's edge passing dragged its short ray's light across the
            // sky behind it as an afterimage; SOLARIK_LIT_FOG_DISOCCLUSION=0
            // the old)
            ("LIT_FOG_DISOCCLUSION", "SOLARIK_LIT_FOG_DISOCCLUSION", 1u32),
            // (lit fog's point and spot lights, a flashlight among them,
            // sampled apart from the light tiles: each pixel picks one by its
            // light along the pixel's ray, LIT_FOG_LOCAL samples a pixel. The
            // tiles presample every light by its power, so a flashlight among
            // c10's emitters was a candidate of one pixel in some fourteen, and
            // one frame's light was 7% of the pixels carrying 94% of it, which
            // the a-trous levels painted into cells, the "leopard" pattern
            // after every move; SOLARIK_LIT_FOG_LOCAL=0 the old)
            ("LIT_FOG_LOCAL", "SOLARIK_LIT_FOG_LOCAL", 1u32),
            // (the clamp band's widening by the prefiltered light's noise
            // faded out as the pixel moves, gone at this many tenths of a
            // render pixel a frame: the widening, for steady dark far fog,
            // let a moved view's stale history through whole, and what it
            // held (the shadows of leaves in the fog, seen from where the
            // view had been) stood in crisp dark cells over the fog for the
            // frames after the move, the "leopard" pattern;
            // SOLARIK_LIT_FOG_NOISE_STILL=0 the old, widened however it moves)
            ("LIT_FOG_NOISE_STILL", "SOLARIK_LIT_FOG_NOISE_STILL", 20u32),
            ("LIT_FOG_DEPTH", "SOLARIK_LIT_FOG_DEPTH", 50u32),
            ("LIT_FOG_CANDIDATES", "SOLARIK_LIT_FOG_CANDIDATES", 16u32),
        ] {
            let value = std::env::var(env).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default);
            if name == "LIT_FOG_REACH" && value == 0 {
                // (once: these definitions are built for every pipeline)
                static LOGGED: std::sync::Once = std::sync::Once::new();
                LOGGED.call_once(|| {
                    tracing::info!("lit fog: a surface past an opaque fog's reach lit as a surface (SOLARIK_LIT_FOG_REACH=0)");
                });
            }
            shader_defs.push(ShaderDefVal::UInt(name.into(), value));
        }
        // (lit fog's a-trous prefilter levels, 0 to 6, 0 none: the old 5x5
        // in lit_fog_accumulate; lit_fog_levels)
        shader_defs.push(ShaderDefVal::UInt("LIT_FOG_LEVELS".into(), lit_fog_levels()));
        // (the DLSS-RR guides faded by the fog where the application fogs
        // the picture before the denoiser, resolve_dlss_rr_textures.wesl;
        // nothing without its SolarikRayFog planar.w; opt-in,
        // SOLARIK_RR_FOG_GUIDE=1: on c10's rock in mid fog it flattened the
        // texture's return when still but left the rock 3.6 of 255 darker and
        // blurred in motion)
        if rr_fog_guide() {
            shader_defs.push("RR_FOG_GUIDE".into());
        }
        // (diagnostics: extra definitions, comma separated)
        if let Ok(defs) = std::env::var("SOLARIK_SHADER_DEFS") {
            shader_defs.extend(defs.split(',').filter(|d| !d.is_empty()).map(|d| d.trim().to_string().into()));
        }
        // (the soft sun on terrain, SUN_SOFT: see sun_soft_shader_defs)
        let sun_soft = sun_soft_shader_defs(&shader_defs);
        shader_defs.extend(sun_soft);
        // (on by default; SOLARIK_CELL_LIGHTS=0 leaves them out)
        if !std::env::var("SOLARIK_CELL_LIGHTS").is_ok_and(|v| v == "0") {
            shader_defs.push("DI_CELL_LIGHTS".into());
        }
        // The world cache's cells keyed by the triangle plane normal on both
        // sides, the camera's pixel's from its depths (on by default;
        // SOLARIK_CELL_KEY_PLANE=0 for the vertex normals, as upstream): a
        // bump-mapped G-buffer normal found another key than the hit's
        // smoothed vertex normal for three pixels in four of c20's halls, so
        // no light candidates by place there (4 November 2026; see
        // world_cache_query.wesl, cell_key_normal).
        if !std::env::var("SOLARIK_CELL_KEY_PLANE").is_ok_and(|v| v == "0") {
            shader_defs.push("CELL_KEY_PLANE".into());
            shader_defs.push("CELL_LOOKUP_GEOMETRIC".into());
        }
        // A new cell keeps the query's own point, not the jittered one (on by
        // default; SOLARIK_CACHE_CELL_UNJITTERED=0 as upstream): the jittered
        // point lay inside the rock for 38% of b40's canyon wall.
        if !std::env::var("SOLARIK_CACHE_CELL_UNJITTERED").is_ok_and(|v| v == "0") {
            shader_defs.push("CACHE_CELL_UNJITTERED".into());
        }
        // A new cell starts from its parent level of detail's radiance as
        // four samples (on by default; SOLARIK_CACHE_WARM_START=0 as
        // upstream): a cell born from one GI ray and four light samples was
        // noisy for its first updates, and a moving camera keeps making them.
        if !std::env::var("SOLARIK_CACHE_WARM_START").is_ok_and(|v| v == "0") {
            shader_defs.push("CACHE_WARM_START".into());
        }
        // The GI spatial pass merges a second neighbour where the pixel's
        // history is short (SOLARIK_GI_ADAPTIVE_SPATIAL=1; the threshold is
        // SOLARIK_GI_ADAPTIVE_CONFIDENCE above). Off: it did nothing for the
        // shaft and roughened the hall's stills (the age limit empties a
        // pixel's reservoir every 16 frames, so short histories happen in a
        // still too); docs/boiling-plan.md, Phase 3.
        if std::env::var("SOLARIK_GI_ADAPTIVE_SPATIAL").is_ok_and(|v| v == "1") {
            shader_defs.push("GI_ADAPTIVE_SPATIAL".into());
        }
        // ReSTIR GI aims part of its initial candidates in a cone at the
        // sample the pixel's temporal reservoir holds, the candidate's density
        // the exact mixture (on by default; SOLARIK_GI_GUIDE=0 the old): a room
        // lit through a small doorway found its light one frame in a hundred
        // and its walls boiled in slow blotches as the reservoir held each
        // find and let it decay (restir_gi.wesl, make_gi_guide; the knobs
        // SOLARIK_GI_GUIDE_SHARE, _CONE and _WIDE above; the view
        // SOLARIK_SHADER_DEFS=VISUALIZE_GI_GUIDE).
        if !std::env::var("SOLARIK_GI_GUIDE").is_ok_and(|v| v == "0") {
            shader_defs.push("GI_GUIDE".into());
        }
        // A cache cell whose direct sample is dark traces two bounce rays an
        // update (on by default; SOLARIK_CACHE_SHADE_SECOND_RAY=0 as before).
        if !std::env::var("SOLARIK_CACHE_SHADE_SECOND_RAY").is_ok_and(|v| v == "0") {
            shader_defs.push("CACHE_SHADE_SECOND_RAY".into());
        }
        // A failed cell lookup probes the six keys one normal component off
        // (on by default; SOLARIK_CELL_LOOKUP_NEIGHBOURS=0 as before).
        if !std::env::var("SOLARIK_CELL_LOOKUP_NEIGHBOURS").is_ok_and(|v| v == "0") {
            shader_defs.push("CELL_LOOKUP_NEIGHBOURS".into());
        }
        // A tile candidate the pixel's cell's slots don't know is weighed as
        // mostly hidden (SOLARIK_SLOT_PRIOR=1; see restir_di's
        // SLOT_VISIBILITY_PRIOR). Off: with the cells keyed by the plane it
        // measured 7% steadier in b40's shaft and the same in c20's halls,
        // too little for a change to the estimator (4 November 2026).
        if std::env::var("SOLARIK_SLOT_PRIOR").is_ok_and(|v| v == "1") {
            shader_defs.push("DI_SLOT_VISIBILITY_PRIOR".into());
        }
        if std::env::var("SOLARIK_ADAPTIVE_CACHE_BUDGET").is_ok_and(|v| v == "1") {
            shader_defs.push("ADAPTIVE_CACHE_BUDGET".into());
        }
        if std::env::var("SOLARIK_REGIONAL_HISTORY").is_ok_and(|v| v == "1") {
            shader_defs.push("REGIONAL_HISTORY".into());
        }
        if std::env::var("SOLARIK_RAY_MATERIAL_FILTERING").is_ok_and(|v| v == "1") {
            shader_defs.push("RAY_MATERIAL_FOOTPRINTS".into());
        }
        // Clear coats as thin films (brdf.wesl's `thin_film_reflectance`):
        // SOLARIK_THIN_FILM=thickness in nm,film IOR,surface IOR.
        if let Some([thickness, ior, base_ior]) = thin_film_coat() {
            shader_defs.push("THIN_FILM_COAT".into());
            shader_defs.push(ShaderDefVal::UInt("THIN_FILM_NM".into(), thickness.round() as u32));
            shader_defs.push(ShaderDefVal::UInt("THIN_FILM_IOR".into(), (ior * 1000.0).round() as u32));
            shader_defs.push(ShaderDefVal::UInt("THIN_FILM_BASE_IOR".into(), (base_ior * 1000.0).round() as u32));
            // the pearl coats' thicker film (SOLARIK_PEARL_FILM=thickness in
            // nm; brdf.wesl's PearlCoatMaterials and G-buffer bit 30)
            if let Some(pearl) = std::env::var("SOLARIK_PEARL_FILM").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|&v| v > 0.0) {
                shader_defs.push("THIN_FILM_PEARL".into());
                shader_defs.push(ShaderDefVal::UInt("THIN_FILM_PEARL_NM".into(), pearl.round() as u32));
            }
        }
        #[cfg(feature = "graphics_debug")]
        if entry_point == "primary_foliage"
            && std::env::var("SOLARIK_PRIMARY_QUERY_ONLY").as_deref() == Ok("1")
        {
            tracing::warn!("Primary canopy query-only diagnostic enabled; radiance is invalid");
            shader_defs.push("PRIMARY_CANOPY_QUERY_ONLY".into());
            let capture = query_capture.as_ref().expect("query capture configured");
            if layout.len() == 2 {
                layout.push(capture.empty_layout.clone());
            }
            layout.push(capture.layout.clone());
        }

        #[cfg(feature = "graphics_debug")]
        if entry_point == "primary_foliage"
            && std::env::var("SOLARIK_PRIMARY_DIRECT_ONLY").as_deref() == Ok("1")
        {
            assert!(
                std::env::var("SOLARIK_PRIMARY_QUERY_ONLY").as_deref() != Ok("1"),
                "Direct-only and query-only diagnostics are exclusive"
            );
            tracing::warn!(
                "Primary canopy direct-only diagnostic enabled; indirect lighting is absent"
            );
            shader_defs.push("PRIMARY_CANOPY_DIRECT_ONLY".into());
            let capture = query_capture
                .as_ref()
                .expect("direct light capture configured");
            if layout.len() == 2 {
                layout.push(capture.empty_layout.clone());
            }
            layout.push(capture.layout.clone());
        }

        pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(label.into()),
            layout,
            immediate_size: 16,
            shader,
            shader_defs,
            entry_point: Some(entry_point.into()),
            ..default()
        })
    };
    let create_pipeline = |label: &'static str,
                           entry_point: &'static str,
                           shader: Handle<Shader>,
                           extra_bind_group_layout: Option<&BindGroupLayoutDescriptor>,
                           extra_shader_defs: Vec<ShaderDefVal>| {
        create_pipeline_with_layouts(label, entry_point, shader, extra_bind_group_layout.as_slice(), extra_shader_defs)
    };

    // NRC's variant of the GI's initial pass is a pipeline of its own: the
    // plain one stays for the frames NRC does not serve (another view, a
    // failure that turned it off).
    #[cfg(feature = "nrc")]
    let nrc = nrc_state.map(|state| {
        let layout = Some(&state.bind_group_layout);
        // With the specular query on, the specular pass writes its queries
        // (NRC_SPECULAR, NRC's buffers at group 2) and the resolve adds their
        // answers; the plain specular pipelines stay for frames without NRC.
        let specular = state.specular_query();
        let mut resolve_defs: Vec<ShaderDefVal> = vec![
            "WORLD_CACHE_FIRST_BOUNCE_LIGHT_LEAK_PREVENTION".into(),
            "FOLIAGE_TRANSMISSION".into(),
            "NRC".into(),
        ];
        if specular {
            resolve_defs.push("NRC_SPECULAR".into());
        }
        // (the second stage, NRC_SPECULAR_REUSE: the rough surfaces after
        // the GI's spatial pass, bound as the plain specular pipelines)
        let specular_reuse_pipeline = specular.then(|| {
            create_pipeline(
                "solarik_lighting_nrc_specular_reuse_pipeline",
                "specular_gi",
                load_embedded_asset!(asset_server.as_ref(), "specular_gi.wesl"),
                None,
                vec!["FOLIAGE_TRANSMISSION".into(), "NRC_SPECULAR_REUSE".into()],
            )
        });
        #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
        let specular_reuse_psr_pipeline = specular.then(|| {
            create_pipeline(
                "solarik_lighting_nrc_specular_reuse_with_psr_pipeline",
                "specular_gi",
                load_embedded_asset!(asset_server.as_ref(), "specular_gi.wesl"),
                Some(&bind_group_layout_resolve_dlss_rr_textures),
                vec![
                    "FOLIAGE_TRANSMISSION".into(),
                    "DLSS_RR_GUIDE_BUFFERS".into(),
                    "NRC_SPECULAR_REUSE".into(),
                ],
            )
        });
        #[cfg(not(all(feature = "dlss", not(feature = "force_disable_dlss"))))]
        let specular_reuse_psr_pipeline = None;
        let specular_pipeline = specular.then(|| {
            create_pipeline(
                "solarik_lighting_nrc_specular_gi_pipeline",
                "specular_gi",
                load_embedded_asset!(asset_server.as_ref(), "specular_gi.wesl"),
                layout,
                vec!["FOLIAGE_TRANSMISSION".into(), "NRC".into(), "NRC_SPECULAR".into()],
            )
        });
        // (the guide textures move to group 3 with NRC_SPECULAR,
        // realtime_bindings.wesl)
        #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
        let specular_psr_pipeline = specular.then(|| {
            create_pipeline_with_layouts(
                "solarik_lighting_nrc_specular_gi_with_psr_pipeline",
                "specular_gi",
                load_embedded_asset!(asset_server.as_ref(), "specular_gi.wesl"),
                &[&state.bind_group_layout, &bind_group_layout_resolve_dlss_rr_textures],
                vec![
                    "FOLIAGE_TRANSMISSION".into(),
                    "DLSS_RR_GUIDE_BUFFERS".into(),
                    "NRC".into(),
                    "NRC_SPECULAR".into(),
                ],
            )
        });
        #[cfg(not(all(feature = "dlss", not(feature = "force_disable_dlss"))))]
        let specular_psr_pipeline = None;
        super::nrc::NrcPipelineIds {
            gi_initial: create_pipeline(
                "solarik_lighting_nrc_gi_initial_pipeline",
                "initial_and_temporal",
                load_embedded_asset!(asset_server.as_ref(), "restir_gi.wesl"),
                layout,
                vec![
                    "WORLD_CACHE_FIRST_BOUNCE_LIGHT_LEAK_PREVENTION".into(),
                    "FOLIAGE_TRANSMISSION".into(),
                    "NRC".into(),
                ],
            ),
            training: create_pipeline(
                "solarik_lighting_nrc_training_pipeline",
                "nrc_train",
                load_embedded_asset!(asset_server.as_ref(), "nrc_training.wesl"),
                layout,
                vec!["NRC".into(), "FOLIAGE_TRANSMISSION".into()],
            ),
            // (the defs of the pass whose temporal step it takes over)
            resolve: create_pipeline(
                "solarik_lighting_nrc_resolve_pipeline",
                "nrc_resolve_and_temporal",
                load_embedded_asset!(asset_server.as_ref(), "restir_gi.wesl"),
                layout,
                resolve_defs,
            ),
            specular: specular_pipeline,
            specular_psr: specular_psr_pipeline,
            specular_reuse: specular_reuse_pipeline,
            specular_reuse_psr: specular_reuse_psr_pipeline,
        }
    });

    commands.insert_resource(SolarikLightingPipelines {
        #[cfg(feature = "nrc")]
        nrc,
        bind_group_layout: bind_group_layout.clone(),
        bind_group_layout_world_cache_active_cells_dispatch:
            bind_group_layout_world_cache_active_cells_dispatch.clone(),
        #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
        bind_group_layout_resolve_dlss_rr_textures: bind_group_layout_resolve_dlss_rr_textures
            .clone(),
        build_sky_rows_pipeline: create_pipeline(
            "solarik_lighting_build_sky_rows_pipeline",
            "build_sky_rows",
            load_embedded_asset!(asset_server.as_ref(), "sky_sampling.wesl"),
            None,
            vec![],
        ),
        build_sky_marginal_pipeline: create_pipeline(
            "solarik_lighting_build_sky_marginal_pipeline",
            "build_sky_marginal",
            load_embedded_asset!(asset_server.as_ref(), "sky_sampling.wesl"),
            None,
            vec![],
        ),
        decay_world_cache_pipeline: create_pipeline(
            "solarik_lighting_decay_world_cache_pipeline",
            "decay_world_cache",
            load_embedded_asset!(asset_server.as_ref(), "world_cache_compact.wesl"),
            Some(&bind_group_layout_world_cache_active_cells_dispatch),
            vec!["WORLD_CACHE_NON_ATOMIC_LIFE_BUFFER".into()],
        ),
        compact_world_cache_single_block_pipeline: create_pipeline(
            "solarik_lighting_compact_world_cache_single_block_pipeline",
            "compact_world_cache_single_block",
            load_embedded_asset!(asset_server.as_ref(), "world_cache_compact.wesl"),
            Some(&bind_group_layout_world_cache_active_cells_dispatch),
            vec!["WORLD_CACHE_NON_ATOMIC_LIFE_BUFFER".into()],
        ),
        compact_world_cache_blocks_pipeline: create_pipeline(
            "solarik_lighting_compact_world_cache_blocks_pipeline",
            "compact_world_cache_blocks",
            load_embedded_asset!(asset_server.as_ref(), "world_cache_compact.wesl"),
            Some(&bind_group_layout_world_cache_active_cells_dispatch),
            vec![],
        ),
        compact_world_cache_write_active_cells_pipeline: create_pipeline(
            "solarik_lighting_compact_world_cache_write_active_cells_pipeline",
            "compact_world_cache_write_active_cells",
            load_embedded_asset!(asset_server.as_ref(), "world_cache_compact.wesl"),
            Some(&bind_group_layout_world_cache_active_cells_dispatch),
            vec!["WORLD_CACHE_NON_ATOMIC_LIFE_BUFFER".into()],
        ),
        clear_world_cache_priority_pipeline: create_pipeline(
            "solarik_lighting_clear_world_cache_priority_pipeline",
            "clear_world_cache_priority",
            load_embedded_asset!(asset_server.as_ref(), "world_cache_priority.wesl"),
            Some(&bind_group_layout_world_cache_active_cells_dispatch),
            vec!["WORLD_CACHE_PRIORITY_ATOMIC_BUFFER".into()],
        ),
        histogram_world_cache_priority_pipeline: create_pipeline(
            "solarik_lighting_histogram_world_cache_priority_pipeline",
            "histogram_world_cache_priority",
            load_embedded_asset!(asset_server.as_ref(), "world_cache_priority.wesl"),
            Some(&bind_group_layout_world_cache_active_cells_dispatch),
            vec!["WORLD_CACHE_PRIORITY_ATOMIC_BUFFER".into()],
        ),
        budget_world_cache_priority_pipeline: create_pipeline(
            "solarik_lighting_budget_world_cache_priority_pipeline",
            "budget_world_cache_priority",
            load_embedded_asset!(asset_server.as_ref(), "world_cache_priority.wesl"),
            Some(&bind_group_layout_world_cache_active_cells_dispatch),
            vec!["WORLD_CACHE_PRIORITY_ATOMIC_BUFFER".into()],
        ),
        select_world_cache_priority_pipeline: create_pipeline(
            "solarik_lighting_select_world_cache_priority_pipeline",
            "select_world_cache_priority",
            load_embedded_asset!(asset_server.as_ref(), "world_cache_priority.wesl"),
            Some(&bind_group_layout_world_cache_active_cells_dispatch),
            vec!["WORLD_CACHE_PRIORITY_ATOMIC_BUFFER".into()],
        ),
        sample_di_for_world_cache_pipeline: create_pipeline(
            "solarik_lighting_sample_di_for_world_cache_pipeline",
            "sample_di",
            load_embedded_asset!(asset_server.as_ref(), "world_cache_update.wesl"),
            None,
            vec!["WORLD_CACHE_PRIORITY_ATOMIC_BUFFER".into()],
        ),
        sample_gi_for_world_cache_pipeline: create_pipeline(
            "solarik_lighting_sample_gi_for_world_cache_pipeline",
            "sample_gi",
            load_embedded_asset!(asset_server.as_ref(), "world_cache_update.wesl"),
            None,
            vec![
                "WORLD_CACHE_PRIORITY_ATOMIC_BUFFER".into(),
                "WORLD_CACHE_QUERY_ATOMIC_MAX_LIFETIME".into(),
                "FOLIAGE_TRANSMISSION".into(),
            ],
        ),
        blend_new_world_cache_samples_pipeline: create_pipeline(
            "solarik_lighting_blend_new_world_cache_samples_pipeline",
            "blend_new_samples",
            load_embedded_asset!(asset_server.as_ref(), "world_cache_update.wesl"),
            None,
            vec!["WORLD_CACHE_PRIORITY_ATOMIC_BUFFER".into()],
        ),
        presample_light_tiles_pipeline: create_pipeline(
            "solarik_lighting_presample_light_tiles_pipeline",
            "presample_light_tiles",
            load_embedded_asset!(asset_server.as_ref(), "presample_light_tiles.wesl"),
            None,
            vec![],
        ),
        build_light_triangles_pipeline: create_pipeline(
            "solarik_lighting_build_light_triangles_pipeline",
            "build_light_triangles",
            load_embedded_asset!(asset_server.as_ref(), "light_triangles.wesl"),
            None,
            vec![],
        ),
        di_initial_and_temporal_pipeline: create_pipeline(
            "solarik_lighting_di_initial_and_temporal_pipeline",
            "initial_and_temporal",
            load_embedded_asset!(asset_server.as_ref(), "restir_di.wesl"),
            None,
            vec![],
        ),
        di_spatial_and_shade_pipeline: create_pipeline(
            "solarik_lighting_di_spatial_and_shade_pipeline",
            "spatial_and_shade",
            load_embedded_asset!(asset_server.as_ref(), "restir_di.wesl"),
            None,
            vec![],
        ),
        gi_initial_and_temporal_pipeline: create_pipeline(
            "solarik_lighting_gi_initial_and_temporal_pipeline",
            "initial_and_temporal",
            load_embedded_asset!(asset_server.as_ref(), "restir_gi.wesl"),
            None,
            vec![
                "WORLD_CACHE_FIRST_BOUNCE_LIGHT_LEAK_PREVENTION".into(),
                "FOLIAGE_TRANSMISSION".into(),
            ],
        ),
        gi_spatial_and_shade_pipeline: create_pipeline(
            "solarik_lighting_gi_spatial_and_shade_pipeline",
            "spatial_and_shade",
            load_embedded_asset!(asset_server.as_ref(), "restir_gi.wesl"),
            None,
            vec![],
        ),
        primary_foliage_pipeline: create_pipeline(
            "solarik_primary_foliage",
            "primary_foliage",
            load_embedded_asset!(asset_server.as_ref(), "primary_foliage.wesl"),
            None,
            vec!["FOLIAGE_TRANSMISSION".into()],
        ),
        #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
        primary_foliage_rr_pipeline: create_pipeline(
            "solarik_primary_foliage_rr",
            "primary_foliage",
            load_embedded_asset!(asset_server.as_ref(), "primary_foliage.wesl"),
            Some(&bind_group_layout_resolve_dlss_rr_textures),
            vec![
                "FOLIAGE_TRANSMISSION".into(),
                "DLSS_RR_GUIDE_BUFFERS".into(),
            ],
        ),
        lit_fog_pipeline: create_pipeline(
            "solarik_lighting_lit_fog_pipeline",
            "lit_fog",
            load_embedded_asset!(asset_server.as_ref(), "lit_fog.wesl"),
            None,
            vec![],
        ),
        lit_fog_filter_pipeline: create_pipeline(
            "solarik_lighting_lit_fog_filter_pipeline",
            "lit_fog_filter",
            load_embedded_asset!(asset_server.as_ref(), "lit_fog.wesl"),
            None,
            vec![],
        ),
        lit_fog_levels: lit_fog_levels(),
        lit_fog_accumulate_pipeline: create_pipeline(
            "solarik_lighting_lit_fog_accumulate_pipeline",
            "lit_fog_accumulate",
            load_embedded_asset!(asset_server.as_ref(), "lit_fog.wesl"),
            None,
            vec![],
        ),
        primary_glass_pipeline: create_pipeline(
            "solarik_primary_glass",
            "primary_glass",
            load_embedded_asset!(asset_server.as_ref(), "primary_glass.wesl"),
            None,
            vec!["FOLIAGE_TRANSMISSION".into()],
        ),
        #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
        primary_glass_rr_pipeline: create_pipeline(
            "solarik_primary_glass_rr",
            "primary_glass",
            load_embedded_asset!(asset_server.as_ref(), "primary_glass.wesl"),
            Some(&bind_group_layout_resolve_dlss_rr_textures),
            vec![
                "FOLIAGE_TRANSMISSION".into(),
                "DLSS_RR_GUIDE_BUFFERS".into(),
            ],
        ),
        specular_gi_pipeline: create_pipeline(
            "solarik_lighting_specular_gi_pipeline",
            "specular_gi",
            load_embedded_asset!(asset_server.as_ref(), "specular_gi.wesl"),
            None,
            vec!["FOLIAGE_TRANSMISSION".into()],
        ),
        #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
        specular_gi_with_psr_pipeline: create_pipeline(
            "solarik_lighting_specular_gi_with_psr_pipeline",
            "specular_gi",
            load_embedded_asset!(asset_server.as_ref(), "specular_gi.wesl"),
            Some(&bind_group_layout_resolve_dlss_rr_textures),
            vec![
                "FOLIAGE_TRANSMISSION".into(),
                "DLSS_RR_GUIDE_BUFFERS".into(),
            ],
        ),
        #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
        resolve_dlss_rr_textures_pipeline: create_pipeline(
            "solarik_lighting_resolve_dlss_rr_textures_pipeline",
            "resolve_dlss_rr_textures",
            load_embedded_asset!(asset_server.as_ref(), "resolve_dlss_rr_textures.wesl"),
            Some(&bind_group_layout_resolve_dlss_rr_textures),
            vec!["DLSS_RR_GUIDE_BUFFERS".into()],
        ),
    });
    #[cfg(feature = "graphics_debug")]
    if let Some(capture) = query_capture {
        commands.insert_resource(capture);
    }
}

// The soft sun on terrain's defaults (sampling.wesl, sun_soft_transmission):
// the blur's radius in metres (SOLARIK_SUN_BLUR, 0 off), the terminator rule
// (SOLARIK_SUN_TERMINATOR=1) and the thickest rock its ray may start in, in
// metres (SOLARIK_SUN_TERMINATOR_REACH, 0 no limit). The blur 1.4 m, chosen
// by eye in play (3.66, a lightmap texel's softness, 1.8 and 1.4 tried);
// SOLARIK_SUN_BLUR=0 the old. The terminator rule off, opt-in.
const SUN_BLUR_DEFAULT: f32 = 1.4;
const SUN_TERMINATOR_DEFAULT: bool = false;
const SUN_TERMINATOR_REACH_DEFAULT: f32 = 0.0;
// (how far the probe runs, in its start-to-disc distances: 2 reaches as
// deep under the pixel's plane as it starts above it; SOLARIK_SUN_BLUR_DROP,
// held to 1 to 8)
const SUN_BLUR_DROP_DEFAULT: f32 = 2.0;

/// `SOLARIK_RR_FOG_GUIDE=1`: the DLSS-RR guides faded by the fog where the
/// application says it fogs the picture before the denoiser
/// (`SolarikRayFog::planar`'s w); else (the default) never, the old. Read
/// once, logged when on.
fn rr_fog_guide() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let on = parse_rr_fog_guide(std::env::var("SOLARIK_RR_FOG_GUIDE").ok().as_deref());
        if on {
            tracing::info!("DLSS-RR guides: faded by the fog where the picture is fogged before the denoiser (SOLARIK_RR_FOG_GUIDE=1)");
        }
        on
    })
}

/// `1` on; anything else, or nothing, off.
fn parse_rr_fog_guide(text: Option<&str>) -> bool {
    text.is_some_and(|text| text.trim() == "1")
}

#[cfg(test)]
mod rr_fog_guide_tests {
    #[test]
    fn rr_fog_guide_switch() {
        assert!(!super::parse_rr_fog_guide(None));
        assert!(super::parse_rr_fog_guide(Some("1")));
        assert!(super::parse_rr_fog_guide(Some(" 1 ")));
        assert!(!super::parse_rr_fog_guide(Some("0")));
    }
}

/// Lit fog's a-trous prefilter levels (`SOLARIK_LIT_FOG_LEVELS`, 4 by
/// default, held to 0 to 6; 0 no prefilter, the old 5x5 inside
/// `lit_fog_accumulate`): read by both the shader definition and the
/// dispatch, so the two cannot disagree.
fn lit_fog_levels() -> u32 {
    std::env::var("SOLARIK_LIT_FOG_LEVELS")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(4)
        .min(6)
}

/// The soft sun's definitions from the environment: `SUN_SOFT` when the blur
/// or the terminator is on (or one of its views, `VISUALIZE_SUN_SOFT` or
/// `VISUALIZE_SUN_SOFT_FACES`, is among `shader_defs`), with its values in
/// millimetres (`SUN_BLUR_MM`, `SUN_TERMINATOR_REACH_MM`), the probe's
/// length in hundredths (`SUN_BLUR_DROP_PCT`) and `SUN_TERMINATOR`; nothing otherwise, so no pass compiles any of it.
fn sun_soft_shader_defs(shader_defs: &[ShaderDefVal]) -> Vec<ShaderDefVal> {
    let metres = |env: &str, default: f32| {
        std::env::var(env).ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(default)
    };
    let blur = metres("SOLARIK_SUN_BLUR", SUN_BLUR_DEFAULT);
    let terminator = std::env::var("SOLARIK_SUN_TERMINATOR").map_or(SUN_TERMINATOR_DEFAULT, |v| v.trim() == "1");
    let reach = metres("SOLARIK_SUN_TERMINATOR_REACH", SUN_TERMINATOR_REACH_DEFAULT);
    let drop = std::env::var("SOLARIK_SUN_BLUR_DROP").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).unwrap_or(SUN_BLUR_DROP_DEFAULT).clamp(1.0, 8.0);
    let viewed = shader_defs.iter().any(|d| {
        matches!(d, ShaderDefVal::Bool(name, true) if name == "VISUALIZE_SUN_SOFT" || name == "VISUALIZE_SUN_SOFT_FACES")
    });
    if blur <= 0.0 && !terminator && !viewed {
        return Vec::new();
    }
    vec![
        "SUN_SOFT".into(),
        ShaderDefVal::UInt("SUN_BLUR_MM".into(), (blur * 1000.0).round() as u32),
        ShaderDefVal::UInt("SUN_TERMINATOR".into(), terminator as u32),
        ShaderDefVal::UInt("SUN_TERMINATOR_REACH_MM".into(), (reach * 1000.0).round() as u32),
        ShaderDefVal::UInt("SUN_BLUR_DROP_PCT".into(), (drop * 100.0).round() as u32),
    ]
}

/// `SOLARIK_THIN_FILM=thickness,ior,base_ior` (nanometres; the film's and the
/// surface's indices of refraction, 1.5 and 2.2 when left out).
fn thin_film_coat() -> Option<[f32; 3]> {
    let text = std::env::var("SOLARIK_THIN_FILM").ok()?;
    let mut values = text.split(',').map(|v| v.trim().parse::<f32>().ok());
    let thickness = values.next()??;
    let ior = values.next().flatten().unwrap_or(1.5);
    let base_ior = values.next().flatten().unwrap_or(2.2);
    (thickness > 0.0 && ior > 1.0 && base_ior > 1.0).then_some([thickness, ior, base_ior])
}
