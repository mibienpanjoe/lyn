#define _GNU_SOURCE
#include "native_channel.h"
#include <glib-unix.h>
#include <errno.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

/* One module connection, all state owned by the GTK main loop. Reconnect and
 * production endpoint discovery are not enabled by the prototype harness. */
static int channel = -1;
static guint reader, writer, timer;
static uint8_t incoming[LYN_FRAME_BYTES], outgoing[LYN_FRAME_BYTES], generation[16];
static size_t received, written;
static gint64 started_at, partial_at, queued_at;
static gboolean ready;
static LynNativeQuery on_query;

static void stop(void)
{
    if (reader) g_source_remove(reader);
    if (writer) g_source_remove(writer);
    if (timer) g_source_remove(timer);
    reader = writer = timer = 0;
    if (channel >= 0) close(channel);
    channel = -1;
    memset(generation, 0, sizeof(generation));
}

static gboolean write_frame(gint fd, GIOCondition condition, gpointer data)
{
    (void)data;
    if (condition & (G_IO_HUP | G_IO_ERR | G_IO_NVAL)) { stop(); return G_SOURCE_REMOVE; }
    ssize_t count = send(fd, outgoing+written, LYN_FRAME_BYTES-written, MSG_NOSIGNAL);
    if (count < 0 && (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR))
        return G_SOURCE_CONTINUE;
    if (count <= 0) { stop(); return G_SOURCE_REMOVE; }
    written += (size_t)count;
    if (written != LYN_FRAME_BYTES) return G_SOURCE_CONTINUE;
    writer = 0;
    return G_SOURCE_REMOVE;
}

static bool queue(const LynFrame *frame)
{
    // Never grow an output queue or block GTK when the broker stops reading.
    if (writer || !lyn_frame_encode(frame, outgoing)) return false;
    written = 0;
    queued_at = g_get_monotonic_time();
    writer = g_unix_fd_add(channel, G_IO_OUT | G_IO_HUP | G_IO_ERR | G_IO_NVAL,
                           write_frame, NULL);
    return writer != 0;
}

static gboolean read_frame(gint fd, GIOCondition condition, gpointer data)
{
    (void)data;
    if (condition & (G_IO_HUP | G_IO_ERR | G_IO_NVAL)) { stop(); return G_SOURCE_REMOVE; }
    // At most one bounded frame per dispatch; GLib re-arms readable data.
    ssize_t count = recv(fd, incoming+received, LYN_FRAME_BYTES-received, 0);
    if (count < 0 && (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR))
        return G_SOURCE_CONTINUE;
    if (count <= 0) { stop(); return G_SOURCE_REMOVE; }
    if (!received) partial_at = g_get_monotonic_time();
    received += (size_t)count;
    if (received != LYN_FRAME_BYTES) return G_SOURCE_CONTINUE;
    LynFrame frame, reply = {0};
    if (!lyn_frame_decode(incoming, sizeof(incoming), &frame)) { stop(); return G_SOURCE_REMOVE; }
    received = 0;
    if (!ready && frame.kind == LYN_BIND) {
        memcpy(generation, frame.generation, 16);
        reply.kind = LYN_READY;
        memcpy(reply.generation, generation, 16);
        ready = TRUE;
    } else if (ready && frame.kind == LYN_QUERY && !memcmp(generation, frame.generation, 16)) {
        reply = frame;
        reply.kind = LYN_REPLY;
        reply.at_us = g_get_monotonic_time();
        reply.status = LYN_UNAVAILABLE;
        on_query(&frame, &reply);
    } else { stop(); return G_SOURCE_REMOVE; }
    if (!queue(&reply)) { stop(); return G_SOURCE_REMOVE; }
    return G_SOURCE_CONTINUE;
}

static gboolean check_deadlines(gpointer data)
{
    (void)data;
    gint64 now = g_get_monotonic_time();
    if ((!ready && now-started_at >= 500000) || (received && now-partial_at >= 120000)
        || (writer && now-queued_at >= 120000)) {
        stop();
        return G_SOURCE_REMOVE;
    }
    return G_SOURCE_CONTINUE;
}

bool lyn_native_connect(const char *endpoint, LynNativeQuery query)
{
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    if (channel >= 0 || !query || !endpoint || !*endpoint || strlen(endpoint) >= sizeof(address.sun_path))
        return false;
    memcpy(address.sun_path, endpoint, strlen(endpoint)+1);
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    // Local connect must complete immediately; a saturated listener is unavailable.
    if (fd < 0) return false;
    struct ucred peer;
    socklen_t length = sizeof(peer);
    if (connect(fd, (struct sockaddr *)&address, sizeof(address)) != 0
        || getsockopt(fd, SOL_SOCKET, SO_PEERCRED, &peer, &length) != 0
        || length != sizeof(peer) || peer.uid != geteuid()) {
        close(fd);
        return false;
    }
    channel = fd;
    received = 0;
    ready = FALSE;
    on_query = query;
    started_at = g_get_monotonic_time();
    // Writable watch exists only while a frame is pending, not an idle busy loop.
    // https://docs.gtk.org/glib-unix/func.fd_add.html
    reader = g_unix_fd_add(fd, G_IO_IN | G_IO_HUP | G_IO_ERR | G_IO_NVAL, read_frame, NULL);
    timer = g_timeout_add(25, check_deadlines, NULL);
    LynFrame hello = {.kind = LYN_HELLO};
    if (!reader || !timer || !queue(&hello)) { stop(); return false; }
    return true;
}
