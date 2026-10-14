# Native local-cohort selected-sound preparation

Windows, Linux and macOS local players now call the existing host-neutral
`local_preparation::prepare_selected_sound_bindings` instead of maintaining
three membership filters. The helper selects bindings by compiled chart object
ID in original order, preserving duplicates and every field bit. Each caller
retains its full original sound map through the complete cohort constructor,
including completion, then restores it at the existing site for backward
practice and full member voice remapping. Original source, PCM and BGM remain
under their current owners. macOS keeps completion replacement before tuple
restoration; no native operation, screen, command option or lifetime changes.

The contract lives in [shared BMS preparation](../kernel/REQ__bms-preparation.md)
and the ownership decision in [native boundaries](../kernel/ADR__native-runtime-boundaries.md).
The implementation adds one cold function to an existing module. It adds no
crate, port framework, lock, dynamic dispatch or callback/per-frame selection.
One explicitly reserved output Vec and an ObjectId set use O(objects + bindings)
auxiliary storage, including possible standard collection scratch. Removing the
macOS repeated linear scan does not establish a measured latency improvement.

## Verification

The source/test pair was frozen before coordinator execution. Actual
`local_preparation_fixtures` passed7/0: the original5 remain intact and two new
tests cover exact stable selection and genuine selected/full cohort preparation.
The pure oracle includes empty/all/subset/unknown IDs, duplicate records, custom
stages, signed zero and NaN payload bits. The integration uses real BMS/WAV,
`end: None`, an excluded original binding and actual `prepare_cohort`: the
unfiltered map fails completion, the selected map succeeds for every member,
and restoration permits full/backward preparation with independent voices and
identical PCM pointers/content and BGM. Existing retained-chart2 and cohort-setup7
also pass; these filters overlap larger suites and are not additive totals.

Host bin checking and Windows/macOS app/platform lib/tests typing exit0. The
foreign checks use C stubs and prove Rust typing only, not native ABI/device
execution. The initial no-default WASM check passes; the explicit browser feature
and full desktop/WebTransport app suite remain for independent qa-cli after
formal review. Read-only coverage/contract and performance advice found no
concrete defect; advisory results do not replace those gates. Raw logs are in
`target/wf/native-cohort-selected-sound-preparation/foundations/` and are ignored
local artifacts. The task's canonical PLAN owns exact commands and acceptance.

## Known ceiling

Known ceiling: standard BTreeSet allocation retains allocator failure behavior — upgrade only if recoverable global OOM becomes required.

Only the explicit output Vec reservation returns `TryReserveError`. Standard
collection scratch, allocator rounding, peak memory, process-global OOM and
physical/native performance are outside that guarantee. No allocator-failure
injection or new memory sandbox is claimed.

## Remaining scope

[WBS01.05/01.06](../kernel/REQ__bms-player-work-breakdown.md) remain W. This
extraction does not finish all DDD contexts, effect injection, application-to-
platform type independence or native resource composition. Current progress
remains106/223(47.53%); original product requirements remain91/193(47.15%).
Actual backend/device, browser, multiplayer, physical timing, stability and
release obligations remain active in the original Goal and WBS. The earlier
exclusive native task remains parked separately; its device blocker does not
block this pure preparation refactor or count as verification of it.
