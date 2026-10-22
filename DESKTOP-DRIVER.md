# Development desktop driver — simulation only

2026-10-21: authorized preparation and simulation; STOP before KDE authorization,
real desktop input, or experimental WayVR runtime startup. No services or global
input changes. Stable WayVR remains separate.

`experimental-kde-driver` enables an early `--desktop-mirror-driver` entry point.
It returns before normal PID handling, logging, config loading or VR initialization.
The driver has **only an in-memory RecordingProvider**: no live-input mode, KWin
connection, permission installation, capture or actual application selection.
Do not invoke the compiled binary without that flag: normal startup is WayVR.
Use the argument-free wrapper below, which hardcodes simulation and mounts only
the separate driver artifact in a read-only, network-disabled container.

## Shared code and verification

Both the VR mirror (`kde.rs`) and this driver call `MirrorInteraction` in
`wayvr/src/overlays/screen/kde_interaction.rs`: target validation, hover, activation,
pointer and scroll, generation/tracking checks, deactivation and release. Keyboard
commands use the existing `HidWrapper` routing and `LeasedProvider`. Only the output
provider and compositor replies are simulated. No duplicate interaction algorithm.

The embedded fixture is clearly labelled SIMULATED disposable KWrite. Commands must
explicitly select its UUID, then verify the same capture UUID, `org.kde.kwrite` and
`disposable:true`. First activation click is consumed; hover does not activate.
Keyboard events require activation. Recorded mouse/keys/modifiers are released on
deactivation, simulated faults, EOF, malformed input and ordinary driver drop.
Recovery does not automatically reactivate. `border_logic` reports intended state;
it does not render a border or demonstrate actual visual feedback.

Before **any future real injection**, obtain separate approval and implement a
live backend with an independent expiry/watchdog tick. Use KDE's user-driven picker
to explicitly select a harmless disposable KWrite document; verify actual app ID,
UUID, capture identity, visibility and geometry before enabling input. A fixture
assertion does not verify a real desktop window. Current simulator blocks waiting
for stdin and must never be connected directly to a live provider.

## Build and run

Use the existing isolated build image and cached dependencies; no host packages:

```sh
cargo test --locked --offline --profile plain --no-default-features --features wayland,openvr,experimental-kde-driver -p wayvr
cargo build --locked --offline --profile plain --no-default-features --features wayland,openvr,experimental-kde-driver -p wayvr
```

Run those build commands inside the existing Podman build environment described in
ARCHIMEDES.md. Package the result separately as
`artifacts/driver-check/wayvr-kde-driver`, with the matching OpenVR loader in
`artifacts/driver-check/lib/libopenvr_api.so`; do not overwrite the original runtime
artifact. The loader is a link dependency, not evidence of VR initialization.

```sh
sh scripts/run-desktop-driver-simulated.sh < tests/desktop-driver.jsonl
python3 tests/desktop-driver-smoke.py
```

The JSON-lines scenario checks selection, activation, pointer mapping, right-button
release, scroll, keyboard routing, held-input release and refusal after recovery.
Rust regression tests also cover focus, movement, resize, scaling, capture closure,
tracking loss, stale metadata, occlusion and popups. Output traces/build logs live
under ignored `evidence/*.log`. These are simulated checks, **not native KDE desktop
or headset testing**, and do not validate SteamVR controller suppression.

## Rollback and next gate

No installed component needs restoring. Stop the foreground simulator normally;
the container is removed on exit. Retain or remove only its source/build artifacts.
The stable AppImage, launcher, config, application profiles and original prototype
artifact are untouched. Future KDE permission, native injection and runtime tests
need explicit approval; none are performed by this wrapper.
