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

/* a model the game draws by itself, posed (first_person_weapons.c): shared
with the next frame */
void bevy_bridge_record_model(
	long model_index,
	struct real_matrix4x3 const *node_matrices,
	short node_count);

#endif /* __BEVY_BRIDGE_H */
