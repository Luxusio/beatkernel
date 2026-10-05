# BMS mine plans and shared judge construction

MinePlan prepares actual BMS mine timing as an optional core HazardTimeline.
It retains the original ScheduledMine records for later application use and
maps ordinal, original absolute time, logical lane control and exact raw damage
into core hazard markers. Caller count bounds precede timing preparation;
existing typed-source, combined-count and checked timing validation still apply.
It performs no asset IO, PCM allocation, gauge calculation or sound scheduling.
Empty sources retain an absent timeline and exact legacy judge state identity.

The shared prepare_judge composition selects the existing input-mode rules and
enables contact tracking for mine-only as well as invisible sources when contact
mode is requested. Stepped solo, common local-member preparation, source-aware
replay setup validation and offline construction use it before returning an
owner. Actual replay setup hashing sees configured hazards; changed mine values,
controls, times and identities cannot masquerade as the same pristine judge.
Local binding coverage includes mine-only controls as well as ordinary and
invisible lanes; missing mine control admission fails before returning members.
The caller supplies the ordinary chart matching the selected source. Practice
source retains original mine timestamps and uses fresh ownership without
inventing earlier held state. The complete contract is
[BMS mine integration](../kernel/REQ__bms-mines.md).

Five independent deferred groups in `mine_plan_fixtures.rs` cover all eighteen
lanes, raw damage/full-width ordinals/exact timing, budget-before-compile and
typed corruption, empty legacy identity and mine-only contact checkpoints,
actual stepped solo/local installation and capture, and source-aware replay
identity/practice/offline boundaries. Typed PreparedBms fixtures deliberately
exercise source composition below the still-active file admission guard; they
do not establish complete mine gameplay. No tests have been executed.

After both writers returned actual terminal STOPPED, scoped Rust formatting and
whitespace checks completed. Four locked compile-only configurations exited 0:
workspace/all targets with WebTransport, no-default WebTransport/all targets,
WASM browser/lib and WASM browser-audio/lib. Existing WASM cadence unused-code
warnings remain. No app, device, browser or generated-binding acceptance ran.

## Known ceiling

The shared playable-source loader continues to refuse nonempty mines. Direct
native solo constructors and consumer-side gauge/death, WAV00, completion,
rendering and complete live/local/replay/practice/offline installation remain
required. This component does not claim playable BMS mine support, historical
conformance, executed tests or browser/device/performance acceptance. Required
formal review/QA and the whole Goal/task remain open.
