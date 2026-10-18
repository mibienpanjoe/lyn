#ifndef LYN_GNOME_CHANNEL_H
#define LYN_GNOME_CHANNEL_H
#include "wire.h"

/* Query callback executes on the GTK main loop; no terminal stream access. */
typedef void (*LynNativeQuery)(const LynFrame *query, LynFrame *reply);
bool lyn_native_connect(const char *endpoint, LynNativeQuery query);
#endif
