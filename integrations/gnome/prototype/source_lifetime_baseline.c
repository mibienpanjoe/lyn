/* Match mapped widget/cache activity, deliberately without any Lyn source. */
#include <gtk/gtk.h>
#include <vte/vte.h>

static gboolean finish_cache_dispatch(gpointer loop)
{
    g_main_loop_quit(loop);
    return G_SOURCE_REMOVE;
}

int main(int argc, char **argv)
{
    gtk_init(&argc, &argv);
    for (guint cycle = 0; cycle < 100; cycle++) {
        GtkWidget *terminal = vte_terminal_new();
        g_object_ref_sink(terminal);
        g_object_notify(G_OBJECT(terminal), "pty");
        GtkWidget *window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
        gtk_container_add(GTK_CONTAINER(window), terminal);
        gtk_widget_show_all(window);
        g_assert_true(gtk_widget_get_mapped(terminal));
        gtk_widget_hide(terminal);
        g_assert_false(gtk_widget_get_mapped(terminal));
        gtk_container_remove(GTK_CONTAINER(window), terminal);
        gtk_widget_destroy(window);
        gtk_widget_destroy(terminal);
        g_object_unref(terminal);
    }
    g_assert_null(gtk_window_list_toplevels());
    GMainLoop *loop = g_main_loop_new(NULL, FALSE);
    g_timeout_add_seconds(35, finish_cache_dispatch, loop);
    g_print("Baseline without Lyn: dispatching VTE cache retirement after 100 mapped widgets.\n");
    g_main_loop_run(loop);
    g_main_loop_unref(loop);
    return 0;
}
