#define _GNU_SOURCE
#include "native_channel.h"
#include "native_discovery.h"
#include <glib-unix.h>
#include <errno.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

/* One module connection, all state owned by the GTK main loop. A discovery
 * record is an address hint only, not connected/admitted provider evidence. */
static int channel = -1;
static guint reader, writer, timer, retry_source;
static gchar *configured_endpoint;
static gchar *configured_runtime;
static guint retry_delay_ms = 100;
static uint8_t incoming[LYN_FRAME_BYTES], outgoing[LYN_FRAME_BYTES], generation[16];
static size_t received, written;
static gint64 started_at, partial_at, queued_at;
static gboolean ready;
static LynNativeQuery on_query;

static void schedule_retry(void);

static bool configured(void)
{
    return configured_endpoint || configured_runtime;
}

static void stop(void)
{
    if (reader) g_source_remove(reader);
    if (writer) g_source_remove(writer);
    if (timer) g_source_remove(timer);
    reader = writer = timer = 0;
    if (channel >= 0) close(channel);
    channel = -1;
    received = written = 0;
    ready = FALSE;
    memset(generation, 0, sizeof(generation));
    memset(incoming, 0, sizeof(incoming));
    memset(outgoing, 0, sizeof(outgoing));
    schedule_retry();
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
    union { struct cmsghdr align; uint8_t bytes[CMSG_SPACE(8 * sizeof(int))]; } control = {0};
    struct iovec buffer = {.iov_base = incoming + received, .iov_len = LYN_FRAME_BYTES - received};
    struct msghdr message = {.msg_iov = &buffer, .msg_iovlen = 1,
                            .msg_control = control.bytes, .msg_controllen = sizeof(control.bytes)};
    ssize_t count = recvmsg(fd, &message, MSG_CMSG_CLOEXEC);
    if (count < 0 && (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR))
        return G_SOURCE_CONTINUE;
    if (count >= 0) {
        /* Kernel-validated SCM_RIGHTS entries own receiver-side duplicates.
         * Close every delivered fd before refusal; Linux closes any excess
         * rights truncated beyond this bounded control buffer. Never use them. */
        for (struct cmsghdr *header = CMSG_FIRSTHDR(&message); header;
             header = CMSG_NXTHDR(&message, header)) {
            if (header->cmsg_level != SOL_SOCKET || header->cmsg_type != SCM_RIGHTS
                || header->cmsg_len < CMSG_LEN(0)) continue;
            size_t bytes = header->cmsg_len - CMSG_LEN(0);
            for (size_t offset = 0; offset + sizeof(int) <= bytes; offset += sizeof(int)) {
                int received_fd;
                memcpy(&received_fd, CMSG_DATA(header) + offset, sizeof(received_fd));
                close(received_fd);
            }
        }
        if (message.msg_controllen || (message.msg_flags & (MSG_TRUNC | MSG_CTRUNC))) {
            stop();
            return G_SOURCE_REMOVE;
        }
    }
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
        retry_delay_ms = 100;
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

static bool connect_once(void)
{
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    const char *endpoint = configured_endpoint;
    char discovered[LYN_NATIVE_ENDPOINT_BYTES];
    if (configured_runtime) {
        if (!lyn_native_discover(configured_runtime, discovered)) return false;
        endpoint = discovered;
    }
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
    started_at = g_get_monotonic_time();
    // Writable watch exists only while a frame is pending, not an idle busy loop.
    // https://docs.gtk.org/glib-unix/func.fd_add.html
    reader = g_unix_fd_add(fd, G_IO_IN | G_IO_HUP | G_IO_ERR | G_IO_NVAL, read_frame, NULL);
    timer = g_timeout_add(25, check_deadlines, NULL);
    LynFrame hello = {.kind = LYN_HELLO};
    if (!reader || !timer || !queue(&hello)) { stop(); return false; }
    return true;
}

static gboolean reconnect(gpointer data)
{
    (void)data;
    retry_source = 0;
    if (configured() && channel < 0 && !connect_once())
        schedule_retry();
    return G_SOURCE_REMOVE;
}

static void schedule_retry(void)
{
    if (!configured() || retry_source || channel >= 0) return;
    retry_source = g_timeout_add(retry_delay_ms, reconnect, NULL);
    retry_delay_ms = MIN(retry_delay_ms * 2, 2000u);
}

static bool configure(const char *endpoint, LynNativeQuery query, bool discovery)
{
    if (configured() || !query || !endpoint || endpoint[0] != '/'
        || strlen(endpoint) >= sizeof(((struct sockaddr_un *)0)->sun_path)) return false;
    if (discovery) configured_runtime = g_strdup(endpoint);
    else configured_endpoint = g_strdup(endpoint);
    on_query = query;
    retry_delay_ms = 100;
    if (!connect_once()) schedule_retry();
    return true;
}

bool lyn_native_start(const char *endpoint, LynNativeQuery query)
{
    return configure(endpoint, query, false);
}

bool lyn_native_start_discovered(const char *runtime, LynNativeQuery query)
{
    return configure(runtime, query, true);
}

void lyn_native_stop(void)
{
    g_clear_pointer(&configured_endpoint, g_free);
    g_clear_pointer(&configured_runtime, g_free);
    if (retry_source) g_source_remove(retry_source);
    retry_source = 0;
    stop();
    on_query = NULL;
    retry_delay_ms = 100;
}
