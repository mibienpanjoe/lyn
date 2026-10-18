/* Metadata only. Wire layout is owned by docs/gnome_native_protocol.md. */
#ifndef LYN_GNOME_WIRE_H
#define LYN_GNOME_WIRE_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define LYN_FRAME_BYTES 112
enum { LYN_HELLO = 1, LYN_BIND, LYN_READY, LYN_QUERY, LYN_REPLY };
enum { LYN_CANDIDATE = 1, LYN_MISSING, LYN_CHANGED, LYN_AMBIGUOUS, LYN_UNAVAILABLE };

/* Never serialize this struct itself (padding, alignment and endian differ). */
typedef struct {
    uint16_t kind, status;
    uint8_t generation[16], request[16];
    uint32_t window, pty, pgid;
    uint64_t source, at_us, changed_us, revision, registered_us;
} LynFrame;

bool lyn_frame_encode(const LynFrame *frame, uint8_t bytes[LYN_FRAME_BYTES]);
/* On refusal, output is unchanged. Input may be unaligned. */
bool lyn_frame_decode(const uint8_t *bytes, size_t length, LynFrame *output);
#endif
