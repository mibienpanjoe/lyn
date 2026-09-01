#!/usr/bin/env bash
set -euo pipefail

integration_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
fixture_dir=$(mktemp -d)
watcher_pid=

cleanup() {
  if [[ -n "$watcher_pid" ]]; then
    kill "$watcher_pid" 2>/dev/null || true
    wait "$watcher_pid" 2>/dev/null || true
  fi
  rm -rf -- "$fixture_dir"
}
trap cleanup EXIT

helper="$fixture_dir/lyn-context"
log="$fixture_dir/arguments"
cat >"$helper" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >"$LYN_TEST_LOG"
sleep 30
EOF
chmod 700 "$helper"

export LYN_CONTEXT_HELPER="$helper"
export LYN_TEST_LOG="$log"
unset KITTY_WINDOW_ID LYN_CONTEXT_WATCHER_PID
source "$integration_dir/lyn-context.sh"
watcher_pid=$LYN_CONTEXT_WATCHER_PID

for _ in {1..20}; do
  [[ -f "$log" ]] && break
  sleep 0.01
done
[[ $(<"$log") == "watch --process $$" ]]

kill "$watcher_pid"
wait "$watcher_pid" 2>/dev/null || true
watcher_pid=
unset LYN_CONTEXT_WATCHER_PID
rm -f -- "$log"

export KITTY_WINDOW_ID=42
source "$integration_dir/lyn-context.sh"
[[ ! -e "$log" ]]
[[ -z "${LYN_CONTEXT_WATCHER_PID:-}" ]]
