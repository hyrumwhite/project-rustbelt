/*
Physics, mass point, material and fog layouts (see probe.c): what a
vehicle's physics reads (physics.c's physics_update, scenario.c's
scenario_location_water_depth).
*/

#include "cseries.h"
#include "physics/physics_definitions.h"
#include "effects/material_effect_definitions.h"
#include "game/game_globals.h"
#include "scenario/fog_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

/* (private to physics/physics.c; copied as it is) */
struct powered_mass_point_definition
{
	char name[32];
	unsigned long flags;
	real antigrav_strength;
	real antigrav_offset;
	real antigrav_height;
	real antigrav_damp_fraction;
	real antigrav_normal_k1;
	real antigrav_normal_k0;
	real unused[17];
};

LAYOUT_OFFSET(physics_definition, radius)
LAYOUT_OFFSET(physics_definition, moment)
LAYOUT_OFFSET(physics_definition, mass)
LAYOUT_OFFSET(physics_definition, gravity_scale)
LAYOUT_OFFSET(physics_definition, ground_friction)
LAYOUT_OFFSET(physics_definition, ground_depth)
LAYOUT_OFFSET(physics_definition, ground_damp_fraction)
LAYOUT_OFFSET(physics_definition, ground_normal_k1)
LAYOUT_OFFSET(physics_definition, ground_normal_k0)
LAYOUT_OFFSET(physics_definition, water_friction)
LAYOUT_OFFSET(physics_definition, water_depth)
LAYOUT_OFFSET(physics_definition, water_density)
LAYOUT_OFFSET(physics_definition, air_friction)
LAYOUT_OFFSET(physics_definition, inertial_matrix)
LAYOUT_OFFSET(physics_definition, powered_mass_points)
LAYOUT_OFFSET(mass_point_definition, powered_mass_point_index)
LAYOUT_OFFSET(mass_point_definition, mass)
LAYOUT_OFFSET(mass_point_definition, density)
LAYOUT_OFFSET(mass_point_definition, forward)
LAYOUT_OFFSET(mass_point_definition, up)
LAYOUT_OFFSET(mass_point_definition, friction_type)
LAYOUT_OFFSET(mass_point_definition, friction_parallel_scale)
LAYOUT_OFFSET(mass_point_definition, friction_perpendicular_scale)
LAYOUT_SIZE(powered_mass_point_definition)
LAYOUT_OFFSET(powered_mass_point_definition, flags)
LAYOUT_OFFSET(powered_mass_point_definition, antigrav_strength)
LAYOUT_OFFSET(powered_mass_point_definition, antigrav_height)
LAYOUT_OFFSET(powered_mass_point_definition, antigrav_damp_fraction)
LAYOUT_OFFSET(powered_mass_point_definition, antigrav_normal_k1)
LAYOUT_OFFSET(powered_mass_point_definition, antigrav_normal_k0)

/* the game globals' materials: their physics scales */
LAYOUT_OFFSET(game_globals, materials)
LAYOUT_SIZE(material_definition)
LAYOUT_OFFSET(material_definition, physics_ground_friction_scale)
LAYOUT_OFFSET(material_definition, physics_ground_friction_normal_k1_scale)
LAYOUT_OFFSET(material_definition, physics_ground_friction_normal_k0_scale)
LAYOUT_OFFSET(material_definition, physics_ground_depth_scale)
LAYOUT_OFFSET(material_definition, physics_ground_damp_fraction_scale)

/* fog: whether it is water */
LAYOUT_OFFSET(fog_definition, flags)
LAYOUT_OFFSET(fog_definition, plane_distance)
