/* Existing third-party GTK module fixture: must survive Lyn's env filtering. */
#include <gmodule.h>

G_MODULE_EXPORT void gtk_module_init(int *argc, char ***argv)
{
    (void)argc;
    (void)argv;
}
