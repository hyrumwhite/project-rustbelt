/*
BEVY_BRIDGE_INPUT.C

Input from a Bevy renderer (port/linux/game/bevy_bridge_protocol.h): the
renderer forwards its window's keyboard and mouse into the shared memory,
and this turns their changes into SDL events, pushed just before the
platform layer polls SDL (sdl_platform.c), so the game handles them as its
own window's: bindings, menus, typing and the aim all work unchanged. When
the renderer stops forwarding, everything it held is released.
*/

#include "../game/bevy_bridge_protocol.h"

#include <SDL3/SDL.h>
#include <string.h>

static struct bevy_bridge_input bevy_bridge_last;
/* the game's window, which the events are for */
static SDL_WindowID bevy_bridge_window;

static SDL_Keymod bevy_bridge_modifiers(const struct bevy_bridge_input *input)
{
	SDL_Keymod modifiers = SDL_KMOD_NONE;
#define HELD(scancode) ((input->keys[(scancode) / 32] >> ((scancode) % 32)) & 1)

	if (HELD(SDL_SCANCODE_LSHIFT))
		modifiers |= SDL_KMOD_LSHIFT;
	if (HELD(SDL_SCANCODE_RSHIFT))
		modifiers |= SDL_KMOD_RSHIFT;
	if (HELD(SDL_SCANCODE_LCTRL))
		modifiers |= SDL_KMOD_LCTRL;
	if (HELD(SDL_SCANCODE_RCTRL))
		modifiers |= SDL_KMOD_RCTRL;
	if (HELD(SDL_SCANCODE_LALT))
		modifiers |= SDL_KMOD_LALT;
	if (HELD(SDL_SCANCODE_RALT))
		modifiers |= SDL_KMOD_RALT;
#undef HELD
	return modifiers;
}

static void bevy_bridge_push_key(int scancode, bool down, SDL_Keymod modifiers)
{
	SDL_Event event;

	memset(&event, 0, sizeof(event));
	event.type = down ? SDL_EVENT_KEY_DOWN : SDL_EVENT_KEY_UP;
	event.key.timestamp = SDL_GetTicksNS();
	event.key.windowID = bevy_bridge_window;
	event.key.scancode = (SDL_Scancode)scancode;
	event.key.key = SDL_GetKeyFromScancode((SDL_Scancode)scancode, modifiers, false);
	event.key.mod = modifiers;
	event.key.down = down;
	SDL_PushEvent(&event);
}

static void bevy_bridge_push_button(int button, bool down)
{
	SDL_Event event;

	memset(&event, 0, sizeof(event));
	event.type = down ? SDL_EVENT_MOUSE_BUTTON_DOWN : SDL_EVENT_MOUSE_BUTTON_UP;
	event.button.timestamp = SDL_GetTicksNS();
	event.button.windowID = bevy_bridge_window;
	event.button.button = (Uint8)button;
	event.button.down = down;
	event.button.clicks = 1;
	SDL_PushEvent(&event);
}

/* sdl_platform.c, before it polls SDL's events */
void bevy_bridge_input_pump(SDL_Window *window)
{
	struct bevy_bridge_shared *shared = (struct bevy_bridge_shared *)bevy_bridge_platform_shared();
	struct bevy_bridge_input input;
	SDL_Keymod modifiers;
	int index;

	if (!shared)
		return;
	bevy_bridge_window = window ? SDL_GetWindowID(window) : 0;
	memcpy(&input, &shared->input, sizeof(input));
	if (!input.active)
	{
		/* forwarding stopped: let go of whatever it held, and take the
		counters as they stand */
		memset(input.keys, 0, sizeof(input.keys));
		input.mouse_buttons = 0;
		input.mouse_x = bevy_bridge_last.mouse_x;
		input.mouse_y = bevy_bridge_last.mouse_y;
		input.wheel = bevy_bridge_last.wheel;
		if (!bevy_bridge_last.active)
		{
			bevy_bridge_last.mouse_x = shared->input.mouse_x;
			bevy_bridge_last.mouse_y = shared->input.mouse_y;
			bevy_bridge_last.wheel = shared->input.wheel;
			return;
		}
	}
	modifiers = bevy_bridge_modifiers(&input);

	for (index = 0; index < 16 * 32 && index < SDL_SCANCODE_COUNT; index++)
	{
		bool was = (bevy_bridge_last.keys[index / 32] >> (index % 32)) & 1;
		bool is = (input.keys[index / 32] >> (index % 32)) & 1;

		if (was != is)
			bevy_bridge_push_key(index, is, modifiers);
	}
	for (index = 1; index < 32; index++)
	{
		bool was = (bevy_bridge_last.mouse_buttons >> index) & 1;
		bool is = (input.mouse_buttons >> index) & 1;

		if (was != is)
			bevy_bridge_push_button(index, is);
	}
	if (input.mouse_x != bevy_bridge_last.mouse_x || input.mouse_y != bevy_bridge_last.mouse_y)
	{
		SDL_Event event;

		memset(&event, 0, sizeof(event));
		event.type = SDL_EVENT_MOUSE_MOTION;
		event.motion.timestamp = SDL_GetTicksNS();
		event.motion.windowID = bevy_bridge_window;
		event.motion.xrel = (float)(input.mouse_x - bevy_bridge_last.mouse_x);
		event.motion.yrel = (float)(input.mouse_y - bevy_bridge_last.mouse_y);
		SDL_PushEvent(&event);
	}
	if (input.wheel != bevy_bridge_last.wheel)
	{
		SDL_Event event;

		memset(&event, 0, sizeof(event));
		event.type = SDL_EVENT_MOUSE_WHEEL;
		event.wheel.timestamp = SDL_GetTicksNS();
		event.wheel.windowID = bevy_bridge_window;
		event.wheel.y = (float)(input.wheel - bevy_bridge_last.wheel);
		event.wheel.integer_y = input.wheel - bevy_bridge_last.wheel;
		SDL_PushEvent(&event);
	}
	bevy_bridge_last = input;
}
