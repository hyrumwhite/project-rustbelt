/*
BEVY_BRIDGE.H

The world, each frame, for a Bevy renderer (bevy_bridge.c).
*/

#ifndef __BEVY_BRIDGE_H
#define __BEVY_BRIDGE_H
#pragma once

struct render_window;
struct real_matrix4x3;

/* render_frame, once a frame: the windows it draws */
void bevy_bridge_export_frame(
	struct render_window const *windows,
	short window_count);

/* a model the game draws by itself, posed and coloured (its change colours,
or NULL; first_person_weapons.c): shared with the next frame */
void bevy_bridge_record_model(
	long model_index,
	struct real_matrix4x3 const *node_matrices,
	short node_count,
	real_rgb_color const *change_colors,
	real active_camouflage);

/* sound_manager.c: the first player's listener, which the renderer may
move to its own camera; TRUE if it did */
boolean bevy_bridge_listener(
	real_matrix4x3 *matrix);

/* with game.bevy_bridge_overlay on: TRUE (the renderer draws the opaque
world itself, so render_window draws it only into the depth buffer) */
boolean bevy_bridge_overlay_enabled(void);

/* the overlay buffer to fill with a width x height picture
(bevy_bridge_protocol.h), or NULL; bevy_bridge_overlay_commit shares it */
unsigned char *bevy_bridge_overlay_begin(
	unsigned long width,
	unsigned long height);
void bevy_bridge_overlay_commit(void);

#endif /* __BEVY_BRIDGE_H */
