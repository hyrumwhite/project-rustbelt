/*
Player effect layouts (see probe.c): what a player's screen flash, camera
impulse, camera shake and continuous shake read of damage effects
(player_effects.c).
*/

#include "cseries.h"
#include "game/game_globals.h"
#include "tag_files/tag_files.h"
#include "tag_files/tag_groups.h"
#include "objects/damage_effect_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

LAYOUT_OFFSET(damage_effect_definition, screen_flash)
LAYOUT_OFFSET(damage_effect_definition, camera_impulse)
LAYOUT_OFFSET(damage_effect_definition, camera_shake)
LAYOUT_OFFSET(damage_effect_definition, sound)
LAYOUT_OFFSET(damage_effect_definition, damage)

LAYOUT_SIZE(screen_flash_definition)
LAYOUT_SIZE(camera_impulse_definition)
LAYOUT_SIZE(camera_shake_definition)

LAYOUT_OFFSET(continuous_damage_effect_definition, falloff_radius)
LAYOUT_OFFSET(continuous_damage_effect_definition, cutoff_radius)
LAYOUT_OFFSET(continuous_damage_effect_definition, vibrate)
LAYOUT_OFFSET(continuous_damage_effect_definition, camera_shake)
LAYOUT_OFFSET(continuous_camera_shake_definition, random_translation_magnitude)
LAYOUT_OFFSET(continuous_camera_shake_definition, random_rotation_magnitude)
LAYOUT_OFFSET(continuous_camera_shake_definition, periodic_function)
LAYOUT_OFFSET(continuous_camera_shake_definition, periodic_period)
LAYOUT_OFFSET(continuous_camera_shake_definition, periodic_weight)
