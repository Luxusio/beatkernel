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
execution. The initial no-default WASM check passed before review. Read-only
coverage/contract and performance advice found no concrete defect; advisory
results did not replace the independent gates below. Foundation logs are in
`target/wf/native-cohort-selected-sound-preparation/foundations/` and are ignored
local artifacts. The task's canonical PLAN owns exact commands and acceptance.

The actual independent DEEP review of frozen `2b9928d3` returned PASS with zero
findings after both discovery attempts. Only then did independent qa-cli run and
return PASS for the pure preparation and software integration scope:

| Check | Actual result |
|---|---|
| App desktop/WebTransport library | 3260 passed, 0 failed, 8 ignored |
| App binaries | 624 passed, 0 failed, 17 ignored |
| Preparation / retained-chart / cohort focused filters | 7 / 2 / 7 passed; overlap the full suite |
| Host binaries, explicit browser-feature WASM | exit0 each |
| Windows/macOS app/platform lib/tests C-stub typing | exit0 each; no native ABI/runtime proof |
| App library Clippy without denying warnings | exit0, 394 warnings; not strict-clean |
| Four production/one fixture format, diff, WBS | pass |
| Local links in the five durable documents | 139 existing targets |
| Three freshly built public native CLI binaries | 12/12 expected argument/help results |

The CLI cases check help/empty arguments at exit0 and unknown flags/missing
values at exit1, including stdout/stderr. They do not invoke the native playback
or selected helper path; the latter is exercised by actual library/cohort tests.
QA confirmed all ten frozen SHA values and the tracked clean tree before and
after execution. Its exact argv, exits and output are retained under
`target/wf/native-cohort-selected-sound-preparation/qa-cli-2b9928d3/`.
Actual review/QA results do not themselves supply hook-owned completion records
or authorize task close; final documentation audit and owning verification remain
separate. This evidence does not complete the full BMS player Goal.

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
