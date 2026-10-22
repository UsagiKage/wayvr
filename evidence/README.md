# Static verification checkpoint — 2026-10-21 PDT

Baseline: upstream v26.8.0, `cb3aa42bd91aeda0e4d0de91593a6f878350c757`.
Untouched baseline build: PASS, Wayland/OpenVR, locked dependency resolution,
`plain` profile. Preserved baseline executable SHA-256:
`49e173527aaf5678fc5580e629d527e9e3bca5e1ac2121d991743987983ff06e`.

Environment: rootless Podman, pinned Rust/Debian image, Rust 1.99.1. No host
compiler/development package installation. Three Cargo jobs, four CPU cores,
12 GiB memory limit. No host GPU, input device, Wayland socket or session bus.

Checks recorded locally (raw logs intentionally excluded from Git):

- Untouched baseline compilation: `baseline-build.log`.
- Original baseline failure: `baseline-first-attempt.log`; missing dav1d headers.
  Corrected inside the build image only. Debian's unavailable libopenvr-dev
  package was omitted; the pinned Rust dependency supplies its OpenVR SDK.
- Experimental feature disabled compilation: `feature-off-build.log`.
- Experimental feature enabled compilation: `prototype-build.log`.
- Rust tests: `prototype-tests.log`; 8 passed. Source/locale tests and mock input/metadata
  tests. Cover multi-button/key/modifier release, tracking loss, stale data,
  capture loss, wrong UUID, geometry epoch, focus/visibility/popup rejection,
  explicit activation acceptance and revocation.
- JavaScript bridge tests: `kde-bridge-tests.log`; 3 mock tests, including no
  hover activation, geometry/scaling, occlusion, closure and popup bounds.
- `cargo fmt --all --check`, shell syntax and `git diff --check`.
- Executable dependencies inspected without launching the executable. The host
  has matching ordinary sonames; the unversioned OpenVR loader is bundled under
  artifacts/lib and selected through the experimental executable's own RUNPATH.

Compiler warnings from the upstream Wayland/OpenVR feature combination remain;
no broad warning cleanup was attempted. Static success does not establish runtime
correctness. Real KWin capture authorization, script execution, native window input,
headset behavior and SteamVR game-input handoff have NOT been tested.

Run `sh scripts/archimedes-build.sh` to reproduce build/static checks. It never
launches the VR application. See ARCHIMEDES.md for the separate runtime gate.
