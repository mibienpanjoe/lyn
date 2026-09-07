# Lyn Browser Context Integration

Local, privacy-bounded context provider for web browsers (Google Chrome, Chromium, Brave, Microsoft Edge, and Mozilla Firefox).

## How it works

1. **Companion Extension (`integrations/browser/`):**
   A lightweight Manifest V3 WebExtension that monitors active tabs and reports focus changes.
   - Strips search queries, authentication tokens (`?auth=...`), and hash fragments.
   - Ignores incognito / private browsing tabs completely.
2. **Native Messaging Host (`lyn-browser-host`):**
   A small, secure Rust binary communicating via standard input/output with Chrome / Firefox, forwarding messages to Lyn's private Unix socket (`$XDG_RUNTIME_DIR/lyn-browser-v1.sock`).
3. **Localhost Port Resolution:**
   When you test local web services (`http://localhost:5173`, `http://127.0.0.1:3000`), Lyn inspects the kernel socket table (`/proc/net/tcp`), maps the listening port to the development server process (`/proc/<pid>/cwd`), and binds the capture directly to your local Git repository and branch.

## Setup

### 1. Build and install the Native Messaging host
```bash
pnpm provider:browser:install
```

### 2. Load the companion extension in your browser
1. Open `chrome://extensions` (or `brave://extensions`, `edge://extensions`).
2. Enable **Developer mode** (toggle in upper right).
3. Click **Load unpacked** and select the `integrations/browser/` directory.
