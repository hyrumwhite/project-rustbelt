/*
Device layouts (see probe.c): what a device's update reads of its definition
(devices.c's device_update, device_machines.c's machine_update).
*/

#include "cseries.h"
#include "tag_files/tag_files.h"
#include "devices/device_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

/* devices: their power and position's motion, and its effects */
LAYOUT_OFFSET(_device_definition, positive_start_effect)
LAYOUT_OFFSET(_device_definition, negative_start_effect)
LAYOUT_OFFSET(_device_definition, positive_stop_effect)
LAYOUT_OFFSET(_device_definition, negative_stop_effect)
LAYOUT_OFFSET(_device_definition, delay_effect)
LAYOUT_OFFSET(_device_definition, automatic_activation_radius)
LAYOUT_OFFSET(_device_definition, runtime_maximum_power_acceleration)
LAYOUT_OFFSET(_device_definition, runtime_maximum_power_velocity)
LAYOUT_OFFSET(_device_definition, runtime_maximum_depowered_position_acceleration)
LAYOUT_OFFSET(_device_definition, runtime_maximum_depowered_position_velocity)
LAYOUT_OFFSET(_device_definition, runtime_maximum_powered_position_acceleration)
LAYOUT_OFFSET(_device_definition, runtime_maximum_powered_position_velocity)
LAYOUT_OFFSET(_device_definition, runtime_delay_ticks)

/* machines: doors, platforms and elevators */
LAYOUT_OFFSET(machine_definition, machine)
LAYOUT_OFFSET(_machine_definition, type)
LAYOUT_OFFSET(_machine_definition, flags)
LAYOUT_OFFSET(_machine_definition, elevator_node_index)
LAYOUT_OFFSET(_machine_definition, runtime_door_open_ticks)
