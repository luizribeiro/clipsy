#!/usr/bin/env bash
# Runs one side of a cross-machine check. The server and client jobs of a
# pair derive both keys from the run id, so each knows the other's node id.
set -euo pipefail

role=$1
pair=$2
clipsy=${CLIPSY:-target/release/clipsy}
seed="$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT-$pair"
done_marker="clipsy-ci-done-$seed"
failed_marker="clipsy-ci-failed-$seed"
work=$(mktemp -d)
chmod 700 "$work"

key_file() {
  local file="$work/$1.key"
  printf '%s' "$seed-$1" | shasum -a 256 | cut -c1-64 > "$file"
  chmod 600 "$file"
  echo "$file"
}

node_id() {
  printf 'key_file = "%s"\n' "$1" > "$work/id.toml"
  "$clipsy" --config "$work/id.toml" id
}

system_clipboard() {
  case "$(uname)" in
    Darwin) pbpaste ;;
    *) xclip -selection clipboard -o 2>/dev/null || true ;;
  esac
}

within() {
  local seconds=$1
  shift
  local deadline=$((SECONDS + seconds))
  until "$@"; do
    if [ "$SECONDS" -ge "$deadline" ]; then
      echo "gave up after ${seconds}s: $*" >&2
      return 1
    fi
    sleep 5
  done
}

server_key=$(key_file server)
client_key=$(key_file client)
server_id=$(node_id "$server_key")
client_id=$(node_id "$client_key")

serve() {
  cat > "$work/server.toml" <<TOML
key_file = "$server_key"

[clients.ci]
id = "$client_id"
read = true
TOML
  "$clipsy" --config "$work/server.toml" serve > "$work/serve.log" 2>&1 &
  server_pid=$!
  trap 'kill "$server_pid" 2>/dev/null || true; cat "$work/serve.log"' EXIT
  client_finished() {
    kill -0 "$server_pid" || { echo "clipsy serve exited" >&2; exit 1; }
    local clipboard
    clipboard=$(system_clipboard)
    [ "$clipboard" != "$failed_marker" ] || { echo "the client reported a failure" >&2; exit 1; }
    [ "$clipboard" = "$done_marker" ]
  }
  within 1500 client_finished
}

report_failure() {
  local status=$?
  if [ "$status" -ne 0 ]; then
    "$clipsy" --config "$work/client.toml" write "$failed_marker" || true
  fi
}

run_client() {
  cat > "$work/client.toml" <<TOML
key_file = "$client_key"

[servers]
ci = "$server_id"
TOML
  c() { "$clipsy" --config "$work/client.toml" "$@"; }
  local greeting="hello across $pair"
  trap report_failure EXIT

  within 1200 c write "$greeting"
  [ "$(c read)" = "$greeting" ] || { echo "read back the wrong entry" >&2; exit 1; }

  c write --concealed "hunter2"
  if output=$(c read 2>&1); then
    echo "the server handed out a concealed entry: $output" >&2
    exit 1
  fi
  grep -q "holds a concealed entry" <<<"$output" || { echo "$output" >&2; exit 1; }

  c write "$done_marker"
  echo "server $server_id passed every check"
}

case "$role" in
  server) serve ;;
  client) run_client ;;
  *) echo "usage: $0 server|client PAIR" >&2; exit 64 ;;
esac
