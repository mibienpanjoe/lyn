# Lyn Browser Context Integration

Local, privacy-bounded context provider for web browsers (Google Chrome, Chromium, Brave, Microsoft Edge, and Mozilla Firefox).

## How it works

1. **Companion Extension (`integrations/browser/`):**
   A lightweight Manifest V3 WebExtension that reports the focused tab and invokes capture from it (`lyn.capture`). Chrome, Chromium, Brave, and Edge cannot bind `Ctrl+Alt+Shift+L` as a suggested key, so the companion suggests `Alt+Shift+L` (rebindable in the browser). Editor, Cursor, and Kitty keep `Ctrl+Alt+Shift+L`.
   - Sends only sanitized `file:` and localhost-family URLs (no query, fragment, or title).
   - Ignores incognito / private browsing tabs completely.
   - Remote sites open capture without automatic project resolution.
2. **Native Messaging Host (`lyn-browser-host`):**
   A small, secure Rust binary communicating via standard input/output with Chrome / Firefox, forwarding messages to Lyn's private Unix socket (`$XDG_RUNTIME_DIR/lyn-browser-v1.sock`). The helper shuts down the Unix write side after the payload so Lyn can bound the read and check the peer uid.
3. **Localhost Port Resolution:**
   When you test local web services (`http://localhost:5173`, `http://127.0.0.1:3000`, `http://[::1]:…`), Lyn inspects `/proc/net/tcp` and `/proc/net/tcp6`, maps listen inodes to same-user processes, skips `docker-proxy`, and binds the capture only when every remaining listener shares one working directory. Two processes with different cwds, or a Docker published port with no other proven project directory, stay unresolved.

## Setup

Packaged Lyn does not require a second binary. Settings → **Register Host** points Native Messaging at `~/.local/share/lyn/bin/lyn-browser-host`, a symlink to the running Lyn app.

### 1. Register the Native Messaging host

In Lyn Settings, click **Register Host**, or from a source checkout:

```bash
pnpm provider:browser:install
```

### 2. Load the companion extension in your browser

#### Chrome, Chromium, Brave, and Edge
1. Open `chrome://extensions` (or `brave://extensions`, `edge://extensions`).
2. Enable **Developer mode** (toggle in upper right).
3. Click **Load unpacked** and select the `integrations/browser/` directory (or `~/.local/share/lyn/integrations/browser/` if generated via Settings).

#### Firefox
Firefox MV3 currently loads an event page (`background.scripts`), not a service worker. Use the Firefox manifest:
1. Open `about:debugging#/runtime/this-firefox`.
2. Click **Load Temporary Add-on…**.
3. Select `integrations/browser/manifest.firefox.json` (or `~/.local/share/lyn/integrations/browser/manifest.firefox.json`).
