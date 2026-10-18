/* GTK lifecycle fixture only, NOT the authenticated GNOME server executable. */
#include <gtk/gtk.h>

int main(int argc, char **argv)
{
    gtk_init(&argc, &argv);
    /* Simulate slow initialization before the first main-loop dispatch.
     * Deliberately exceeds the channel's unchanged 120ms output deadline. */
    g_usleep(300000);
    gtk_main();
    return 0;
}
