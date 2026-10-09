/*
Model and object layouts (see probe.c). The model part structures are
defined in the port's tag schema for models.
*/

#include "cseries.h"
#include "tag_schema_models.c"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

/* objects: every object group's definition starts with this */
LAYOUT_SIZE(_object_definition)
LAYOUT_OFFSET(_object_definition, type)
LAYOUT_OFFSET(_object_definition, model)
LAYOUT_OFFSET(_object_definition, animation_graph)
LAYOUT_OFFSET(_object_definition, modifier_shader)
LAYOUT_OFFSET(_object_definition, attachments)
LAYOUT_OFFSET(_object_definition, change_colors)

/* models */
LAYOUT_SIZE(model)
LAYOUT_OFFSET(model, flags)
LAYOUT_OFFSET(model, base_map_scale)
LAYOUT_OFFSET(model, markers)
LAYOUT_OFFSET(model, nodes)
LAYOUT_OFFSET(model, regions)
LAYOUT_OFFSET(model, geometries)
LAYOUT_OFFSET(model, shaders)
LAYOUT_SIZE(model_node)
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
