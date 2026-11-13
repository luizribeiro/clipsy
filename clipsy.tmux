#!/usr/bin/env bash
plugin_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
tmux set -s copy-command "$(printf '%q' "$plugin_dir/scripts/clipsy-copy")"
