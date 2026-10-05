# Shared committed BMS mine damage

MineDamageSummary consumes actual one-shot core hazard outcomes using checked
full-width triggered/avoided counters, exact accumulated nonfatal damage units
and latched instant-death evidence. It validates every raw value before atomic
batch admission. Avoided markers add no damage and fatal tokens never become a
numeric percentage. Invalid values and arithmetic overflow retain the previous
summary. No collection, arbitrary event deduplication or asset work is added.
Raw units follow the documented [BMS mine source policy](../kernel/REQ__bms-mines.md).

Stepped solo and independent local members consume actual runtime reports,
including committed prefixes on later judge/audio failure. Read-only accessors
expose the retained state. Summary failure fences its owner with report and
other score/capture evidence; local member failures add a public `mine_error`
field whose external literal constructors must initialize it.

ReplayVisual consumes each successful recorded judge operation immediately,
before a later operation replaces that engine report. Multiple operations in
one presentation target therefore retain the complete damage prefix. Repeated
targets or targets with no new operation add nothing, and display time never
synthesizes a judge advance. StepReplay exposes that same summary without a
second damage rule. Fresh owners start empty; restoring a judge alone does not
restore application counters.

Five independent deferred groups in `mine_damage_fixtures.rs` cover the complete
nonfatal/fatal raw range, invalid-batch and counter/damage-overflow atomicity,
actual live audio-failure prefixes, independent local contact ownership, and
capture-to-ReplayVisual/StepReplay equality across multiple recorded operations
in one display target. Equal targets, empty reports and unassigned/unknown input
do not fabricate additional damage. Tests remain authored but unexecuted.

After both writers returned actual terminal STOPPED, scoped Rust formatting and
whitespace checks completed. Four locked compile-only configurations exited 0:
workspace/all targets with WebTransport, no-default WebTransport/all targets,
WASM browser/lib and WASM browser-audio/lib. Existing WASM cadence unused-code
warnings remain. These checks do not establish runtime or Windows/macOS
target-specific acceptance.

## Known ceiling

This component records exact damage and fatal evidence. It does not choose a
normal-note gauge curve, initial gauge or failure threshold, stop playback,
schedule WAV00, render mines or enable source admission. Native presentation/
report consumers and complete gauge/death/audio/completion integration remain
required. No test/device/browser/performance acceptance, formal review/QA or
whole Goal/task completion is claimed.
