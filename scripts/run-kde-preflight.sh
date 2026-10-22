#!/bin/sh
# Native capture/identity checks ONLY: no uinput provider, no normal WayVR startup.
set -eu
test "$#" -eq 3 || { echo 'Usage: run-kde-preflight.sh UUID KWRITE_PID wayvr-disposable-NAME.txt' >&2; exit 2; }
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
binary="$repo/artifacts/native-check/wayvr-kde-desktop-check"
test -x "$binary"
test -n "${WAYLAND_DISPLAY:-}" && test -n "${DBUS_SESSION_BUS_ADDRESS:-}"
export XDG_CONFIG_HOME="$repo/dev-session/native-check/config"
export XDG_DATA_HOME="$repo/dev-session/native-check/data"
export XDG_CACHE_HOME="$repo/dev-session/native-check/cache"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME"
exec "$binary" --desktop-mirror-probe "$1" --kwrite-test-pid "$2" --kwrite-test-title "$3"
