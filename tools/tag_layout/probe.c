/*
Struct layouts the Rust map reader needs, measured by the compiler from
OpenCE's own definitions (i686 MSVC ABI, as the game is built).

gen.py compiles this to LLVM IR and reads the constants back; nothing is
linked or run. Add a LAYOUT_* line here and re-run gen.py to expose a new
field to Rust.
*/

#include "cseries.h"
#include "tag_files/tag_groups.h"
#include "structures/structure_bsp_definitions.h"
#include "scenario/scenario_definitions.h"
#include "bitmaps/bitmap_group.h"
#include "models/model_definitions.h"
/* the shader, sky and light structures are defined in the port's tag schema */
#include "tag_schema_render.c"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

/* tag primitives */
LAYOUT_SIZE(tag_block)
LAYOUT_OFFSET(tag_block, count)
LAYOUT_OFFSET(tag_block, address)
LAYOUT_SIZE(tag_reference)
LAYOUT_OFFSET(tag_reference, group_tag)
LAYOUT_OFFSET(tag_reference, name)
LAYOUT_OFFSET(tag_reference, index)
LAYOUT_SIZE(tag_data)
LAYOUT_OFFSET(tag_data, size)
LAYOUT_OFFSET(tag_data, file_offset)
LAYOUT_OFFSET(tag_data, address)

/* scenario */
LAYOUT_SIZE(scenario)
LAYOUT_OFFSET(scenario, sky_references)
LAYOUT_OFFSET(scenario, type)
LAYOUT_OFFSET(scenario, scenery)
LAYOUT_OFFSET(scenario, scenery_palette)
LAYOUT_OFFSET(scenario, players)
LAYOUT_OFFSET(scenario, structure_bsp_references)
LAYOUT_SIZE(player_starting_location)
LAYOUT_OFFSET(player_starting_location, position)
LAYOUT_OFFSET(player_starting_location, facing)
LAYOUT_OFFSET(player_starting_location, structure_bsp_reference_index)
LAYOUT_SIZE(scenario_structure_bsp_reference)
LAYOUT_OFFSET(scenario_structure_bsp_reference, file_offset)
LAYOUT_OFFSET(scenario_structure_bsp_reference, file_size)
LAYOUT_OFFSET(scenario_structure_bsp_reference, base_address)
LAYOUT_OFFSET(scenario_structure_bsp_reference, structure_bsp)

/* structure bsp */
LAYOUT_SIZE(structure_bsp)
LAYOUT_OFFSET(structure_bsp, lightmap_group)
LAYOUT_OFFSET(structure_bsp, world_bounds)
LAYOUT_OFFSET(structure_bsp, surfaces)
LAYOUT_OFFSET(structure_bsp, lightmaps)
LAYOUT_SIZE(structure_lightmap)
LAYOUT_OFFSET(structure_lightmap, bitmap_index)
LAYOUT_OFFSET(structure_lightmap, materials)
LAYOUT_SIZE(structure_material)
LAYOUT_OFFSET(structure_material, shader)
LAYOUT_OFFSET(structure_material, first_surface_index)
LAYOUT_OFFSET(structure_material, surface_count)
LAYOUT_OFFSET(structure_material, vertices)
LAYOUT_OFFSET(structure_material, lightmap_vertices)
LAYOUT_OFFSET(structure_material, uncompressed_vertex_data)
LAYOUT_OFFSET(structure_material, compressed_vertex_data)
LAYOUT_SIZE(structure_surface)
LAYOUT_SIZE(vertex_buffer)
LAYOUT_OFFSET(vertex_buffer, type)
LAYOUT_OFFSET(vertex_buffer, count)
LAYOUT_OFFSET(vertex_buffer, offset)
LAYOUT_OFFSET(vertex_buffer, base_address)
LAYOUT_OFFSET(vertex_buffer, hardware_format)

/* bitmaps */
LAYOUT_OFFSET(bitmap_group, pixel_data)
LAYOUT_OFFSET(bitmap_group, bitmaps)
LAYOUT_SIZE(bitmap_data)
LAYOUT_OFFSET(bitmap_data, signature)
LAYOUT_OFFSET(bitmap_data, width)
LAYOUT_OFFSET(bitmap_data, height)
LAYOUT_OFFSET(bitmap_data, depth)
LAYOUT_OFFSET(bitmap_data, type)
LAYOUT_OFFSET(bitmap_data, format)
LAYOUT_OFFSET(bitmap_data, flags)
LAYOUT_OFFSET(bitmap_data, mipmap_count)
LAYOUT_OFFSET(bitmap_data, pixels_offset)
LAYOUT_OFFSET(bitmap_data, pixels_size)

/* shaders */
LAYOUT_OFFSET(shader_base, radiosity)
LAYOUT_OFFSET(shader_base, type)
LAYOUT_OFFSET(shader_radiosity_properties, flags)
LAYOUT_OFFSET(shader_radiosity_properties, power)
LAYOUT_OFFSET(shader_radiosity_properties, color_of_emitted_light)
LAYOUT_SIZE(shader_environment_definition)
LAYOUT_OFFSET(shader_environment_definition, flags)
LAYOUT_OFFSET(shader_environment_definition, type)
LAYOUT_OFFSET(shader_environment_definition, base_map)
LAYOUT_OFFSET(shader_environment_definition, diffuse_flags)
LAYOUT_OFFSET(shader_environment_definition, detail_map_function)
LAYOUT_OFFSET(shader_environment_definition, micro_detail_map_function)
LAYOUT_OFFSET(shader_environment_definition, micro_detail_map_scale)
LAYOUT_OFFSET(shader_environment_definition, micro_detail_map)
LAYOUT_OFFSET(shader_environment_definition, primary_detail_map_scale)
LAYOUT_OFFSET(shader_environment_definition, primary_detail_map)
LAYOUT_OFFSET(shader_environment_definition, secondary_detail_map_scale)
LAYOUT_OFFSET(shader_environment_definition, secondary_detail_map)
LAYOUT_OFFSET(shader_environment_definition, material_color)
LAYOUT_OFFSET(shader_environment_definition, bump_map_scale)
LAYOUT_OFFSET(shader_environment_definition, bump_map)
LAYOUT_OFFSET(shader_environment_definition, self_illumination_flags)
LAYOUT_OFFSET(shader_environment_definition, primary_on_color)
LAYOUT_OFFSET(shader_environment_definition, secondary_on_color)
LAYOUT_OFFSET(shader_environment_definition, plasma_on_color)
LAYOUT_OFFSET(shader_environment_definition, self_illumination_map_scale)
LAYOUT_OFFSET(shader_environment_definition, self_illumination_map)
LAYOUT_SIZE(shader_model_definition)
LAYOUT_OFFSET(shader_model_definition, flags)
LAYOUT_OFFSET(shader_model_definition, self_illumination_flags)
LAYOUT_OFFSET(shader_model_definition, self_illumination_color_source)
LAYOUT_OFFSET(shader_model_definition, self_illumination_color_lower_bound)
LAYOUT_OFFSET(shader_model_definition, self_illumination_color_upper_bound)
LAYOUT_OFFSET(shader_model_definition, detail_map)
LAYOUT_OFFSET(shader_model_definition, map_u_scale)
LAYOUT_OFFSET(shader_model_definition, map_v_scale)
LAYOUT_OFFSET(shader_model_definition, base_map)
LAYOUT_OFFSET(shader_model_definition, multipurpose_map)

/* sky */
LAYOUT_SIZE(sky_definition)
LAYOUT_OFFSET(sky_definition, model)
LAYOUT_OFFSET(sky_definition, lights)
LAYOUT_SIZE(sky_light)

/* transparent shaders (generic and chicago share `transparent`) */
LAYOUT_OFFSET(shader_transparent_generic_definition, transparent)
LAYOUT_OFFSET(shader_transparent_generic_definition, stages)
LAYOUT_OFFSET(shader_transparent_chicago_definition, transparent)
LAYOUT_OFFSET(shader_transparent, flags)
LAYOUT_OFFSET(shader_transparent, first_map_type)
LAYOUT_OFFSET(shader_transparent, framebuffer_blend_function)
LAYOUT_OFFSET(shader_transparent, extra_layers)
LAYOUT_OFFSET(shader_transparent, maps)
LAYOUT_SIZE(shader_transparent_generic_map)
LAYOUT_OFFSET(shader_transparent_generic_map, map_u_scale)
LAYOUT_OFFSET(shader_transparent_generic_map, map_v_scale)
LAYOUT_OFFSET(shader_transparent_generic_map, map_u_offset)
LAYOUT_OFFSET(shader_transparent_generic_map, map_v_offset)
LAYOUT_OFFSET(shader_transparent_generic_map, map)
LAYOUT_SIZE(shader_transparent_chicago_map)
LAYOUT_OFFSET(shader_transparent_chicago_map, map_u_scale)
LAYOUT_OFFSET(shader_transparent_chicago_map, map_v_scale)
LAYOUT_OFFSET(shader_transparent_chicago_map, map_u_offset)
LAYOUT_OFFSET(shader_transparent_chicago_map, map_v_offset)
LAYOUT_OFFSET(shader_transparent_chicago_map, map)
