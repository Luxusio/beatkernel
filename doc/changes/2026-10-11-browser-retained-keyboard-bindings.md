# Retained browser keyboard bindings

Status: implementation candidate; independent review and complete runtime QA
remain pending. This does not close TASK__browser-retained-binding-editor or
promote a WBS item. Overall WBS remains 91/193 (47.15%).

Settings now carries the existing eighteen physical keyboard codes after its
sixteen scalar/policy fields. The Worker validates all34 before Apply, while
Window publishes only accepted values and snapshots the input bindings before
playback. Initial raw acquisition remains editable even with unfinished values
or duplicate codes. Legacy13/16 Apply preserves the current keyboard bindings.
The existing version1 profile codec, retained pages, editor and ports are reused.

The original DOM binding selectors and reset are locked while a Settings
ancestor owns its draft, including Practice/Players/Devices descendants; exit
restores standalone acquisition. Full profile admission replaces the entire
retained34 draft through the existing correlated T0/T1/T2 transaction. Practice
Done changes only section fields. A16-field replacement cannot erase bindings
from a34-field parent, even when an early field is selected.

Development evidence so far (not independent QA):

| Scope | Observed result |
|---|---|
| Shared profile conversion |17 tests passed; exact34 ordering, strict validation, frozen DTOs and legacy compatibility|
| Rust browser menu |53 tests passed after fixing an owned fixture coordinate type; eight new binding/lifecycle tests|
| Worker |91 tests passed using rebuilt production WASM|
| Window full suite |211 passed, one new fixture failed because its explicit section end did not match the generic preparation reply|
| Corrected Window binding fixtures |5 passed; open-ended section matches the original preparation fixture, physical KeyA19 assertions retained|
| JS syntax / diff whitespace |passed for changed product modules|

Logs, including contrary evidence, are retained under
`target/wf/bindings-development-12ad2/`. The full Window suite has not yet been
rerun after that fixture correction. Actual browser input/capture/replay
acceptance is being prepared; no new browser PASS is claimed.

The [owning requirement](../ui/REQ__browser-retained-keyboard-bindings.md)
requires real KeyA Down/Up for lane11, absence of old KeyZ acquisition, nonzero
judgment, actual recorded input bytes and playback of those captured bytes
independent of later live bindings. DTO roundtrip or miss-only completion is
insufficient. Previous audio-WASM cancellation diagnostics remain unresolved in
TASK__browser-retained-settings-policy and all strict browser/audio/network
checks remain in force here.

Known ceiling: physical device/GPU latency, physical OS IME and screen-reader
completeness are not established by software fixtures; upgrade the evidence
when those original WBS acceptance environments are available.
