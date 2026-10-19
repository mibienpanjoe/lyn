#ifndef LYN_GNOME_DISCOVERY_H
#define LYN_GNOME_DISCOVERY_H
#include <stdbool.h>
#include <sys/un.h>

#define LYN_NATIVE_ENDPOINT_BYTES sizeof(((struct sockaddr_un *)0)->sun_path)

/* Linux-private address hint only, never peer authentication. Output is left
 * unchanged on refusal. Does not connect, scan directories or unlink nodes. */
bool lyn_native_discover(const char *runtime, char endpoint[LYN_NATIVE_ENDPOINT_BYTES]);
#endif
