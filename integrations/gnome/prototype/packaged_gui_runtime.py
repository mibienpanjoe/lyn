"""Run the ordinary packaged GUI, only after filesystem and desktop admission."""
import json
import os
from pathlib import Path
import select
import signal
import sqlite3
import subprocess
import sys
import time

from packaged_gui_guard import require_private_filesystem
from desktop_isolation import require_owned_display
from desktop_accessibility import page_heading
from packaged_gui_pixels import read_text, normalize
from desktop_lifecycle import _pin_owned_app


def preserves_previous(rows, contexts, previous_rows, previous_contexts):
    return (all(rows.get(key) == value for key, value in previous_rows.items())
            and (not previous_contexts or contexts == previous_contexts))


def main():
    control_path = Path(sys.argv[1])
    control = json.loads(control_path.read_text())
    proof = require_private_filesystem(control)
    sdk = Path(sys.argv[2]) if sys.argv[2] != '-' else None
    require_owned_display(sdk)
    print(json.dumps({'check': 'packaged_gui_isolation', **proof,
                      'systemRootOwner': Path('/').stat().st_uid}), flush=True)
    if sys.argv[3:] == ['--guard-only']:
        return
    assert len(sys.argv) == 3
    root = Path(os.environ['XDG_RUNTIME_DIR']).parent
    assert root.name.startswith('lyn-gnome-run-')
    assert os.environ['DBUS_SESSION_BUS_ADDRESS'].startswith(f'unix:path={root}/runtime/')
    environment = dict(os.environ, LANG='C.UTF-8', GTK_MODULES='atk-bridge')
    for name in ('NO_AT_BRIDGE', 'GTK_A11Y'):
        environment.pop(name, None)
        os.environ.pop(name, None)
    os.environ.update(environment)
    import gi
    gi.require_version('Atspi', '2.0')
    gi.require_version('Gdk', '3.0')
    from gi.repository import Atspi, Gdk, GLib
    Atspi.set_timeout(500, 500)
    assert Gdk.init_check([])[0]
    children = []
    applications = []
    app = None
    pin = None

    def start(command, env=environment, **kwargs):
        process = subprocess.Popen(command, env=env, stdin=subprocess.DEVNULL, **kwargs)
        children.append(process)
        return process

    def command(*args):
        return subprocess.check_output(args, env=environment, text=True, timeout=3).strip()

    def nodes():
        for _ in range(32):
            if not GLib.MainContext.default().pending(): break
            GLib.MainContext.default().iteration(False)
        found = []
        def walk(node, depth=0):
            assert depth <= 32 and len(found) < 4096
            found.append(node)
            for index in range(node.get_child_count()):
                child = node.get_child_at_index(index)
                if child: walk(child, depth + 1)
        desktop = Atspi.get_desktop(0)
        for index in range(desktop.get_child_count()):
            candidate = desktop.get_child_at_index(index)
            if candidate and candidate.get_process_id() == app.pid:
                for frame_index in range(candidate.get_child_count()):
                    frame = candidate.get_child_at_index(frame_index)
                    if frame and frame.get_state_set().contains(Atspi.StateType.SHOWING):
                        walk(frame)
        return found

    def wait(check, message):
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            assert app is None or app.poll() is None, 'Packaged app exited'
            try: result = check()
            except GLib.GError as error:
                if error.domain != 'atspi_error': raise
                result = None
            if result: return result
            time.sleep(.05)
        raise AssertionError(message)

    def button(name):
        return next((node for node in nodes() if node.get_name() == name
                     and node.get_role() == Atspi.Role.PUSH_BUTTON
                     and node.get_state_set().contains(Atspi.StateType.SHOWING)), None)

    def named(name):
        return next((node for node in nodes() if node.get_name() == name
                     and node.get_attributes().get('tag') in ('input', 'textarea')), None)

    def contains(text):
        return any(text in node.get_name() or ('Text' in node.get_interfaces()
                   and text in node.get_text(0, -1)) for node in nodes())

    def pixels():
        Gdk.Display.get_default().sync()
        window = Gdk.get_default_root_window()
        image = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
        assert image
        return image

    def screenshot(name, image=None):
        image = image or pixels()
        assert image.savev(str(control_path.parent / name), 'png', [], [])

    def region(node, image):
        box = node.get_component_iface().get_extents(Atspi.CoordType.SCREEN)
        if (box.x < 0 or box.y < 0 or box.width <= 0 or box.height <= 0
                or box.x + box.width > image.get_width() or box.y + box.height > image.get_height()):
            return None
        return (box.x, box.y, box.width, box.height)

    paint_state = {}

    def painted(title, body=None):
        current = nodes()
        heading = page_heading(current, title)
        if not heading: return None
        image = pixels()
        rectangles = [region(heading, image)]
        if body is not None:
            row = next((node for node in current if node.get_state_set().contains(Atspi.StateType.SHOWING)
                        and node.get_attributes().get('id', '').startswith('capture-row-')
                        and body in node.get_name()), None)
            if not row: return None
            rectangles.append(region(row, image))
        if not all(rectangles): return None
        title_text = read_text(image, rectangles[0], sdk)
        paint_state.update(title=title, titleOcr=title_text, rectangles=rectangles)
        if title_text != normalize(title): return None
        body_text = read_text(image, rectangles[1], sdk, single_line=False) if body is not None else None
        paint_state.update(bodyOcr=body_text)
        if body is not None and normalize(body) not in body_text: return None
        print(json.dumps({'check': 'packaged_gui_painted', 'title': title_text,
                          'bodyOcr': body_text, 'rectangles': rectangles}), flush=True)
        return image

    database = Path(environment['XDG_DATA_HOME']) / 'com.mibienpanjoe.lyn/lyn.db'

    def snapshot():
        with sqlite3.connect(f'file:{database}?mode=ro', uri=True) as db:
            db.row_factory = sqlite3.Row
            rows = {row['id']: dict(row) for row in db.execute('SELECT * FROM captures ORDER BY id')}
            contexts = [dict(row) for row in db.execute('SELECT * FROM contexts ORDER BY id')]
            return rows, contexts

    def stop_app():
        nonlocal app, pin
        if app is not None and app.poll() is None and pin is not None:
            signal.pidfd_send_signal(pin, signal.SIGTERM)
            try: app.wait(timeout=3)
            except subprocess.TimeoutExpired:
                signal.pidfd_send_signal(pin, signal.SIGKILL)
                app.wait(timeout=3)
        if pin is not None: os.close(pin)
        app, pin = None, None

    try:
        wm = sdk / 'root/usr/bin/openbox' if sdk else Path('/usr/bin/openbox')
        wm_env = dict(environment)
        if sdk:
            wm_env.update(LD_LIBRARY_PATH=str(sdk / 'root/usr/lib/x86_64-linux-gnu'),
                          XDG_DATA_DIRS=str(sdk / 'root/usr/share') + ':/usr/share')
        start([str(wm), '--sm-disable', '--config-file', str(Path(__file__).with_name('desktop-openbox.xml'))],
              env=wm_env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait(lambda: '_NET_ACTIVE_WINDOW' in command('xprop', '-root', '_NET_SUPPORTED'), 'Private Openbox not ready')
        address = f'unix:path={root}/runtime/package-a11y.sock'
        bus = start(['/usr/bin/dbus-daemon', '--nofork', '--print-address',
                     '--config-file=' + str(Path(__file__).with_name('desktop-a11y.conf')),
                     '--address=' + address], env=dict(environment, AT_SPI_BUS_ADDRESS=address),
                    stdout=subprocess.PIPE, text=True)
        assert select.select([bus.stdout], [], [], 5)[0]
        observed = bus.stdout.readline().strip()
        assert observed.startswith(address)
        environment['AT_SPI_BUS_ADDRESS'] = observed
        os.environ['AT_SPI_BUS_ADDRESS'] = observed
        assert len(control['runs']) in (1, 3)
        bodies = []
        previous_rows, previous_contexts = {}, []
        for step, run in enumerate(control['runs']):
            prefix = f'{step + 1}-' if len(control['runs']) == 3 else ''
            executable = Path(run['binary'])
            with (control_path.parent / (prefix + 'app.log')).open('w') as log:
                app = start([str(executable)], cwd=control_path.parent, stdout=log, stderr=subprocess.STDOUT)
                applications.append(app)
                pin = _pin_owned_app(app, executable, root / 'runtime', expected_cwd=control_path.parent)
                wait(lambda: page_heading(nodes(), 'Recent'), 'Packaged Library heading not accessible')
                rows, contexts = snapshot()
                assert rows == previous_rows and contexts == previous_contexts, 'Replacement changed existing data'
                for retained in bodies:
                    wait(lambda: painted('Recent', retained), 'Retained note did not paint after replacement')
                if bodies:
                    screenshot(prefix + 'retained-library.png')
                windows = command('xdotool', 'search', '--onlyvisible', '--pid', str(app.pid)).split()
                assert len(windows) == 1
                library_window = windows[0]
                command('xdotool', 'windowactivate', '--sync', library_window)
                assert wait(lambda: button('Settings'), 'Settings action absent').do_action(0)
                wait(lambda: page_heading(nodes(), 'Settings') and contains(run['version']), 'Real settings/version not rendered')
                screenshot(prefix + 'settings.png', wait(lambda: painted('Settings'), 'Settings heading did not paint'))
                # The ordinary production startup has no test-only helper override.
                for helper in ('lyn-context', 'lyn-browser-host'):
                    path = Path(control['home']) / '.local/share/lyn/bin' / helper
                    assert path.is_symlink() and path.samefile(executable), helper
                command('xdotool', 'key', '--clearmodifiers', 'ctrl+shift+space')
                field = wait(lambda: named('Capture text'), 'Global shortcut did not open capture')
                chooser = button('Choose context')
                if chooser:
                    assert chooser.do_action(0)
                    if bodies:
                        assert wait(lambda: button('Use context Package GUI context'), 'Saved context missing after replacement').do_action(0)
                    else:
                        entry = wait(lambda: named('New context name'), 'Context creation field absent')
                        assert entry.get_component_iface().grab_focus()
                        command('xdotool', 'type', '--clearmodifiers', '--delay', '0', 'Package GUI context')
                        assert wait(lambda: button('Create context'), 'Create context action absent').do_action(0)
                    field = wait(lambda: named('Capture text'), 'Context creation did not return to capture')
                assert field.get_component_iface().grab_focus()
                body = run['body']
                command('xdotool', 'type', '--clearmodifiers', '--delay', '0', body)
                wait(lambda: contains(body), 'Draft text not visible')
                command('xdotool', 'key', 'Return')
                def persisted():
                    rows, contexts = snapshot()
                    valid = (len(rows) == len(bodies) + 1
                            and {row['text_body'] for row in rows.values()} == set(bodies + [body])
                            and preserves_previous(rows, contexts, previous_rows, previous_contexts)
                            and len(contexts) == 1 and contexts[0]['name'] == 'Package GUI context'
                            and {row['context_id'] for row in rows.values()} == {contexts[0]['id']})
                    return (rows, contexts) if valid else None
                accepted = wait(persisted, 'Packaged save did not persist exactly one note')
                wait(lambda: named('Capture text') is None, 'Saved popup did not dismiss')
                command('xdotool', 'windowactivate', '--sync', library_window)
                assert wait(lambda: button('Recent'), 'Recent navigation absent').do_action(0)
                wait(lambda: page_heading(nodes(), 'Recent') and contains(body), 'Saved note not rendered in Library')
                bodies.append(body)
                previous_rows, previous_contexts = accepted
                for retained in bodies:
                    wait(lambda: painted('Recent', retained), 'Saved or retained note did not paint')
                screenshot(prefix + 'saved-library.png', wait(lambda: painted('Recent', body), 'Library heading/note did not paint'))
                assert snapshot() == accepted, 'Rendering changed accepted capture/context data'
                print(json.dumps({'check': 'packaged_gui', 'version': run['version'],
                                  'exactSave': True, 'libraryAccessible': True, 'helpersInPrivateHome': True,
                                  'captureCount': len(previous_rows), 'preservedPreviousData': step > 0}), flush=True)
            stop_app()
            assert snapshot() == (previous_rows, previous_contexts), 'Stopping changed accepted capture/context data'
    except BaseException:
        if app is not None and app.poll() is None:
            try:
                screenshot('failure.png')
                print(json.dumps({'check': 'packaged_gui_paint_refused', **paint_state}), flush=True)
                print(json.dumps({'check': 'packaged_gui_failure_nodes',
                    'nodes': [{'name': node.get_name(), 'role': node.get_role_name(),
                               'interfaces': list(node.get_interfaces()), 'attributes': node.get_attributes()} for node in nodes()][:200]}), flush=True)
                print(command('xdotool', 'search', '--onlyvisible', '--pid', str(app.pid)), flush=True)
            except Exception as diagnostic:
                print('GUI diagnostic unavailable: ' + str(diagnostic), flush=True)
        raise
    finally:
        stop_app()
        for process in reversed(children):
            if process in applications: continue
            if process.poll() is None: process.terminate()
            try: process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)


if __name__ == '__main__':
    main()
