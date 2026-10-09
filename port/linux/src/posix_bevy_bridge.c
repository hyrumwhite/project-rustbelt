/*
POSIX_BEVY_BRIDGE.C

The shared memory of the Bevy bridge on Linux and Android: a POSIX shared
memory object (port/linux/game/bevy_bridge_protocol.h). The Windows build
replaces this file with port/windows/src/win32_bevy_bridge.c.
*/

#include "../game/bevy_bridge_protocol.h"

#include <fcntl.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

static void *bevy_bridge_view;

void *bevy_bridge_platform_shared(void)
{
	return bevy_bridge_view;
}

void *bevy_bridge_platform_map(unsigned long size)
{
	int descriptor = shm_open(BEVY_BRIDGE_POSIX_NAME, O_CREAT | O_RDWR, 0600);
	void *view;

	if (descriptor < 0)
		return NULL;
	if (ftruncate(descriptor, (off_t)size) != 0)
	{
		close(descriptor);
		return NULL;
	}
	view = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, descriptor, 0);
	close(descriptor);
	if (view == MAP_FAILED)
		return NULL;
	memset(view, 0, size);
	bevy_bridge_view = view;
	return view;
}

unsigned long bevy_bridge_platform_process_id(void)
{
	return (unsigned long)getpid();
}
