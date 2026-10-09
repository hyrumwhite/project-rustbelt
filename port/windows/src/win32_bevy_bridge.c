/*
WIN32_BEVY_BRIDGE.C

The shared memory of the Bevy bridge on Windows: a named file mapping in
the session's namespace (port/linux/game/bevy_bridge_protocol.h), which the
renderer opens by name.
*/

#include <windows.h>

#include "../../linux/game/bevy_bridge_protocol.h"

void *bevy_bridge_platform_map(unsigned long size)
{
	HANDLE mapping = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, size, BEVY_BRIDGE_MAPPING_NAME);
	void *view;

	if (!mapping)
		return NULL;
	view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, size);
	if (!view)
	{
		CloseHandle(mapping);
		return NULL;
	}
	/* (the mapping lives as long as the game: the handle is never closed) */
	ZeroMemory(view, size);
	return view;
}

unsigned long bevy_bridge_platform_process_id(void)
{
	return GetCurrentProcessId();
}
