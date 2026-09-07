#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MANIFEST_NAME="com.mibienpanjoe.lyn.json"
MANIFEST_SRC="$SCRIPT_DIR/$MANIFEST_NAME"

# Locate or build lyn-browser-host
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
HOST_BIN="$REPO_ROOT/src-tauri/target/release/lyn-browser-host"
if [ ! -f "$HOST_BIN" ]; then
  HOST_BIN="$REPO_ROOT/src-tauri/target/debug/lyn-browser-host"
fi

if [ ! -f "$HOST_BIN" ]; then
  echo "Building lyn-browser-host..."
  cargo build --manifest-path "$REPO_ROOT/src-tauri/Cargo.toml" --bin lyn-browser-host
  HOST_BIN="$REPO_ROOT/src-tauri/target/debug/lyn-browser-host"
fi

CHROMIUM_EXTENSION_ID="aecihlceemkggejjmpphmnhpdcgnhife"
FIREFOX_ADDON_ID="lyn-context-provider@mibienpanjoe.com"

# Prepare distinct Chromium and Firefox manifests with absolute host binary path
TMP_CHROMIUM_MANIFEST="/tmp/${MANIFEST_NAME}.chromium"
TMP_FIREFOX_MANIFEST="/tmp/${MANIFEST_NAME}.firefox"

cat <<EOF > "$TMP_CHROMIUM_MANIFEST"
{
  "name": "com.mibienpanjoe.lyn",
  "description": "Lyn Desktop Browser Context Host",
  "path": "$HOST_BIN",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://$CHROMIUM_EXTENSION_ID/"
  ]
}
EOF

cat <<EOF > "$TMP_FIREFOX_MANIFEST"
{
  "name": "com.mibienpanjoe.lyn",
  "description": "Lyn Desktop Browser Context Host",
  "path": "$HOST_BIN",
  "type": "stdio",
  "allowed_extensions": [
    "$FIREFOX_ADDON_ID"
  ]
}
EOF

CHROMIUM_TARGET_DIRS=(
  "$HOME/.config/google-chrome/NativeMessagingHosts"
  "$HOME/.config/chromium/NativeMessagingHosts"
  "$HOME/.config/BraveSoftware/Brave-Browser/NativeMessagingHosts"
  "$HOME/.config/microsoft-edge/NativeMessagingHosts"
)

FIREFOX_TARGET_DIRS=(
  "$HOME/.mozilla/native-messaging-hosts"
)

INSTALLED_COUNT=0
for dir in "${CHROMIUM_TARGET_DIRS[@]}"; do
  parent="$(dirname "$dir")"
  if [ -d "$parent" ] || [ -d "$dir" ]; then
    mkdir -p "$dir"
    cp "$TMP_CHROMIUM_MANIFEST" "$dir/$MANIFEST_NAME"
    chmod 644 "$dir/$MANIFEST_NAME"
    echo "Installed Chromium Native Messaging manifest to: $dir/$MANIFEST_NAME"
    INSTALLED_COUNT=$((INSTALLED_COUNT + 1))
  fi
done

for dir in "${FIREFOX_TARGET_DIRS[@]}"; do
  parent="$(dirname "$dir")"
  if [ -d "$parent" ] || [ -d "$dir" ]; then
    mkdir -p "$dir"
    cp "$TMP_FIREFOX_MANIFEST" "$dir/$MANIFEST_NAME"
    chmod 644 "$dir/$MANIFEST_NAME"
    echo "Installed Firefox Native Messaging manifest to: $dir/$MANIFEST_NAME"
    INSTALLED_COUNT=$((INSTALLED_COUNT + 1))
  fi
done

if [ "$INSTALLED_COUNT" -eq 0 ]; then
  # Fallback to standard Google Chrome and Mozilla directories
  mkdir -p "$HOME/.config/google-chrome/NativeMessagingHosts"
  cp "$TMP_CHROMIUM_MANIFEST" "$HOME/.config/google-chrome/NativeMessagingHosts/$MANIFEST_NAME"
  chmod 644 "$HOME/.config/google-chrome/NativeMessagingHosts/$MANIFEST_NAME"

  mkdir -p "$HOME/.mozilla/native-messaging-hosts"
  cp "$TMP_FIREFOX_MANIFEST" "$HOME/.mozilla/native-messaging-hosts/$MANIFEST_NAME"
  chmod 644 "$HOME/.mozilla/native-messaging-hosts/$MANIFEST_NAME"
  echo "Installed fallback Native Messaging manifests for Chrome and Firefox."
fi

rm -f "$TMP_CHROMIUM_MANIFEST" "$TMP_FIREFOX_MANIFEST"

echo "Lyn browser host integration registered successfully."
