# Separate original BMS mine timing

The BMS adapter now preserves selected D1..D9/E1..E9 mine channels in a separate
typed source timeline. Damage is case-insensitive base36 independent of resource
BASE; 00 is a rest, nonfatal values retain exact half-percent units, and ZZ
retains distinct instant-death semantics. Optional WAV00 remains an opaque
resource definition. No damage token becomes a sample reference or ordinary
scored note. Mine positions may overlap visible, long or invisible source
events without overwriting their namespaces.

`MineDamage`, `MineEvent`, `ScheduledMine` and `BmsChart::compile_mines` expose
the checked source component. Mine subdivisions use their own grid; existing
gameplay, BGM, image and invisible grids, object identities and compiled times
are preserved. Actual seeded conditional selection, duplicate policy, raw/final
combined item limits and physical line diagnostics apply. Compilation validates
typed lanes, grids, duplicate positions/ordinals, total source bounds and BPM/
STOP rescaling, then uses the existing core compiler with pre-STOP same-beat
timing. Fatal damage has no numeric half-percent representation.

The shared playable preparation path explicitly refuses nonempty mine data
immediately after parsing, before its gain accessor, replay setup and asset IO.
Rest-only or inactive mine rows retain ordinary behavior. This boundary is
temporary: the parsed source is available to build real hazard processing, but
a playable loader cannot silently omit its mines. Parser errors still precede
this admission refusal, including malformed ordinary WAV references or VOLWAV.

The full integration contract lives in [BMS mines](../kernel/REQ__bms-mines.md).
Four independent adapter groups and two actual shared-source admission groups
are authored for later execution. They cover all 18 lane mappings, fixed
damage radix, fatal/nonfatal units, duplicate/conditional/source limits,
typed-source corruption, unchanged ordinary timeline/judge identity and
20-hour/week/overflow timing. Admission groups exercise actual parser error
precedence, refusal before replay/asset work and legacy empty/rest/inactive PCM
loading without automatically loading WAV00.

After both writers returned actual terminal STOPPED, scoped Rust formatting
and whitespace checks finished successfully. Four locked compile-only checks
finished with exit 0: workspace/all targets with WebTransport, no-default
WebTransport/all targets, WASM browser/lib and WASM browser-audio/lib. Existing
WASM cadence unused-code warnings remain. No tests or runtime acceptance were
executed, and Windows/macOS target-specific checks remain pending.

## Known ceiling

Actual mine gameplay is not installed. Held/contact occupancy, simultaneous
release/press ordering, one-shot outcomes, gauge/instant-death processing,
WAV00 playback, presentation, local voice isolation, completion and replay
identity/live-replay equivalence remain required work. The source component
does not establish historical player conformance, platform/audio/browser
acceptance or performance. Tests and required review/QA remain deferred; the
whole Goal and task stay open without PASS or completion claims.
