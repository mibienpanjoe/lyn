/* White-box transport fault fixture, never production admission or GTK proof. */
#define _GNU_SOURCE
#include <glib.h>
static gint64 controlled_time;
static gint64 fixture_monotonic(void)
{
    return controlled_time ? controlled_time : g_get_monotonic_time();
}
/* Only this standalone translation unit controls the channel's clock. GLib's
 * scheduler and the separate wall-clock watchdog continue using real time. */
#define g_get_monotonic_time fixture_monotonic
#include "../native_channel.c"
#undef g_get_monotonic_time
#include <assert.h>

static guint ticks, queries;

static gboolean heartbeat(gpointer unused)
{
    (void)unused;
    ticks++;
    return G_SOURCE_CONTINUE;
}

static void query(const LynFrame *request, LynFrame *reply)
{
    (void)request;
    (void)reply;
    queries++;
}

static void pump(void)
{
    g_main_context_iteration(NULL, FALSE);
    g_usleep(1000);
}

static void closed_within(gint64 budget)
{
    gint64 until = g_get_monotonic_time() + budget;
    guint before = ticks;
    while (channel >= 0 && g_get_monotonic_time() < until) pump();
    assert(channel == -1 && !reader && !writer && !timer && retry_source);
    assert(ticks > before + 10); /* Other main-context work was dispatched. */
}

static void exact_boundary(gint64 *origin, gint64 budget)
{
    controlled_time = g_get_monotonic_time();
    *origin = controlled_time;
    controlled_time += budget - 1;
    assert(check_deadlines(NULL) == G_SOURCE_CONTINUE && channel >= 0);
    controlled_time++;
    assert(check_deadlines(NULL) == G_SOURCE_REMOVE && channel == -1);
    controlled_time = 0;
    lyn_native_stop();
}

static LynFrame receive_frame(int peer)
{
    uint8_t bytes[LYN_FRAME_BYTES];
    size_t length = 0;
    gint64 until = g_get_monotonic_time() + 500000;
    while (length < sizeof(bytes) && g_get_monotonic_time() < until) {
        ssize_t count = recv(peer, bytes + length, sizeof(bytes) - length, 0);
        if (count > 0) length += count;
        else assert(count < 0 && (errno == EAGAIN || errno == EWOULDBLOCK));
        pump();
    }
    LynFrame frame;
    assert(length == sizeof(bytes) && lyn_frame_decode(bytes, length, &frame));
    return frame;
}

static int connect_peer(int listener, const char *path, bool admit)
{
    assert(lyn_native_start(path, query));
    int peer = accept4(listener, NULL, NULL, SOCK_CLOEXEC | SOCK_NONBLOCK);
    assert(peer >= 0 && receive_frame(peer).kind == LYN_HELLO);
    if (admit) {
        LynFrame bind = {.kind = LYN_BIND};
        bind.generation[0] = 1;
        uint8_t bytes[LYN_FRAME_BYTES];
        assert(lyn_frame_encode(&bind, bytes));
        assert(send(peer, bytes, sizeof(bytes), MSG_NOSIGNAL) == sizeof(bytes));
        assert(receive_frame(peer).kind == LYN_READY);
    }
    return peer;
}

int main(int argc, char **argv)
{
    assert(argc == 2);
    gchar *path = g_build_filename(argv[1], "channel-fault.sock", NULL);
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    assert(strlen(path) < sizeof(address.sun_path));
    memcpy(address.sun_path, path, strlen(path) + 1);
    int listener = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    assert(listener >= 0 && bind(listener, (struct sockaddr *)&address, sizeof(address)) == 0);
    assert(listen(listener, 1) == 0);
    guint beat = g_timeout_add(1, heartbeat, NULL);

    int peer = connect_peer(listener, path, false);
    closed_within(650000); /* Silent peer, original 500ms handshake. */
    lyn_native_stop();
    close(peer);

    peer = connect_peer(listener, path, true);
    assert(send(peer, "L", 1, MSG_NOSIGNAL) == 1);
    closed_within(250000); /* Incomplete input, original 120ms budget. */
    lyn_native_stop();
    close(peer);

    peer = connect_peer(listener, path, true);
    int size = 4096;
    assert(setsockopt(channel, SOL_SOCKET, SO_SNDBUF, &size, sizeof(size)) == 0);
    /* Fill this fixture's module-to-broker socket, not any PTY. Filler is not
     * protocol data and the peer deliberately never reads it. */
    uint8_t filler[4096] = {0};
    size_t filled = 0;
    while (filled < 1048576) {
        ssize_t count = send(channel, filler, sizeof(filler), MSG_NOSIGNAL);
        if (count < 0) {
            assert(errno == EAGAIN || errno == EWOULDBLOCK);
            break;
        }
        assert(count > 0);
        filled += (size_t)count;
    }
    assert(filled > 0 && filled < 1048576 && !writer);
    LynFrame reply = {.kind = LYN_REPLY, .status = LYN_UNAVAILABLE, .window = 42,
                      .at_us = g_get_monotonic_time()};
    reply.generation[0] = reply.request[0] = 1;
    assert(queue(&reply));
    closed_within(250000); /* Pending output cannot block or grow a queue. */
    assert(queries == 0);
    lyn_native_stop();
    close(peer);

    /* Exact thresholds are separate from the wall-clock watchdog margins.
     * Increasing OR shortening a native budget must fail these checks. */
    peer = connect_peer(listener, path, false);
    exact_boundary(&started_at, 500000);
    close(peer);

    peer = connect_peer(listener, path, true);
    received = 1; /* Same state as the real partial-input case above. */
    exact_boundary(&partial_at, 120000);
    close(peer);

    peer = connect_peer(listener, path, true);
    assert(queue(&reply));
    exact_boundary(&queued_at, 120000);
    close(peer);
    assert(g_source_remove(beat));
    close(listener);
    assert(unlink(path) == 0);
    g_free(path);
    g_print("PASS: exact 500/120ms boundaries; silent/partial/saturated channel closes while GLib dispatches\n");
    return 0;
}
