/* Same-user transport fixture, NOT production peer admission or capture proof. */
#define _GNU_SOURCE
#include "../native_channel.h"
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <glib.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

static unsigned queries;

static void query(const LynFrame *request, LynFrame *reply)
{
    assert(request->kind == LYN_QUERY && reply->kind == LYN_REPLY);
    queries++;
}

static void pump(void)
{
    g_main_context_iteration(NULL, FALSE);
    g_usleep(1000);
}

static void pump_for(gint64 duration)
{
    gint64 deadline = g_get_monotonic_time() + duration;
    while (g_get_monotonic_time() < deadline) pump();
}

static int accept_client(int listener)
{
    gint64 deadline = g_get_monotonic_time() + 2000000;
    while (g_get_monotonic_time() < deadline) {
        int client = accept4(listener, NULL, NULL, SOCK_NONBLOCK | SOCK_CLOEXEC);
        if (client >= 0) return client;
        assert(errno == EAGAIN || errno == EWOULDBLOCK);
        pump();
    }
    assert(!"discovery connection deadline exceeded");
    return -1;
}

static LynFrame receive_frame(int fd)
{
    uint8_t bytes[LYN_FRAME_BYTES];
    size_t received = 0;
    gint64 deadline = g_get_monotonic_time() + 1000000;
    while (received < sizeof(bytes) && g_get_monotonic_time() < deadline) {
        ssize_t count = recv(fd, bytes + received, sizeof(bytes) - received, 0);
        if (count > 0) received += (size_t)count;
        else assert(count < 0 && (errno == EAGAIN || errno == EWOULDBLOCK));
        pump();
    }
    LynFrame frame;
    assert(received == sizeof(bytes) && lyn_frame_decode(bytes, sizeof(bytes), &frame));
    return frame;
}

static void send_frame(int fd, const LynFrame *frame)
{
    uint8_t bytes[LYN_FRAME_BYTES];
    assert(lyn_frame_encode(frame, bytes));
    assert(send(fd, bytes, sizeof(bytes), MSG_NOSIGNAL) == sizeof(bytes));
}

static void handshake(int fd, unsigned generation)
{
    LynFrame hello = receive_frame(fd);
    assert(hello.kind == LYN_HELLO);
    LynFrame bind = {.kind = LYN_BIND};
    bind.generation[15] = (uint8_t)generation;
    send_frame(fd, &bind);
    LynFrame ready = receive_frame(fd);
    assert(ready.kind == LYN_READY && !memcmp(ready.generation, bind.generation, 16));
}

static void one_query(int fd, unsigned generation)
{
    LynFrame request = {.kind = LYN_QUERY, .window = 123, .at_us = g_get_monotonic_time()};
    request.generation[15] = (uint8_t)generation;
    request.request[15] = (uint8_t)generation;
    send_frame(fd, &request);
    LynFrame reply = receive_frame(fd);
    assert(reply.kind == LYN_REPLY && reply.status == LYN_UNAVAILABLE
           && !memcmp(reply.generation, request.generation, 16)
           && !memcmp(reply.request, request.request, 16));
}

static int listener_at(const char *root, const char *instance, gchar **directory, gchar **path)
{
    *directory = g_build_filename(root, instance, NULL);
    assert(mkdir(*directory, 0700) == 0);
    *path = g_build_filename(*directory, "lyn-gnome-v1.sock", NULL);
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    assert(strlen(*path) < sizeof(address.sun_path));
    strcpy(address.sun_path, *path);
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0);
    assert(fd >= 0 && bind(fd, (struct sockaddr *)&address, sizeof(address)) == 0);
    assert(chmod(*path, 0600) == 0 && listen(fd, 1) == 0);
    return fd;
}

static void publish(const char *root, const char *instance)
{
    gchar *temporary = g_build_filename(root, "discovery.next", NULL);
    gchar *current = g_build_filename(root, "lyn-gnome-v1.current", NULL);
    int fd = open(temporary, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    assert(fd >= 0 && strlen(instance) == 35);
    assert(write(fd, instance, 35) == 35 && write(fd, "\n", 1) == 1);
    close(fd);
    assert(rename(temporary, current) == 0);
    g_free(current);
    g_free(temporary);
}

int main(int argc, char **argv)
{
    assert(argc == 2);
    const char *alpha = "gn-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const char *beta = "gn-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const char *root = argv[1];
    assert(!lyn_native_start_discovered(NULL, query));
    assert(!lyn_native_start_discovered("relative", query));
    assert(!lyn_native_start_discovered(root, NULL));
    gchar *copied_root = g_strdup(root);
    assert(lyn_native_start_discovered(copied_root, query));
    memset(copied_root, 'x', strlen(copied_root));
    g_free(copied_root);
    assert(!lyn_native_start_discovered(root, query));
    assert(!lyn_native_start("/tmp/not-used.sock", query));
    gchar *directory_a, *path_a, *directory_b, *path_b;
    int listener_a = listener_at(root, alpha, &directory_a, &path_a);
    publish(root, alpha);
    int client_a = accept_client(listener_a);
    handshake(client_a, 1);
    one_query(client_a, 1);
    assert(queries == 1);
    struct stat before, after;
    assert(lstat(path_a, &before) == 0);
    int listener_b = listener_at(root, beta, &directory_b, &path_b);
    gchar *current = g_build_filename(root, "lyn-gnome-v1.current", NULL);
    assert(chmod(current, 0644) == 0);
    close(client_a);
    close(listener_a); /* Leave its abandoned socket; discovery cannot unlink it. */
    pump_for(250000);
    assert(accept(listener_b, NULL, NULL) == -1 && errno == EAGAIN);
    publish(root, beta);
    int client_b = accept_client(listener_b);
    handshake(client_b, 2);
    pump_for(150000);
    assert(queries == 1); /* No query is replayed through reconnection. */
    uint8_t byte;
    assert(recv(client_b, &byte, 1, 0) == -1 && errno == EAGAIN);
    one_query(client_b, 2);
    assert(queries == 2);
    assert(lstat(path_a, &after) == 0 && before.st_dev == after.st_dev && before.st_ino == after.st_ino);
    lyn_native_stop();
    lyn_native_stop();
    close(client_b);
    pump_for(250000);
    assert(accept(listener_b, NULL, NULL) == -1 && errno == EAGAIN);
    close(listener_b);
    /* Only this test's own fixture nodes are removed by its controller. */
    assert(unlink(current) == 0 && unlink(path_a) == 0 && unlink(path_b) == 0);
    assert(rmdir(directory_a) == 0 && rmdir(directory_b) == 0);
    g_free(current);
    g_free(path_a); g_free(path_b); g_free(directory_a); g_free(directory_b);
    g_print("PASS: late discovery, refused publication, new endpoint/generation, no query replay; stop cancels retry\n");
    return 0;
}
