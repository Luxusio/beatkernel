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

The application now combines original native validation and audio-authority
admission under one exclusive owner. Eight pure integration tests pass,
including genuine Runtime mapping, original ASIO brackets, exact basis identity
and authority refusal without native metadata mutation. Primed replacement and
same-epoch restart stage two original associations inline and preserve every
committed watermark on publication. Seven new priming tests pass within the
43-test authority/Step/browser filter. Neither priming nor a native observation
alone grants gameplay advancement. These foundations are not yet connected to
the native production pumps or held-output replacement state machine.

Original observation acquisition now runs through an opt-in static output port
implemented by the native adapters and selected Windows output owner. Remix and
switching preserve exact snapshots and errors; the output owner admits evidence
only after snapshot/render acquisition succeeds and reads accepted ASIO brackets
for pause/end. Seven new pure port tests pass within the 53-test output regression
filter (two existing tests ignored). Windows without optional ASIO and macOS
all-target Rust checks pass using C/archive stubs: these are source/type checks,
not native SDK builds or hardware execution. Optional ASIO SDK compilation
remains unverified. Linux all-target and browser WASM checks also pass. This port
is implemented but not yet selected by the native
gameplay pumps, and held-output audio publication still needs integration.

Native pause/resume now exposes the committed original physical output cutoff
alongside its conservative HOST boundary and epoch. Logical Transport staging
checks that boundary against an authority-prepared control descriptor and uses
the logical output point; active owners remain unchanged during preparation.
Control commitment records only an actual Runtime operation, while explicit held
frontier servicing closes acquisition/presentation without advancing Runtime or
inventing an input occurrence. The 17 new lifecycle/control tests pass within
the 60-test audio-authority filter; the 79-test pause filter also passes. These
overlapping filters are not summed. Browser WASM checking also passes. The native gameplay pumps and held-output
publication still need to select these APIs; AC008 remains partial.

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
