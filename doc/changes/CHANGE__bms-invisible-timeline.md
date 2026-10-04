# BMS invisible keysound timeline foundation

The adapter represents invisible keysound-selection channels separately from
judged gameplay objects and automatic BGM. An independent exact tick grid and
core-compiled timeline preserve source lane, sample, line and ordering, without
changing the visible chart's timing grid. Shared preparation explicitly refuses
nonempty invisible data until actual empty-key sound and replay integration is
implemented, before acquiring any sound asset.

Four independent parser/timing and two actual shared preparation fixture groups
are prepared for deferred execution. Scoped rustfmt and whitespace checks
completed. The workspace with all targets and WebTransport, the headless
WebTransport runtime, WASM browser and WASM browser-audio configurations each
completed cargo check with exit zero. The WASM checks retain the three existing
cadence dead-code warnings. These compile-only checks do not establish runtime
behavior, playable invisible-note support or audio correctness.

## Known ceiling

Empty-key sound selection, replay identity and actual playback for invisible
channels remain pending. Current player admission rejects those charts explicitly.
Mines remain unsupported. Runtime tests, browser/device/audio execution, formal
review and QA remain deferred; the full player task remains active.
