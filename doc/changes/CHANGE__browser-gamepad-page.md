# Automatic solo browser Gamepad session

Connect the optional Gamepad acquisition owner to the live browser page and
actual Worker setup/input path. Window discovers exposed connections, preserves
original sample timing and forwards bounded descriptors/samples. Worker admits
standard nine-button solo bindings and interprets deltas through the common
physical input API. No device chooser is required for solo play; unknown layouts
have no invented controls. Replay opens no live Gamepad acquisition.

Gamepad and HID owners share a session source allocator; standalone HID keeps
its prior default. Actual source admission, participating disconnect, stop and
cleanup guard the current session. Polling uses the existing live input pump
without an extra render loop or timer. Source IDs and original event times are
retained; old callbacks cannot restart a disposed session.

Align Worker input chronology with the core's per-device acquisition sequences.
Preflight original source order and stably sort actual nonempty step entries by
original timestamp before Runtime calls. This allows interleaved physical sources
without rewriting sequence metadata. Keep the committed global time boundary,
whole-step draft validation and packet capacity, and refuse changed late input
rather than retimestamping. Main cursor/watermark use the maximum original
batch time and prior cursor so unchanged snapshots cannot rewind them.

Known ceiling: Browser/device execution, measured input/render/main-thread
performance and capture/replay acceptance remain unverified. Nonstandard
automatic controller mappings and local multiplayer device assignment remain
work. Polling cannot recover intermediate transitions, and changed samples
behind the committed frontier remain explicitly refused. Standard solo defaults
do not imply analog judgment or support for every controller layout.

Seven independently authored deferred groups cover automatic native bindings,
full-width shared source allocation, cross-source timestamp ordering with
original sequence metadata, same-source refusal, actual Window raw acquisition,
unavailable/ignored/replay paths, exact source receipts, stale/disconnect/reentry
fences, and operational versus cleanup failure evidence. Totals are Window 67,
Worker 59, profile 6, HID 4; acquisition remains 8 with cleanup assertions
aligned. Scoped tracked and staged whitespace emitted no diagnostics. No
parser, assertions, tests, Node, generated bindings, runtime/browser/device,
audio, formal review or QA was executed. Unchanged Rust checks were not
repeated for this JavaScript-only change. The full player Goal and Harness
task remain active and unproven.
