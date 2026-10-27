/*
BEVY_BRIDGE_INPUT.C

Input from a Bevy renderer (port/linux/game/bevy_bridge_protocol.h): the
renderer forwards its window's keyboard and mouse into the shared memory,
and this turns their changes into SDL events, pushed just before the
platform layer polls SDL (sdl_platform.c), so the game handles them as its
own window's: bindings, menus, typing and the aim all work unchanged. When
the renderer stops forwarding, everything it held is released.

While the menus take a pointer (bevy_bridge_input_menus_pointer), the
renderer's cursor is free and its place comes as a fraction of the
renderer's window (the game draws at that window's shape, its overlay
filling it), here put at the same place in the game's own window.
*/

#include "../game/bevy_bridge_protocol.h"

#include <SDL3/SDL.h>
#include <string.h>

static struct bevy_bridge_input bevy_bridge_last;
/* the game's window, which the events are for */
static SDL_WindowID bevy_bridge_window;
/* the pointer's place in the game's window, as last sent */
static float bevy_bridge_pointer_x, bevy_bridge_pointer_y;

/* sdl_platform.c: whether the menus take a pointer, for the renderer */
void bevy_bridge_input_menus_pointer(int active)
{
	struct bevy_bridge_shared *shared = (struct bevy_bridge_shared *)bevy_bridge_platform_shared();

	if (shared)
		__atomic_store_n(&shared->menus_pointer, active ? 1ul : 0ul, __ATOMIC_RELEASE);
	/* (and where the cursor is goes to the menus again, moved or not) */
	bevy_bridge_last.pointer = 0;
}

/* whether the bridge drives this window's input */
int bevy_bridge_input_attached(void)
{
	return bevy_bridge_platform_shared() != NULL;
}

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
	event.button.x = bevy_bridge_pointer_x;
	event.button.y = bevy_bridge_pointer_y;
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

	/* the pointer first, so that a click in the same frame is where it is */
	if (input.pointer & (1ul << 30))
	{
		int width = 0, height = 0;
		float x, y;

		SDL_GetWindowSize(window, &width, &height);
		x = (float)(input.pointer & 0x7FFF) / 32767.0f * (float)width;
		y = (float)((input.pointer >> 15) & 0x7FFF) / 32767.0f * (float)height;
		if (input.pointer != bevy_bridge_last.pointer)
		{
			SDL_Event event;

			memset(&event, 0, sizeof(event));
			event.type = SDL_EVENT_MOUSE_MOTION;
			event.motion.timestamp = SDL_GetTicksNS();
			event.motion.windowID = bevy_bridge_window;
			event.motion.x = x;
			event.motion.y = y;
			SDL_PushEvent(&event);
		}
		bevy_bridge_pointer_x = x;
		bevy_bridge_pointer_y = y;
	}

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

/* ---------- gamepads */

int bevy_bridge_gamepad_count(void)
{
	struct bevy_bridge_shared *shared = (struct bevy_bridge_shared *)bevy_bridge_platform_shared();
	unsigned long count;

	if (!shared || !__atomic_load_n(&shared->gamepads.active, __ATOMIC_ACQUIRE))
		return -1;
	count = shared->gamepads.count;
	return (int)(count < BEVY_BRIDGE_GAMEPADS ? count : BEVY_BRIDGE_GAMEPADS);
}

int bevy_bridge_gamepad_read(int index, unsigned long *buttons, short axes[6])
{
	struct bevy_bridge_shared *shared = (struct bevy_bridge_shared *)bevy_bridge_platform_shared();
	struct bevy_bridge_gamepad volatile const *pad;
	int axis;

	if (!shared || index < 0 || index >= BEVY_BRIDGE_GAMEPADS)
		return 0;
	pad = &shared->gamepads.pads[index];
	*buttons = pad->buttons;
	for (axis = 0; axis < 6; axis++)
		axes[axis] = pad->axes[axis];
	return 1;
}

void bevy_bridge_gamepad_rumble(int index, unsigned short low, unsigned short high)
{
	struct bevy_bridge_shared *shared = (struct bevy_bridge_shared *)bevy_bridge_platform_shared();

	if (!shared || index < 0 || index >= BEVY_BRIDGE_GAMEPADS)
		return;
	shared->gamepads.pads[index].rumble[0] = low;
	shared->gamepads.pads[index].rumble[1] = high;
}
