/*
DEVICE_MACHINES.C

symbols in this file:
00084100 0010:
	_machines_initialize (0000)
00084110 0010:
	_machines_dispose (0000)
00084120 0010:
	_machines_initialize_for_new_map (0000)
00084130 0010:
	_machines_dispose_from_old_map (0000)
00084140 0080:
	_machine_place (0000)
000841C0 0080:
	_machine_new (0000)
00084240 0010:
	_machine_delete (0000)
00084250 0030:
	_machine_bumped (0000)
00084280 0050:
	_machine_try_to_open_with_damage (0000)
000842D0 0400:
	_machine_update (0000)
*/

/* ---------- headers */

#include "cseries.h"

#include "device_machines.h"
#include "game/game.h"
#include "game/game_allegiance.h"
#include "memory/data.h"
#include "units/bipeds.h"
#include "units/unit_definitions.h"
#include "halo_sim.h" /* port: port/linux/game/halo_sim.c */

/* ---------- constants */

enum
{
	_machine_type_door = 0,
	_machine_type_platform = 2,
};

enum
{
	_device_position_changed_bit = 2,
};

enum
{
	_object_function2_active_bit = 2,
	_object_runtime_unit_cannot_open_doors_automatically_bit = 14,
};

/* ---------- macros */

/* ---------- structures */

struct device_group_datum
{
	short identifier;
	word pad;
	real actual_value;
};

/* ---------- prototypes */

/* ---------- globals */

/* ---------- public code */

void machines_initialize(
	void)
{
	return;
}

void machines_dispose(
	void)
{
	return;
}

void machines_initialize_for_new_map(
	void)
{
	return;
}

void machines_dispose_from_old_map(
	void)
{
	return;
}

void machine_place(
	long machine_index,
	struct scenario_machine_datum *scenario_machine)
{
	struct machine_datum *machine = machine_get(machine_index);

	device_add_scenario_information(machine_index, &scenario_machine->device);
	if (TEST_FLAG(scenario_machine->flags, _machine_does_not_operate_automatically_bit))
	{
		machine->machine.flags |= FLAG(_machine_does_not_operate_automatically_bit);
	}
	if (TEST_FLAG(scenario_machine->flags, _machine_one_sided_bit))
	{
		machine->machine.flags |= FLAG(_machine_one_sided_bit);
	}
	if (TEST_FLAG(scenario_machine->flags, _machine_never_appears_locked_bit))
	{
		machine->machine.flags |= FLAG(_machine_never_appears_locked_bit);
	}
	if (TEST_FLAG(scenario_machine->flags, _machine_opened_by_melee_attack_bit))
	{
		machine->machine.flags |= FLAG(_machine_opened_by_melee_attack_bit);
	}

	return;
}

boolean machine_new(
	long machine_index)
{
	struct machine_datum *machine = machine_get(machine_index);
	struct machine_definition *definition = machine_definition_get(machine->definition_index);

	machine->object.flags |= FLAG(13);
	if (TEST_FLAG(definition->machine.flags, _machine_is_elevator_bit))
	{
		machine->object.flags |= FLAG(14);
	}
	else
	{
		machine->object.flags &= ~FLAG(14);
	}

	if (TEST_FLAG(definition->machine.flags, _machine_is_elevator_bit))
	{
		machine->object.flags |= FLAG(15);
	}
	else
	{
		machine->object.flags &= ~FLAG(15);
	}

	return TRUE;
}

void machine_delete(
	long machine_index)
{
	return;
}

void machine_bumped(
	long machine_index,
	long biped_index)
{
	struct machine_datum *machine = machine_get(machine_index);

	machine_definition_get(machine->definition_index);

	return;
}

void machine_try_to_open_with_damage(
	long machine_index)
{
	struct machine_datum *machine = machine_get(machine_index);

	machine_definition_get(machine->definition_index);
	if (TEST_FLAG(machine->machine.flags, _machine_opened_by_melee_attack_bit))
	{
		device_set_actual_position(machine_index, 1.0f);
	}

	return;
}

long device_call(long device_index, long kind, long object_index, long a, long b, real const *f);
short device_bipeds_in_sphere(long machine_index, struct location const *location, real_point3d const *center,
	real radius, long *object_indices, short maximum_count);

static boolean machine_update_internal(
	long machine_index)
{
	boolean activate;
	struct machine_definition *definition;
	long *object_index;
	real_point3d *node_position;
	long *elevator_object_index;
	short object_count;
	real_vector3d elevator_offset;
	real_point3d new_position;
	long object_indices[16];
	long elevator_object_indices[2048];
	struct machine_datum *machine = machine_get(machine_index);

	definition = machine_definition_get(machine->definition_index);
	if (definition->machine.type == _machine_type_platform)
	{
		machine->device.position +=
			(1.0f - machine->device.power) * definition->device.runtime_maximum_depowered_position_velocity +
			machine->device.power * definition->device.runtime_maximum_powered_position_velocity;
		if (machine->device.position >= 1.0f)
		{
			machine->device.position -= 1.0f;
		}
		machine->device.position_velocity = 0.0f;
		machine->device.flags |= FLAG(_device_position_changed_bit);
		if (machine->device.position_group_index != NONE)
		{
			real value[3];

			value[0] = machine->device.position;
			value[1] = 0.f;
			value[2] = 0.f;
			device_call(machine_index, _device_call_set_group_actual, machine_index, machine->device.position_group_index, 0,
				value);
		}
	}

	if (!TEST_FLAG(machine->machine.flags, _machine_does_not_operate_automatically_bit) &&
		definition->machine.type == _machine_type_door &&
		((game_time_get() + machine_index) & 3) == 0)
	{
		real automatic_activation_radius;

		activate = FALSE;
		automatic_activation_radius = definition->device.automatic_activation_radius < _real_epsilon
			? machine->object.bounding_sphere_radius
			: definition->device.automatic_activation_radius;
		object_count = device_bipeds_in_sphere(
			machine_index,
			&machine->object.location,
			&machine->object.bounding_sphere_center,
			automatic_activation_radius,
			object_indices,
			NUMBEROF(object_indices));
		if (object_count > 0)
		{
			long remaining_object_count;

			object_index = object_indices;
			remaining_object_count = (unsigned short)object_count;
			do
			{
				struct unit_datum *unit = unit_get(*object_index);
				struct unit_definition *unit_definition = unit_definition_get(unit->definition_index);
				boolean can_open = TRUE;

				if (TEST_FLAG(*((byte *)unit + 0xb6), _object_function2_active_bit) ||
					TEST_FLAG(*(unsigned long *)((byte *)unit_definition + 0x17c), _object_runtime_unit_cannot_open_doors_automatically_bit))
				{
					can_open = FALSE;
				}

				if (!(TEST_FLAG(machine->machine.flags, _machine_one_sided_bit) &&
					machine->device.position == 0.0f &&
					!device_call(machine_index, _device_call_team_is_enemy, NONE, _game_team_player,
						unit->object.owner_team_index, NULL) &&
					dot_product3d(
						vector_from_points3d(
							&machine->object.bounding_sphere_center,
							&unit->object.bounding_sphere_center,
							&elevator_offset),
						&machine->object.forward) > 0.0f))
				{
					if (can_open)
					{
						activate = TRUE;
					}
				}

				object_index++;
			}
			while (--remaining_object_count);
		}

		if (activate)
		{
			if (machine->device.position_group_index != NONE)
			{
				real value[3] = { 1.0f, 0.f, 0.f };

				device_call(machine_index, _device_call_set_group_desired, machine_index,
					machine->device.position_group_index, 0, value);
			}
			machine->machine.door_open_ticks = -3;
		}
	}

	if (definition->machine.type == _machine_type_door)
	{
		if (machine->device.position == 1.0f)
		{
			machine->machine.door_open_ticks++;
			if (machine->machine.door_open_ticks > definition->machine.runtime_door_open_ticks &&
				machine->device.position_group_index != NONE)
			{
				real value[3] = { 0.0f, 0.f, 0.f };

				device_call(machine_index, _device_call_set_group_desired, machine_index,
					machine->device.position_group_index, 0, value);
			}
		}
		else
		{
			machine->machine.door_open_ticks = 0;
		}
	}

	if (TEST_FLAG(definition->machine.flags, _machine_is_elevator_bit) &&
		definition->machine.elevator_node_index != NONE)
	{
		real_matrix4x3 *node_matrix = object_get_node_matrix(
			machine_index,
			(unsigned short)definition->machine.elevator_node_index);

		elevator_offset.i = node_matrix->position.x - machine->machine.elevator_position.x;
		node_position = &node_matrix->position;
		elevator_offset.j = node_position->y - machine->machine.elevator_position.y;
		elevator_offset.k = node_position->z - machine->machine.elevator_position.z;
		if (elevator_offset.i != 0.0f || elevator_offset.j != 0.0f || elevator_offset.k != 0.0f)
		{
			object_count = device_bipeds_in_sphere(
				machine_index,
				&machine->object.location,
				&machine->object.bounding_sphere_center,
				machine->object.bounding_sphere_radius,
				elevator_object_indices,
				NUMBEROF(elevator_object_indices));
			if (object_count > 0)
			{
				long remaining_object_count;

				elevator_object_index = elevator_object_indices;
				remaining_object_count = (unsigned short)object_count;
				do
				{
					long biped_index = *elevator_object_index;
					struct biped_datum *biped = biped_get(biped_index);

					if (biped->biped.elevator_object_index == machine_index)
					{
						add_vectors3d(&elevator_offset, (real_vector3d const *)&biped->object.position, (real_vector3d *)&new_position);
						device_call(machine_index, _device_call_translate, biped_index, 0, 0, &new_position.x);
					}

					elevator_object_index++;
				}
				while (--remaining_object_count);
			}
		}
		machine->machine.elevator_position = *node_position;
	}

	if (TEST_FLAG(machine->device.flags, _device_position_changed_bit))
	{
		device_call(machine_index, _device_call_translate, machine_index, 0, 0, &machine->object.position.x);
		machine->device.flags &= ~FLAG(_device_position_changed_bit);
	}

	return TRUE;
}

/* ---------- private code */

/* ---------- port: machine updates' Rust port (halo_sim.c) */

boolean machine_update(
	long machine_index)
{
	boolean changed;

	/* the Rust port's in place of the game's, or (in check mode) the state
	it is checked from */
	if (halo_sim_device_update(machine_index, 1, &changed))
		return changed;
	changed = machine_update_internal(machine_index);
	halo_sim_device_update_check(machine_index, 1, changed);

	return changed;
}
