/*
ACTOR_TYPE_PORT.H

port: the actor types' handlers (actions.c's) through actor_types.c's
actor_type_call, as the Rust port's (crates/halo_map's actor_types) ask for
them; included by each actor_type_*.c after its headers
*/

#ifndef __ACTOR_TYPE_PORT_H
#define __ACTOR_TYPE_PORT_H

#include "halo_sim.h" /* port: port/linux/game/halo_sim.c */

#define actor_action_handle_initial_action(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_initial_action, (actor_index), 0, 0, 0))
#define actor_action_handle_pending_command_list(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_pending_command_list, (actor_index), 0, 0, 0))
#define actor_action_handle_surprise(actor_index, a) \
	((boolean)actor_type_port_handle(_actor_type_call_surprise, (actor_index), (long)(a), 0, 0))
#define actor_action_deny_transition(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_deny_transition, (actor_index), 0, 0, 0))
#define actor_action_handle_panic_from_surprise(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_panic_from_surprise, (actor_index), 0, 0, 0))
#define actor_action_handle_panic_from_damage(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_panic_from_damage, (actor_index), 0, 0, 0))
#define actor_action_handle_panic_from_attached_projectiles(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_panic_from_attached_projectiles, (actor_index), 0, 0, 0))
#define actor_action_handle_panic_from_attached_melee_attackers(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_panic_from_attached_melee_attackers, (actor_index), 0, 0, 0))
#define actor_action_handle_panic_from_burning_to_death(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_panic_from_burning_to_death, (actor_index), 0, 0, 0))
#define actor_action_handle_panic_transition(actor_index, a, b, c) \
	((boolean)actor_type_port_handle(_actor_type_call_panic_transition, (actor_index), (long)(a), (long)(b), (long)(c)))
#define actor_action_handle_combat_transition(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_combat_transition, (actor_index), 0, 0, 0))
#define actor_action_handle_vehicle_entry(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_vehicle_entry, (actor_index), 0, 0, 0))
#define actor_action_handle_vehicle_exit(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_vehicle_exit, (actor_index), 0, 0, 0))
#define actor_action_handle_grenade_throwing(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_grenade_throwing, (actor_index), 0, 0, 0))
#define actor_action_handle_danger_avoidance(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_danger_avoidance, (actor_index), 0, 0, 0))
#define actor_action_handle_combat_status(actor_index, a, b) \
	((boolean)actor_type_port_handle(_actor_type_call_combat_status, (actor_index), (long)(a), (long)(b), 0))
#define actor_action_handle_combat_failure(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_combat_failure, (actor_index), 0, 0, 0))
#define actor_action_handle_evasion(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_evasion, (actor_index), 0, 0, 0))
#define actor_action_can_stop_guarding(actor_index, a, b) \
	((boolean)actor_type_port_handle(_actor_type_call_can_stop_guarding, (actor_index), (long)(a), (long)(b), 0))
#define actor_action_handle_done_fleeing(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_done_fleeing, (actor_index), 0, 0, 0))
#define actor_action_consider_grenade(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_consider_grenade, (actor_index), 0, 0, 0))
#define actor_action_handle_exit_pursuit(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_exit_pursuit, (actor_index), 0, 0, 0))
#define actor_action_can_stop_conversing(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_can_stop_conversing, (actor_index), 0, 0, 0))
#define actor_action_handle_berserking_from_attacking_mode(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_berserking_from_attacking_mode, (actor_index), 0, 0, 0))
#define actor_action_handle_berserking_from_damage(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_berserking_from_damage, (actor_index), 0, 0, 0))
#define actor_action_handle_berserking_from_proximity(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_berserking_from_proximity, (actor_index), 0, 0, 0))
#define actor_action_handle_berserk_transition(actor_index, a) \
	((boolean)actor_type_port_handle(_actor_type_call_berserk_transition, (actor_index), (long)(a), 0, 0))
#define actor_action_handle_berserking_from_attached_projectiles(actor_index) \
	((boolean)actor_type_port_handle(_actor_type_call_berserking_from_attached_projectiles, (actor_index), 0, 0, 0))
#define actor_action_handle_active_cover_seeking(actor_index, a, b) \
	((boolean)actor_type_port_handle(_actor_type_call_active_cover_seeking, (actor_index), (long)(a), (long)(b), 0))

#endif
