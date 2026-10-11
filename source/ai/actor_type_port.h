/*
ACTOR_TYPE_PORT.H

port: the actor types' handlers (actions.c's) that are still the game's,
through actor_types.c's actor_type_call, as the Rust port's (crates/halo_map's
actor_types) ask for them (the rest it calls directly); included by each
actor_type_*.c after its headers
*/

#ifndef __ACTOR_TYPE_PORT_H
#define __ACTOR_TYPE_PORT_H

#include "halo_sim.h" /* port: port/linux/game/halo_sim.c */

#define actor_action_handle_vehicle_entry(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_vehicle_entry, (actor_index), 0, 0, 0))
#define actor_action_handle_grenade_throwing(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_grenade_throwing, (actor_index), 0, 0, 0))
#define actor_action_consider_grenade(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_consider_grenade, (actor_index), 0, 0, 0))

#endif
