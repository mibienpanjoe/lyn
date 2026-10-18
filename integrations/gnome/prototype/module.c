/* Developer-only GTK/VTE proof, not connected to Lyn's production broker. */
#include <gtk/gtk.h>
#include <gmodule.h>
#include <gdk/gdkx.h>
#include <vte/vte.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/ioctl.h>
#include <sys/un.h>
#include <unistd.h>

static gchar *endpoint;
static gchar *module_path;
static guint64 next_source;

G_MODULE_EXPORT const gchar *g_module_check_init(GModule *module)
{
    module_path = g_strdup(g_module_name(module));
    /* GTK callbacks must remain valid even if settings remove the module. */
    g_module_make_resident(module);
    return NULL;
}

static void remove_own_module(const char *variable)
{
    const gchar *value = g_getenv(variable);
    if (!value || !module_path)
        return;
    gchar **parts = g_strsplit(value, ":", -1);
    GPtrArray *keep = g_ptr_array_new();
    for (gchar **part = parts; *part; part++)
        if (strcmp(*part, module_path) != 0)
            g_ptr_array_add(keep, *part);
    if (keep->len == 0) {
        g_unsetenv(variable);
    } else {
        g_ptr_array_add(keep, NULL);
        gchar *joined = g_strjoinv(":", (gchar **)keep->pdata);
        g_setenv(variable, joined, TRUE);
        g_free(joined);
    }
    g_ptr_array_free(keep, TRUE);
    g_strfreev(parts);
}

/* Prototype only: enumerate native widget identities, never screen text. */
static void collect(GtkWidget *widget, gpointer data)
{
    GString *frame = data;
    if (frame->len > 6000)
        return;
    if (VTE_IS_TERMINAL(widget)) {
        VtePty *pty = vte_terminal_get_pty(VTE_TERMINAL(widget));
        GtkWidget *top = gtk_widget_get_toplevel(widget);
        GdkWindow *native = gtk_widget_get_window(top);
        if (!pty || !native || !GDK_IS_X11_WINDOW(native))
            return;
        int fd = vte_pty_get_fd(pty);
        unsigned int number;
        pid_t foreground = tcgetpgrp(fd);
        if (foreground <= 0 || ioctl(fd, TIOCGPTN, &number) != 0)
            return;
        guint64 *source = g_object_get_data(G_OBJECT(widget), "lyn-spike-source");
        if (!source) {
            source = g_new(guint64, 1);
            *source = ++next_source;
            g_object_set_data_full(G_OBJECT(widget), "lyn-spike-source", source, g_free);
        }
        if (frame->str[frame->len-1] != '[')
            g_string_append_c(frame, ',');
        g_string_append_printf(frame,
            "{\"sourceId\":%llu,\"windowId\":%lu,\"ptyNumber\":%u,"
            "\"foregroundPgid\":%ld,\"active\":%s,\"windowFocused\":%s}",
            (unsigned long long)*source, (unsigned long)gdk_x11_window_get_xid(native),
            number, (long)foreground, gtk_widget_get_mapped(widget) ? "true" : "false",
            GTK_IS_WINDOW(top) && gtk_window_is_active(GTK_WINDOW(top)) ? "true" : "false");
    }
    if (GTK_IS_CONTAINER(widget))
        gtk_container_forall(GTK_CONTAINER(widget), collect, frame);
}

static void publish(const char *frame, size_t length)
{
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    memcpy(address.sun_path, endpoint, strlen(endpoint) + 1);
    int channel = socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    if (channel >= 0) {
        (void)sendto(channel, frame, length, MSG_NOSIGNAL, (struct sockaddr *)&address,
                     offsetof(struct sockaddr_un, sun_path) + strlen(endpoint) + 1);
        close(channel);
    }
}

static gboolean snapshot(gpointer data)
{
    (void)data;
    GString *frame = g_string_new("{\"version\":0,\"kind\":\"snapshot\",\"sources\":[");
    GList *windows = gtk_window_list_toplevels();
    for (GList *item = windows; item; item = item->next)
        collect(GTK_WIDGET(item->data), frame);
    g_list_free(windows);
    /* Never publish a partially enumerated set as exact evidence. */
    if (frame->len <= 6000) {
        g_string_append(frame, "]}");
        publish(frame->str, frame->len);
    }
    g_string_free(frame, TRUE);
    return G_SOURCE_CONTINUE;
}

G_MODULE_EXPORT void gtk_module_init(gint *argc, gchar ***argv)
{
    (void)argc;
    (void)argv;
    static gboolean initialized = FALSE;
    if (initialized)
        return;
    initialized = TRUE;

    gchar *executable = g_file_read_link("/proc/self/exe", NULL);
    if (!executable)
        return;
    gboolean expected = g_str_has_suffix(executable, "/gnome-terminal-server");
    g_free(executable);
    const gchar *path = g_getenv("LYN_GNOME_TEST_SOCKET");
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    if (!expected || !path || strlen(path) >= sizeof(address.sun_path))
        return;
    /* At startup only: do not make child GTK apps load our terminal adapter. */
    remove_own_module("GTK3_MODULES");
    remove_own_module("GTK_MODULES");
    endpoint = g_strdup(path);
    char frame[128];
    int length = snprintf(frame, sizeof(frame),
                          "{\"version\":0,\"kind\":\"loaded\",\"processId\":%ld}",
                          (long)getpid());
    if (length > 0 && (size_t)length < sizeof(frame))
        publish(frame, (size_t)length);
    g_timeout_add(25, snapshot, NULL);
}
