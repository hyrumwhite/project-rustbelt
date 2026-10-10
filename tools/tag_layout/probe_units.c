/*
Unit, biped, game globals and animation layouts (see probe.c): what biped
movement reads (bipeds.c's biped_update_moving).
*/

#include "cseries.h"
#include "units/biped_definitions.h"
#include "game/game_globals.h"
#include "models/model_animation_definitions.h"
#include "physics/physics_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

/* bipeds */
LAYOUT_OFFSET(biped_definition, unit)
LAYOUT_OFFSET(biped_definition, biped)
LAYOUT_OFFSET(_unit_definition, flags)
LAYOUT_OFFSET(_unit_definition, stunned_movement_threshold)
LAYOUT_OFFSET(_biped_definition, flags)
LAYOUT_OFFSET(_biped_definition, moving_turning_speed)
LAYOUT_OFFSET(_biped_definition, flying_bank_apply_time)
LAYOUT_OFFSET(_biped_definition, flying_bank_decay_time)
LAYOUT_OFFSET(_biped_definition, flying_pitch_amount)
LAYOUT_OFFSET(_biped_definition, flying_angular_velocity)
LAYOUT_OFFSET(_biped_definition, flying_angular_acceleration)
LAYOUT_OFFSET(_biped_definition, runtime_cosine_stationary_turning_threshold)
LAYOUT_OFFSET(_biped_definition, flying_bank_amount)
LAYOUT_OFFSET(_biped_definition, flying_velocity)
LAYOUT_OFFSET(_biped_definition, flying_sidestep_velocity)
LAYOUT_OFFSET(_biped_definition, flying_acceleration)
LAYOUT_OFFSET(_biped_definition, flying_deceleration)
LAYOUT_OFFSET(_biped_definition, flying_crouch_velocity_modifier)
LAYOUT_OFFSET(_biped_definition, downhill_velocity_scale)
LAYOUT_OFFSET(_biped_definition, uphill_velocity_scale)
LAYOUT_OFFSET(_biped_definition, jump_velocity)
LAYOUT_OFFSET(_biped_definition, maximum_soft_landing_time)
LAYOUT_OFFSET(_biped_definition, maximum_hard_landing_time)
LAYOUT_OFFSET(_biped_definition, minimum_soft_landing_velocity)
LAYOUT_OFFSET(_biped_definition, minimum_hard_landing_velocity)
LAYOUT_OFFSET(_biped_definition, maximum_hard_landing_velocity)
LAYOUT_OFFSET(_biped_definition, collision_height_standing)
LAYOUT_OFFSET(_biped_definition, collision_height_crouching)
LAYOUT_OFFSET(_biped_definition, collision_radius)
LAYOUT_OFFSET(_biped_definition, runtime_crouch_transition_velocity)
LAYOUT_OFFSET(_biped_definition, runtime_minimum_normal_k)
LAYOUT_OFFSET(_biped_definition, runtime_downhill_k0)
LAYOUT_OFFSET(_biped_definition, runtime_downhill_k1)
LAYOUT_OFFSET(_biped_definition, runtime_uphill_k0)
LAYOUT_OFFSET(_biped_definition, runtime_uphill_k1)

/* game globals: the player's movement */
LAYOUT_OFFSET(game_globals, player_information)
LAYOUT_SIZE(game_globals_player_information)
LAYOUT_OFFSET(game_globals_player_information, walking_speed)
LAYOUT_OFFSET(game_globals_player_information, double_speed_multiplier)
LAYOUT_OFFSET(game_globals_player_information, run_forward_speed)
LAYOUT_OFFSET(game_globals_player_information, run_backward_speed)
LAYOUT_OFFSET(game_globals_player_information, run_sideways_speed)
LAYOUT_OFFSET(game_globals_player_information, run_acceleration)
LAYOUT_OFFSET(game_globals_player_information, sneak_forward_speed)
LAYOUT_OFFSET(game_globals_player_information, sneak_backward_speed)
LAYOUT_OFFSET(game_globals_player_information, sneak_sideways_speed)
LAYOUT_OFFSET(game_globals_player_information, sneak_acceleration)
LAYOUT_OFFSET(game_globals_player_information, airborne_acceleration)
LAYOUT_OFFSET(game_globals_player_information, stun_movement_penalty)
LAYOUT_OFFSET(game_globals_player_information, stun_turning_penalty)

/* animations: each frame's root motion */
LAYOUT_OFFSET(animation_graph, animations)
LAYOUT_OFFSET(animation_graph, unit_seats)
LAYOUT_SIZE(animation)
LAYOUT_OFFSET(animation, frame_count)
LAYOUT_OFFSET(animation, frame_info_type)
LAYOUT_OFFSET(animation, frame_info)
LAYOUT_OFFSET(animation, private_loop_frame_index)
LAYOUT_OFFSET(animation, private_key_frame_index)
LAYOUT_OFFSET(animation, private_second_key_frame_index)
LAYOUT_OFFSET(animation, next_animation_index)
LAYOUT_OFFSET(animation, sound_index)
LAYOUT_OFFSET(animation, private_sound_frame_index)
LAYOUT_OFFSET(animation, runtime_parent_animation_index)
LAYOUT_OFFSET(animation, runtime_normalized_weight)
LAYOUT_OFFSET(animation, type)
LAYOUT_OFFSET(animation, frame_size)
LAYOUT_OFFSET(animation, node_list_checksum)
LAYOUT_OFFSET(animation, node_count)
LAYOUT_OFFSET(animation, flags)
LAYOUT_OFFSET(animation, nodes_with_translation_flags)
LAYOUT_OFFSET(animation, nodes_with_rotation_flags)
LAYOUT_OFFSET(animation, nodes_with_scale_flags)
LAYOUT_OFFSET(animation, compressed_data_offset)
LAYOUT_OFFSET(animation, default_data)
LAYOUT_OFFSET(animation, data)
LAYOUT_OFFSET(animation_graph, object_overlays)

/* physics: an object's centre of mass */
LAYOUT_OFFSET(physics_definition, center_of_mass)

/* animation graphs: which animation a unit plays */
LAYOUT_SIZE(animation_graph_unit_seat)
LAYOUT_OFFSET(animation_graph_unit_seat, label)
LAYOUT_OFFSET(animation_graph_unit_seat, looking_screen_bounds)
LAYOUT_OFFSET(animation_graph_unit_seat, animations)
LAYOUT_OFFSET(animation_graph_unit_seat, weapon_classes)
LAYOUT_SIZE(animation_graph_weapon_class)
LAYOUT_OFFSET(animation_graph_weapon_class, label)
LAYOUT_OFFSET(animation_graph_weapon_class, aiming_screen_bounds)
LAYOUT_OFFSET(animation_graph_weapon_class, animations)
LAYOUT_OFFSET(animation_graph_weapon_class, weapon_types)
LAYOUT_SIZE(animation_graph_weapon_type)
LAYOUT_OFFSET(animation_graph_weapon_type, label)
