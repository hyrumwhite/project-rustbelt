/*
BEVY_BRIDGE.H

The world, each frame, for a Bevy renderer (bevy_bridge.c).
*/

#ifndef __BEVY_BRIDGE_H
#define __BEVY_BRIDGE_H
#pragma once

struct render_window;

/* render_frame, once a frame: the windows it draws */
void bevy_bridge_export_frame(
	struct render_window const *windows,
	short window_count);

#endif /* __BEVY_BRIDGE_H */
