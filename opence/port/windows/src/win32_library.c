/*
WIN32_LIBRARY.C

Loading a library at run time on Windows (halo_sim.c's Rust simulation DLL).
*/

#include <windows.h>

void *platform_library_open(char const *path)
{
	return (void *)LoadLibraryA(path);
}

void *platform_library_symbol(void *library, char const *name)
{
	return (void *)GetProcAddress((HMODULE)library, name);
}

/* the process's GPU scheduling priority class (D3DKMT: 0 idle, 1 below
normal, 2 normal, 3 above normal, 4 high, 5 realtime) through gdi32's
D3DKMTSetProcessSchedulingPriorityClass, found at run time: 1 if set, 0 if
refused (*status the NTSTATUS), -1 if the function is not there */
int platform_gpu_priority_set(int priority_class, long *status)
{
	typedef LONG (WINAPI *set_priority_proc)(HANDLE process, int priority_class);
	HMODULE gdi32 = LoadLibraryA("gdi32.dll");
	set_priority_proc set_priority = gdi32 ?
		(set_priority_proc)(void *)GetProcAddress(gdi32, "D3DKMTSetProcessSchedulingPriorityClass") : NULL;

	*status = 0;
	if (!set_priority)
		return -1;
	*status = (long)set_priority(GetCurrentProcess(), priority_class);
	return *status == 0 ? 1 : 0;
}
