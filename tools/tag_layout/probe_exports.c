/*
Function export layouts (see probe.c): what each object type's export of its
function values reads of its definition (object_type_export_function_values's
parts).
*/

#include "cseries.h"
#include "tag_files/tag_files.h"
#include "objects/object_definitions.h"
#include "units/unit_definitions.h"
#include "units/biped_definitions.h"
#include "items/weapon_definitions.h"
#include "items/projectile_definitions.h"
#include "devices/device_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

LAYOUT_OFFSET(_object_definition, function_modes)
LAYOUT_OFFSET(_unit_definition, function_modes)
LAYOUT_OFFSET(_biped_definition, function_modes)
LAYOUT_OFFSET(_weapon_definition, function_modes)
LAYOUT_OFFSET(_weapon_definition, heat_illumination)
LAYOUT_OFFSET(weapon_trigger_definition, charging_time)
LAYOUT_OFFSET(weapon_trigger_definition, charged_illumination)
LAYOUT_OFFSET(_projectile_definition, function_inputs)
LAYOUT_OFFSET(_device_definition, function_modes)
