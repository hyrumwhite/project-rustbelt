/*
Scenario object placement layouts (see probe.c). The placement element
structures are defined in the port's tag schema for scenarios.
*/

#include "cseries.h"
#include "tag_schema_scenario.c"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

/* the object blocks and their palettes */
LAYOUT_OFFSET(scenario, object_names)
LAYOUT_OFFSET(scenario, scenery)
LAYOUT_OFFSET(scenario, scenery_palette)
LAYOUT_OFFSET(scenario, bipeds)
LAYOUT_OFFSET(scenario, biped_palette)
LAYOUT_OFFSET(scenario, vehicles)
LAYOUT_OFFSET(scenario, vehicle_palette)
LAYOUT_OFFSET(scenario, equipment)
LAYOUT_OFFSET(scenario, equipment_palette)
LAYOUT_OFFSET(scenario, weapons)
LAYOUT_OFFSET(scenario, weapon_palette)
LAYOUT_OFFSET(scenario, machines)
LAYOUT_OFFSET(scenario, machine_palette)
LAYOUT_OFFSET(scenario, controls)
LAYOUT_OFFSET(scenario, control_palette)
LAYOUT_OFFSET(scenario, light_fixtures)
LAYOUT_OFFSET(scenario, light_fixtures_palette)

LAYOUT_SIZE(scenario_object_palette_entry)
LAYOUT_OFFSET(scenario_object_palette_entry, reference)
LAYOUT_SIZE(scenario_object_datum)
LAYOUT_OFFSET(scenario_object_datum, palette_entry_index)
LAYOUT_OFFSET(scenario_object_datum, name_index)
LAYOUT_OFFSET(scenario_object_datum, placement_flags)
LAYOUT_OFFSET(scenario_object_datum, position)
LAYOUT_OFFSET(scenario_object_datum, rotation)
LAYOUT_OFFSET(scenario_object_datum, on_bsp_flags)
LAYOUT_SIZE(scenario_object_permutation)
LAYOUT_OFFSET(scenario_object_permutation, region_permutations)

/* each object block's element (object_types.c strides them) */
LAYOUT_SIZE(scenery_placement)
LAYOUT_OFFSET(scenery_placement, permutation)
LAYOUT_SIZE(unit_placement)
LAYOUT_OFFSET(unit_placement, permutation)
LAYOUT_SIZE(equipment_placement)
LAYOUT_SIZE(scenario_weapon_datum)
LAYOUT_OFFSET(scenario_weapon_datum, permutation)
LAYOUT_SIZE(machine_placement)
LAYOUT_SIZE(control_placement)
LAYOUT_SIZE(light_fixture_placement)
LAYOUT_SIZE(scenario_light_fixture_datum)
LAYOUT_OFFSET(scenario_light_fixture_datum, color)
LAYOUT_OFFSET(scenario_light_fixture_datum, intensity)
LAYOUT_OFFSET(scenario_light_fixture_datum, falloff_angle)
LAYOUT_OFFSET(scenario_light_fixture_datum, cutoff_angle)
