# Controlled KDE desktop test — refused before injection

2026-10-22 PDT. Starting checkpoint: `f1ac1bdf742090750911c85444cb1eaca1ae0342`
on `codex/interactive-kde-window`; clean when inspected. User authorized controlled
KWrite testing, including development-only KDE authorization and input. No SteamVR
or headset startup, stable installation change, global settings or privileged service.
Ask before interrupting the desktop; stop on uncertain authorization or targeting.

## Actual native observations — 2026-10-22 PDT

The user approved opening the disposable document and KDE picker. KWrite was
started with separate config/data/cache directories and no preexisting KWrite
process. The user-driven picker selected:

- Application: `org.kde.kwrite`; PID `3763140`.
- Title: `wayvr-disposable-20261022.txt — KWrite`.
- UUID: `{7fbc05fb-b6ea-4e27-ab53-716460ed008f}`.
- Picker frame geometry: approximately `[1987.118, 73.234, 899, 884]` logical units.
- `excludeFromCapture: false`.

At 08:12:16 PDT, native preflight refused the PID because the Rust decoder only
accepted unsigned D-Bus values. KDE's independent picker result had confirmed the
expected signed PID. A lossless signed/unsigned conversion was added, retaining
rejection of zero, negative, oversized or string values; its regression test passed.

At 08:13:17 PDT, the retry verified UUID, KWrite identity, PID and document title,
then created native capture node `104`. This establishes authorization and creation
of the requested window's capture stream, **not successful frame consumption**.
It then timed out with:

> No matching fresh capture and KWin telemetry; input remains disabled

The test STOPPED. The current diagnostics do not distinguish absent PipeWire frames
from absent KWin telemetry; neither a validated scale/capture extent nor reliable
target mapping was established. Targeted KWin journal searches found no corresponding
script error. Do not infer the timer, script, PipeWire negotiation or GPU as the cause.

No input provider/device was created and no desktop input was injected. Activation,
first-click consumption, pointer/click/scroll/keyboard routing, held-input release,
movement and resize refusal remain **unvalidated on the real desktop**. Unrelated
windows received no injected test events because injection never began. Existing
simulated checks remain available; they do not establish native desktop success.

## Native non-injecting entry point

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
The actual picker/identity observations above supersede the earlier preparation
checkpoint. A fixture or title match alone never authorizes real input.

## Evidence and remaining work

`evidence/native-probe-tests.log`: 14 Rust tests passed, including the signed-PID
regression. These are unit/simulated tests, separate from the native observations.
`evidence/native-probe-build.log`: build passed, existing compiler warnings remain.
`evidence/native-probe-startup.log`: original simulation still returns before normal
startup; invalid native UUID refused without desktop access. Formatting, shell
syntax and whitespace checks passed. These are build/simulation results only.

Local native logs: `evidence/native-kde-preflight-20261022.log` (initial decoder
refusal), `evidence/native-kde-preflight-20261022-retry.log` (verified identity,
capture node and timeout), `evidence/native-kde-scripting-after-20261022.log`
(no exported script children after cleanup). The selected-window result,
authorization manifest and cleanup record live under ignored `dev-session/native-check`.

Next experiment: instrument capture-frame arrival versus KWin snapshot receipt and
repeat **only the non-injecting preflight**, with the same deliberately selected
disposable target. Do not proceed to native input until that gate passes.

After successful native preflight, complete a narrowly gated live desktop backend
with an independent expiry tick, using the existing interaction and lease logic.
Require explicit verification before enabling it; retain the synthetic driver for
unsafe faults. Native input and a real capture are separate from headset acceptance.
Do not recommend proceeding to a headset test until the desktop milestone passes.

## Rollback

The newly created authorization entry
`/home/Usagi/.local/share/applications/org.dqi.WayVRMirror.DesktopTest.desktop`
was removed after its contents were compared with the staged copy. No prior file
existed or was overwritten. Session teardown closed the capture and attempted helper
unload; subsequent `/Scripting` introspection showed no script children and no
`/tmp/dqi-wayvr-mirror-*` directories remained. No persistent service/input setting
was created. All nine stable WayVR file checksums still match; original prototype
and simulation artifacts are unchanged.

The expendable KWrite document remains open for the user to close normally; no
existing application was terminated. Its document and isolated settings are under
`dev-session/native-check` in this checkout. Keep or remove that folder only after
closing this disposable KWrite window; it contains no ordinary KWrite settings.
Stable WayVR needs no restoration. Never restart KWin as cleanup.

Upstream checks used during preparation:
[KWin window identity information](https://github.com/KDE/kwin/blob/Plasma/6.7/src/dbusinterface.cpp),
[native interface filtering](https://github.com/KDE/kwin/blob/Plasma/6.7/src/wayland_server.cpp).
