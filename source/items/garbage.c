/*
GARBAGE.C

symbols in this file:
000E6010 0040:
	_garbage_update (0000)
000E6050 0050:
	_garbage_new (0000)
*/

/* ---------- headers */

#include "cseries.h"
#include "garbage.h"
#include "halo_sim.h" /* port: port/linux/game/halo_sim.c */

#include "items/items.h"
#include "math/real_math.h"
#include "objects/object_types.h"
#include "objects/objects.h"

/* ---------- constants */

/* ---------- macros */

/* ---------- structures */

/* ---------- prototypes */

/* ---------- globals */

/* ---------- public code */

boolean garbage_update(
	long garbage_index)
{
	struct garbage_datum *garbage = garbage_get(garbage_index);
	boolean active;

	/* port: the Rust port's in place of the game's (port/linux/game/halo_sim.c) */
	if (halo_sim_garbage_lifetime(&garbage->lifetime_ticks, &active))
	{
		if (!active)
			object_delete(garbage_index);
		return active;
	}
	--garbage->lifetime_ticks;
	active = garbage->lifetime_ticks > 0;
	halo_sim_garbage_lifetime_check(garbage->lifetime_ticks, active);

	if (!active)
		object_delete(garbage_index);

	return active;
}

boolean garbage_new(
	long garbage_index)
{
	struct garbage_datum *garbage = garbage_get(garbage_index);

	object_set_garbage(garbage_index, TRUE);
	garbage->object.flags |= FLAG(_object_shadowless_bit) | FLAG(_object_deleted_when_deactivated_bit);
	garbage->lifetime_ticks = random_range(10 * TICKS_PER_SECOND, 20 * TICKS_PER_SECOND);

	return TRUE;
}

/* ---------- private code */
