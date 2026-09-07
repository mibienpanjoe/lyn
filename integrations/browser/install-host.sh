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

# Ensure absolute path in manifest copy
TMP_MANIFEST="/tmp/$MANIFEST_NAME"
sed "s|\"/usr/local/bin/lyn-browser-host\"|\"$HOST_BIN\"|" "$MANIFEST_SRC" > "$TMP_MANIFEST"

TARGET_DIRS=(
  "$HOME/.config/google-chrome/NativeMessagingHosts"
  "$HOME/.config/chromium/NativeMessagingHosts"
  "$HOME/.config/BraveSoftware/Brave-Browser/NativeMessagingHosts"
  "$HOME/.config/microsoft-edge/NativeMessagingHosts"
  "$HOME/.mozilla/native-messaging-hosts"
)

INSTALLED_COUNT=0
for dir in "${TARGET_DIRS[@]}"; do
  parent="$(dirname "$dir")"
  if [ -d "$parent" ] || [ -d "$dir" ]; then
    mkdir -p "$dir"
    cp "$TMP_MANIFEST" "$dir/$MANIFEST_NAME"
    chmod 644 "$dir/$MANIFEST_NAME"
    echo "Installed Native Messaging manifest to: $dir/$MANIFEST_NAME"
    INSTALLED_COUNT=$((INSTALLED_COUNT + 1))
  fi
done

rm -f "$TMP_MANIFEST"

if [ "$INSTALLED_COUNT" -eq 0 ]; then
  # Fallback to standard Google Chrome directory
  mkdir -p "$HOME/.config/google-chrome/NativeMessagingHosts"
  sed "s|\"/usr/local/bin/lyn-browser-host\"|\"$HOST_BIN\"|" "$MANIFEST_SRC" > "$HOME/.config/google-chrome/NativeMessagingHosts/$MANIFEST_NAME"
  echo "Installed Native Messaging manifest to: $HOME/.config/google-chrome/NativeMessagingHosts/$MANIFEST_NAME"
fi

echo "Lyn browser host integration registered successfully."
