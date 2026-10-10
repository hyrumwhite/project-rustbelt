/*
Item layouts (see probe.c): what an item's update reads of its definition and
of its model's markers (items.c's item_update, models.c's
model_get_marker_by_name).
*/

#include "cseries.h"
#include "tag_files/tag_files.h"
#include "items/item_definitions.h"
#include "models/model_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

/* items */
LAYOUT_OFFSET(item_definition, item)
LAYOUT_OFFSET(_item_definition, flags)
LAYOUT_OFFSET(_item_definition, material_effects)
LAYOUT_OFFSET(_item_definition, collision_sound)
LAYOUT_OFFSET(_item_definition, detonation_effect)

/* a model's markers */
LAYOUT_SIZE(model_marker)
LAYOUT_OFFSET(model_marker, name)
LAYOUT_OFFSET(model_marker, instances)
LAYOUT_SIZE(model_marker_instance)
LAYOUT_OFFSET(model_marker_instance, region_index)
LAYOUT_OFFSET(model_marker_instance, permutation_index)
LAYOUT_OFFSET(model_marker_instance, node_index)
LAYOUT_OFFSET(model_marker_instance, translation)
LAYOUT_OFFSET(model_marker_instance, rotation)
