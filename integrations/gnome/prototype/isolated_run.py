"""Keep display, D-Bus, user directories and runtime completely test-owned."""

import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile


def run_fixture(command, environment, timeout=60, stdout=None):
    # The direct child leads a new session/group. On timeout it has not been
    # reaped, so its group identity cannot have been recycled before killpg.
    process = subprocess.Popen(command, env=environment, start_new_session=True, stdout=stdout)
    try:
        return process.wait(timeout=timeout)
    except BaseException:
        if process.returncode is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait(timeout=5)
        raise


def main(arguments):
    with tempfile.TemporaryDirectory(prefix="lyn-gnome-run-") as directory:
        root = Path(directory)
        environment = dict(os.environ, GSETTINGS_BACKEND="memory", GIO_USE_VFS="local",
                           GTK_USE_PORTAL="0", NO_AT_BRIDGE="1", GTK_A11Y="none",
                           MESA_SHADER_CACHE_DISABLE="true")
        # These ephemeral displays need no persistent GPU compilation cache.
        # Mesa otherwise creates cache files during Xvfb teardown, racing cleanup.
        # https://docs.mesa3d.org/envvars.html#envvar-MESA_SHADER_CACHE_DISABLE
        for variable, subdirectory in (("XDG_RUNTIME_DIR", "runtime"),
                                       ("XDG_CONFIG_HOME", "config"),
                                       ("XDG_DATA_HOME", "data"),
                                       ("XDG_CACHE_HOME", "cache")):
            location = root / subdirectory
            location.mkdir(mode=0o700)
            environment[variable] = str(location)
        if arguments[0] != "-":
            sdk = Path(arguments[0]).resolve(strict=True)
            environment["PATH"] = str(sdk / "root/usr/bin") + os.pathsep + environment["PATH"]
        # No service directories: never auto-start portals, keyrings or a second
        # terminal server. Only the fixture processes are allowed to exist here.
        bus_config = root / "session.conf"
        bus_config.write_text(f"""<busconfig>
          <type>session</type>
          <listen>unix:tmpdir={root / 'runtime'}</listen>
          <auth>EXTERNAL</auth>
          <policy context="default">
            <allow user="*"/>
            <allow own="*"/>
            <allow send_destination="*"/>
            <allow receive_sender="*"/>
          </policy>
        </busconfig>""")
        result = run_fixture([
            "xvfb-run", "-a", "-s",
            "-screen 0 1280x800x24 -nolisten tcp", "dbus-run-session",
            "--config-file", str(bus_config), "--",
            sys.executable, *arguments[1:]], environment)
        assert not list((root / "cache").glob("mesa_shader_cache*")), "Unexpected test-owned GPU cache"
        return result


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
