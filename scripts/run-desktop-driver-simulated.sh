#!/bin/sh
# Simulation ONLY: no desktop bus/socket/device mounts, no native runtime startup.
set -eu
test "$#" -eq 0 || { echo 'This simulation wrapper accepts no arguments.' >&2; exit 2; }
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
test -x "$repo/artifacts/driver-check/wayvr-kde-driver" || {
    echo 'Build the experimental-kde-driver artifact first; see DESKTOP-DRIVER.md.' >&2
    exit 1
}
exec podman run --rm -i --network=none --read-only --cap-drop=all \
    --security-opt=no-new-privileges --memory=512m --cpus=1 \
    --tmpfs /tmp:rw,noexec,nosuid,size=16m \
    -v "$repo/artifacts/driver-check:/driver:ro,z" \
    -e XDG_CONFIG_HOME=/tmp/config -e XDG_DATA_HOME=/tmp/data \
    -e XDG_CACHE_HOME=/tmp/cache -e XDG_RUNTIME_DIR=/tmp/runtime \
    localhost/archimedes-wayvr-build:26.8.0 \
    /driver/wayvr-kde-driver --desktop-mirror-driver
