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
#include "../native_channel.h"

static gchar *endpoint;
static gchar *module_path;
static guint64 next_source;

typedef struct {
    guint64 id;
    guint64 revision;
    gint64 registered_at;
    gint64 changed_at;
} TerminalIdentity;

/* GTK3 map/unmap run on its main loop. Record transitions, not just polled
 * final state: A -> B -> A must invalidate an invocation between those events.
 * https://docs.gtk.org/gtk3/signal.Widget.map.html
 * https://docs.gtk.org/glib/func.get_monotonic_time.html */
static void topology_changed(GtkWidget *widget, gpointer data)
{
    (void)widget;
    TerminalIdentity *identity = data;
    identity->changed_at = g_get_monotonic_time();
    identity->revision++;
}

static void hierarchy_changed(GtkWidget *widget, GtkWidget *previous, gpointer data)
{
    (void)previous;
    topology_changed(widget, data);
}

static void pty_changed(GObject *object, GParamSpec *property, gpointer data)
{
    (void)property;
    topology_changed(GTK_WIDGET(object), data);
}

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

static gboolean observe(GtkWidget *widget, LynFrame *result)
{
    VtePty *pty = vte_terminal_get_pty(VTE_TERMINAL(widget));
    GtkWidget *top = gtk_widget_get_toplevel(widget);
    GdkWindow *native = gtk_widget_get_window(top);
    if (!pty || !native || !GDK_IS_X11_WINDOW(native))
        return FALSE;
    int fd = vte_pty_get_fd(pty);
    unsigned int number;
    pid_t foreground = tcgetpgrp(fd);
    if (foreground <= 0 || ioctl(fd, TIOCGPTN, &number) != 0)
        return FALSE;
    TerminalIdentity *source = g_object_get_data(G_OBJECT(widget), "lyn-spike-source");
    if (!source) {
        source = g_new0(TerminalIdentity, 1);
        source->id = ++next_source;
        source->revision = 1;
        source->registered_at = source->changed_at = g_get_monotonic_time();
        g_object_set_data_full(G_OBJECT(widget), "lyn-spike-source", source, g_free);
        g_signal_connect_after(widget, "map", G_CALLBACK(topology_changed), source);
        g_signal_connect_after(widget, "unmap", G_CALLBACK(topology_changed), source);
        g_signal_connect_after(widget, "hierarchy-changed", G_CALLBACK(hierarchy_changed), source);
        g_signal_connect_after(widget, "notify::pty", G_CALLBACK(pty_changed), source);
    }
    result->window = gdk_x11_window_get_xid(native);
    result->pty = number;
    result->pgid = foreground;
    result->source = source->id;
    result->registered_us = source->registered_at;
    result->changed_us = source->changed_at;
    result->revision = source->revision;
    result->at_us = g_get_monotonic_time();
    return TRUE;
}

/* Prototype only: enumerate native widget identities, never screen text. */
static void collect(GtkWidget *widget, gpointer data)
{
    GString *frame = data;
    if (frame->len > 6000)
        return;
    if (VTE_IS_TERMINAL(widget)) {
        LynFrame source = {0};
        if (!observe(widget, &source)) return;
        GtkWidget *top = gtk_widget_get_toplevel(widget);
        if (frame->str[frame->len-1] != '[')
            g_string_append_c(frame, ',');
        g_string_append_printf(frame,
            "{\"sourceId\":%llu,\"windowId\":%lu,\"ptyNumber\":%u,"
            "\"foregroundPgid\":%ld,\"active\":%s,\"windowFocused\":%s,"
            "\"registeredAtUs\":%lld,\"changedAtUs\":%lld,\"observedAtUs\":%lld,\"revision\":%llu}",
            (unsigned long long)source.source, (unsigned long)source.window,
            source.pty, (long)source.pgid, gtk_widget_get_mapped(widget) ? "true" : "false",
            GTK_IS_WINDOW(top) && gtk_window_is_active(GTK_WINDOW(top)) ? "true" : "false",
            (long long)source.registered_us, (long long)source.changed_us,
            (long long)source.at_us, (unsigned long long)source.revision);
    }
    if (GTK_IS_CONTAINER(widget))
        gtk_container_forall(GTK_CONTAINER(widget), collect, frame);
}

typedef struct {
    const LynFrame *query;
    LynFrame source;
    guint visits, depth, matches;
    gboolean unavailable;
} NativeLookup;

static void lookup_widget(GtkWidget *widget, gpointer data)
{
    NativeLookup *lookup = data;
    if (lookup->unavailable) return;
    if (++lookup->visits > 2048 || lookup->depth >= 32) {
        lookup->unavailable = TRUE;
        return;
    }
    if (VTE_IS_TERMINAL(widget) && gtk_widget_get_mapped(widget)) {
        GtkWidget *top = gtk_widget_get_toplevel(widget);
        GdkWindow *native = gtk_widget_get_window(top);
        if (!native || !GDK_IS_X11_WINDOW(native) ||
            gdk_x11_window_get_xid(native) != lookup->query->window) return;
        LynFrame source = {0};
        if (!observe(widget, &source)) lookup->unavailable = TRUE;
        else if (source.window == lookup->query->window) {
            lookup->source = source;
            lookup->matches++;
        }
    }
    if (GTK_IS_CONTAINER(widget)) {
        lookup->depth++;
        gtk_container_forall(GTK_CONTAINER(widget), lookup_widget, lookup);
        lookup->depth--;
    }
}

static void answer_query(const LynFrame *query, LynFrame *reply)
{
    NativeLookup lookup = {.query = query};
    GList *windows = gtk_window_list_toplevels();
    for (GList *item = windows; item; item = item->next) {
        if (lookup.unavailable) break;
        lookup_widget(GTK_WIDGET(item->data), &lookup);
    }
    g_list_free(windows);
    reply->at_us = g_get_monotonic_time();
    if (lookup.unavailable || query->at_us > reply->at_us) reply->status = LYN_UNAVAILABLE;
    else if (!lookup.matches) reply->status = LYN_MISSING;
    else if (lookup.matches > 1) reply->status = LYN_AMBIGUOUS;
    else if (lookup.source.changed_us >= query->at_us) reply->status = LYN_CHANGED;
    else {
        // Preserve broker-issued generation/request/window; copy metadata only.
        reply->status = LYN_CANDIDATE;
        reply->pty = lookup.source.pty;
        reply->pgid = lookup.source.pgid;
        reply->source = lookup.source.source;
        reply->registered_us = lookup.source.registered_us;
        reply->changed_us = lookup.source.changed_us;
        reply->revision = lookup.source.revision;
    }
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

static gboolean connect_when_dispatching(gpointer data)
{
    /* Start protocol deadlines only once GTK can dispatch channel I/O.
     * Initialization work before the main loop is not peer response latency. */
    (void)lyn_native_connect((const gchar *)data, answer_query);
    return G_SOURCE_REMOVE;
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
    const gchar *stream = g_getenv("LYN_GNOME_TEST_STREAM");
    if (stream)
        g_idle_add_full(G_PRIORITY_DEFAULT_IDLE, connect_when_dispatching,
                        g_strdup(stream), g_free);
}
