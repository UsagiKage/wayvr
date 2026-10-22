# Controlled KDE desktop test — preparation checkpoint

2026-10-22 PDT. Starting checkpoint: `f1ac1bdf742090750911c85444cb1eaca1ae0342`
on `codex/interactive-kde-window`; clean when inspected. User authorized controlled
KWrite testing, including development-only KDE authorization and input. No SteamVR
or headset startup, stable installation change, global settings or privileged service.
Ask before interrupting the desktop; stop on uncertain authorization or targeting.

## Prepared, not yet performed

The previous driver has no live backend. A separate **non-injecting** early-return
`--desktop-mirror-probe` entry point now accepts only an explicitly selected KWin
UUID, expected KWrite PID and disposable document basename. It verifies the actual
window information, starts the existing native capture/telemetry session, and reads
PipeWire frame metadata using the existing capture code. No image data is saved.
It refuses missing/invalid identities, geometry/scale/extent disagreement, transients,
unknown capture orientation and missing fresh telemetry. It never creates a desktop
input provider, loads normal WayVR configuration, or initializes a VR runtime.

Separate artifact: `artifacts/native-check/wayvr-kde-desktop-check`; original
prototype and simulator artifacts remain untouched. Entry wrapper:

```sh
sh scripts/run-kde-preflight.sh '<selected-uuid>' '<kwrite-pid>' 'wayvr-disposable-NAME.txt'
```

This wrapper is intended only after the actual user-driven KDE picker and review
of development-only capture authorization. It does not install permission metadata.
It isolates config/data/cache in `dev-session/native-check`, retaining the real
session connections required for native capture. Do not invoke the artifact with
no arguments, which would enter normal WayVR startup.

The test document must be expendable and begin with `wayvr-disposable-`. A matching
title alone is insufficient: KWrite application identity, PID and UUID must agree.
No KWrite target, authorization metadata or temporary KWin helper has been created
at this checkpoint. The desktop-picker readiness question is pending.

## Evidence and remaining work

`evidence/native-probe-tests.log`: 13 existing Rust tests passed.
`evidence/native-probe-build.log`: build passed, existing compiler warnings remain.
`evidence/native-probe-startup.log`: original simulation still returns before normal
startup; invalid native UUID refused without desktop access. Formatting, shell
syntax and whitespace checks passed. These are build/simulation results only.

Native identity/capture/geometry checks have NOT run. Actual activation, mouse,
keyboard, scrolling, held-input release and movement/resize refusal have NOT been
tested. No claim can be made that unrelated windows received no events during a
live test, because no live test occurred. No input was injected by this work.

After successful native preflight, complete a narrowly gated live desktop backend
with an independent expiry tick, using the existing interaction and lease logic.
Require explicit verification before enabling it; retain the synthetic driver for
unsafe faults. Native input and a real capture are separate from headset acceptance.
Do not recommend proceeding to a headset test until the desktop milestone passes.

## Rollback

This checkpoint requires no installed-state rollback: no authorization entry,
input device, KWrite process, temporary script or persistent service was installed
or started. The preflight source/artifact can remain unused. Stable WayVR is intact.
If future permission metadata is installed, preserve any preexisting file first
and remove only the exact new development desktop entry afterward. The existing
Session cleanup closes its capture and unloads its uniquely named temporary KWin
script; verify that cleanup after any real invocation. Never restart KWin for cleanup.

Upstream checks used during preparation:
[KWin window identity information](https://github.com/KDE/kwin/blob/Plasma/6.7/src/dbusinterface.cpp),
[native interface filtering](https://github.com/KDE/kwin/blob/Plasma/6.7/src/wayland_server.cpp).
