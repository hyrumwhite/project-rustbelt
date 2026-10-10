/*
Model and object layouts (see probe.c). The model part structures are
defined in the port's tag schema for models.
*/

#include "cseries.h"
#include "tag_schema_models.c"
#include "physics/collision_bsp_definitions.h"
#include "physics/collision_model_definitions.h"
#include "items/projectile_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

/* objects: every object group's definition starts with this */
LAYOUT_SIZE(_object_definition)
LAYOUT_OFFSET(_object_definition, type)
LAYOUT_OFFSET(_object_definition, model)
LAYOUT_OFFSET(_object_definition, collision_model)
LAYOUT_OFFSET(_object_definition, animation_graph)
LAYOUT_OFFSET(_object_definition, modifier_shader)
LAYOUT_OFFSET(_object_definition, attachments)
LAYOUT_OFFSET(_object_definition, change_colors)
LAYOUT_OFFSET(_object_definition, origin_offset)
LAYOUT_OFFSET(_object_definition, physics)
LAYOUT_OFFSET(_object_definition, functions)

/* models */
LAYOUT_SIZE(model)
LAYOUT_OFFSET(model, flags)
LAYOUT_OFFSET(model, node_list_checksum)
LAYOUT_OFFSET(model, base_map_scale)
LAYOUT_OFFSET(model, markers)
LAYOUT_OFFSET(model, nodes)
LAYOUT_OFFSET(model, regions)
LAYOUT_OFFSET(model, geometries)
LAYOUT_OFFSET(model, shaders)
LAYOUT_SIZE(model_node)
LAYOUT_OFFSET(model_node, name)
LAYOUT_OFFSET(model_node, next_sibling_node_index)
LAYOUT_OFFSET(model_node, first_child_node_index)
LAYOUT_OFFSET(model_node, parent_node_index)
LAYOUT_OFFSET(model_node, default_translation)
LAYOUT_OFFSET(model_node, default_rotation)
LAYOUT_OFFSET(model_node, runtime_default_inverse_matrix)
LAYOUT_SIZE(model_region)
LAYOUT_OFFSET(model_region, permutations)
LAYOUT_SIZE(model_region_permutation)
LAYOUT_OFFSET(model_region_permutation, flags)
LAYOUT_OFFSET(model_region_permutation, geometry_indices)
LAYOUT_SIZE(model_geometry)
LAYOUT_OFFSET(model_geometry, parts)
LAYOUT_SIZE(model_geometry_part)
LAYOUT_OFFSET(model_geometry_part, flags)
LAYOUT_OFFSET(model_geometry_part, shader_index)
LAYOUT_OFFSET(model_geometry_part, triangle_buffer)
LAYOUT_OFFSET(model_geometry_part, vertex_buffer)
LAYOUT_SIZE(model_shader_reference)
LAYOUT_OFFSET(model_shader_reference, shader)
LAYOUT_OFFSET(model_shader_reference, permutation_index)
LAYOUT_SIZE(triangle_buffer)
LAYOUT_OFFSET(triangle_buffer, type)
LAYOUT_OFFSET(triangle_buffer, count)
LAYOUT_OFFSET(triangle_buffer, base_address)
LAYOUT_OFFSET(triangle_buffer, hardware_format)

/* collision models (collision_models.c) */
LAYOUT_SIZE(collision_model)
LAYOUT_OFFSET(collision_model, nodes)
LAYOUT_OFFSET(collision_model, resistance)
LAYOUT_OFFSET(damage_resistance, materials)
LAYOUT_SIZE(damage_resistance_material)
LAYOUT_OFFSET(damage_resistance_material, material_type)
LAYOUT_SIZE(collision_node)
LAYOUT_OFFSET(collision_node, region_index)
LAYOUT_OFFSET(collision_node, bsps)

/* projectiles (projectiles.c) */
LAYOUT_OFFSET(projectile_definition, projectile)
LAYOUT_OFFSET(_projectile_definition, flags)
LAYOUT_OFFSET(_projectile_definition, detonation_timer_starts)
LAYOUT_OFFSET(_projectile_definition, collision_radius)
LAYOUT_OFFSET(_projectile_definition, arming_time)
LAYOUT_OFFSET(_projectile_definition, timer_lower_bound)
LAYOUT_OFFSET(_projectile_definition, timer_upper_bound)
LAYOUT_OFFSET(_projectile_definition, minimum_velocity)
LAYOUT_OFFSET(_projectile_definition, maximum_range)
LAYOUT_OFFSET(_projectile_definition, air_gravity_scale)
LAYOUT_OFFSET(_projectile_definition, air_damage_range_lower_bound)
LAYOUT_OFFSET(_projectile_definition, air_damage_range_upper_bound)
LAYOUT_OFFSET(_projectile_definition, water_gravity_scale)
LAYOUT_OFFSET(_projectile_definition, water_damage_range_lower_bound)
LAYOUT_OFFSET(_projectile_definition, water_damage_range_upper_bound)
LAYOUT_OFFSET(_projectile_definition, initial_velocity)
LAYOUT_OFFSET(_projectile_definition, final_velocity)
LAYOUT_OFFSET(_projectile_definition, guided_angular_velocity)
LAYOUT_OFFSET(_projectile_definition, impact_damage)
LAYOUT_OFFSET(_projectile_definition, material_responses)
LAYOUT_SIZE(projectile_material_response_definition)
LAYOUT_OFFSET(projectile_material_response_definition, flags)
LAYOUT_OFFSET(projectile_material_response_definition, default_response)
LAYOUT_OFFSET(projectile_material_response_definition, potential_response)
LAYOUT_OFFSET(projectile_material_response_definition, potential_flags)
LAYOUT_OFFSET(projectile_material_response_definition, potential_skip_fraction)
LAYOUT_OFFSET(projectile_material_response_definition, potential_angle_lower_bound)
LAYOUT_OFFSET(projectile_material_response_definition, potential_angle_upper_bound)
LAYOUT_OFFSET(projectile_material_response_definition, potential_velocity_lower_bound)
LAYOUT_OFFSET(projectile_material_response_definition, potential_velocity_upper_bound)
LAYOUT_OFFSET(projectile_material_response_definition, scale_effects_by)
LAYOUT_OFFSET(projectile_material_response_definition, angular_noise)
LAYOUT_OFFSET(projectile_material_response_definition, velocity_noise)
LAYOUT_OFFSET(projectile_material_response_definition, initial_friction)
LAYOUT_OFFSET(projectile_material_response_definition, maximum_distance)
LAYOUT_OFFSET(projectile_material_response_definition, parallel_friction)
LAYOUT_OFFSET(projectile_material_response_definition, perpendicular_friction)
