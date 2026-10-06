# Linux live output channel settings

The Linux paused-live output panel now includes OUTPUT CHANNEL MATRIX. Rows
separated by semicolons describe output channels; comma-separated finite f32
coefficients address original source channels. Input is bounded to 32×32 and
4096 bytes. The pure common parser/canonical formatter owns this policy.

Typed request mapping validates source width before output retirement. Empty
preserves the current matrix, including when changing device/buffer/period;
`exact` explicitly restores original source channels. ALSA keeps accepted matrix
metadata and successful output replies advertise canonical coefficients. The
existing native solo owner now composes RemixedOutputBackend and the unchanged
output UI/controller, so no additional lifecycle or judge logic is introduced.

This is live-output-only. Initial launch arguments and saved profiles remain
strict; other OS panels do not advertise the field. Matrix routing is output
presentation policy, not a judge/capture identity change. Rate stays fixed.
See [player requirements](../kernel/REQ__bms-player.md).

The retained panel now supports four output fields. Four-row spacing keeps the
matrix input/hint above messages and buttons while preserving three-row layout
for unchanged host panels. A desktop fixture exercises field edit→Apply→canonical
reply in the same child scope; this is component/bridge execution, not a live
physical-input GUI session.

Current verification after final source edits (actual process exits observed):

- Runtime library: 1,583 passed, 0 failed, 2 ignored, exit 0.
- Main app tests: 223 passed, 0 failed, exit 0.
- Separate actual ALSA null settings diagnostic: 1 passed, exit 0. It applies
  strict→`1;0.5`→`exact`, verifies preserve/reject paths and recovers the same
  original mono/paused Mixer at each native retirement.
- Workspace all-targets with webtransport and WASM browser checks: exit 0;
  existing dead-code warnings remain.
- Actual latest binary help: exit 0. Initial `play --output-matrix '1;0.5'`:
  exit 1 with unknown native setting, confirming the live-only schema boundary.
- Independent code/security review and scoped CLI QA: PASS. CLI QA executed
  the binary/native diagnostic; the retained UI proof remains test-suite tier.

Ignored logs: `target/wf/live-matrix-qa-cli-{lib,main,native-actual,workspace,wasm,help,invalid}.log`.
The earlier zero-test wrong-filter native log is excluded from evidence.
Full paused-play GUI Apply execution is unverified: this Linux host has no
`/dev/input` or `/dev/uinput` despite having Xvfb/xdotool. Physical-device clock
and acoustics are not established by the software sink. No full-task receipt
attestation, task verify/close or full Goal completion is claimed.
Actual physical-device acoustics,
Windows/macOS UI mappings, persistence/launch options, rate conversion and full
player acceptance remain unfinished.
