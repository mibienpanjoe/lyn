"""Release only the fixed private FIFO awaited by the synthetic Bash child."""

from contextlib import ExitStack
import os
from pathlib import Path
import stat


def release_shell(runtime):
    runtime = Path(runtime)
    root = runtime.parent
    assert runtime.is_absolute() and runtime.name == "runtime"
    assert root.name.startswith("lyn-gnome-run-") and root.resolve(strict=True) == root
    def private(metadata, kind, mode):
        assert kind(metadata.st_mode) and metadata.st_uid == os.getuid()
        assert metadata.st_mode & 0o7777 == mode
    with ExitStack() as cleanup:
        root_fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        cleanup.callback(os.close, root_fd)
        private(os.fstat(root_fd), stat.S_ISDIR, 0o700)
        directory = os.open("runtime", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=root_fd)
        cleanup.callback(os.close, directory)
        private(os.fstat(directory), stat.S_ISDIR, 0o700)
        before = os.stat("shell-exit.fifo", dir_fd=directory, follow_symlinks=False)
        private(before, stat.S_ISFIFO, 0o600)
        assert before.st_nlink == 1
        writer = os.open("shell-exit.fifo", os.O_WRONLY | os.O_NONBLOCK | os.O_NOFOLLOW, dir_fd=directory)
        cleanup.callback(os.close, writer)
        after = os.fstat(writer)
        private(after, stat.S_ISFIFO, 0o600)
        assert after.st_nlink == 1 and (before.st_dev, before.st_ino) == (after.st_dev, after.st_ino)
        assert os.write(writer, b"\n") == 1
