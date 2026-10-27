/*
RASTERIZER_XBOX_MOTION_SENSOR.C

symbols in this file:
0015D220 01b0:
	_code_0015d220 (D3DDevice_SetRenderState)
0015D3D0 0050:
	_code_0015d3d0 (D3DDevice_SetTextureStageState)
0015D420 0220:
	_code_0015d420 (IDirect3DDevice8_SetRenderState)
0015D640 0060:
	_code_0015d640 (IDirect3DDevice8_SetTextureStageState)
0015D6A0 0010:
	_code_0015d6a0 (IDirect3DDevice8_SetVertexShaderConstant)
0015D6B0 0020:
	_code_0015d6b0 (IDirect3DDevice8_SetVertexData2f)
0015D6D0 0030:
	_code_0015d6d0 (IDirect3DDevice8_SetVertexData4f)
0015D700 0010:
	_code_0015d700 (IDirect3DDevice8_SetVertexData2s)
0015D710 0010:
	_code_0015d710 (IDirect3DDevice8_Begin)
0015D720 0010:
	_code_0015d720 (IDirect3DDevice8_End)
0015D730 0280:
	__rasterizer_hud_motion_sensor_blip_begin (0000)
0015D9B0 0180:
	__rasterizer_hud_motion_sensor_blip_draw (0000)
0015DB30 0780:
	__rasterizer_hud_motion_sensor_blip_end (0000)
00291ECC 003f:
	??_C@_0DP@DPMIHOMG@c?3?2halo?2SOURCE?2rasterizer?2xbox?2r@ (0000)
00291F0C 0004:
	__real@bd000000 (0000)
00465E27 0001:
	_bss_00465e27 (0000)
*/

/* ---------- headers */

#include "cseries.h"
#include "cache/texture_cache.h"
#include "cseries/errors.h"
#include "bitmaps/bitmap_group.h"
#include "game/players.h"
#include "interface/interface.h"
#include "math/real_math.h"
#include "rasterizer/rasterizer.h"
#include "rasterizer/rasterizer_console_vars.h"
#include <xtl.h>
#include "rasterizer/xbox/rasterizer_xbox_pixel_shader.h"

/* ---------- constants */

/* ---------- macros */

/* ---------- structures */

/* ---------- prototypes */

struct bitmap_data *bitmap_group_try_and_get_bitmap(
	long bitmap_group_index,
	short bitmap_index);

void rasterizer_set_target(
	word target,
	boolean use_depth_buffer,
	boolean clear_color,
	boolean clear_depth,
	boolean clear_stencil);

void rasterizer_set_target_as_texture(
	short stage,
	long target,
	boolean filtered);

void rasterizer_set_texture_bitmap_data(
	short stage,
	struct bitmap_data const *bitmap);

void rasterizer_set_vertex_shader_permutation(
	short vertex_type,
	short permutation,
	boolean one_node);

void rasterizer_set_pixel_shader(
	struct pixel_shader_definition const *definition);

void SetRenderStateSmart(
	unsigned long state,
	unsigned long value);

/* ---------- globals */

extern void *global_d3d_device;
extern struct pixel_shader_definition pixel_shader;

static boolean bss_00465e27 = {0};

/* port: with the renderer drawing the HUD (bevy_bridge.c), the motion
sensor's draws shared with it instead: its blips, sweep and mask into its
own target, then that target onto the screen */
#include "bevy_bridge.h" /* port: port/linux/game/bevy_bridge.c */
#include "bevy_bridge_protocol.h"

/* the motion sensor's target's pixels: RASTERIZER_TARGET_SUN_GLOW_SIZE */
#define SHARED_MOTION_SENSOR_SIZE 64.0f

/* a quad shared: corners (x, y) in the target's pixels (or the window's),
texture coordinates (u, v), one colour; the pixel shader as set */
static void share_motion_sensor_quad(
	unsigned long target,
	struct bitmap_data const *bitmap,
	boolean motion_sensor_texture,
	real const corners[4][4],
	real red, real green, real blue, real alpha,
	unsigned long blend_source, unsigned long blend_destination,
	unsigned long border_color,
	real viewport_width, real viewport_height)
{
	struct bevy_bridge_screen_draw draw;
	struct bevy_bridge_screen_vertex vertices[4];
	short index;
	unsigned long color;

	memset(&draw, 0, sizeof(draw));
	for (index = 0; index < 3; index++)
	{
		draw.maps[index].bitmap = 0xFFFFFFFFUL;
		draw.maps[index].texture = 0xFFFFFFFFUL;
	}
	if (motion_sensor_texture)
		draw.maps[0].texture = BEVY_BRIDGE_SCREEN_TEXTURE_MOTION_SENSOR;
	else if (!bitmap || !bevy_bridge_screen_map_from_bitmap(bitmap, &draw.maps[0]))
		return;
	/* (linear, and outside it the border colour; the target's own clamped) */
	draw.maps[0].flags = motion_sensor_texture ? 0 : 4;
	color = ((unsigned long)(alpha * 255.0f + 0.5f) << 24) |
		((unsigned long)(PIN(red, 0.0f, 1.0f) * 255.0f + 0.5f) << 16) |
		((unsigned long)(PIN(green, 0.0f, 1.0f) * 255.0f + 0.5f) << 8) |
		(unsigned long)(PIN(blue, 0.0f, 1.0f) * 255.0f + 0.5f);
	for (index = 0; index < 4; index++)
	{
		vertices[index].position[0] = corners[index][0];
		vertices[index].position[1] = corners[index][1];
		vertices[index].texcoords[0][0] = corners[index][2];
		vertices[index].texcoords[0][1] = corners[index][3];
		vertices[index].color = color;
	}
	draw.vertex_count = 4;
	draw.viewport[0] = viewport_width;
	draw.viewport[1] = viewport_height;
	draw.combiner_count = pixel_shader.combiner_count;
	memcpy(draw.rgb_inputs, pixel_shader.rgb_inputs, sizeof(draw.rgb_inputs));
	memcpy(draw.rgb_outputs, pixel_shader.rgb_outputs, sizeof(draw.rgb_outputs));
	memcpy(draw.alpha_inputs, pixel_shader.alpha_inputs, sizeof(draw.alpha_inputs));
	memcpy(draw.alpha_outputs, pixel_shader.alpha_outputs, sizeof(draw.alpha_outputs));
	memcpy(draw.constant_0, pixel_shader.constant_0, sizeof(draw.constant_0));
	memcpy(draw.constant_1, pixel_shader.constant_1, sizeof(draw.constant_1));
	draw.final_combiner_inputs_abcd = pixel_shader.final_combiner_inputs_abcd;
	draw.final_combiner_inputs_efg = pixel_shader.final_combiner_inputs_efg;
	draw.blend_source = blend_source;
	draw.blend_destination = blend_destination;
	draw.blend_operation = D3DBLENDOP_ADD;
	/* blended; into its own target, colour only, as the game writes it */
	draw.flags = 1 | (target == BEVY_BRIDGE_SCREEN_TARGET_MOTION_SENSOR ? 4 : 0);
	draw.target = target;
	draw.border_color = border_color;
	bevy_bridge_note_screen_draw(&draw, vertices);
}

/* a corner in the target's clip space (as the motion sensor's identity
transform takes it) to its pixels */
static void motion_sensor_corner(real corner[4], real x, real y, real u, real v)
{
	corner[0] = (x + 1.0f) * 0.5f * SHARED_MOTION_SENSOR_SIZE;
	corner[1] = (1.0f - y) * 0.5f * SHARED_MOTION_SENSOR_SIZE;
	corner[2] = u;
	corner[3] = v;
}

/* ---------- public code */

void _rasterizer_hud_motion_sensor_blip_begin(
	void)
{
	struct bitmap_data *blip_bitmap;
	struct bitmap_data *large_blip_bitmap;
	real vertex_constants[5][4];

	blip_bitmap = bitmap_group_try_and_get_bitmap(
		interface_get_tag_index(_interface_bitmap_motion_blip),
		0);
	large_blip_bitmap = bitmap_group_try_and_get_bitmap(
		interface_get_tag_index(_interface_bitmap_iface_map1),
		0);

	match_assert(
		"c:\\halo\\SOURCE\\rasterizer\\xbox\\rasterizer_xbox_motion_sensor.c",
		27,
		global_d3d_device);

	bss_00465e27 = FALSE;
	if (rasterizer_debug_options.draw_hud_motion_sensor &&
		_texture_cache_bitmap_get_hardware_format(blip_bitmap, FALSE, TRUE) &&
		_texture_cache_bitmap_get_hardware_format(large_blip_bitmap, FALSE, TRUE))
	{
		bss_00465e27 = TRUE;
		rasterizer_set_target(4, FALSE, FALSE, TRUE, FALSE);
		rasterizer_set_texture_bitmap_data(0, blip_bitmap);

		IDirect3DDevice8_SetTextureStageState(
			global_d3d_device,
			0,
			D3DTSS_ADDRESSU,
			D3DTADDRESS_BORDER);
		IDirect3DDevice8_SetTextureStageState(
			global_d3d_device,
			0,
			D3DTSS_ADDRESSV,
			D3DTADDRESS_BORDER);
		IDirect3DDevice8_SetTextureStageState(
			global_d3d_device,
			0,
			D3DTSS_MAGFILTER,
			D3DTEXF_LINEAR);
		IDirect3DDevice8_SetTextureStageState(
			global_d3d_device,
			0,
			D3DTSS_MINFILTER,
			D3DTEXF_LINEAR);
		IDirect3DDevice8_SetTextureStageState(
			global_d3d_device,
			0,
			D3DTSS_MIPFILTER,
			D3DTEXF_POINT);

		IDirect3DDevice8_SetRenderState(
			global_d3d_device,
			D3DRS_CULLMODE,
			0x901);
		IDirect3DDevice8_SetRenderState(
			global_d3d_device,
			D3DRS_COLORWRITEENABLE,
			D3DCOLORWRITEENABLE_RED |
			D3DCOLORWRITEENABLE_GREEN |
			D3DCOLORWRITEENABLE_BLUE);
		IDirect3DDevice8_SetRenderState(
			global_d3d_device,
			D3DRS_ALPHABLENDENABLE,
			TRUE);
		IDirect3DDevice8_SetRenderState(
			global_d3d_device,
			D3DRS_SRCBLEND,
			D3DBLEND_ONE);
		IDirect3DDevice8_SetRenderState(
			global_d3d_device,
			D3DRS_DESTBLEND,
			D3DBLEND_ONE);
		IDirect3DDevice8_SetRenderState(
			global_d3d_device,
			D3DRS_BLENDOP,
			D3DBLENDOP_ADD);
		IDirect3DDevice8_SetRenderState(
			global_d3d_device,
			D3DRS_ALPHATESTENABLE,
			FALSE);
		IDirect3DDevice8_SetRenderState(
			global_d3d_device,
			D3DRS_ZENABLE,
			FALSE);
		IDirect3DDevice8_SetRenderState(
			global_d3d_device,
			D3DRS_ZBIAS,
			0);

		rasterizer_set_vertex_shader_permutation(4, 8, FALSE);

		vertex_constants[0][0] = 1.0f;
		vertex_constants[0][1] = 0.0f;
		vertex_constants[0][2] = 0.0f;
		vertex_constants[0][3] = 0.0f;
		vertex_constants[1][0] = 0.0f;
		vertex_constants[1][1] = 1.0f;
		vertex_constants[1][2] = 0.0f;
		vertex_constants[1][3] = 0.0f;
		vertex_constants[2][0] = 0.0f;
		vertex_constants[2][1] = 0.0f;
		vertex_constants[2][2] = 1.0f;
		vertex_constants[2][3] = 0.0f;
		vertex_constants[3][0] = 0.0f;
		vertex_constants[3][1] = 0.0f;
		vertex_constants[3][2] = 0.0f;
		vertex_constants[3][3] = 1.0f;
		vertex_constants[4][0] = 1.0f;
		vertex_constants[4][1] = 1.0f;
		vertex_constants[4][2] = 0.0f;
		vertex_constants[4][3] = 1.0f;
		IDirect3DDevice8_SetVertexShaderConstant(
			global_d3d_device,
			-68,
			vertex_constants,
			5);

		csmemset(&pixel_shader, 0, sizeof(pixel_shader));
		pixel_shader.texture_modes = 1;
		pixel_shader.combiner_count = 1;
		pixel_shader.final_combiner_inputs_abcd = 0x08040000;
		rasterizer_set_pixel_shader(&pixel_shader);
	}

	return;
}

void _rasterizer_hud_motion_sensor_blip_draw(
	real_point2d const *position,
	real intensity,
	real size,
	real_rgb_color const *color,
	boolean large_blip)
{
	struct bitmap_data *blip_bitmap;
	struct bitmap_data *large_blip_bitmap;
	real_point2d scaled_position;
	real radius;

	blip_bitmap = bitmap_group_try_and_get_bitmap(
		interface_get_tag_index(_interface_bitmap_motion_blip),
		0);
	large_blip_bitmap = bitmap_group_try_and_get_bitmap(
		interface_get_tag_index(_interface_bitmap_iface_map1),
		0);

	match_assert(
		"c:\\halo\\SOURCE\\rasterizer\\xbox\\rasterizer_xbox_motion_sensor.c",
		109,
		global_d3d_device);

	rasterizer_set_texture_bitmap_data(
		0,
		large_blip ? large_blip_bitmap : blip_bitmap);
	if (rasterizer_debug_options.draw_hud_motion_sensor && bss_00465e27)
	{
		radius = size * 0.0625f;
		scaled_position.x = position->x * -0.03125f;
		scaled_position.y = position->y * -0.03125f;

		/* port: the renderer's, when it draws the HUD */
		if (bevy_bridge_viewer_draws_hud())
		{
			real corners[4][4];

			motion_sensor_corner(corners[0], scaled_position.x - radius, scaled_position.y + radius, 0.0f, 0.0f);
			motion_sensor_corner(corners[1], scaled_position.x + radius, scaled_position.y + radius, 1.0f, 0.0f);
			motion_sensor_corner(corners[2], scaled_position.x + radius, scaled_position.y - radius, 1.0f, 1.0f);
			motion_sensor_corner(corners[3], scaled_position.x - radius, scaled_position.y - radius, 0.0f, 1.0f);
			share_motion_sensor_quad(BEVY_BRIDGE_SCREEN_TARGET_MOTION_SENSOR, large_blip ? large_blip_bitmap : blip_bitmap,
				FALSE, corners, color->red * intensity, color->green * intensity, color->blue * intensity, 1.0f,
				D3DBLEND_ONE, D3DBLEND_ONE, 0, SHARED_MOTION_SENSOR_SIZE, SHARED_MOTION_SENSOR_SIZE);
			return;
		}

		IDirect3DDevice8_Begin(global_d3d_device, D3DPT_TRIANGLEFAN);
		IDirect3DDevice8_SetVertexData4f(
			global_d3d_device,
			9,
			color->red * intensity,
			color->green * intensity,
			color->blue * intensity,
			1.0f);
		IDirect3DDevice8_SetVertexData2s(global_d3d_device, 4, 0, 0);
		IDirect3DDevice8_SetVertexData2f(
			global_d3d_device,
			0,
			scaled_position.x - radius,
			scaled_position.y + radius);
		IDirect3DDevice8_SetVertexData2s(global_d3d_device, 4, 1, 0);
		IDirect3DDevice8_SetVertexData2f(
			global_d3d_device,
			0,
			scaled_position.x + radius,
			scaled_position.y + radius);
		IDirect3DDevice8_SetVertexData2s(global_d3d_device, 4, 1, 1);
		IDirect3DDevice8_SetVertexData2f(
			global_d3d_device,
			0,
			scaled_position.x + radius,
			scaled_position.y - radius);
		IDirect3DDevice8_SetVertexData2s(global_d3d_device, 4, 0, 1);
		IDirect3DDevice8_SetVertexData2f(
			global_d3d_device,
			0,
			scaled_position.x - radius,
			scaled_position.y - radius);
		IDirect3DDevice8_End(global_d3d_device);
	}

	return;
}

void _rasterizer_hud_motion_sensor_blip_end(
	real_point2d const *center,
	real scale)
{
	struct bitmap_data *sweep_bitmap;
	struct bitmap_data *sweep_mask_bitmap;
	real vertex_constants[5][4];
	real half_scale;
	real high;
	real low;
	real radius;
	short height;
	short width;

	sweep_bitmap = bitmap_group_try_and_get_bitmap(
		interface_get_tag_index(_interface_bitmap_motion_sweep),
		0);
	sweep_mask_bitmap = bitmap_group_try_and_get_bitmap(
		interface_get_tag_index(_interface_bitmap_motion_sweep_mask),
		0);

	match_assert(
		"c:\\halo\\SOURCE\\rasterizer\\xbox\\rasterizer_xbox_motion_sensor.c",
		156,
		global_d3d_device);

	if (rasterizer_debug_options.draw_hud_motion_sensor)
	{
		if (bss_00465e27 &&
			_texture_cache_bitmap_get_hardware_format(sweep_bitmap, FALSE, TRUE) &&
			_texture_cache_bitmap_get_hardware_format(sweep_mask_bitmap, FALSE, TRUE))
		{
			rasterizer_set_texture_bitmap_data(0, sweep_bitmap);
			IDirect3DDevice8_SetTextureStageState(
				global_d3d_device,
				0,
				D3DTSS_ADDRESSU,
				D3DTADDRESS_BORDER);
			IDirect3DDevice8_SetTextureStageState(
				global_d3d_device,
				0,
				D3DTSS_ADDRESSV,
				D3DTADDRESS_BORDER);
			IDirect3DDevice8_SetTextureStageState(
				global_d3d_device,
				0,
				D3DTSS_BORDERCOLOR,
				0x46000000);
			IDirect3DDevice8_SetTextureStageState(
				global_d3d_device,
				0,
				D3DTSS_MAGFILTER,
				D3DTEXF_LINEAR);
			IDirect3DDevice8_SetTextureStageState(
				global_d3d_device,
				0,
				D3DTSS_MINFILTER,
				D3DTEXF_LINEAR);
			IDirect3DDevice8_SetTextureStageState(
				global_d3d_device,
				0,
				D3DTSS_MIPFILTER,
				D3DTEXF_POINT);

			IDirect3DDevice8_SetRenderState(
				global_d3d_device,
				D3DRS_CULLMODE,
				0x901);
			IDirect3DDevice8_SetRenderState(
				global_d3d_device,
				D3DRS_COLORWRITEENABLE,
				D3DCOLORWRITEENABLE_RED |
				D3DCOLORWRITEENABLE_GREEN |
				D3DCOLORWRITEENABLE_BLUE);
			IDirect3DDevice8_SetRenderState(
				global_d3d_device,
				D3DRS_ALPHABLENDENABLE,
				TRUE);
			IDirect3DDevice8_SetRenderState(
				global_d3d_device,
				D3DRS_SRCBLEND,
				D3DBLEND_ONE);
			IDirect3DDevice8_SetRenderState(
				global_d3d_device,
				D3DRS_DESTBLEND,
				D3DBLEND_SRCALPHA);
			IDirect3DDevice8_SetRenderState(
				global_d3d_device,
				D3DRS_BLENDOP,
				D3DBLENDOP_ADD);
			IDirect3DDevice8_SetRenderState(
				global_d3d_device,
				D3DRS_ALPHATESTENABLE,
				FALSE);
			IDirect3DDevice8_SetRenderState(
				global_d3d_device,
				D3DRS_ZENABLE,
				FALSE);
			IDirect3DDevice8_SetRenderState(
				global_d3d_device,
				D3DRS_ZBIAS,
				0);
			rasterizer_set_vertex_shader_permutation(4, 8, FALSE);

			vertex_constants[0][0] = 1.0f;
			vertex_constants[0][1] = 0.0f;
			vertex_constants[0][2] = 0.0f;
			vertex_constants[0][3] = 0.0f;
			vertex_constants[1][0] = 0.0f;
			vertex_constants[1][1] = 1.0f;
			vertex_constants[1][2] = 0.0f;
			vertex_constants[1][3] = 0.0f;
			vertex_constants[2][0] = 0.0f;
			vertex_constants[2][1] = 0.0f;
			vertex_constants[2][2] = 1.0f;
			vertex_constants[2][3] = 0.0f;
			vertex_constants[3][0] = 0.0f;
			vertex_constants[3][1] = 0.0f;
			vertex_constants[3][2] = 0.0f;
			vertex_constants[3][3] = 1.0f;
			vertex_constants[4][0] = 1.0f;
			vertex_constants[4][1] = 1.0f;
			vertex_constants[4][2] = 0.0f;
			vertex_constants[4][3] = 1.0f;
			IDirect3DDevice8_SetVertexShaderConstant(
				global_d3d_device,
				-68,
				vertex_constants,
				5);

			csmemset(&pixel_shader, 0, sizeof(pixel_shader));
			pixel_shader.texture_modes = 1;
			pixel_shader.combiner_count = 1;
			pixel_shader.rgb_inputs[0] = 0x08040000;
			pixel_shader.rgb_outputs[0] = 0xC0;
			pixel_shader.alpha_inputs[0] = 0x18140000;
			pixel_shader.alpha_outputs[0] = 0xC0;
			pixel_shader.final_combiner_inputs_abcd = 0xC;
			pixel_shader.final_combiner_inputs_efg = 0x1C00;
			rasterizer_set_pixel_shader(&pixel_shader);

			half_scale = scale * 0.5f;
			low = 0.5f - half_scale;
			high = 0.5f + half_scale;
			/* port: the renderer's, when it draws the HUD: the sweep, its mask
			and the target onto the screen */
			if (bevy_bridge_viewer_draws_hud())
			{
				real corners[4][4];

				motion_sensor_corner(corners[0], -1.015625f, 1.046875f, high, low);
				motion_sensor_corner(corners[1], 1.046875f, 1.046875f, low, low);
				motion_sensor_corner(corners[2], 1.046875f, -1.015625f, low, high);
				motion_sensor_corner(corners[3], -1.015625f, -1.015625f, high, high);
				share_motion_sensor_quad(BEVY_BRIDGE_SCREEN_TARGET_MOTION_SENSOR, sweep_bitmap, FALSE, corners,
					0.4588f, 0.7294f, 1.0f, 1.0f, D3DBLEND_ONE, D3DBLEND_SRCALPHA, 0x46000000UL,
					SHARED_MOTION_SENSOR_SIZE, SHARED_MOTION_SENSOR_SIZE);
				csmemset(&pixel_shader, 0, sizeof(pixel_shader));
				pixel_shader.texture_modes = 1;
				pixel_shader.combiner_count = 1;
				pixel_shader.final_combiner_inputs_abcd = 8;
				pixel_shader.final_combiner_inputs_efg = 0x1800;
				motion_sensor_corner(corners[0], -1.015625f, 1.046875f, 1.0f, 0.0f);
				motion_sensor_corner(corners[1], 1.046875f, 1.046875f, 0.0f, 0.0f);
				motion_sensor_corner(corners[2], 1.046875f, -1.015625f, 0.0f, 1.0f);
				motion_sensor_corner(corners[3], -1.015625f, -1.015625f, 1.0f, 1.0f);
				share_motion_sensor_quad(BEVY_BRIDGE_SCREEN_TARGET_MOTION_SENSOR, sweep_mask_bitmap, FALSE, corners,
					0.4f, 0.8f, 0.4f, 1.0f, D3DBLEND_ZERO, D3DBLEND_SRCALPHA, 0,
					SHARED_MOTION_SENSOR_SIZE, SHARED_MOTION_SENSOR_SIZE);
				/* (the window's: its pixels) */
				width = global_window_parameters.camera.viewport_bounds.x1 - global_window_parameters.camera.viewport_bounds.x0;
				height = global_window_parameters.camera.viewport_bounds.y1 - global_window_parameters.camera.viewport_bounds.y0;
				radius = local_player_count() > 1 ? 32.0f : 42.0f;
				corners[0][0] = center->x - radius; corners[0][1] = center->y - radius; corners[0][2] = 0.0f; corners[0][3] = 0.0f;
				corners[1][0] = center->x + radius; corners[1][1] = center->y - radius; corners[1][2] = 1.0f; corners[1][3] = 0.0f;
				corners[2][0] = center->x + radius; corners[2][1] = center->y + radius; corners[2][2] = 1.0f; corners[2][3] = 1.0f;
				corners[3][0] = center->x - radius; corners[3][1] = center->y + radius; corners[3][2] = 0.0f; corners[3][3] = 1.0f;
				share_motion_sensor_quad(BEVY_BRIDGE_SCREEN_TARGET_SCREEN, NULL, TRUE, corners,
					1.0f, 1.0f, 1.0f, 1.0f, D3DBLEND_ONE, D3DBLEND_INVSRCALPHA, 0, (real)width, (real)height);
				rasterizer_set_target(global_window_parameters.rasterizer_target, FALSE, FALSE, FALSE, TRUE);
				return;
			}

			IDirect3DDevice8_Begin(global_d3d_device, D3DPT_TRIANGLEFAN);
			IDirect3DDevice8_SetVertexData4f(
				global_d3d_device,
				9,
				0.4588f,
				0.7294f,
				1.0f,
				1.0f);
			IDirect3DDevice8_SetVertexData2f(global_d3d_device, 4, high, low);
			IDirect3DDevice8_SetVertexData2f(global_d3d_device, 0, -1.015625f, 1.046875f);
			IDirect3DDevice8_SetVertexData2f(global_d3d_device, 4, low, low);
			IDirect3DDevice8_SetVertexData2f(global_d3d_device, 0, 1.046875f, 1.046875f);
			IDirect3DDevice8_SetVertexData2f(global_d3d_device, 4, low, high);
			IDirect3DDevice8_SetVertexData2f(global_d3d_device, 0, 1.046875f, -1.015625f);
			IDirect3DDevice8_SetVertexData2f(global_d3d_device, 4, high, high);
			IDirect3DDevice8_SetVertexData2f(global_d3d_device, 0, -1.015625f, -1.015625f);
			IDirect3DDevice8_End(global_d3d_device);

			rasterizer_set_texture_bitmap_data(0, sweep_mask_bitmap);
			IDirect3DDevice8_SetRenderState(
				global_d3d_device,
				D3DRS_ALPHABLENDENABLE,
				TRUE);
			IDirect3DDevice8_SetRenderState(
				global_d3d_device,
				D3DRS_SRCBLEND,
				D3DBLEND_ZERO);
			IDirect3DDevice8_SetRenderState(
				global_d3d_device,
				D3DRS_DESTBLEND,
				D3DBLEND_SRCALPHA);

			csmemset(&pixel_shader, 0, sizeof(pixel_shader));
			pixel_shader.texture_modes = 1;
			pixel_shader.combiner_count = 1;
			pixel_shader.final_combiner_inputs_abcd = 8;
			pixel_shader.final_combiner_inputs_efg = 0x1800;
			rasterizer_set_pixel_shader(&pixel_shader);

			IDirect3DDevice8_Begin(global_d3d_device, D3DPT_TRIANGLEFAN);
			IDirect3DDevice8_SetVertexData4f(
				global_d3d_device,
				9,
				0.4f,
				0.8f,
				0.4f,
				1.0f);
			IDirect3DDevice8_SetVertexData2f(global_d3d_device, 4, 1.0f, 0.0f);
			IDirect3DDevice8_SetVertexData2f(global_d3d_device, 0, -1.015625f, 1.046875f);
			IDirect3DDevice8_SetVertexData2f(global_d3d_device, 4, 0.0f, 0.0f);
			IDirect3DDevice8_SetVertexData2f(global_d3d_device, 0, 1.046875f, 1.046875f);
			IDirect3DDevice8_SetVertexData2f(global_d3d_device, 4, 0.0f, 1.0f);
			IDirect3DDevice8_SetVertexData2f(global_d3d_device, 0, 1.046875f, -1.015625f);
			IDirect3DDevice8_SetVertexData2f(global_d3d_device, 4, 1.0f, 1.0f);
			IDirect3DDevice8_SetVertexData2f(global_d3d_device, 0, -1.015625f, -1.015625f);
			IDirect3DDevice8_End(global_d3d_device);

			rasterizer_set_target(
				global_window_parameters.rasterizer_target,
				FALSE,
				FALSE,
				FALSE,
				TRUE);
			rasterizer_set_target_as_texture(0, 4, FALSE);
			IDirect3DDevice8_SetTextureStageState(global_d3d_device, 0, D3DTSS_ADDRESSU, D3DTADDRESS_CLAMP);
			IDirect3DDevice8_SetTextureStageState(global_d3d_device, 0, D3DTSS_ADDRESSV, D3DTADDRESS_CLAMP);
			IDirect3DDevice8_SetTextureStageState(global_d3d_device, 0, D3DTSS_MAGFILTER, D3DTEXF_LINEAR);
			IDirect3DDevice8_SetTextureStageState(global_d3d_device, 0, D3DTSS_MINFILTER, D3DTEXF_LINEAR);
			IDirect3DDevice8_SetTextureStageState(global_d3d_device, 0, D3DTSS_MIPFILTER, D3DTEXF_POINT);
			IDirect3DDevice8_SetTextureStageState(global_d3d_device, 0, D3DTSS_BORDERCOLOR, 0);
			IDirect3DDevice8_SetRenderState(global_d3d_device, D3DRS_CULLMODE, 0x901);
			IDirect3DDevice8_SetRenderState(
				global_d3d_device,
				D3DRS_COLORWRITEENABLE,
				D3DCOLORWRITEENABLE_RED |
				D3DCOLORWRITEENABLE_GREEN |
				D3DCOLORWRITEENABLE_BLUE);
			SetRenderStateSmart(0x3B, TRUE);
			SetRenderStateSmart(0x3E, TRUE);
			SetRenderStateSmart(0x3F, 0x303);
			SetRenderStateSmart(0x4A, 0x8006);
			SetRenderStateSmart(0x3C, FALSE);
			SetRenderStateSmart(0x7B, FALSE);
			IDirect3DDevice8_SetRenderState(global_d3d_device, D3DRS_ZBIAS, 0);
			rasterizer_set_vertex_shader_permutation(4, 8, FALSE);

			width = global_window_parameters.camera.viewport_bounds.x1 - global_window_parameters.camera.viewport_bounds.x0;
			height = global_window_parameters.camera.viewport_bounds.y1 - global_window_parameters.camera.viewport_bounds.y0;
			vertex_constants[0][0] = 2.0f / width;
			vertex_constants[0][1] = 0.0f;
			vertex_constants[0][2] = 0.0f;
			vertex_constants[0][3] = -1.0f - 1.0f / width;
			vertex_constants[1][0] = 0.0f;
			vertex_constants[1][1] = -2.0f / height;
			vertex_constants[1][2] = 0.0f;
			vertex_constants[1][3] = 1.0f + 1.0f / height;
			vertex_constants[2][0] = 0.0f;
			vertex_constants[2][1] = 0.0f;
			vertex_constants[2][2] = 0.0f;
			vertex_constants[2][3] = 0.5f;
			vertex_constants[3][0] = 0.0f;
			vertex_constants[3][1] = 0.0f;
			vertex_constants[3][2] = 0.0f;
			vertex_constants[3][3] = 1.0f;
			vertex_constants[4][0] = 1.0f;
			vertex_constants[4][1] = 1.0f;
			vertex_constants[4][2] = 0.0f;
			vertex_constants[4][3] = 1.0f;
			IDirect3DDevice8_SetVertexShaderConstant(
				global_d3d_device,
				-68,
				vertex_constants,
				5);

			csmemset(&pixel_shader, 0, sizeof(pixel_shader));
			pixel_shader.texture_modes = 1;
			pixel_shader.combiner_count = 1;
			pixel_shader.final_combiner_inputs_abcd = 8;
			pixel_shader.final_combiner_inputs_efg = 0x1800;
			rasterizer_set_pixel_shader(&pixel_shader);

			radius = local_player_count() > 1 ? 32.0f : 42.0f;
			IDirect3DDevice8_Begin(global_d3d_device, D3DPT_TRIANGLEFAN);
			IDirect3DDevice8_SetVertexData2s(global_d3d_device, 4, 0, 0);
			IDirect3DDevice8_SetVertexData2f(
				global_d3d_device,
				0,
				center->x - radius,
				center->y - radius);
			IDirect3DDevice8_SetVertexData2s(global_d3d_device, 4, 1, 0);
			IDirect3DDevice8_SetVertexData2f(
				global_d3d_device,
				0,
				center->x + radius,
				center->y - radius);
			IDirect3DDevice8_SetVertexData2s(global_d3d_device, 4, 1, 1);
			IDirect3DDevice8_SetVertexData2f(
				global_d3d_device,
				0,
				center->x + radius,
				center->y + radius);
			IDirect3DDevice8_SetVertexData2s(global_d3d_device, 4, 0, 1);
			IDirect3DDevice8_SetVertexData2f(
				global_d3d_device,
				0,
				center->x - radius,
				center->y + radius);
			IDirect3DDevice8_End(global_d3d_device);
			return;
		}
	}

	if (rasterizer_debug_options.draw_hud_motion_sensor && bss_00465e27)
	{
		rasterizer_set_target(
			global_window_parameters.rasterizer_target,
			FALSE,
			FALSE,
			FALSE,
			TRUE);
	}

	return;
}

/* ---------- private code */
