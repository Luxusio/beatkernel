# Audio-authoritative player migration

Status: shared foundation, Step integration and browser adapter migration are
implemented, as are common native audio loops and held-output publication;
native launcher/startup selection is now connected and its wider platform
checks and independent final QA remain pending. The browser acquisition-envelope refusal has been
corrected and passed three independent pre-review Chromium smoke executions.

Linux, Windows and macOS solo/local launchers now construct the cold shared
audio owner, retain original startup input and select the audio gameplay
bridges. Their configuration keeps requested section start separate from
preroll and selected physical playback frame. Offline finite startup retains
the first original lower observation and defers any already-established end
boundary for one normal gameplay delivery; network startup keeps its existing
pre-arm observer. Startup bounds account for negotiated callback size instead
of assuming that every valid device produces two observations within two seconds.
The focused native-audio library/bin run passes 36 tests: 14 startup,
14 gameplay and eight owner fixtures. Native-end and native-start filtered
regressions pass 15 and 20 tests respectively. These overlapping development
checks do not establish Windows/macOS
SDK, physical hardware or ordered independent QA.
The launcher macOS all-target Rust check passed with C/archive stubs before
two warning cleanups. The first Windows launcher check failed with 39
import/scope errors; the public imports are repaired and their all-target
recheck passes with C/archive stubs. Fresh browser WASM checking also passes.
Fresh app library/bin regression with desktop and
WebTransport passes 2,175 tests with zero failures and two existing ignored
tests. Existing capture/playback/native-feed integration regressions pass
21 tests with unchanged assertions. Formal review/QA remain pending.
The joined native capture/replay fixture now also passes: two real native
hits roundtrip through codec and reconstruction with the same judgment events,
judge hash, gauge, EX score, logical timestamps and original HOST provenance.
Replay keysounds preserve their identities and order under the explicitly
chosen replay output origin and preroll. Existing legacy integration tests keep
their recorded interpretation; one joined fixture does not prove every recording
combination.
The current JavaScript browser regression run also passes all 524 tests using
the documented Node VM-module flag. Real browser QA remains required after
formal review. The next queued browser-isolation task starts by removing
gamepad sampling from keyboard/HID/touch dispatch; splitting rendering from
the gameplay Worker requires a separately verified bounded visual-state port.

Independent review clarified preparation tokens as operation-relevant semantic
state: commits re-prepare their actual guards and mapping/result, while clearing
operations may accept different unused interior observations that they discard.
Two new regressions prove both clearing equivalence with retained watermarks
and atomic refusal when an interior observation changes the selected input or
frontier result. The focused authority/native/Step filter passes 54 tests. No
production logic, hot allocation or recording schema changed. The 2,175-test
full app run above precedes these two added fixtures; final independent QA is
still required. Stale freshness, ordinary-resume, live/replay browser method and
blanket test-deferral descriptions were reconciled with the actual callers.

The core can map two supplied clock observations with explicitly unknown
accuracy, without inventing an error bound. The application's finite authority
joins original acquired HOST input with raw output observations on a separate
logical audio timeline. Shared solo/local Step owners use normal-rate audio
Transport, preserve input provenance and raw scheduling points, and commit
genuine Runtime reports before fallible score/gauge/capture observers. Generic
HOST/PLL entry points reject these new owners before mutation. InputMerger adds
explicit cold-preallocated registration of up to 4096 actual sources while
preserving fixed local rosters, ordering and pending budgets.

## Earlier development checkpoints

The counts and platform checks below belong to earlier migration stages and
do not establish fresh regression after the connected launcher changes.

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

The earlier Rust app library/bin regression with desktop and WebTransport passed
2,158 tests with two existing ignored tests. This does not replace the pending
full workspace and platform-target checks or formal QA.

The native presentation validator stages original WASAPI, supplied-pair and
ASIO evidence without an estimator or Transport. ASIO brackets remain intact
for lifecycle consumers. Platform library and integration tests pass, including
11 independently authored validator tests (214 total passed, one existing
test ignored).
At that checkpoint, production native selection was still pending; the
connected launchers described above now select this validation through the
exclusive audio owner.

The application now combines original native validation and audio-authority
admission under one exclusive owner. Eight pure integration tests pass,
including genuine Runtime mapping, original ASIO brackets, exact basis identity
and authority refusal without native metadata mutation. Primed replacement and
same-epoch restart stage two original associations inline and preserve every
committed watermark on publication. Seven new priming tests pass within the
43-test authority/Step/browser filter. Neither priming nor a native observation
alone grants gameplay advancement. The shared loop and publication integration
below connects these foundations; launcher selection was pending at that
checkpoint and is now connected as described above.

Original observation acquisition now runs through an opt-in static output port
implemented by the native adapters and selected Windows output owner. Remix and
switching preserve exact snapshots and errors; the output owner admits evidence
only after snapshot/render acquisition succeeds and reads accepted ASIO brackets
for pause/end. Seven new pure port tests pass within the 53-test output regression
filter (two existing tests ignored). Windows without optional ASIO and macOS
all-target Rust checks pass using C/archive stubs: these are source/type checks,
not native SDK builds or hardware execution. Optional ASIO SDK compilation
remains unverified. Linux all-target and browser WASM checks also pass. This port
is selected by the common audio loop/publication APIs. Native launcher selection
was pending at that checkpoint and is now connected as described above.

Native pause/resume now exposes the committed original physical output cutoff
alongside its conservative HOST boundary and epoch. Logical Transport staging
checks that boundary against an authority-prepared control descriptor and uses
the logical output point; active owners remain unchanged during preparation.
Control commitment records only an actual Runtime operation, while explicit held
frontier servicing closes acquisition/presentation without advancing Runtime or
inventing an input occurrence. The 17 new lifecycle/control tests pass within
the 60-test audio-authority filter; the 79-test pause filter also passes. These
overlapping filters are not summed. Browser WASM checking also passes. Common
audio loops and held publication select these APIs. Platform startup/entrypoint
selection was pending at that checkpoint and is now connected; full production
AC008 acceptance still requires fresh independent verification.

The existing solo/cohort business loops now use static legacy/audio timing
wrappers, with no duplicate judge/pause/completion loop or instantiated legacy
estimator in audio sessions. Twelve genuine memory-device tests pass for mapped
original inputs, raw scheduling, logical capture/score, future ASIO associations,
pause/resume, finite completion and report/observer failure ordering. Actual
Runtime domain identity is checked before IO even without capture. Original
input at a resume cutoff remains queued until after the control operation.

The same output controller stages held audio replacement using two original
associations, preserves committed watermarks and original ASIO end evidence,
and releases the lease after all ownership/cache swaps. Ten independent
publication tests pass. Four genuine UI request tests pass for correlated audio
publication, mapping refusal, reply backpressure and error sanitization. All
new tests also pass in the full app library/bin regression above. Windows and
macOS all-target source/type checks and browser WASM checking pass; optional
ASIO SDK/hardware execution is still unverified. Launcher startup, normalized
headers and live calls were not yet switched at that checkpoint; the connected
launchers described above now construct logical-domain gameplay and setup.

## Known ceiling

Native launchers now select the common audio loops, startup correlation and
logical-domain capture/competition setup. Wider platform checks, replay
compatibility and real native execution remain required. Browser production selects the authority and
has passed bounded actual-play diagnostics; final independent review and QA
remain required. Generated frames cannot replace presentation.
Generic estimator APIs remain available for explicit legacy consumers; this is
not permission to keep them as BMS play-time authority. Real browser/native QA,
output replacement/resume, replay compatibility and measured latency remain
required before this migration is complete. No hardware or osu superiority is
established by pure fixtures or type checks.
