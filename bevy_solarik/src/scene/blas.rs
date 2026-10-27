use super::light_sampling::{triangle_edges, world_area};
use alloc::collections::VecDeque;
use bevy_asset::AssetId;
use bevy_ecs::{
    resource::Resource,
    system::{Query, Res, ResMut},
};
use bevy_math::{Mat4, Vec3};
use bevy_mesh::{Indices, Mesh};
use bevy_pbr::{MeshMaterial3d, StandardMaterial};
use bevy_platform::collections::{HashMap, HashSet};
use bevy_render::{
    mesh::{
        RenderMesh,
        allocator::{MeshAllocator, MeshBufferSlice},
    },
    render_asset::ExtractedAssets,
    extract_resource::ExtractResource,
    render_resource::*,
    renderer::{RenderDevice, RenderQueue},
};

/// After compacting this many vertices worth of meshes per frame, no further BLAS will be compacted.
/// Lower this number to distribute the work across more frames.
const MAX_COMPACTION_VERTICES_PER_FRAME: u32 = 400_000;

/// A deformable mesh's BLAS is refit this many times, then built again: a
/// refit keeps the tree built for the first pose, which traces slower as the
/// mesh moves away from it.
const MAX_REFITS: u32 = 240;

/// Meshes whose vertices move from frame to frame while their topology stays
/// (a skinned character's ray-traced copy, changed in place): their BLAS is
/// built to allow updates and never compacted, and a change that keeps the
/// vertex and index counts refits it in place instead of building it again.
#[derive(Resource, ExtractResource, Clone, Default, Debug)]
#[extract_app(bevy_render::RenderApp)]
pub struct SolarikDeformableMeshes(pub HashSet<AssetId<Mesh>>);

#[derive(Resource, Default)]
pub struct BlasManager {
    pub(crate) generation: u64,
    revisions: HashMap<AssetId<Mesh>, u64>,
    pub(super) bounds: HashMap<AssetId<Mesh>, super::history::Bounds>,
    blas: HashMap<AssetId<Mesh>, Blas>,
    triangle_edges: HashMap<AssetId<Mesh>, Vec<[Vec3; 2]>>,
    // A mesh can be replaced while an older allocation is still queued.
    compaction_queue: VecDeque<(AssetId<Mesh>, u64, u32, bool)>,
    opacity: OpacityBook,
    /// Meshes whose BLAS was dropped because its opacity no longer matched
    /// the materials on it; rebuilt by `prepare_raytracing_blas` next frame.
    rebuild_queue: Vec<AssetId<Mesh>>,
    /// Deformable meshes' BLAS: the vertex and index counts and opacity each
    /// was built with, and how many times it has been refit since.
    updatable: HashMap<AssetId<Mesh>, (u32, u32, bool, u32)>,
}

impl BlasManager {
    pub(super) fn revision(&self, mesh: &AssetId<Mesh>) -> u64 {
        self.revisions.get(mesh).copied().unwrap_or(0)
    }
    pub fn mesh_world_area(&self, mesh: &AssetId<Mesh>, transform: Mat4) -> f64 {
        self.triangle_edges
            .get(mesh)
            .map_or(0.0, |edges| world_area(edges, transform))
    }

    pub fn get(&self, mesh: &AssetId<Mesh>) -> Option<&Blas> {
        self.blas.get(mesh)
    }

    /// Tell the manager which meshes carry an alpha-masked or blended
    /// material this frame. Their BLAS is built without the OPAQUE flag so
    /// the shader gets to alpha-test every candidate hit; a BLAS built with
    /// the wrong flag is dropped here and rebuilt next frame.
    pub fn set_non_opaque_meshes(&mut self, meshes: HashSet<AssetId<Mesh>>) {
        for mesh in self.opacity.require(meshes) {
            if self.blas.remove(&mesh).is_some() {
                self.generation = self.generation.wrapping_add(1);
                self.revisions.insert(mesh, self.generation);
                self.rebuild_queue.push(mesh);
            }
        }
    }
}

/// Which meshes need alpha-tested (non-opaque) acceleration structures, and
/// which opacity each built BLAS actually has. Pure bookkeeping, so the
/// rebuild decision can be tested without a GPU.
#[derive(Default)]
struct OpacityBook {
    required_non_opaque: HashSet<AssetId<Mesh>>,
    built_non_opaque: HashMap<AssetId<Mesh>, bool>,
}

impl OpacityBook {
    /// Replace the requirement; returns the meshes whose existing BLAS was
    /// built with the other opacity and must be rebuilt.
    fn require(&mut self, meshes: HashSet<AssetId<Mesh>>) -> Vec<AssetId<Mesh>> {
        self.required_non_opaque = meshes;
        let stale: Vec<_> = self
            .built_non_opaque
            .iter()
            .filter(|(mesh, built)| **built != self.required_non_opaque.contains(*mesh))
            .map(|(mesh, _)| *mesh)
            .collect();
        for mesh in &stale {
            self.built_non_opaque.remove(mesh);
        }
        stale
    }

    fn is_non_opaque(&self, mesh: &AssetId<Mesh>) -> bool {
        self.required_non_opaque.contains(mesh)
    }

    fn record_built(&mut self, mesh: AssetId<Mesh>, non_opaque: bool) {
        self.built_non_opaque.insert(mesh, non_opaque);
    }

    fn forget(&mut self, mesh: &AssetId<Mesh>) {
        self.built_non_opaque.remove(mesh);
    }
}

pub fn prepare_raytracing_blas(
    mut blas_manager: ResMut<BlasManager>,
    extracted_meshes: Res<ExtractedAssets<RenderMesh>>,
    (instances, materials, alpha_testing): (
        Query<(&super::RaytracingMesh3d, &MeshMaterial3d<StandardMaterial>)>,
        Res<super::extract::StandardMaterialAssets>,
        Res<super::binder::SolarikAlphaTesting>,
    ),
    readiness: Res<bevy_render::scene_readiness::SceneGeometryReadiness>,
    deformable: Res<SolarikDeformableMeshes>,
    mesh_allocator: Res<MeshAllocator>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    diagnostics: Option<Res<bevy_render::diagnostic::DiagnosticsRecorder>>,
) {
    let _cpu_profile = bevy_render::diagnostic::profile_scope("scene.blas_prepare");
    let mut required = blas_manager.opacity.required_non_opaque.clone();
    required.extend(readiness.alpha_requested::<Mesh>());
    // A new mesh already on an instance with an alpha-tested material is built
    // non-opaque at once. (Otherwise it was built opaque, the binder found it
    // wanted alpha testing, and it was rebuilt a frame later: a mesh replaced
    // every frame, such as a skinned character's copy, was never in the TLAS.)
    if alpha_testing.0 && !extracted_meshes.extracted.is_empty() {
        let new: HashSet<AssetId<Mesh>> = extracted_meshes
            .extracted
            .iter()
            .map(|(id, _)| *id)
            .collect();
        required.extend(
            instances
                .iter()
                .filter(|(mesh, material)| {
                    new.contains(&mesh.id())
                        && materials.get(&material.id()).is_some_and(|material| {
                            super::binder::material_alpha(material).flags
                                & super::binder::MATERIAL_FLAG_OPAQUE
                                == 0
                        })
                })
                .map(|(mesh, _)| mesh.id()),
        );
    }
    blas_manager.set_non_opaque_meshes(required);
    // Deformable meshes changed in place (the same vertex and index counts,
    // the same opacity) keep their BLAS, refit below
    let refits: HashSet<AssetId<Mesh>> = extracted_meshes
        .modified
        .iter()
        .filter(|asset_id| {
            deformable.0.contains(*asset_id)
                && !extracted_meshes.removed.contains(*asset_id)
                && blas_manager.blas.contains_key(*asset_id)
                && blas_manager.updatable.get(*asset_id).is_some_and(
                    |&(vertices, indices, non_opaque, refits)| {
                        refits < MAX_REFITS
                            && blas_manager.opacity.is_non_opaque(asset_id) == non_opaque
                            && mesh_allocator
                                .mesh_vertex_slice(asset_id)
                                .is_some_and(|slice| slice.range.len() as u32 == vertices)
                            && mesh_allocator
                                .mesh_index_slice(asset_id)
                                .is_some_and(|slice| slice.range.len() as u32 == indices)
                    },
                )
        })
        .copied()
        .collect();
    // Delete BLAS for deleted or modified meshes
    for asset_id in extracted_meshes
        .removed
        .iter()
        .chain(extracted_meshes.modified.iter())
        .filter(|asset_id| !refits.contains(*asset_id))
    {
        blas_manager.updatable.remove(asset_id);
        if blas_manager.blas.remove(asset_id).is_some() {
            blas_manager.generation = blas_manager.generation.wrapping_add(1);
        }
        blas_manager.revisions.remove(asset_id);
        blas_manager.triangle_edges.remove(asset_id);
        blas_manager.bounds.remove(asset_id);
        blas_manager.opacity.forget(asset_id);
    }

    for (asset_id, mesh) in &extracted_meshes.extracted {
        if is_mesh_raytracing_compatible(mesh) {
            if let Some(bounds) = super::history::Bounds::from_mesh(mesh) {
                blas_manager.bounds.insert(*asset_id, bounds);
            }
            blas_manager
                .triangle_edges
                .insert(*asset_id, triangle_edges(mesh));
        }
    }

    // Meshes whose opacity changed keep their allocation; only the BLAS is
    // rebuilt (a mesh that was freed meanwhile is simply skipped).
    let rebuilds: Vec<AssetId<Mesh>> = blas_manager
        .rebuild_queue
        .drain(..)
        .filter(|asset_id| {
            !extracted_meshes.modified.contains(asset_id)
                && !extracted_meshes.removed.contains(asset_id)
                && mesh_allocator.mesh_vertex_slice(asset_id).is_some()
                && mesh_allocator.mesh_index_slice(asset_id).is_some()
        })
        .collect();

    if extracted_meshes.extracted.is_empty() && rebuilds.is_empty() {
        return;
    }

    // Create new BLAS for added or changed meshes
    let mut scheduled: HashSet<AssetId<Mesh>> = HashSet::default();
    let blas_resources = extracted_meshes
        .extracted
        .iter()
        .filter(|(_, mesh)| is_mesh_raytracing_compatible(mesh))
        .map(|(asset_id, _)| *asset_id)
        .chain(rebuilds)
        .filter(|asset_id| scheduled.insert(*asset_id))
        .map(|asset_id| {
            let vertex_slice = mesh_allocator.mesh_vertex_slice(&asset_id).unwrap();
            let index_slice = mesh_allocator.mesh_index_slice(&asset_id).unwrap();

            let non_opaque = blas_manager.opacity.is_non_opaque(&asset_id);
            let blas_size = blas_size(&vertex_slice, &index_slice, non_opaque);
            // (a new revision either way: the mesh's slices may have moved)
            blas_manager.generation = blas_manager.generation.wrapping_add(1);
            let revision = blas_manager.generation;
            blas_manager.revisions.insert(asset_id, revision);
            if refits.contains(&asset_id) {
                // built again into the same BLAS, which wgpu refits
                if let Some(entry) = blas_manager.updatable.get_mut(&asset_id) {
                    entry.3 += 1;
                }
                return (asset_id, vertex_slice, index_slice, blas_size);
            }

            let updatable = deformable.0.contains(&asset_id);
            let blas = allocate_blas(&blas_size, &asset_id, updatable, &render_device);
            blas_manager.blas.insert(asset_id, blas);
            blas_manager.opacity.record_built(asset_id, non_opaque);
            if updatable {
                // (a compacted BLAS can't be updated)
                blas_manager.updatable.insert(
                    asset_id,
                    (blas_size.vertex_count, blas_size.index_count.unwrap_or(0), non_opaque, 0),
                );
            } else {
                blas_manager.updatable.remove(&asset_id);
                blas_manager.compaction_queue.push_back((
                    asset_id,
                    revision,
                    blas_size.vertex_count,
                    false,
                ));
            }

            (asset_id, vertex_slice, index_slice, blas_size)
        })
        .collect::<Vec<_>>();

    // Build geometry into each BLAS
    let build_entries = blas_resources
        .iter()
        .map(|(asset_id, vertex_slice, index_slice, blas_size)| {
            let geometry = BlasTriangleGeometry {
                size: blas_size,
                vertex_buffer: vertex_slice.buffer,
                first_vertex: vertex_slice.range.start,
                vertex_stride: 48,
                index_buffer: Some(index_slice.buffer),
                first_index: Some(index_slice.range.start),
                transform_buffer: None,
                transform_buffer_offset: None,
            };
            BlasBuildEntry {
                blas: &blas_manager.blas[asset_id],
                geometry: BlasGeometries::TriangleGeometries(vec![geometry]),
            }
        })
        .collect::<Vec<_>>();

    let mut command_encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("build_blas_command_encoder"),
    });
    use bevy_render::diagnostic::RecordDiagnostics;
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(&mut command_encoder, "scene/blas_build");
    command_encoder.build_acceleration_structures(&build_entries, &[]);
    span.end(&mut command_encoder);
    render_queue.submit([command_encoder.finish()]);
}

/// Publish only requested BLAS after build commands have been submitted. Running
/// after binding also observes opacity-triggered revocation in the current frame.
pub fn publish_readiness(
    manager: Res<BlasManager>,
    readiness: Option<Res<bevy_render::scene_readiness::SceneGeometryReadiness>>,
    materials: Res<super::extract::StandardMaterialAssets>,
    bindings: Res<bevy_pbr::RenderMaterialBindings>,
    images: Res<bevy_render::render_asset::RenderAssets<bevy_render::texture::GpuImage>>,
    registrations: Res<crate::material::Registrations>,
) {
    if let Some(readiness) = readiness {
        for id in readiness.requested::<bevy_image::Image>() {
            readiness.publish(id.untyped(), images.get(id).is_some());
        }
        for id in readiness.requested::<Mesh>() {
            readiness.publish(id.untyped(), manager.get(&id).is_some());
        }
        for id in readiness.requested::<StandardMaterial>() {
            let ready = registrations.gpu_ready(id, &bindings, &images)
                && bindings.contains_key(&id.untyped())
                && materials.get(&id).is_some_and(|material| {
                    [
                        &material.base_color_texture,
                        &material.normal_map_texture,
                        &material.emissive_texture,
                        &material.metallic_roughness_texture,
                        #[cfg(feature = "pbr_multi_layer_material_textures")]
                        &material.clearcoat_texture,
                    ]
                    .into_iter()
                    .flatten()
                    .all(|image| images.get(image.id()).is_some())
                });
            readiness.publish(id.untyped(), ready);
        }
    }
}

/// wgpu 29 compaction locks pending writes before command indices, while
/// submission locks them in the opposite order. Run this bounded work alone
/// in the render schedule, including against device-only asset uploads.
pub fn compact_raytracing_blas(world: &mut bevy_ecs::world::World) {
    world.resource_scope(
        |world, mut manager: bevy_ecs::change_detection::Mut<BlasManager>| {
            compact_ready_blas(&mut manager, world.resource::<RenderQueue>());
        },
    );
}

fn compact_ready_blas(blas_manager: &mut BlasManager, render_queue: &RenderQueue) {
    let queue_size = blas_manager.compaction_queue.len();
    let mut meshes_processed = 0;
    let mut vertices_compacted = 0;

    while !blas_manager.compaction_queue.is_empty()
        && vertices_compacted < MAX_COMPACTION_VERTICES_PER_FRAME
        && meshes_processed < queue_size
    {
        meshes_processed += 1;

        let (mesh, revision, vertex_count, compaction_started) =
            blas_manager.compaction_queue.pop_front().unwrap();

        if blas_manager.revision(&mesh) != revision {
            continue;
        }
        let Some(blas) = blas_manager.get(&mesh) else {
            continue;
        };

        if !compaction_started {
            blas.prepare_compaction_async(|_| {});
        }

        if blas.ready_for_compaction() {
            let compacted_blas = render_queue.compact_blas(blas);
            blas_manager.blas.insert(mesh, compacted_blas);
            blas_manager.generation = blas_manager.generation.wrapping_add(1);

            vertices_compacted += vertex_count;
            continue;
        }

        // BLAS not ready for compaction, put back in queue
        blas_manager
            .compaction_queue
            .push_back((mesh, revision, vertex_count, true));
    }
}

fn blas_size(
    vertex_slice: &MeshBufferSlice,
    index_slice: &MeshBufferSlice,
    non_opaque: bool,
) -> BlasTriangleGeometrySizeDescriptor {
    BlasTriangleGeometrySizeDescriptor {
        vertex_format: Mesh::ATTRIBUTE_POSITION.format,
        vertex_count: vertex_slice.range.len() as u32,
        index_format: Some(IndexFormat::Uint32),
        index_count: Some(index_slice.range.len() as u32),
        // Without OPAQUE every hit on this geometry is a candidate the
        // shader's trace_ray loop alpha-tests before confirming.
        flags: if non_opaque {
            AccelerationStructureGeometryFlags::empty()
        } else {
            AccelerationStructureGeometryFlags::OPAQUE
        },
    }
}

/// A BLAS for geometry of this size: compactable, or (a deformable mesh's)
/// updatable in place.
fn allocate_blas(
    blas_size: &BlasTriangleGeometrySizeDescriptor,
    asset_id: &AssetId<Mesh>,
    updatable: bool,
    render_device: &RenderDevice,
) -> Blas {
    let (flags, update_mode) = if updatable {
        (
            AccelerationStructureFlags::PREFER_FAST_TRACE | AccelerationStructureFlags::ALLOW_UPDATE,
            AccelerationStructureUpdateMode::PreferUpdate,
        )
    } else {
        (
            AccelerationStructureFlags::PREFER_FAST_TRACE
                | AccelerationStructureFlags::ALLOW_COMPACTION,
            AccelerationStructureUpdateMode::Build,
        )
    };
    render_device.wgpu_device().create_blas(
        &CreateBlasDescriptor {
            label: Some(&asset_id.to_string()),
            flags,
            update_mode,
        },
        BlasGeometrySizeDescriptors::Triangles {
            descriptors: vec![blas_size.clone()],
        },
    )
}

fn is_mesh_raytracing_compatible(mesh: &Mesh) -> bool {
    let triangle_list = mesh.primitive_topology() == PrimitiveTopology::TriangleList;
    let vertex_attributes = mesh.attributes().map(|(attribute, _)| attribute.id).eq([
        Mesh::ATTRIBUTE_POSITION.id,
        Mesh::ATTRIBUTE_NORMAL.id,
        Mesh::ATTRIBUTE_UV_0.id,
        Mesh::ATTRIBUTE_TANGENT.id,
    ]);
    let indexed_32 = matches!(mesh.indices(), Some(Indices::U32(..)));
    mesh.enable_raytracing && triangle_list && vertex_attributes && indexed_32
}

#[cfg(test)]
#[path = "blas_compaction_tests.rs"]
mod compaction_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_asset::uuid::Uuid;

    fn mesh(n: u128) -> AssetId<Mesh> {
        AssetId::from(Uuid::from_u128(n))
    }

    #[test]
    fn a_new_requirement_rebuilds_only_the_blas_built_the_other_way() {
        let mut book = OpacityBook::default();
        book.record_built(mesh(1), false);
        book.record_built(mesh(2), false);
        book.record_built(mesh(3), true);

        // Mesh 1 now needs alpha testing, mesh 3 no longer does, mesh 2 is unchanged.
        let stale: HashSet<_> = book
            .require(HashSet::from_iter([mesh(1)]))
            .into_iter()
            .collect();
        assert_eq!(stale, HashSet::from_iter([mesh(1), mesh(3)]));
        assert!(book.is_non_opaque(&mesh(1)));
        assert!(!book.is_non_opaque(&mesh(2)));
        assert!(!book.is_non_opaque(&mesh(3)));

        // Once rebuilt the right way, the same requirement asks for nothing.
        book.record_built(mesh(1), true);
        book.record_built(mesh(3), false);
        assert!(book.require(HashSet::from_iter([mesh(1)])).is_empty());
    }

    #[test]
    fn a_mesh_never_built_is_not_a_rebuild() {
        let mut book = OpacityBook::default();
        assert!(book.require(HashSet::from_iter([mesh(9)])).is_empty());
        assert!(book.is_non_opaque(&mesh(9)));
        book.record_built(mesh(9), true);
        book.forget(&mesh(9));
        assert!(
            book.require(HashSet::new()).is_empty(),
            "forgotten meshes are not rebuilt"
        );
    }
}
