# Audio-authoritative player migration

Status: shared foundation, Step integration and browser adapter migration are
implemented; native production migration, lifecycle integration and independent
final QA remain pending. The browser acquisition-envelope refusal has been
corrected and passed three independent pre-review Chromium smoke executions.

The core can map two supplied clock observations with explicitly unknown
accuracy, without inventing an error bound. The application's finite authority
joins original acquired HOST input with raw output observations on a separate
logical audio timeline. Shared solo/local Step owners use normal-rate audio
Transport, preserve input provenance and raw scheduling points, and commit
genuine Runtime reports before fallible score/gauge/capture observers. Generic
HOST/PLL entry points reject these new owners before mutation. InputMerger adds
explicit cold-preallocated registration of up to 4096 actual sources while
preserving fixed local rosters, ordering and pending budgets.

Focused evidence: core calibration 11 tests; authority 16; genuine Step audio
integration 11; source registration 7; existing Step regression 119 and input
regression 22. All pass. Native all-target and browser WASM checks pass with the
new shared code. These overlapping filters are not summed as a whole-suite
count. The latest full workspace run belongs to the earlier source revision.

The migrated browser queues original inputs before joining their acquired prefix
with presentation evidence. Pending touch positions retain acquisition geometry;
page changes wait for Rust-held input as well as message acknowledgement. The
earlier Node web regression run passed all 522 tests. An actual Chromium smoke
then exposed `Gameplay acquisition time exceeds its current Window receipt`.
Admission now uses the verified same-Window acquisition envelope; freshly
reconstructed time remains exclusive to service and cannot clamp original
inputs. After this fix, three independent actual-browser runs completed input,
2.5 seconds of playback, Stop and historical-record navigation. Exact failing
clock operands were not captured, so numerical quantization is not established
as the specific cause. The subsequent Node regression run passes all 524 tests,
including unchanged input retention and future-prefix holds across the distinct
acquisition/service samples.
These are development diagnostics, not ordered independent QA acceptance.

Current Rust app library/bin regression with desktop and WebTransport passes
2,091 tests with two existing ignored tests. This does not replace the pending
full workspace and platform-target checks or formal QA.

The native presentation validator stages original WASAPI, supplied-pair and
ASIO evidence without an estimator or Transport. ASIO brackets remain intact
for lifecycle consumers. Platform library and integration tests pass, including
11 independently authored validator tests (214 total passed, one existing
test ignored).
This extraction is not yet selected by the production native player.

## Known ceiling

Native pumps still need to select the new authority and join actual output
evidence with acquired prefixes. Browser production selects the authority and
has passed bounded actual-play diagnostics; final independent review and QA
remain required. Generated frames cannot replace presentation.
Generic estimator APIs remain available for explicit legacy consumers; this is
not permission to keep them as BMS play-time authority. Real browser/native QA,
output replacement/resume, replay compatibility and measured latency remain
required before this migration is complete. No hardware or osu superiority is
established by pure fixtures or type checks.
