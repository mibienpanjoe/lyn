#include "wire.h"
#include <limits.h>
#include <string.h>

static uint64_t read_be(const uint8_t *bytes, unsigned width)
{
    uint64_t value = 0;
    for (unsigned i = 0; i < width; i++) value = (value << 8) | bytes[i];
    return value;
}

static void write_be(uint8_t *bytes, uint64_t value, unsigned width)
{
    for (unsigned i = width; i > 0; i--) {
        bytes[i-1] = value & 255;
        value >>= 8;
    }
}

static bool nonzero(const uint8_t id[16])
{
    static const uint8_t nil[16] = {0};
    return memcmp(id, nil, 16) != 0;
}

static bool valid(const LynFrame *frame)
{
    if (frame->kind < LYN_HELLO || frame->kind > LYN_REPLY) return false;
    if (frame->kind == LYN_HELLO) return true;
    if (!nonzero(frame->generation)) return false;
    if (frame->kind <= LYN_READY) return true;
    if (!nonzero(frame->request) || !frame->window || !frame->at_us) return false;
    if (frame->kind == LYN_QUERY) return true;
    if (frame->status < LYN_CANDIDATE || frame->status > LYN_UNAVAILABLE) return false;
    if (frame->status != LYN_CANDIDATE) return true;
    return frame->source && frame->pgid && frame->pgid <= INT32_MAX && frame->revision
        && frame->registered_us && frame->registered_us <= frame->changed_us
        && frame->changed_us <= frame->at_us;
}

bool lyn_frame_encode(const LynFrame *frame, uint8_t bytes[LYN_FRAME_BYTES])
{
    if (!valid(frame)) return false;
    memset(bytes, 0, LYN_FRAME_BYTES);
    memcpy(bytes, "LYGN", 4);
    write_be(bytes+4, 1, 2);
    write_be(bytes+6, frame->kind, 2);
    if (frame->kind <= LYN_READY) write_be(bytes+92, 3, 2);
    if (frame->kind >= LYN_BIND) memcpy(bytes+8, frame->generation, 16);
    if (frame->kind >= LYN_QUERY) {
        memcpy(bytes+24, frame->request, 16);
        write_be(bytes+40, frame->window, 4);
        write_be(bytes+60, frame->at_us, 8);
    }
    if (frame->kind == LYN_REPLY) {
        write_be(bytes+94, frame->status, 2);
        if (frame->status == LYN_CANDIDATE) {
            write_be(bytes+44, frame->pty, 4);
            write_be(bytes+48, frame->pgid, 4);
            write_be(bytes+52, frame->source, 8);
            write_be(bytes+68, frame->changed_us, 8);
            write_be(bytes+76, frame->revision, 8);
            write_be(bytes+84, frame->registered_us, 8);
        }
    }
    return true;
}

bool lyn_frame_decode(const uint8_t *bytes, size_t length, LynFrame *output)
{
    if (length != LYN_FRAME_BYTES || memcmp(bytes, "LYGN", 4) || read_be(bytes+4, 2) != 1)
        return false;
    LynFrame frame = {0};
    frame.kind = read_be(bytes+6, 2);
    if (frame.kind >= LYN_BIND) memcpy(frame.generation, bytes+8, 16);
    if (frame.kind >= LYN_QUERY) {
        memcpy(frame.request, bytes+24, 16);
        frame.window = read_be(bytes+40, 4);
        frame.at_us = read_be(bytes+60, 8);
    }
    if (frame.kind == LYN_REPLY) {
        frame.status = read_be(bytes+94, 2);
        if (frame.status == LYN_CANDIDATE) {
            frame.pty = read_be(bytes+44, 4);
            frame.pgid = read_be(bytes+48, 4);
            frame.source = read_be(bytes+52, 8);
            frame.changed_us = read_be(bytes+68, 8);
            frame.revision = read_be(bytes+76, 8);
            frame.registered_us = read_be(bytes+84, 8);
        }
    }
    uint8_t canonical[LYN_FRAME_BYTES];
    if (!lyn_frame_encode(&frame, canonical) || memcmp(bytes, canonical, LYN_FRAME_BYTES))
        return false;
    *output = frame;
    return true;
}
