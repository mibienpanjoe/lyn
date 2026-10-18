#include "../wire.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static LynFrame expected(unsigned index)
{
    LynFrame frame = {0};
    frame.kind = index < 4 ? index + 1 : LYN_REPLY;
    if (index >= 1) frame.generation[15] = 1;
    if (index >= 3) {
        frame.request[15] = 2;
        frame.window = 0x12345678;
        frame.at_us = index == 3 ? 258 : 220;
    }
    if (index >= 4) frame.status = index == 9 ? 1 : index - 3;
    if (index == 4 || index == 9) {
        frame.pgid = 42;
        frame.source = 17;
        frame.changed_us = 150;
        frame.revision = 3;
        frame.registered_us = 100;
    }
    if (index == 9) {
        frame.pty = frame.pgid = 0x01020304;
        frame.source = UINT64_C(0x0102030405060708);
        frame.at_us = UINT64_C(0x0100000000000200);
        frame.changed_us = UINT64_C(0x0100000000000100);
        frame.revision = UINT64_C(0x0807060504030201);
        frame.registered_us = UINT64_C(0x0100000000000000);
    }
    return frame;
}

int main(int argc, char **argv)
{
    assert(argc == 3);
    FILE *file = fopen(argv[1], "r");
    assert(file);
    char name[32], hex[225];
    unsigned count = 0;
    const char *names[] = {"hello", "bind", "ready", "query", "candidate", "missing",
                          "changed", "ambiguous", "unavailable", "wide_candidate"};
    while (fscanf(file, "%31s %224s", name, hex) == 2) {
        assert(count < 10 && strcmp(name, names[count]) == 0 && strlen(hex) == 224);
        uint8_t golden[LYN_FRAME_BYTES], encoded[LYN_FRAME_BYTES];
        for (unsigned i = 0; i < LYN_FRAME_BYTES; i++) {
            char pair[] = {hex[2*i], hex[2*i+1], 0};
            char *end;
            unsigned long value = strtoul(pair, &end, 16);
            assert(*end == 0 && value <= 255);
            golden[i] = value;
        }
        LynFrame frame = expected(count), decoded;
        assert(lyn_frame_encode(&frame, encoded));
        assert(memcmp(golden, encoded, sizeof(golden)) == 0);
        assert(lyn_frame_decode(golden, sizeof(golden), &decoded));
        assert(lyn_frame_encode(&decoded, encoded));
        assert(memcmp(golden, encoded, sizeof(golden)) == 0);
        for (unsigned length = 0; length < LYN_FRAME_BYTES; length++)
            assert(!lyn_frame_decode(golden, length, &decoded));
        uint8_t oversized[LYN_FRAME_BYTES+1] = {0};
        memcpy(oversized, golden, sizeof(golden));
        assert(!lyn_frame_decode(oversized, sizeof(oversized), &decoded));
        for (unsigned i = 96; i < LYN_FRAME_BYTES; i++) {
            golden[i] = 1;
            assert(!lyn_frame_decode(golden, sizeof(golden), &decoded));
            golden[i] = 0;
        }
        count++;
    }
    assert(feof(file) && count == 10);
    fclose(file);
    file = fopen(argv[2], "r");
    assert(file);
    unsigned refused = 0;
    while (fscanf(file, "%31s %224s", name, hex) == 2) {
        assert(strlen(hex) == LYN_FRAME_BYTES * 2);
        uint8_t invalid[LYN_FRAME_BYTES];
        for (unsigned i = 0; i < LYN_FRAME_BYTES; i++) {
            char pair[] = {hex[2*i], hex[2*i+1], 0};
            char *end;
            unsigned long value = strtoul(pair, &end, 16);
            assert(*end == 0 && value <= 255);
            invalid[i] = value;
        }
        LynFrame output = expected(4);
        unsigned char before[sizeof(output)];
        memcpy(before, &output, sizeof(output));
        assert(!lyn_frame_decode(invalid, sizeof(invalid), &output));
        assert(memcmp(before, &output, sizeof(output)) == 0);
        refused++;
    }
    assert(feof(file) && refused == 22);
    fclose(file);
    puts("PASS: C codec matches 10 valid / 22 refused shared Rust vectors; bounded decode and unchanged output");
    return 0;
}
