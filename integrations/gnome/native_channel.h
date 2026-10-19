#ifndef LYN_GNOME_CHANNEL_H
#define LYN_GNOME_CHANNEL_H
#include "wire.h"

/* Query callback executes on the GTK main loop; no terminal stream access. */
typedef void (*LynNativeQuery)(const LynFrame *query, LynFrame *reply);
/* GTK main context only. Copies an absolute endpoint; true means configured,
 * not connected/admitted. Retries connections, never queries or captures. */
bool lyn_native_start(const char *endpoint, LynNativeQuery query);
/* Same lifecycle, but copies a trusted absolute runtime directory and resolves
 * its private publication anew before each connection attempt. */
bool lyn_native_start_discovered(const char *runtime, LynNativeQuery query);
/* Cancels watches/retry and frees the copied endpoint; safe when stopped. */
void lyn_native_stop(void);
#endif
