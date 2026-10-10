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
