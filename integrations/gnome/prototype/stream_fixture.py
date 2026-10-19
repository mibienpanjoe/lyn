"""Fixture broker for the native wire contract; never a production authority."""

from pathlib import Path
import os
import socket
import struct
import time


class NativeStream:
    def __init__(self, directory, socket_name="native.sock"):
        self.path = str(Path(directory) / socket_name)
        self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.listener.bind(self.path)
        self.listener.listen(1)
        self.listener.settimeout(5)
        self.channel = None
        self.counter = 0
        self.vectors = {name: bytes.fromhex(hexadecimal) for name, hexadecimal in
                        (line.split() for line in Path(__file__).resolve().parent.parent.joinpath(
                            "wire_vectors.txt").read_text().splitlines())}

    def receive(self):
        result = bytearray()
        while len(result) < 112:
            part = self.channel.recv(112-len(result))
            assert part, "Native module disconnected before completing its frame"
            result.extend(part)
        return bytes(result)

    def handshake(self, server):
        self.channel, _ = self.listener.accept()
        self.channel.settimeout(2)
        credentials = struct.unpack("3i", self.channel.getsockopt(
            socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
        assert credentials == (server.pid, os.getuid(), os.getgid())
        assert self.receive() == self.vectors["hello"]
        # Fragment deliberately: the module must assemble a bounded full frame.
        bind = self.vectors["bind"]
        for fragment in (bind[:7], bind[7:41], bind[41:]):
            self.channel.sendall(fragment)
        assert self.receive() == self.vectors["ready"]

    def query(self, source, invoked_us=None, status=1):
        invoked_us = invoked_us if invoked_us is not None else time.monotonic_ns() // 1000
        self.counter += 1
        request = bytearray(self.vectors["query"])
        request[24:40] = self.counter.to_bytes(16, "big")
        struct.pack_into("!I", request, 40, source["windowId"])
        struct.pack_into("!Q", request, 60, invoked_us)
        self.channel.sendall(request)
        reply = self.receive()
        assert reply[:8] == b"LYGN\x00\x01\x00\x05"
        assert reply[8:44] == request[8:44]
        actual_status = struct.unpack_from("!H", reply, 94)[0]
        allowed = status if isinstance(status, tuple) else (status,)
        assert actual_status in allowed, ("native reply status", actual_status, "expected", status)
        assert not any(reply[92:94] + reply[96:112])
        observed_us = struct.unpack_from("!Q", reply, 60)[0]
        assert observed_us <= time.monotonic_ns() // 1000
        if actual_status == 1:
            pty, pgid, identity = struct.unpack_from("!IIQ", reply, 44)
            changed, revision, registered = struct.unpack_from("!QQQ", reply, 68)
            assert (pty, pgid, identity) == (source["ptyNumber"], source["foregroundPgid"],
                                            source["sourceId"])
            assert 0 < registered <= changed < invoked_us <= observed_us
            assert revision == source["revision"]
        else:
            assert not any(reply[44:60] + reply[68:92])
        return reply

    def reject_wrong_generation(self):
        query = bytearray(self.vectors["query"])
        query[23] = 9
        self.channel.sendall(query)
        assert self.channel.recv(1) == b"", "Wrong generation must close the native connection"

    def close(self):
        if self.channel:
            self.channel.close()
        self.listener.close()
