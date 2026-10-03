# BMS LNTYPE2 normalization

AC-205 adds selected LNTYPE2 cell spans to the BMS adapter. Each occupied
subdivision retains exact rational start/end positions; overlapping or touching
accepted spans merge on the same supported long-note lane. Zeros and missing
measures leave gaps, and the final occupied subdivision ends at its actual
measure boundary. The first retained chronological cell supplies the head
keysound; continuation values and synthesized endpoints are unsounded.

These spans become the existing SourceObject/Hold representation and run
through the same live and replay judge. Exact same-position duplicates retain
Reject/LastWins behavior before union, including cell-extent replacement.
Existing paired LNTYPE1, visible LNOBJ, same-lane collision, seeded branch,
physical source and checked resolution rules remain. Zero-filled rows do not
create gameplay events for every zero; nonzero spans retain source-item bounds.

The implementation policy follows the
[original RDM/ruv-it documentation](https://nvyu.net/rdm/rby_ex.php),
with the [format memo](https://saxxonpike.github.io/bms-command-memo/index.html#LNTYPE2)
as corroboration. No historical extended MGQ keyboard layout, mines, invisible
notes or universal legacy-conformance claim is added. Parser and genuine
runtime/PCM/capture/replay source fixtures are prepared for later execution.
Compiler checks cannot establish actual format or device/browser acceptance;
formal review and QA remain deferred and the full Goal stays active.


Six independent fixture groups were authored: four parser/compiler groups and
two genuine stepped-runtime/PCM/capture/replay groups. Prior groups are retained;
the existing unsupported-command example now uses LNTYPE3. Source fixtures
cover occupied-span union, gaps and measure endpoints, duplicate replacement,
head-only sound admission, rational resolution, timing/seeded bounds, same-lane
conflicts, live/replay parity and a selected-section EarlyRelease prefix. They
remain unexecuted; the actual application fixture total is 30.


Scoped rustfmt and tracked/staged whitespace checks completed. Four Cargo check
paths reached terminal exit0: workspace/all-targets (including the new adapter
test target), headless app/all-targets, WASM browser library and WASM
browser-audio library. Existing three platform render-cadence dead-code warnings
remain on WASM. These commands compile fixture bodies; no parser fixture,
assertion, real player, Mixer output or generated browser binding was executed.
No full-task review/QA acceptance or Goal completion is claimed.
