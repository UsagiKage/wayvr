#!/bin/sh
# LIVE TEST ENTRY POINT. Prepared only; do not run before the user's runtime approval.
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
test "$#" -eq 1 || { echo 'Usage: run-archimedes-prototype.sh KWIN-WINDOW-UUID' >&2; exit 2; }
if pgrep -x wayvr >/dev/null || pgrep -x wayvr-kde-proto >/dev/null; then
    echo 'A WayVR instance is present. Close it manually when safe; this launcher will not replace it.' >&2
    exit 1
fi
if ! pgrep -x vrserver >/dev/null || ! pgrep -x vrcompositor >/dev/null; then
    echo 'SteamVR must already be running for an approved test; this launcher will not start it.' >&2
    exit 1
fi
test -x "$repo/artifacts/wayvr-kde-prototype"
test -f "$repo/artifacts/lib/libopenvr_api.so"
mkdir -p "$repo/dev-session/config/wayvr" "$repo/dev-session/data" "$repo/dev-session/cache"
export XDG_CONFIG_HOME="$repo/dev-session/config"
export XDG_DATA_HOME="$repo/dev-session/data"
export XDG_CACHE_HOME="$repo/dev-session/cache"
export WAYVR_EXPERIMENTAL_KDE_WINDOW="$1"
exec "$repo/artifacts/wayvr-kde-prototype" --openvr --no-autostart --show --log-to "$repo/dev-session/wayvr-prototype.log"
