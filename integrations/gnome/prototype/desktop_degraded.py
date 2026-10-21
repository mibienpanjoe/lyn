"""Unresolved native context must not prevent real input or manual capture."""

import json
import sqlite3
import time


def run_module_absent(*, app, window, library_window, database, command, named,
                      nodes, wait_for, pixels, save_screenshot, focus_is):
    from gi.repository import Atspi
    from desktop_accessibility import page_heading
    from desktop_pixels import region_changed, region_has_contrast

    body = "synthetic-module-absent-manual-draft-ENDOFNOTE"
    manual_name = "Synthetic module absent"
    started = time.monotonic()
    command("xdotool", "key", "--delay", "0", "--clearmodifiers", "ctrl+shift+space")

    def input_ready():
        field = named(app.pid, "Capture text")
        return field if field and field.get_state_set().contains(Atspi.StateType.FOCUSED) else None

    wait_for(input_ready, "Module absence blocked the focused capture input")
    input_observed_ms = (time.monotonic() - started) * 1000
    assert command("xdotool", "getwindowpid", command("xdotool", "getactivewindow")) == str(app.pid)
    command("xdotool", "type", "--clearmodifiers", "--delay", "0", body)

    def field():
        candidates = [node for node in nodes(app.pid) if "Text" in node.get_interfaces()
                      and (node.get_attributes() or {}).get("id") == "capture-text"
                      and node.get_text(0, -1) == body]
        assert len(candidates) <= 1
        return candidates[0] if candidates else None

    wait_for(field, "Module absence lost typed input")
    wait_for(lambda: named(app.pid, "Choose context"), "Missing module incorrectly resolved native context")
    command("xdotool", "key", "Return")
    back = wait_for(lambda: named(app.pid, "Back to note"), "Unresolved save did not open chooser")
    with sqlite3.connect(database) as db:
        assert db.execute("SELECT COUNT(*) FROM captures").fetchone()[0] == 0
    assert back.do_action(0)

    def draft_painted():
        text = field()
        if text is None:
            return False
        first = text.get_character_extents(len(body) - 6, Atspi.CoordType.SCREEN)
        last = text.get_character_extents(len(body) - 1, Atspi.CoordType.SCREEN)
        region = (first.x, first.y, last.x + last.width - first.x, first.height)
        current = pixels()
        x, y, width, height = region
        if min(x, y) < 0 or min(width, height) <= 0 or x + width > current.get_width() or y + height > current.get_height():
            return False
        return current if region_has_contrast(current, region) else False

    painted = wait_for(draft_painted, "Unresolved save lost painted draft")
    print(json.dumps({"check": "native_module_absent_input", "savedCount": 0,
                      "inputObservedMs": round(input_observed_ms, 2),
                      "latencyDistribution": False,
                      "screenshot": save_screenshot(painted, "gnome-module-absent-draft-")}), flush=True)
    assert named(app.pid, "Choose context").do_action(0)
    name_input = wait_for(lambda: named(app.pid, "New context name"), "Manual fallback form missing")
    assert name_input.grab_focus()
    command("xdotool", "type", "--clearmodifiers", "--delay", "0", manual_name)
    # Do not move focus before the native input consumes every XTest key.
    wait_for(lambda: "Text" in name_input.get_interfaces() and
             name_input.get_text(0, -1) == manual_name, "Manual context name did not arrive")
    assert named(app.pid, "Create context").do_action(0)
    wait_for(lambda: named(app.pid, f"Context {manual_name}. Change context"), "Manual context not applied")
    wait_for(field, "Manual fallback changed the exact draft")
    assert named(app.pid, "Save").do_action(0)

    def saved():
        with sqlite3.connect(database) as db:
            rows = db.execute("SELECT c.kind,c.text_body,c.branch_name,p.name,p.kind FROM captures c "
                              "JOIN contexts p ON p.id=c.context_id").fetchall()
            if not rows:
                return False
            assert rows == [("text", body, None, manual_name, "standalone")], rows
            assert db.execute("SELECT COUNT(*) FROM media_assets").fetchone()[0] == 0
            return True

    wait_for(saved, "Module absence blocked exact single manual save")
    wait_for(lambda: focus_is(window), "Manual fallback did not restore the original GNOME window")
    command("xdotool", "windowactivate", "--sync", library_window)
    before_navigation = pixels()
    assert named(app.pid, "All captures").do_action(0)

    def library_painted():
        visible = nodes(app.pid)
        heading = page_heading(visible, "All captures")
        row = next((node for node in visible if node.get_name().startswith(
            f"text capture in {manual_name} at ") and body in node.get_name()), None)
        if heading is None or row is None:
            return False
        current = pixels()
        for node in (heading, row):
            rect = node.get_extents(Atspi.CoordType.SCREEN)
            if (min(rect.x, rect.y) < 0 or min(rect.width, rect.height) <= 0
                    or rect.x + rect.width > current.get_width() or rect.y + rect.height > current.get_height()
                    or not region_changed(before_navigation, current, (rect.x, rect.y, rect.width, rect.height))):
                return False
        return current

    painted = wait_for(library_painted, "Manual fallback did not render in Library")
    print(json.dumps({"check": "native_module_absent_manual_save", "savedCount": 1,
                      "draftPreserved": True, "focusRestored": True,
                      "screenshot": save_screenshot(painted, "gnome-module-absent-library-")}), flush=True)
