/* Real VTE/GObject ownership; no terminal child, PTY or native admission. */
#include <gtk/gtk.h>
#include <vte/vte.h>

static gpointer watched_identity;
static guint identity_frees;
static void counted_free(gpointer value)
{
    if (value == watched_identity) identity_frees++;
    g_free(value);
}
#define g_free counted_free
#include "module.c"
#undef g_free

static void finalized(gpointer data, GObject *object)
{
    (void)object;
    *(gboolean *)data = TRUE;
}

static gboolean finish_cache_dispatch(gpointer loop)
{
    g_main_loop_quit(loop);
    return G_SOURCE_REMOVE;
}

int main(int argc, char **argv)
{
    gtk_init(&argc, &argv);
    guint64 previous_id = 0;
    for (guint cycle = 0; cycle < 100; cycle++) {
        GtkWidget *terminal = vte_terminal_new();
        g_object_ref_sink(terminal);
        gboolean destroyed = FALSE;
        g_object_weak_ref(G_OBJECT(terminal), finalized, &destroyed);
        LynFrame absent = {0};
        g_assert_false(observe(terminal, &absent));
        g_assert_null(g_object_get_data(G_OBJECT(terminal), "lyn-spike-source"));
        g_assert_cmpuint(next_source, ==, previous_id);
        TerminalIdentity *identity = terminal_identity(terminal);
        watched_identity = identity;
        identity_frees = 0;
        g_assert_cmpuint(identity->id, >, previous_id);
        previous_id = identity->id;
        g_assert_cmpuint(identity->revision, ==, 1);
        g_assert_true(terminal_identity(terminal) == identity);
        g_assert_cmpint(identity->registered_at, ==, identity->changed_at);
        guint64 revision = identity->revision;
        /* Real GObject notification, not a physical PTY replacement. Repeated
         * registration must not duplicate the notify callback. */
        g_object_notify(G_OBJECT(terminal), "pty");
        g_assert_cmpuint(identity->revision, ==, revision + 1);
        g_assert_cmpint(identity->changed_at, >=, identity->registered_at);
        GtkWidget *window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
        gboolean window_destroyed = FALSE;
        g_object_weak_ref(G_OBJECT(window), finalized, &window_destroyed);
        revision = identity->revision;
        gtk_container_add(GTK_CONTAINER(window), terminal);
        g_assert_cmpuint(identity->revision, ==, revision + 1);
        revision = identity->revision;
        gtk_widget_show_all(window);
        g_assert_true(gtk_widget_get_mapped(terminal));
        g_assert_cmpuint(identity->revision, ==, revision + 1);
        revision = identity->revision;
        gtk_widget_hide(terminal);
        g_assert_false(gtk_widget_get_mapped(terminal));
        g_assert_cmpuint(identity->revision, ==, revision + 1);
        revision = identity->revision;
        gtk_container_remove(GTK_CONTAINER(window), terminal);
        g_assert_cmpuint(identity->revision, ==, revision + 1);
        gtk_widget_destroy(window);
        g_assert_true(window_destroyed);
        g_assert_cmpuint(identity_frees, ==, 0);
        g_assert_false(destroyed);
        gtk_widget_destroy(terminal);
        g_object_unref(terminal);
        g_assert_true(destroyed);
        g_assert_cmpuint(identity_frees, ==, 1);
        watched_identity = NULL;
    }
    g_assert_null(gtk_window_list_toplevels());
    /* VTE 0.68 FontInfo::unref schedules normal cache destruction after 30s.
     * Dispatch it naturally, without patching VTE or forcing global teardown. */
    GMainLoop *loop = g_main_loop_new(NULL, FALSE);
    g_timeout_add_seconds(35, finish_cache_dispatch, loop);
    g_print("Dispatching the private GTK loop for VTE's 30-second font cache retirement.\n");
    g_main_loop_run(loop);
    g_main_loop_unref(loop);
    g_print("PASS: 100 real VTE identity lifetimes; unique registration, map/unmap/notify/hierarchy callbacks and final free\n");
    return 0;
}
