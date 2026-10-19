/* Public lifecycle boundary, no GTK window or terminal content. */
#define _GNU_SOURCE
#include "../native_channel.h"
#include <assert.h>
#include <errno.h>
#include <glib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

static void query(const LynFrame *request, LynFrame *reply)
{
    (void)request;
    (void)reply;
    assert(!"No query is expected with no broker");
}

static guint descriptors(void)
{
    GDir *directory = g_dir_open("/proc/self/fd", 0, NULL);
    assert(directory);
    guint count = 0;
    while (g_dir_read_name(directory)) count++;
    g_dir_close(directory);
    return count;
}

static gboolean finish(gpointer data)
{
    g_main_loop_quit(data);
    return G_SOURCE_REMOVE;
}

int main(int argc, char **argv)
{
    assert(argc == 2);
    gchar *endpoint = g_build_filename(argv[1], "lifecycle.sock", NULL);
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    assert(strlen(endpoint) < sizeof(address.sun_path));
    strcpy(address.sun_path, endpoint);
    g_main_context_iteration(NULL, FALSE);
    guint baseline = descriptors();
    assert(!lyn_native_start(NULL, query));
    assert(!lyn_native_start("relative", query));
    assert(!lyn_native_start(endpoint, NULL));
    for (guint iteration = 0; iteration < 100; iteration++) {
        assert(lyn_native_start(endpoint, query));
        assert(!lyn_native_start(endpoint, query));
        assert(descriptors() == baseline);
        lyn_native_stop();
        lyn_native_stop();
        assert(descriptors() == baseline);
    }
    int listener = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    assert(listener >= 0);
    assert(bind(listener, (struct sockaddr *)&address, sizeof(address)) == 0);
    assert(listen(listener, 1) == 0);
    GMainLoop *loop = g_main_loop_new(NULL, FALSE);
    g_timeout_add(250, finish, loop);  /* Beyond the cancelled initial retry. */
    g_main_loop_run(loop);
    g_main_loop_unref(loop);
    assert(accept(listener, NULL, NULL) == -1 && errno == EAGAIN);
    close(listener);
    assert(unlink(endpoint) == 0);
    g_free(endpoint);
    assert(descriptors() == baseline);
    g_print("PASS: 100 configured/stopped channel lifecycles retain no FD; stop cancels reconnect\n");
    return 0;
}
