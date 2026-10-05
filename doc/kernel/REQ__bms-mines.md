# BMS mine source and runtime integration

## Original source and timing

Accept selected mine channels D1..D9/E1..E9 in a separate typed namespace,
mapping to original visible lanes 11..19/21..29. Mine payloads are direct
case-insensitive base36 damage values, independent of resource BASE16/36/62.
00 is a rest; 01..ZY retain exact half-percent damage units; ZZ is a distinct
instant-death value. Never interpret a damage token as a WAV reference.
Optional WAV00 remains an opaque explosion-sound definition; absence requires
no fabricated asset or implicit sound. Damage semantics follow the public
[Angolmois developer documentation](https://github.com/lifthrasiir/angolmois/blob/master/INTERNALS.md#data-commands);
BeatKernel preserves the full nonfatal range instead of discarding values
above 100%. This is an explicit policy, not full historical compatibility.

`BmsChart` stores original mine events and an independent exact tick grid
encompassing gameplay resolution. Record beat, original lane, typed damage,
independent ordinal and physical source line. Mine subdivisions must never
enlarge existing gameplay, BGM, BGA or invisible grids or change normal object
identities. Merge same-lane positions under existing Reject/LastWins rules in
the mine namespace only. Preserve overlapping visible/hold/invisible source
positions for the runtime policy; do not silently convert or discard mines.
Seeded conditional selection, source/line/grid bounds and combined raw/final
source counts include mines, also before LastWins replacement.

`compile_mines()` returns a separate sorted `ScheduledMine` timeline using the
existing checked core timing compiler and pre-STOP same-beat semantics.
Validate fabricated grids, lane identities, duplicate positions/ordinals,
rescaled BPM/STOP arithmetic and combined source limits. Mine compilation does
not insert judged objects or automatic BGM into the returned ordinary chart.
Empty mine timelines preserve existing compilation and replay setup behavior.
Author parser, timing, identity, duplicates, BASE, seeded, cap, malformed typed
source and long-duration fixtures for deferred execution.

## Integration boundary

During source integration, shared `prepare_from_source` rejects any nonempty
mine timeline immediately after actual parsing and before gain/replay/asset
lookup, reads, decode or PCM allocation. Empty/rest-only/inactive mine rows
retain the ordinary loading path. This prevents loading a playable chart which
silently omits its hazards. Adapter parsing and explicit timing are available;
the temporary admission refusal is removed only when actual portable hazard
processing is installed in live, local, replay, practice and offline owners.

Full mine support still requires contact/held-button occupancy at original-song
mine times, deterministic simultaneous release/press ordering and per-mine
one-shot outcomes shared by live and replay. Rendering, gauge damage/instant
death, WAV00 output, local voice separation, completion and source-aware replay
identity must use those actual outcomes. Do not claim these integration
requirements complete from a separate source timeline or compile-only checks.
Execution, platform/audio/browser acceptance and required review/QA remain
deferred under the active task; the whole player Goal remains open.
