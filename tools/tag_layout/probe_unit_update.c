/*
Unit, weapon and animation layouts (see probe.c): what a unit's update
reads (units.c's unit_update), and a biped's footsteps.
*/

#include "cseries.h"
#include "units/unit_definitions.h"
#include "items/weapon_definitions.h"
#include "models/model_animation_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

/* units */
LAYOUT_OFFSET(unit_definition, unit)
LAYOUT_OFFSET(_unit_definition, flags)
LAYOUT_OFFSET(_unit_definition, constant_sound)
LAYOUT_OFFSET(_unit_definition, aiming_velocity_maximum)
LAYOUT_OFFSET(_unit_definition, aiming_acceleration_maximum)
LAYOUT_OFFSET(_unit_definition, casual_aiming_modifier)
LAYOUT_OFFSET(_unit_definition, looking_velocity_maximum)
LAYOUT_OFFSET(_unit_definition, looking_acceleration_maximum)
LAYOUT_OFFSET(_unit_definition, powered_seats)
LAYOUT_SIZE(powered_seat_definition)
LAYOUT_OFFSET(powered_seat_definition, powerup_time)
LAYOUT_OFFSET(powered_seat_definition, powerdown_time)

/* weapons: what a camouflaged unit's firing regrows */
LAYOUT_OFFSET(_weapon_definition, active_camo_regrowth_rate)

/* animations: a walk's footsteps */
LAYOUT_OFFSET(animation, private_left_foot_frame_index)
LAYOUT_OFFSET(animation, private_right_foot_frame_index)
