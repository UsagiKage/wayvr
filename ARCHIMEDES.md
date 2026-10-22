# Archimedes WayVR development

Date recorded: 2026-10-21 PDT. Status: experimental source and static verification;
desktop/KWin integration and headset acceptance are pending user authorization.

## Authority and baseline

- Canonical upstream: https://github.com/wayvr-org/wayvr
- Development fork: https://github.com/UsagiKage/wayvr
- Feature branch: `codex/interactive-kde-window`.
- Upstream baseline: `v26.8.0`, commit `cb3aa42bd91aeda0e4d0de91593a6f878350c757`.
- Local baseline tag: `archimedes/v26.8.0-baseline` (same commit).
- License: GPL-3.0-only, as declared in `wayvr/Cargo.toml`; retain upstream LICENSE.
- The vendored KDE capture protocol retains its LGPL-2.1-or-later attribution.
  Source protocol revision: `18afc45c092857e998c890afce1e7e71f808d819` in
  `KDE/plasma-wayland-protocols`.
- Source authority is this fork. High-level approvals/state belong to
  `UsagiKage/drivemind`, `projects/archimedes-wayvr/README.md`, on `main`.

Local checkout: `/home/Usagi/Documents/ChatGPT/Proto-Morrow/development/wayvr`.
Detached baseline worktree: sibling `wayvr-baseline`.
The installed `/home/Usagi/Applications/WayVR/WayVR.AppImage`, desktop/Steam
launch methods and `/home/Usagi/.config/wayvr` remain separate and unchanged.
The installed AppImage SHA-256 matches the official 26.8.0 release digest:
`4b40719a860138cde4b97a5acbe5d5c300e9c7a186ab6bed423be8768e0c0794`.

## Build isolation and maintenance

The rootless Podman image uses pinned Debian/Rust base-image content, Rust 1.99.1,
and build dependencies inside the image. No host development packages were installed.
Builds have no desktop session bus, Wayland socket, GPU or input devices mounted.
CPU and memory limits are four cores and 12 GiB; Cargo uses three jobs.

The verified baseline feature set follows upstream's Wayland/OpenVR CI:
`cargo build --locked --profile plain --no-default-features --features wayland,openvr -p wayvr`.
This does not establish a build of every optional upstream feature (OpenXR/Whisper/X11).

Run `sh scripts/archimedes-build.sh` for repeatable build/static checks and packaging.
It fetches locked dependencies before offline compilation. The first isolated build
takes longer; the named `archimedes-wayvr-cargo` volume caches dependencies.
Image: `localhost/archimedes-wayvr-build:26.8.0`.
Outputs and raw logs are ignored by Git under `artifacts/` and `evidence/*.log`.
The executable's process-local RUNPATH uses `artifacts/lib/libopenvr_api.so`, copied
from the pinned OpenVR Rust dependency. No host library symlink or loader setting changes.

Keep `origin` on our fork and `upstream` on the canonical repository. Fetch upstream
to review changes; do not silently rebase the pinned comparison baseline or update
the installed application. Push feature branches to origin. No upstream PR is authorized.

## Prototype scope and implementation

The feature `experimental-kde-mirror` is off by default. In that build only,
`WAYVR_EXPERIMENTAL_KDE_WINDOW=<KWin UUID>` makes the existing Add Mirror operation
create the experimental target. Only one experimental target is permitted.

- `kde_capture.rs`: native KWin `stream_window(uuid)` requests exactly the UUID
  used for metadata. No window-title or dimension-based identity guessing.
- `kde_session.rs` / `kde_bridge.js`: temporary KWin script; checks UUID, focus,
  visibility, logical frame geometry, output scale and `windowAt()` hit testing.
  D-Bus messages must come from KWin's unique owner and carry the per-session token.
  A 160 ms telemetry expiry fails closed. Initialization runs on a worker thread.
- `kde.rs`: first click requests activation and is consumed. Input starts only
  after that trigger is released and KDE focus/capture geometry are validated.
  Pointer positions are compositor-checked before injection. Clicks more than
  four logical pixels from the approved position are refused, rather than guessed.
  Scroll positions are committed before wheel events. Existing virtual keyboard
  routing supplies desktop key events. Green border means active; orange/dim means
  inactive. AltClick (bundled Index right-trackpad click), hiding or grabbing releases.
- `input_lease.rs`: decorator around the selected desktop input provider. Tracks
  this process's held buttons, keys and modifiers; flushes individual releases on
  revocation, expiry, focus/capture loss, controller tracking loss and normal drop.
  A revoked lease does not resume when tracking/focus returns. A deliberate new
  target selection is required. Other normal desktop overlays can clear it on click.
- `WlxCapture::is_alive()` and PipeWire transport state detect a failed capture;
  the last image remaining on screen does not authorize continued input.

No geometry setter is used. Normal explicit KDE activation can raise the desktop
window but never moves or resizes it. Desktop applications are neither launched nor
owned by this mirror; their processes remain independent of WayVR.

## Deliberate limits

- SteamVR controller-input suppression is a separate, unvalidated milestone.
  Do not test with an active game receiving controller actions in this first pass.
- KWin's native screencast interface is a restricted, unstable KDE integration API.
  The development executable needs explicit `X-KDE-Wayland-Interfaces` authorization.
  Missing authorization is an error; there is no fallback that disables permission checks.
  The desktop-entry template is staged only, not installed.
- The temporary script's real KWin API/QJSEngine behavior remains untested. Mock
  tests are not evidence that KDE accepts the script, interface permission or capture.
- Visible ordinary windows only. Popups/transient children are refused because
  KWin can enlarge capture bounds. Minimized/off-desktop windows are refused.
- Geometry or scale changes revoke interaction; reactivation is required once the
  new capture extent agrees. No automatic window resize, stereo or capture cropping.
- Pointer probes are asynchronous (40 ms timer). Fast movement may cause refused
  clicks. Ordinary capture/input latency remains; global uinput and asynchronous
  metadata cannot provide atomic per-window event delivery during concurrent desktop changes.
- No claim of cleanup after SIGKILL, an OS/GPU crash, or uninterrupted Codex work.
  Normal device teardown is separate from the tested software release paths.

## Controlled runtime gate — not performed

Obtain user approval before installing development authorization metadata, loading
the temporary KWin script, injecting input, launching this binary or closing stable WayVR.
Never use ChatGPT/Codex as the first target. Use a harmless KWrite document selected
by the user; never launch/close the user's existing applications automatically.

1. User safely closes stable WayVR; SteamVR must already be running. No restart.
2. Review/stage the dedicated config under `dev-session/config/wayvr`. Its isolated
   controller bindings can be copied from the user's accepted bindings after approval.
   Keep application autostart disabled. Do not copy capture tokens into source control.
3. Review and install only the development desktop-entry template with the exact
   artifact executable substituted. It is NoDisplay authorization metadata, not the
   launch entry point. Use the wrapper below; do not launch that desktop entry directly.
4. Obtain the harmless target UUID with KWin's user-driven `queryWindowInfo` picker.
5. Launch using `sh scripts/run-archimedes-prototype.sh '<uuid>'`. The wrapper refuses
   an existing WayVR instance, requires SteamVR, isolates XDG config/data/cache, uses
   `--no-autostart`, and never uses `--replace`, `--multi` or `--install`.
6. Test activation without forwarding the first click; left/right click, scrolling,
   keyboard focus and explicit release. Then test hide, focus change, target closure,
   tracking loss, resizing and scaling. Stop on any misdirected event or stuck input.
7. Only after desktop correctness, separately authorize headset acceptance and
   controller/game input ownership tests. Never infer either from compilation.

## Rollback

Nothing in the stable installation needs restoring after this source/build phase.
Return to the existing AppImage/launcher; no service, binding or SteamVR setting changed.
The fork, worktrees, image and cache can be retained without running anything.

After a future approved runtime test: close only the experimental WayVR normally,
remove only its installed development authorization desktop entry, and return to
the existing launcher. Normal shutdown unloads this session's uniquely named KWin
script and removes its private temporary directory. If the process was killed,
read `Loaded temporary KWin script: dqi-wayvr-mirror-...` in its log and unload only
that exact script with `/Scripting` `org.kde.kwin.Scripting.unloadScript(name)`.
Delete only its matching temporary directory after checking the name. No KWin restart.
Do not use broad process termination, config deletion or a factory reset.

## Supporting sources

- [Pinned mirror backend](https://github.com/wayvr-org/wayvr/blob/v26.8.0/wayvr/src/overlays/screen/mirror.rs)
- [KDE capture permission filtering](https://github.com/KDE/kwin/blob/Plasma/6.7/src/wayland_server.cpp)
- [KWin UUID capture and window hit testing](https://github.com/KDE/kwin/blob/Plasma/6.7/src/scripting/workspace_wrapper.cpp)
- [KWin popup capture bounds and scale](https://github.com/KDE/kwin/blob/Plasma/6.7/src/plugins/screencast/windowscreencastsource.cpp)
- [KDE screencast protocol](https://github.com/KDE/plasma-wayland-protocols/blob/master/src/protocols/zkde-screencast-unstable-v1.xml)
