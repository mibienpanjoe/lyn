"""Synthetic image on the owned Xvfb clipboard; no user's media or microphone."""

from desktop_isolation import require_owned_display


def fixture_rgba():
    row = bytes([230, 30, 30, 255]) * 96 + bytes([30, 220, 60, 255]) * 96
    return 192, 96, row * 96


def publish_image(sdk):
    # Refuse before loading GTK or resolving a clipboard on another display.
    require_owned_display(sdk)
    import os
    os.environ["GDK_BACKEND"] = "x11"
    os.environ.pop("WAYLAND_DISPLAY", None)
    import gi
    gi.require_version("Gtk", "3.0")
    from gi.repository import Gtk, Gdk, GdkPixbuf, GLib
    initialized, _ = Gtk.init_check([])
    assert initialized, "Private clipboard display unavailable"
    assert Gdk.Display.get_default().get_name().split(".")[0] == os.environ["DISPLAY"].split(".")[0]
    width, height, rgba = fixture_rgba()
    image = GdkPixbuf.Pixbuf.new_from_bytes(GLib.Bytes.new(rgba), GdkPixbuf.Colorspace.RGB,
                                          True, 8, width, height, width * 4)
    clipboard = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)
    clipboard.set_image(image)
    # Keep both objects alive while the controller services its GLib context.
    return clipboard, image


def has_fixture_colors(pixbuf, region):
    part = pixbuf.new_subpixbuf(*region)
    pixels, red, green = part.get_pixels(), 0, 0
    for y in range(part.get_height()):
        for x in range(part.get_width()):
            offset = y * part.get_rowstride() + x * part.get_n_channels()
            r, g, b = pixels[offset:offset + 3]
            red += r > 180 and g < 80 and b < 80
            green += r < 80 and g > 180 and b < 100
    return red >= 100 and green >= 100


def run_image_closed_source(*, sdk, app, window, library_window, database, start,
                            command, named, nodes, wait_for, pixels, save_screenshot, close_source):
    """Exercise only normal UI/media gateways after the caller's native readiness."""
    import hashlib
    import json
    import os
    from pathlib import Path
    import select
    import sqlite3
    import subprocess
    from gi.repository import Atspi, GdkPixbuf
    from desktop_accessibility import page_heading
    from desktop_pixels import region_has_contrast

    caption = "synthetic-image-closed-source-caption-ENDOFNOTE"
    manual_name = "Synthetic corrected image"
    command("xdotool", "key", "--delay", "0", "--clearmodifiers", "ctrl+shift+space")
    wait_for(lambda: named(app.pid, "Capture text") and
             named(app.pid, "Context alpha, branch gnome-alpha. Change context"), "Native image context absent")
    draft_window = command("xdotool", "getactivewindow")
    assert command("xdotool", "getwindowpid", draft_window) == str(app.pid)
    # A clipboard owner must dispatch selection requests independently of the
    # controller's synchronous AT-SPI queries and the app's native read.
    clipboard_owner = start(["/usr/bin/python3", str(Path(__file__).resolve()), str(sdk) if sdk else "-"],
                            env=dict(os.environ, GTK_MODULES="", NO_AT_BRIDGE="1", GTK_A11Y="none"),
                            stdout=subprocess.PIPE)
    assert select.select([clipboard_owner.stdout], [], [], 3)[0], "Private image owner did not start"
    assert clipboard_owner.stdout.readline() == b"READY\n" and clipboard_owner.poll() is None
    command("xdotool", "type", "--clearmodifiers", "--delay", "0", caption)
    command("xdotool", "key", "--clearmodifiers", "ctrl+v")
    try:
        wait_for(lambda: named(app.pid, "Screenshot ready to save") and
                 named(app.pid, "Screenshot caption (optional)"), "Native clipboard image did not stage")
    except AssertionError:
        print(json.dumps({"check": "native_image_stage_refused",
                          "nodes": [{"name": node.get_name(), "attributes": node.get_attributes()}
                                    for node in nodes(app.pid) if node.get_name() or (node.get_attributes() or {}).get("id") == "capture-error"],
                          "stagingFiles": [str(path.relative_to(database.parent)) for path in (database.parent / "media/staging").glob("*/*")],
                          "screenshot": save_screenshot(pixels(), "gnome-image-stage-failed-")}), flush=True)
        raise
    staging = database.parent / "media/staging"
    staged = list(staging.glob("*/*.png"))
    assert len(staged) == 1 and not staged[0].is_symlink()
    original = staged[0].read_bytes()
    decoded = GdkPixbuf.Pixbuf.new_from_file(str(staged[0]))
    width, height, rgba = fixture_rgba()
    assert (decoded.get_width(), decoded.get_height(), decoded.get_n_channels()) == (width, height, 4)
    rows = decoded.get_pixels()
    assert b"".join(rows[y * decoded.get_rowstride():y * decoded.get_rowstride() + width * 4]
                    for y in range(height)) == rgba

    def field():
        matches = [node for node in nodes(app.pid) if "Text" in node.get_interfaces()
                   and (node.get_attributes() or {}).get("id") == "capture-text"
                   and node.get_text(0, -1) == caption]
        assert len(matches) <= 1
        return matches[0] if matches else None

    def image_painted(name, with_caption=False):
        images = [node for node in nodes(app.pid) if (node.get_attributes() or {}).get("tag") == "img"
                  and node.get_name() == name and "Component" in node.get_interfaces()]
        assert len(images) <= 1
        if not images or (with_caption and field() is None):
            return False
        rect = images[0].get_extents(Atspi.CoordType.SCREEN)
        current = pixels()
        if (min(rect.x, rect.y) < 0 or min(rect.width, rect.height) <= 0
                or rect.x + rect.width > current.get_width() or rect.y + rect.height > current.get_height()):
            return False
        if not has_fixture_colors(current, (rect.x, rect.y, rect.width, rect.height)):
            return False
        if with_caption:
            text = field()
            first = text.get_character_extents(len(caption) - 6, Atspi.CoordType.SCREEN)
            last = text.get_character_extents(len(caption) - 1, Atspi.CoordType.SCREEN)
            region = (first.x, first.y, last.x + last.width - first.x, first.height)
            x, y, w, h = region
            if min(x, y) < 0 or min(w, h) <= 0 or x + w > current.get_width() or y + h > current.get_height():
                return False
            if not region_has_contrast(current, region):
                return False
        return current

    wait_for(lambda: image_painted("Screenshot ready to save", True), "Staged image/caption did not paint")
    command("xdotool", "windowactivate", "--sync", window)
    assert command("xdotool", "getactivewindow") == window
    command("xdotool", "key", "--delay", "0", "--clearmodifiers", "ctrl+shift+space")
    wait_for(lambda: command("xdotool", "getactivewindow") == draft_window and
             image_painted("Screenshot ready to save", True), "Second invocation did not restore existing image draft")
    assert list(staging.glob("*/*.png")) == staged and staged[0].read_bytes() == original
    assert command("xdotool", "getactivewindow") == draft_window
    close_source()
    assert command("xdotool", "getwindowpid", command("xdotool", "getactivewindow")) == str(app.pid)
    assert named(app.pid, "Save").do_action(0)
    back = wait_for(lambda: named(app.pid, "Back to note"), "Stale image save did not open chooser")
    wait_for(lambda: named(app.pid, "Context source stale. Choose another context"), "Old image lease was not stale")
    with sqlite3.connect(database) as db:
        assert db.execute("SELECT (SELECT COUNT(*) FROM captures), (SELECT COUNT(*) FROM media_assets)").fetchone() == (0, 0)
    assert staged[0].read_bytes() == original
    assert back.do_action(0)
    painted = wait_for(lambda: image_painted("Screenshot ready to save", True), "Refused image save lost painted draft")
    print(json.dumps({"check": "native_closed_source_image_draft", "savedCount": 0,
                      "stagedBytesUnchanged": True, "duplicateInvocationPreserved": True,
                      "screenshot": save_screenshot(painted, "gnome-image-closed-draft-")}), flush=True)
    assert named(app.pid, "Context source stale. Choose another context").do_action(0)
    name_input = wait_for(lambda: named(app.pid, "New context name"), "Manual context form missing")
    assert name_input.grab_focus()
    command("xdotool", "type", "--clearmodifiers", "--delay", "0", manual_name)
    assert named(app.pid, "Create context").do_action(0)
    wait_for(lambda: named(app.pid, f"Context {manual_name}. Change context"), "Manual image context not applied")
    wait_for(lambda: image_painted("Screenshot ready to save", True), "Manual correction lost image/caption")
    assert staged[0].read_bytes() == original
    assert named(app.pid, "Save").do_action(0)

    def saved():
        with sqlite3.connect(database) as db:
            rows = db.execute("SELECT c.kind,c.caption,c.caption_source,c.branch_name,p.name,p.kind,"
                              "m.relative_path,m.checksum,m.byte_size,m.width_px,m.height_px "
                              "FROM captures c JOIN contexts p ON p.id=c.context_id "
                              "JOIN media_assets m ON m.capture_id=c.id").fetchall()
            if not rows:
                return False
            assert len(rows) == 1 and rows[0][:6] == ("image", caption, "user", None, manual_name, "standalone"), rows
            path, checksum, size, saved_width, saved_height = rows[0][6:]
            relative = Path(path)
            assert not relative.is_absolute() and ".." not in relative.parts
            final = database.parent / "media" / relative
            assert final.read_bytes() == original and checksum == hashlib.sha256(original).hexdigest()
            assert (size, saved_width, saved_height) == (len(original), width, height)
            assert db.execute("SELECT COUNT(*) FROM captures").fetchone()[0] == 1
            return True

    wait_for(saved, "Corrected image was not durably saved exactly once")
    wait_for(lambda: named(app.pid, "Screenshot caption (optional)") is None, "Saved image popup remained open")
    assert not staged[0].exists()
    command("xdotool", "windowactivate", "--sync", library_window)
    assert named(app.pid, "All captures").do_action(0)

    def row():
        return next((node for node in nodes(app.pid) if node.get_name().startswith(
            f"image capture in {manual_name} at ") and caption in node.get_name()), None)

    capture = wait_for(row, "Saved image absent from Library")
    assert capture.do_action(0)
    wait_for(lambda: page_heading(nodes(app.pid), "All captures"), "Library heading absent")
    painted = wait_for(lambda: image_painted(caption), "Saved Library detail image did not paint")
    print(json.dumps({"check": "native_closed_source_image_saved", "savedCount": 1,
                      "manualContext": manual_name, "captionAndBytesMatch": True,
                      "sourceWindowClosed": True, "screenshot": save_screenshot(painted, "gnome-image-library-")}), flush=True)
    assert clipboard_owner.poll() is None and app.poll() is None


if __name__ == "__main__":
    import signal
    import sys
    from pathlib import Path
    signal.alarm(45)
    owner = publish_image(None if sys.argv[1] == "-" else Path(sys.argv[1]).resolve(strict=True))
    from gi.repository import GLib
    print("READY", flush=True)
    GLib.MainLoop().run()
