/* Native activation entry point. No diagnostic opt-in or telemetry transport.
 * Shares the audited GTK/VTE metadata implementation with the prototype. */
#include <stdbool.h>
#include <sys/stat.h>

#ifdef LYN_GNOME_TEST_METRICS
#error "Diagnostic instrumentation is not part of the native activation module"
#endif

static bool native_server_process(void)
{
    static const char *paths[] = {
        "/usr/libexec/gnome-terminal-server",
        "/usr/lib/gnome-terminal/gnome-terminal-server"
    };
    struct stat current;
    if (stat("/proc/self/exe", &current) != 0 || !S_ISREG(current.st_mode))
        return false;
    for (unsigned int index = 0; index < sizeof(paths) / sizeof(paths[0]); index++) {
        struct stat expected;
        if (stat(paths[index], &expected) == 0 && S_ISREG(expected.st_mode)
            && expected.st_uid == 0 && !(expected.st_mode & 0022)
            && expected.st_dev == current.st_dev && expected.st_ino == current.st_ino)
            return true;
    }
    return false;
}

#define LYN_GNOME_NATIVE_MODULE
#include "prototype/module.c"
