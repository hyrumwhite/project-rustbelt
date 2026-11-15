use super::{Pathtracer, prepare::PathtracerAccumulationTexture};
use bevy_camera::Camera;
use bevy_ecs::{
    entity::EntityHashMap,
    system::{Commands, Local, ResMut},
};
use bevy_render::{MainWorld, sync_world::RenderEntity};
use bevy_transform::components::GlobalTransform;

pub fn extract_pathtracer(
    mut main_world: ResMut<MainWorld>,
    mut previous_transforms: Local<EntityHashMap<GlobalTransform>>,
    mut commands: Commands,
) {
    let mut cameras_3d =
        main_world.query::<(RenderEntity, &Camera, &GlobalTransform, Option<&mut Pathtracer>)>();
    for (entity, camera, global_transform, pathtracer) in cameras_3d.iter_mut(&mut main_world) {
        let mut entity_commands = commands
            .get_entity(entity)
            .expect("Camera entity wasn't synced.");
        if let Some(mut main_pathtracer) = pathtracer
            && camera.is_active
        {
            let mut pathtracer = main_pathtracer.clone();
            // Compared by value rather than by change tick: read from this
            // extract system, `Ref<GlobalTransform>::is_changed()` reported a
            // camera that had not moved as changed every frame, so the
            // accumulation never got past one sample.
            let moved =
                previous_transforms.insert(entity, *global_transform) != Some(*global_transform);
            pathtracer.reset |= moved;
            entity_commands.insert(pathtracer);
            // A reset asked for in the main world (a camera cut, a level
            // change) is taken once, as SolarikLighting's is: left set, it
            // cleared the accumulation every frame from then on.
            main_pathtracer.reset = false;
        } else {
            previous_transforms.remove(&entity);
            entity_commands.remove::<(Pathtracer, PathtracerAccumulationTexture)>();
        }
    }
}
