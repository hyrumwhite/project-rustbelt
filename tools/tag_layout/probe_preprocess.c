/*
Object overlay, vehicle and device layouts (see probe.c): what an object's
overlay animations (objects.c's object_compute_node_matrices) and the
vehicle and device preprocesses of a pose read.
*/

#include "cseries.h"
#include "objects/object_definitions.h"
#include "units/unit_definitions.h"
#include "devices/device_definitions.h"
#include "models/model_animation_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

/* (private to units/vehicles.c and devices/devices.c; copied as they are) */
struct vehicle_definition
{
	struct unit_definition unit;
	unsigned long flags;
	short vehicle_type;
	short pad2f6;
	real unknown2f8;
	real unknown2fc;
	real unknown300;
	real unknown304;
	real unknown308;
	real unknown30c;
	real wheel_circumference;
};

struct vehicle_animation
{
	struct animation_aiming_screen_bounds steering_screen_bounds;
	long unused[0x11];
	struct tag_block animations;
	struct tag_block suspensions;
};

struct vehicle_suspension
{
	short mass_point_index;
	short animation_index;
	real unknown4;
	real unknown8;
	byte unknownc[8];
};

struct animation_graph_device_animations
{
	long unused[21];
	struct tag_block animations;
};

/* object overlays: animations driven by the object's function values */
LAYOUT_OFFSET(_object_definition, functions)
LAYOUT_SIZE(object_function_definition)
LAYOUT_OFFSET(object_function_definition, flags)
LAYOUT_OFFSET(animation_graph, vehicle_animations)
LAYOUT_OFFSET(animation_graph, device_animations)
LAYOUT_SIZE(animation_graph_object_overlay)
LAYOUT_OFFSET(animation_graph_object_overlay, animation_index)
LAYOUT_OFFSET(animation_graph_object_overlay, function_index)
LAYOUT_OFFSET(animation_graph_object_overlay, mode)

/* vehicles: steering, speed and wheel overlays, suspensions */
LAYOUT_OFFSET(vehicle_definition, unknown2f8)
LAYOUT_OFFSET(vehicle_definition, unknown2fc)
LAYOUT_OFFSET(vehicle_definition, wheel_circumference)
LAYOUT_SIZE(vehicle_animation)
LAYOUT_OFFSET(vehicle_animation, steering_screen_bounds)
LAYOUT_OFFSET(vehicle_animation, animations)
LAYOUT_OFFSET(vehicle_animation, suspensions)
LAYOUT_SIZE(vehicle_suspension)
LAYOUT_OFFSET(vehicle_suspension, animation_index)

/* devices: position and power overlays */
LAYOUT_OFFSET(device_definition, device)
LAYOUT_OFFSET(_device_definition, flags)
LAYOUT_SIZE(animation_graph_device_animations)
LAYOUT_OFFSET(animation_graph_device_animations, animations)
