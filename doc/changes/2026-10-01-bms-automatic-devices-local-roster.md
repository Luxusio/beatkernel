# Automatic solo devices and scalable local player ownership

The user corrected the earlier mandatory device picker: solo play must start
without device selection; multiple local players assign their own input devices,
including three/four and further growth. Low-level platform APIs stay explicit
after application preparation. Shared audio is independent of player count.

The primary play/player app resolves omitted native device/configuration options
on the game owner. Windows selects the active OS multimedia default WASAPI
endpoint with shared mode and Any-keyboard input. Linux uses ALSA default and the
first readable standard keyboard in numeric event order. macOS queries the OS
default media output and ordered keyboard registry metadata. Advanced explicit
values are preserved. Strict parsing with temporary parser-only values runs
before discovery; syntax validation performs no native I/O and never persists
or executes placeholder IDs. ASIO has no OS-default driver and remains advanced
explicit configuration. Native preparation still validates actual access/support.

Keyboard metadata APIs are independent of acquisition: Linux uses bounded
read-only nonblocking metadata ioctls; macOS uses owned IOKit service/CF metadata
without a HID acquisition manager; Windows queries unregistered Raw Input.
The solo UI exposes no Input Devices prompt. The bounded catalog remains reusable
for future per-player assignment. Native traversal/allocation ceilings and
classification subsets are in the [discovery contract](../platform/REQ__keyboard-discovery.md).

Windows also supports an optional exact advanced keyboard interface path. Bind
its freshly resolved session DeviceId, exclude other sources before runtime
admission, and fail on removal without reconnect substitution. Omitted path
retains Any-keyboard behavior.

The portable LocalPlayers primitive uses stable member IDs and a caller-bounded
collection (up to 64), rather than P1/P2 or fixed four-player slots. Solo input
is automatic; N>=2 requires unique explicit native identities before sealing.
Resolving different aliases to one actual DeviceId rejects the session routes.
Growth/shrink preserves retained IDs, retired IDs are not reused, and invalid
assignments/capacity changes preserve accepted roster state.

This increment does not provide executable local multi-player sessions. Roster
assignment UI, separate Runtime/judge/replay/score owners, shared song/audio
composition and roster-driven playfield layout remain required implementation.
Fixtures were authored only; actual native discovery/gameplay/UI execution and
formal review/QA remain deferred. The full BMS player Goal stays active.

Scoped formatting and source checks succeeded on 2026-10-01: workspace all-targets
on Linux, app all-targets for Windows GNU/macOS x86_64, headless all-targets and
WASM graphics library. Existing WASM cadence dead-code and macOS `block 0.1.6`
future-incompatibility warnings remain. No fixtures or native queries ran.
