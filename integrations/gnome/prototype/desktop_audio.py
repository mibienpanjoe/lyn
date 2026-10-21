"""Real recording controls with test-only fixed samples, never a microphone."""

import io
import wave


def assert_synthetic_wav(data):
    with wave.open(io.BytesIO(data), "rb") as reader:
        assert (reader.getnchannels(), reader.getframerate(), reader.getsampwidth(),
                reader.getnframes(), reader.getcomptype()) == (1, 16000, 2, 1600, "NONE")
        assert reader.readframes(1600) == bytes([0, 32]) * 1600


def run_audio_closed_source(*, app, window, library_window, database,
                            command, named, nodes, wait_for, pixels, save_screenshot, close_source):
    import hashlib
    import json
    from pathlib import Path
    import sqlite3
    from gi.repository import Atspi
    from desktop_accessibility import page_heading
    from desktop_pixels import region_has_contrast

    caption = "synthetic-audio-closed-source-caption-ENDOFNOTE"
    manual_name = "Synthetic corrected audio"
    command("xdotool", "key", "--delay", "0", "--clearmodifiers", "ctrl+shift+space")
    wait_for(lambda: named(app.pid, "Capture text") and
             named(app.pid, "Context alpha, branch gnome-alpha. Change context"), "Native audio context absent")
    draft_window = command("xdotool", "getactivewindow")
    assert command("xdotool", "getwindowpid", draft_window) == str(app.pid)

    def caption_field():
        fields = [node for node in nodes(app.pid) if "Text" in node.get_interfaces()
                  and (node.get_attributes() or {}).get("id") == "capture-text"
                  and node.get_text(0, -1) == caption]
        assert len(fields) <= 1
        return fields[0] if fields else None

    command("xdotool", "type", "--clearmodifiers", "--delay", "0", caption)
    # XTest event dispatch returning does not mean WebKit consumed every key.
    # Do not move focus to Voice until the complete caption actually arrived.
    wait_for(caption_field, "Typed audio caption did not arrive before recording")
    assert named(app.pid, "Voice").do_action(0)
    stop = wait_for(lambda: named(app.pid, "Stop recording"), "Synthetic recording did not start")
    wait_for(lambda: any(node.get_name() == "Recording voice note" or
                         ("Text" in node.get_interfaces() and node.get_text(0, -1) == "Recording voice note")
                         for node in nodes(app.pid)), "Recording status absent")
    assert stop.do_action(0)
    wait_for(lambda: named(app.pid, "Remove recording") and
             named(app.pid, "Voice caption (optional)"), "Synthetic WAV did not stage")
    staging = database.parent / "media/staging"
    staged = list(staging.glob("*/*.wav"))
    assert len(staged) == 1 and not staged[0].is_symlink()
    original = staged[0].read_bytes()
    assert_synthetic_wav(original)

    def draft_painted():
        if not (named(app.pid, "Remove recording") and named(app.pid, "Play")
                and named(app.pid, "Voice caption (optional)")):
            return False
        field = caption_field()
        if field is None:
            return False
        first = field.get_character_extents(len(caption) - 6, Atspi.CoordType.SCREEN)
        last = field.get_character_extents(len(caption) - 1, Atspi.CoordType.SCREEN)
        region = (first.x, first.y, last.x + last.width - first.x, first.height)
        current = pixels()
        x, y, w, h = region
        if min(x, y) < 0 or min(w, h) <= 0 or x + w > current.get_width() or y + h > current.get_height():
            return False
        return current if region_has_contrast(current, region) else False

    try:
        wait_for(draft_painted, "Synthetic recording/caption did not paint")
    except AssertionError:
        print(json.dumps({"check": "native_audio_draft_paint_refused",
                          "fields": [{"name": node.get_name(), "attributes": node.get_attributes(),
                                      "text": node.get_text(0, -1)} for node in nodes(app.pid)
                                     if "Text" in node.get_interfaces()],
                          "controls": [node.get_name() for node in nodes(app.pid) if node.get_name()],
                          "screenshot": save_screenshot(pixels(), "gnome-audio-stage-failed-")}), flush=True)
        raise
    command("xdotool", "windowactivate", "--sync", window)
    assert command("xdotool", "getactivewindow") == window
    command("xdotool", "key", "--delay", "0", "--clearmodifiers", "ctrl+shift+space")
    wait_for(lambda: command("xdotool", "getactivewindow") == draft_window and draft_painted(),
             "Second invocation did not restore existing WAV draft")
    assert list(staging.glob("*/*.wav")) == staged and staged[0].read_bytes() == original
    close_source()
    assert command("xdotool", "getwindowpid", command("xdotool", "getactivewindow")) == str(app.pid)
    assert named(app.pid, "Save").do_action(0)
    back = wait_for(lambda: named(app.pid, "Back to note"), "Stale audio save did not open chooser")
    wait_for(lambda: named(app.pid, "Context source stale. Choose another context"), "Old audio lease was not stale")
    with sqlite3.connect(database) as db:
        assert db.execute("SELECT (SELECT COUNT(*) FROM captures), (SELECT COUNT(*) FROM media_assets)").fetchone() == (0, 0)
    assert staged[0].read_bytes() == original
    assert back.do_action(0)
    painted = wait_for(draft_painted, "Refused WAV save lost painted draft")
    label = next(node for node in nodes(app.pid) if (node.get_attributes() or {}).get("tag") == "label"
                 and "Text" in node.get_interfaces() and node.get_text(0, -1) == "Voice caption (optional)")
    label_rect = label.get_extents(Atspi.CoordType.SCREEN)
    context_rect = named(app.pid, "Context source stale. Choose another context").get_extents(Atspi.CoordType.SCREEN)
    label_gap = label_rect.y - context_rect.y - context_rect.height
    assert label_gap >= 4, f"Context focus outline overlaps audio caption label: gap={label_gap}"
    print(json.dumps({"check": "native_closed_source_audio_draft", "savedCount": 0,
                      "stagedBytesUnchanged": True, "duplicateInvocationPreserved": True,
                      "captionLabelGapPx": label_gap,
                      "screenshot": save_screenshot(painted, "gnome-audio-closed-draft-")}), flush=True)
    assert named(app.pid, "Context source stale. Choose another context").do_action(0)
    name_input = wait_for(lambda: named(app.pid, "New context name"), "Manual audio context form missing")
    assert name_input.grab_focus()
    command("xdotool", "type", "--clearmodifiers", "--delay", "0", manual_name)
    assert named(app.pid, "Create context").do_action(0)
    wait_for(lambda: named(app.pid, f"Context {manual_name}. Change context"), "Manual audio context not applied")
    wait_for(draft_painted, "Manual correction lost recording/caption")
    assert staged[0].read_bytes() == original
    assert named(app.pid, "Save").do_action(0)

    def saved():
        with sqlite3.connect(database) as db:
            rows = db.execute("SELECT c.kind,c.caption,c.caption_source,c.branch_name,p.name,p.kind,"
                              "m.relative_path,m.checksum,m.byte_size,m.duration_ms,m.mime_type,m.width_px,m.height_px "
                              "FROM captures c JOIN contexts p ON p.id=c.context_id "
                              "JOIN media_assets m ON m.capture_id=c.id").fetchall()
            if not rows:
                return False
            assert len(rows) == 1 and rows[0][:6] == ("audio", caption, "user", None, manual_name, "standalone"), rows
            path, checksum, size, duration, mime, width, height = rows[0][6:]
            relative = Path(path)
            assert not relative.is_absolute() and ".." not in relative.parts
            final = database.parent / "media" / relative
            assert final.read_bytes() == original and checksum == hashlib.sha256(original).hexdigest()
            assert (size, duration, mime, width, height) == (len(original), 100, "audio/wav", None, None)
            assert db.execute("SELECT COUNT(*) FROM captures").fetchone()[0] == 1
            return True

    wait_for(saved, "Corrected WAV was not durably saved exactly once")
    wait_for(lambda: named(app.pid, "Voice caption (optional)") is None, "Saved WAV popup remained open")
    assert not staged[0].exists()
    command("xdotool", "windowactivate", "--sync", library_window)
    assert named(app.pid, "All captures").do_action(0)

    def row():
        return next((node for node in nodes(app.pid) if node.get_name().startswith(
            f"audio capture in {manual_name} at ") and caption in node.get_name()), None)

    assert wait_for(row, "Saved WAV absent from Library").do_action(0)

    def detail_painted():
        visible = nodes(app.pid)
        if not page_heading(visible, "All captures") or named(app.pid, "Play voice note") is None:
            return False
        text = next((node for node in visible if (node.get_attributes() or {}).get("tag") == "p"
                     and (node.get_name() == caption or ("Text" in node.get_interfaces()
                                                        and node.get_text(0, -1) == caption))), None)
        if text is None:
            return False
        rect = text.get_extents(Atspi.CoordType.SCREEN)
        current = pixels()
        if min(rect.x, rect.y) < 0 or min(rect.width, rect.height) <= 0 or rect.x + rect.width > current.get_width() or rect.y + rect.height > current.get_height():
            return False
        return current if region_has_contrast(current, (rect.x, rect.y, rect.width, rect.height)) else False

    painted = wait_for(detail_painted, "Library voice detail/caption did not paint")
    print(json.dumps({"check": "native_closed_source_audio_saved", "savedCount": 1,
                      "manualContext": manual_name, "captionAndBytesMatch": True, "durationMs": 100,
                      "sourceWindowClosed": True, "microphoneOpened": False,
                      "screenshot": save_screenshot(painted, "gnome-audio-library-")}), flush=True)
