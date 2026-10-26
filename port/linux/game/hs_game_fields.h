#ifndef __HS_GAME_FIELDS_H
#define __HS_GAME_FIELDS_H
#pragma once

/* the game's fields in the context's game_fields (hs_runtime.c's
hs_runtime_port_game_fields; crates/halo_map's hs_runtime::game_field) */
enum
{
	_hs_game_field_main_saving_map, /* boolean */
	_hs_game_field_main_save_map_safely, /* boolean */
	_hs_game_field_main_save_map_timeout, /* boolean */
	_hs_game_field_main_ticks_until_next_save_check, /* long */
	_hs_game_field_main_ticks_unable_to_save, /* long */
	_hs_game_field_main_safe_intervals, /* short */
	_hs_game_field_main_won_map, /* boolean */
	_hs_game_field_main_lost_map, /* boolean */
	_hs_game_field_main_loss_timer, /* short */
	_hs_game_field_main_loss_last_tick, /* long */
	_hs_game_field_main_revert_map, /* boolean */
	_hs_game_field_main_reset_map, /* boolean */
	_hs_game_field_main_switch_to_structure_bsp_index, /* short */
	_hs_game_field_main_skip_ticks, /* short */
	_hs_game_field_main_skip, /* boolean */
	_hs_game_field_difficulty_level, /* short: the next map's */
	_hs_game_field_players_input_disabled, /* boolean */
	_hs_game_field_player_control_globals, /* action, action test and suppressed action flags, flags (longs) */
	_hs_game_field_player_control_zoom_level0, /* short, each local player's */
	_hs_game_field_player_control_zoom_level1,
	_hs_game_field_player_control_zoom_level2,
	_hs_game_field_player_control_zoom_level3,
	_hs_game_field_camera_script_timer, /* real */
	_hs_game_field_game_time_speed, /* real */
	_hs_game_field_game_state_revert_time, /* long */
	_hs_game_field_player_effect_scripted, /* translation, rotation, intensity (reals), timer, total (shorts) */
	_hs_game_field_player_effect_flags, /* unsigned long */
	_hs_game_field_cinematic_globals, /* struct cinematic_global_data */
	_hs_game_field_screen_effect_globals, /* struct rasterizer_cinematic_screen_effect_state (0 for none) */
	_hs_game_field_hud_scripted_globals, /* show_hud, show_hud_help_text (booleans) */
	_hs_game_field_unit_hud_script_flags, /* long */
	_hs_game_field_weapon_hud_script_flags, /* long */
	_hs_game_field_hud_messaging_globals, /* struct hud_messaging_globals_definition */
	NUMBER_OF_HS_GAME_FIELDS
};

#endif // __HS_GAME_FIELDS_H
