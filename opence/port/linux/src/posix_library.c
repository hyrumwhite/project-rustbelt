/*
POSIX_LIBRARY.C

Loading a library at run time (halo_sim.c's Rust simulation library).
*/

#include <dlfcn.h>

void *platform_library_open(char const *path)
{
	return dlopen(path, RTLD_NOW);
}

void *platform_library_symbol(void *library, char const *name)
{
	return dlsym(library, name);
}
