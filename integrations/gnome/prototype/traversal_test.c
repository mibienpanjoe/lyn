/* Real GTK widget trees; no GNOME admission or terminal stream is involved. */
#define _GNU_SOURCE
#include "module.c"
#include <errno.h>

static void check_snapshot(int diagnostic, gboolean expected)
{
    char bytes[8192];
    g_assert_true(snapshot(NULL));
    ssize_t count = recv(diagnostic, bytes, sizeof(bytes), MSG_DONTWAIT);
    if (expected) g_assert_cmpint(count, >, 0);
    else {
        g_assert_cmpint(count, ==, -1);
        g_assert_true(errno == EAGAIN || errno == EWOULDBLOCK);
    }
}

int main(int argc, char **argv)
{
    gtk_init(&argc, &argv);
    endpoint = g_build_filename(g_getenv("XDG_RUNTIME_DIR"), "traversal.sock", NULL);
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    g_assert_cmpuint(strlen(endpoint), <, sizeof(address.sun_path));
    memcpy(address.sun_path, endpoint, strlen(endpoint) + 1);
    int diagnostic = socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    g_assert_cmpint(diagnostic, >=, 0);
    g_assert_cmpint(bind(diagnostic, (struct sockaddr *)&address, sizeof(address)), ==, 0);

    GtkWidget *window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    GtkWidget *box = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    gtk_container_add(GTK_CONTAINER(window), box);
    check_snapshot(diagnostic, TRUE);
    /* Exactly 2048 visited widgets including window/box is still complete. */
    for (guint index = 0; index < 2046; index++)
        gtk_container_add(GTK_CONTAINER(box), gtk_label_new("fixture"));
    check_snapshot(diagnostic, TRUE);
    gtk_container_add(GTK_CONTAINER(box), gtk_label_new("over limit"));
    check_snapshot(diagnostic, FALSE);
    gtk_widget_destroy(window);
    check_snapshot(diagnostic, TRUE);

    window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    GtkWidget *parent = window;
    for (guint index = 0; index < 40; index++) {
        GtkWidget *child = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
        gtk_container_add(GTK_CONTAINER(parent), child);
        parent = child;
    }
    check_snapshot(diagnostic, FALSE);
    gtk_widget_destroy(window);
    check_snapshot(diagnostic, TRUE);
    close(diagnostic);
    g_assert_cmpint(unlink(endpoint), ==, 0);
    g_clear_pointer(&endpoint, g_free);
    g_print("PASS: diagnostic widget count/depth refusal and recovery on real GTK trees\n");
    return 0;
}
