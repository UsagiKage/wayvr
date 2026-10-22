#!/bin/sh
# Build/static checks only. Never launches WayVR or mounts runtime/device sockets.
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
podman build -t localhost/archimedes-wayvr-build:26.8.0 -f "$repo/Containerfile.archimedes" "$repo"
mkdir -p "$repo/evidence"
node "$repo/tests/kde-bridge.test.mjs" > "$repo/evidence/kde-bridge-tests.log" 2>&1
podman run --rm --memory=12g --cpus=4 \
    -v "$repo:$repo:z" -v archimedes-wayvr-cargo:/cargo \
    -e CARGO_HOME=/cargo -e CARGO_BUILD_JOBS=3 -w "$repo" \
    localhost/archimedes-wayvr-build:26.8.0 sh -eu -c '
    cargo fetch --locked > evidence/dependency-fetch.log 2>&1
    cargo fmt --all --check > evidence/format.log 2>&1
    cargo build --locked --offline --profile plain --no-default-features --features wayland,openvr -p wayvr > evidence/feature-off-build.log 2>&1
    cargo test --locked --offline --profile plain --no-default-features --features wayland,openvr,experimental-kde-mirror -p wayvr > evidence/prototype-tests.log 2>&1
    cargo build --locked --offline --profile plain --no-default-features --features wayland,openvr,experimental-kde-mirror -p wayvr > evidence/prototype-build.log 2>&1
    mkdir -p artifacts/lib
    cp target/plain/wayvr artifacts/wayvr-kde-prototype
    for loader in target/plain/build/ovr_overlay_sys-*/out/libopenvr_api.so; do
        test -f "$loader" || continue
        cp "$loader" artifacts/lib/libopenvr_api.so
        break
    done
    test -f artifacts/lib/libopenvr_api.so
    sha256sum artifacts/wayvr-kde-prototype artifacts/lib/libopenvr_api.so
    '
