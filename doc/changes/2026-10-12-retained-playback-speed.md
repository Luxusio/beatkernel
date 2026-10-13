# Retained fixed positive playback speed: implementation in progress

The active child is TASK__retained-positive-playback-speed. This document does
not claim final implementation, independent QA PASS or Goal completion.
The behavior contract is [retained playback speed](../kernel/REQ__retained-playback-speed.md).

## Core development evidence

The first actual focused execution ran
`cargo test -p beatkernel --locked --test practice_speed`: **17 passed,
0 failed**, compilation 16.73 seconds, test execution 0.03 seconds.
Original log: `target/wf/retained-speed-development-12e0c/core-speed-c1.log`.
The tested source is an uncommitted intermediate working tree, before the
origin-aware finite-horizon correction described below.

The tests exercise literal original PCM and keysound output at 1/2, 1, 2 and
3/2, odd signed preroll, original source selection, finite cue exclusion,
non-grid loops, scrub/disable, acknowledged pause, pressure atomicity,
revision refusal, converted projection and callback allocation probes.
PCM fixtures and independent expected values stay below the Mixer's output
clamp. This is coordinator development evidence, not an independent QA receipt.

After the finite-horizon correction, existing core lib/practice/allocation
targets passed **12 + 27 + 7 tests**. Log:
`target/wf/retained-speed-development-12e0c/core-existing-c1.log`.
The expanded selected-speed suite then ran **19 tests: 18 passed, 1 failed**
in `core-speed-c2.log`. Its failed assertion incorrectly required the total
rendered-frame counter to remain unchanged after a successful paused silence
callback. Command credits and playback position must freeze there; physical
rendered frames must advance. After correcting that assertion, the actual
fresh `core-speed-c3.log` execution passed **19/19**, with no ignored tests,
in 0.04 seconds. It includes origin-zero atomic refusal, representable
origin-MIN playback, and actual nonzero-cut horizon refusal with pause/resume.

The subsequent actual full core command
`cargo test -p beatkernel --locked --no-fail-fast` finished with **566 passed,
0 failed, 0 ignored across 65 test targets**, including documentation tests.
Compilation took 3 minutes 2 seconds. Original log:
`target/wf/retained-speed-development-12e0c/core-full-c1.log`.
This establishes standalone core regression evidence; it does not prove the
unimplemented native fresh-attempt phase bridge or required independent QA.

## Current open work

- Remove an unnecessarily narrow signed-duration rejection in generic core:
  actual timestamp representability includes the original output origin.
  Validate finite installation and callback transitions before mutation.
- Audit fresh-attempt phase at nondivisor sample Hz and pauses. A rounded
  absolute timestamp difference can differ from a separately rounded relative
  frame duration by one nanosecond. Converter projection must be included in
  the analysis before any shared-authority interface changes.
  Static inspection confirms the discrepancy at 44100 Hz: cut frame 2 to
  frame 3 yields 22675 ns from relative-frame floor and 22676 ns from absolute
  timestamp subtraction. Existing selected-rate pause validation also uses
  cumulative playback frames, which is incorrect after a nonzero fresh cut.
  These are implementation obligations, not an accepted timing tolerance.
- Common preparation and canonical fresh capture/comparison are now assigned
  to separate source and test writers after the standalone core regression
  passed. Their implementation and verification remain pending. Scalar
  selected-rate transport alone cannot satisfy the phase integration gate.
  Solo/cohort pumps and thin Linux/Windows/macOS adapters follow verified
  common interfaces.
- Run final full regressions, independent ordered code/security review and
  CLI/browser/desktop QA. No review or QA completion is claimed here.

WBS remains **91/193 (47.15%)**. Runtime rate changes, reverse policies,
pitch-preserving DSP, retained browser/network play, physical timing and
foreign OS execution remain obligations of the complete player Goal. Prior
browser audio-WASM ERR_ABORTED and required desktop x11-MCP blockers remain
unresolved; these focused core tests do not clear them.

## Common cold preparation development evidence

The common speed-aware preparation, initial owner validation, fresh capture
and saved-opponent comparison implementation has been written. Immutable
controller getters validate the installed program's actual rate, preroll and
open-end intent before a speed-aware owner is published. P=0 cannot prove
program speed through the Started receipt alone. Frozen launch speed and
fresh replay speed must agree; reduced equivalent fractions are accepted,
conflicting or repeated speed flags are refused by the opted path.

The actual current app lib command passed **2429 tests**, with **0 failures
and 6 preexisting ignored tests**, compile 2 minutes 3 seconds and execution
6.14 seconds. Log: `target/wf/retained-speed-development-12e0c/common-lib-c1.log`.
The first external run, `common-speed-c1.log`, passed all 8 identity tests but
failed the 11 preparation tests before reaching production behavior: the
fixture declared an 8-byte PCM asset limit for eight f32 samples (32 bytes).
After fixing that fixture limit, `common-speed-c2.log` passed the 8 identity
and 10 preparation tests. Its one remaining failure used stale generation 1
after the actual finite Ended transition had advanced generation. The test
must consume and assert that real receipt before requesting the next attempt;
the production stale-generation guard remains intact. Fresh execution of
that correction passed all **19 tests (8 identity + 11 preparation)** in
`common-speed-c3.log`: compile 30.77 seconds, actual test execution
0.07 + 0.04 seconds. These development runs are not independent QA, nor
proof of native phase or pause integration.

An independently reviewed additive core phase port is now planned as AC010.
It retains normalized ClockPoint chronology and original audio scheduling,
while carrying relative active source elapsed and attempt/basis identity for
song mapping. Phase-enabled transports must reject scalar Runtime fallback;
elapsed regression cannot hide behind an equal rounded/capped song time.
The native qualification/projection/pause bridge still remains to implement.

## Additive core phase port development evidence

AC010 has additive Transport and Runtime phase APIs. They preserve normalized
ClockPoint chronology and raw audio scheduling, while using one elapsed floor
from relative active source position for song mapping. Ordinary scalar calls
refuse a phase-enabled transport before gameplay effects. The first actual
focused execution passed **26 tests (12 arithmetic/transport + 14 Runtime)**
in `phase-port-c1.log`, compile 21.01 seconds, execution 0.03 + 0.05 seconds.

Further static inspection found a historical-control gap: retaining only the
last acknowledged elapsed allowed contradictory queries between earlier
no-op controls. Independent regressions reproduced it: `phase-history-red-c1.log`
had **12 passing and 2 failing** tests. The source now preserves successful
control metadata, including no-ops, and validates historical lower/upper
elapsed bounds without adding or rerounding song anchors. A fresh 28-test
execution passed **28/28** in `phase-port-c2.log`, compile 15.35 seconds,
execution 0.03 + 0.05 seconds. Final broad regressions and independent
review/QA remain pending.

Native metadata inspection found original WASAPI counter/frequency and ASIO
render coordinates already retained. ALSA and CoreAudio have original frame
evidence before conversion into supplied ClockPair. The bridge must preserve
these original coordinates, actual converter segments and acknowledged silent
frames; a latest pull report or arbitrary ClockPoint inverse is insufficient.
Native phase admission, HOST interpolation and integration remain unverified.
Exact HOST interpolation may have a denominator wider than the u64
SourcePosition fraction even when its elapsed is representable. AC010 adds
an explicitly mathematical checked elapsed builder for that handoff;
the native caller must still prove the original relation and admission.
The elapsed-builder revision passed **31 focused tests (16 Transport/math +
15 Runtime)** in the actual `phase-port-c3.log` run, compile 16.92 seconds,
execution 0.04 + 0.04 seconds. The final broad core regression passed **597 tests across 67 targets**,
with zero failures or ignored tests, in `phase-core-full-c1.log` after this
revision (compile 3 minutes 6 seconds). This completes the AC010 development
gate; native admission, app wiring and independent review/QA remain pending.

The cold prepared-attempt owner now exposes `transport_at_output_phase`,
retaining its configured domain, original start/end, canonical selected rate
and zero fresh preroll through the existing core phase constructor. It checks
both domains without modifying judge, capture or pinned launch. Five added
identity tests cover the two actual converter fraction literals, a nonzero
44100Hz cut at four rates, finite capping and scalar refusal. A twelve-case
phase Runtime → original capture → unchanged codec → replay rejudge matrix
preserves literal song times, Hit/Miss events and stable hashes. The fresh
`common-phase-c1.log` execution passed **24 tests (13 identity + 11
preparation)** with zero failures/ignored tests, compile 1 minute 53 seconds
and execution 0.05 + 0.03 seconds. This is development evidence; native
admission/pause wiring and final app regression remain pending.

Converter exploration confirmed that exact active correspondence can merge
across ordinary blocks and target-rate changes, while held output freezes
converter source phase. AC011 plans an opted bounded transition history,
before first generation, instead of retaining every callback. ALSA currently
publishes generated telemetry before positive writes; generation does not
establish submission or presentation. An independent arithmetic/API planning
assessment precedes that implementation, including fractions wider than the
existing SourcePosition descriptor. No native timing completion is claimed.

The AC011 independent planning assessment selected an opaque exact generated
phase descriptor with `elapsed_nanos_since(integer_source_cut)`. An actual
1Hz source, active 3Hz / held 5Hz / active 3Hz sequence accepts a lookup whose
exact source fraction denominator exceeds u64, while its elapsed remains
466666666ns. The implementation must retain that answer instead of narrowing
the fraction. Four private u64 limbs suffice for the bounded final fractional
carry; integer terms remain i128. A separate implementation writer and test
author now own the three core files and one external test file.

Known ceiling: this initial history has fixed cold capacity and no automatic
eviction. Enough active/held transitions will refuse further generation until
native-qualified retirement is implemented. Native feature activation remains
gated on that follow-up; observation history capacity alone proves no safe
retirement bound. This prerequisite is not complete retained player support.

AC011 source and external tests are now frozen. The first focused run passed
**18 tests** in `converted-phase-c1.log` (compile 17.10 seconds, execution
0.03 seconds). The subsequent revision reuses the existing report source
position instead of calculating it twice. An additional independent rational
oracle exercises the upper limbs with three near-u32::MAX rates, five exact
u64::MAX-denominator coordinates and three source cuts. The fresh
`converted-phase-c2.log` passed **19 tests**, zero failures/ignored tests,
compile 13.50 seconds and execution 0.04 seconds. Capacity refusal preserves
PCM, queued commands, pull/cursor/counters and history; the scoped allocation
probe observed zero allocation, reallocation or destruction during opted
active/held callbacks and historical queries. Existing disabled-path PCM and
reports match. The broad core regression passed **616 tests across 68 targets**,
zero failures/ignored tests, in `converted-core-full-c1.log` (compile
3 minutes 12 seconds). Independent source review and QA are unrun.

AC012 now retains original native counter/frame coordinates in additive
private snapshot wrappers and separate static output traits. WASAPI compares
the original clock position/frequency and captured basis. ASIO derives the
stream frame from its unchanged final rendered-block/HOST-bracket observation.
ALSA uses the same validated played-frame estimate; CoreAudio subtracts the
creation offset from its actual integer first frame. Existing legacy snapshot
literals and status guards remain. ASIO may perform earlier guard reads; the
coordinate and evidence share the final retained observation, rather than
claiming exactly one total driver read. The wrappers do not authenticate
native acquisition or admit epochs into the gameplay authority.

The fresh `native-coordinate-c1.log` run passed **52 tests**: 28 coordinate,
13 retained identity and 11 preparation tests, zero failures/ignored tests.
Compile took 2 minutes 10 seconds; execution 0.04 + 0.05 + 0.03 seconds.
Windows/macOS all-targets Rust type checking with webtransport both exited0:
Windows 2 minutes 8 seconds, macOS 1 minute 42 seconds. Logs:
`native-coordinate-windows-c1.log`, `native-coordinate-macos-c1.log`. Their
C/C++ stubs provide type evidence only; this is not driver execution.

AC013 exposes fixed eight-word generated-span transfer with structural decode
and exact range checks, sharing the existing elapsed implementation. Empty
history provides an initial coordinate, without a fabricated generated span.
The target basis now supplies exact native-counter duration before its final
timestamp floor. `phase-transfer-c1.log` passed **34 tests (19 history + 15
transfer)**, zero failures/ignored tests, compile 15.43 seconds and execution
0.04 + 0.04 seconds. Actual submitted-prefix publication, qualified retirement,
HOST interpolation and phase-aware authority/pump integration remain pending.

AC014 now forwards cold history setup/read access through the complete
converted native state and retains `admitted_target_time()`. Only a successful
positive `admit` updates that exact target-time endpoint; invalid admission,
fresh generation and reconfiguration do not fabricate progress. Complete state
moves preserve it, while wrapping a generated converter starts with no
submission evidence. This getter is software admission evidence, without
native epoch or presentation authentication.

`submitted-frontier-c1.log` passed **16 tests**, including seven new regression
tests, zero failures/ignored tests; compile 18.12 seconds, execution 0.04 seconds.
`submitted-frontier-platform-full-c1.log` passed **138 platform unit tests**,
zero failures and one existing ignored ALSA null-plugin diagnostic; compile
3.13 seconds, execution 0.31 seconds. Fresh current core all-targets regression
`phase-transfer-core-full-c1.log` passed **616 tests**, zero failures/ignored
tests; compile 3 minutes 2 seconds. Its 85 result lines include empty example
targets; the actual transfer target ran all 15 tests.
The normally ignored ALSA null-plugin diagnostic was then explicitly executed:
`submitted-frontier-null-c1.log` passed one test, zero failures/ignored tests,
compile 3.56 seconds and execution 0.03 seconds. This exercises the installed
native plugin lifecycle, without proving acoustic-device timing.

Native atomic publication immediately after positive writes is still pending,
along with HOST interpolation, pause-aware mapping, authority admission and
retirement. Required independent final review and browser/desktop/CLI QA have
not run for this unfinished integrated feature. WBS remains 91/193.

AC015 connects opted generated/submitted publication to the actual converted
ALSA pump. Cold slots match the complete owner's history capacity and are
allocated before native acquisition; cold recovery seeds generated history
but resets current-stream submission proof. One existing generation protects
atomic descriptor slots, exact bounds and associated output facts. Ordinary
callbacks append at most one transition descriptor. Actual positive admission
publishes before fallible timing/clock acquisition. The new fixed snapshot
lookup uses at most three attempts and accepts only coordinates within the
current stream's positive submitted range. It authenticates no native epoch
or acoustic presentation.

Fifteen new tests exercise actual pump failures after positive writes,
nonpositive/refused writes, allocation refusal, exact Held/retarget fractions,
capacity preservation, malformed publication and concurrent coherence.
Development inspection found inherited associated-report structural decoding
too permissive for opted queries; opted-only validation and regression tests
now reject bad rates/state/fractions/ranges while preserving legacy decoding.
`publish_generated(None, ...)` is tested after genuinely advanced generation
and preserves the prior report/facts/submitted range. An actual production
pump error after generation commit has not been reproduced; this helper test
does not prove that failure path.

Final changed-source `atomic-phase-platform-full-c2.log` passed **154 tests**,
zero failures/ignored tests, including the installed ALSA null diagnostic.
Earlier c1 checks passed ALSA67 and platform151 with the same diagnostic
normally ignored, then explicit null1 PASS. c1 predates the final two tests
and opted report guards; c2 is the current evidence. These are development
checks, not final independent review/QA. HOST interpolation, actual pause
mapping, authority admission, qualified retirement and native gameplay
integration remain unfinished; WBS stays 91/193.

AC016 adds exact signed HOST interpolation without narrowing the physical
coordinate to nanoseconds before selecting the generated phase. The opaque
coordinate preserves fractional terms, checks representability, and handles
negative offsets and actual converter retarget spans. It grants no native
epoch, extrapolation, submission or presentation authority. Focused
`signed-host-phase-c1.log` passed 52 tests (18 new interpolation tests plus
34 existing history/transfer tests). `signed-host-core-full-c1.log` passed
634 core tests with zero failures or ignored tests. These are development
checks; allocation-free arithmetic does not establish its CPU cost.

Replay inspection confirmed that the recorder stores the literal song time
used by Runtime judgment and replay applies that value directly. No codec
migration is needed to preserve this precision; native admission remains a
separate unfinished obligation.

AC017 connects exact interpolated queries to the existing coherent ALSA phase
reader. Its first scoped run passed 79 tests, failed one, and ignored the
existing null-plugin diagnostic. The failure occurred before exercising the
lookup: the test requested a sample rate rejected by `DeviceFormat`.
The corrected test uses supported 44,100 → 48,000 → 32,000 Hz formats while
preserving large signed HOST denominators. Independent Python Fraction
arithmetic confirmed its expected elapsed values. Fresh
`interpolated-publication-platform-full-c2.log` passed **165 tests**, zero
failures or ignored tests, including the installed ALSA null-plugin diagnostic;
compile 15.02 seconds, execution 0.43 seconds. Eleven new tests cover exact
submitted ceilings, fractional transition selection, signed retarget queries,
malformed publication, bounded retries and concurrent coherence. Bounded
development inspection found no concrete shared-reader defect. These are
development checks, not final task review or QA. Native pause mapping,
authority admission, qualified retirement and gameplay integration remain
unfinished. WBS remains 91/193; changes are uncommitted.

AC018 adds caller-qualified Running/Frozen source intervals and exact physical
source boundary comparison before rounding. The running calculation removes
the interval's integer physical/playback gap; the frozen calculation keeps its
declared playback frame. An effective physical cut larger than u64 is retained
as i128 until relative elapsed calculation. Existing elapsed APIs share the
same private fractional arithmetic. Fifteen independent tests are authored;
the initial build found missing public API documentation, which was corrected
without changing behavior or suppressing the lint. The fresh
`source-playback-phase-c2.log` passed all **67 tests**: 15 new playback mapping,
18 signed interpolation, 19 converter history and 15 descriptor transfer.
Compile took 17.24 seconds; test execution took 0.69 seconds in total. Bounded
independent development inspection found no concrete mathematical or guard
defect. Fresh `source-playback-core-full-c1.log` passed **649 core tests**,
zero failures or ignored tests; build took 2 minutes 16 seconds. Its 87 result
lines include empty example targets. Final task review and QA remain pending.

## Known ceiling

The source interval descriptor is mathematical data only. Actual Mixer source
adoption history, coherent publication of that history, native admission and
pause acknowledgments are not implemented by AC018. A converter Held segment
does not establish a source pause or its native acknowledgment. Lookup and
mapping CPU performance remain unmeasured; no zero-cost performance claim is
made from allocation-free arithmetic alone.

## Actual source playback history

AC019 adds cold optional source playback history to the complete converter
owner. Mixer uses the same sampled pause/start-gate values to preflight exact
interval capacity before mutating output, queues, practice state or cursors.
Actual nonempty source pulls commit startup Frozen, Running and finite Frozen
tail intervals; identical continuous mappings merge. Empty source callbacks
and target-only Held rendering create no source adoption. Exact historical
lookup accepts the integer actual pulled endpoint and rejects fractional
future positions. Retarget and complete-owner moves preserve the history.

Initial `source-playback-history-c1.log` passed **45 tests** (12 new history,
15 playback mapping and 18 interpolation). Compile took 17.47 seconds; test
execution took 0.73 seconds. `source-playback-history-core-full-c1.log` passed
**661 core tests**, zero failures or ignored tests, compile 2 minutes 47
seconds; 88 result lines include empty example targets. Bounded development
inspection found no concrete implementation defect. Two additional coverage
cases now pass: full-capacity cached output with an empty source callback,
and actual retained practice request/receipt preservation on refusal followed
by successful processing of the original request in the same owner's retry.
Fresh `source-playback-history-c2.log` passes all **14 history tests**, zero
failures or ignored tests, compile 12.63 seconds, execution 0.03 seconds.
Final `source-playback-history-core-full-c2.log` passes **663 core tests**, zero
failures or ignored tests, 88 result lines. Its cached build took 4.63 seconds;
the previous 661-test run predates the two supplemental tests. Native publication,
authority and ACK integration and qualified retirement remain unfinished.

AC020 publishes actual source history and converter/submitted facts under the
same existing ALSA generation. Cold optional three-word source slots append at
most three transitions per callback; merged intervals update only the source
frontier. A compound reader selects source playback before the same generation
recheck, preserving the three-attempt bound and existing converter-only APIs.
Source-only changes also prevent the error-path early return. The lag fixture
uses genuine earlier source facts in a synthetic lagged publication; it does
not reproduce a production post-generation error.

Seventeen new tests include actual concurrent pause/resume transitions,
historical boundary selection, source corruption, recovery and positive writes
followed by timing failure. The alternate zero fraction encoding test uses
equivalent structural `0/7` encoding of the actual converter's normalized
`0/1`, without claiming the converter emitted that encoding. Final
`source-playback-publication-platform-full-c2.log` passes **182 tests**, zero
failures or ignored tests, including the installed ALSA null diagnostic.
Compile 15.48 seconds, execution 0.39 seconds. Earlier scoped95/one ignored
and platform180 PASS precede the two supplements. Bounded development
inspection found no implementation defect; its two coverage gaps are now
tested. This is development evidence, not final task review or QA.

Cross-platform inspection confirms unconverted WASAPI/CoreAudio/ASIO own
bare Mixer instances. The current conversion-owner source history does not
cover those paths. The next required common step is Mixer ownership and direct
source-duration mapping, followed by publication at each actual native delivery
boundary. No synthetic converter, epoch authentication, pause ACK, qualified
retirement or cross-platform execution proof is supplied by AC020.

AC021 moves optional source history into the common Mixer. Ordinary rendering
records it automatically with the existing exact preflight; ConvertedMixer
delegates and retains its positive-target late-enable guard. No callback moves
or takes the history Vec. Cold enabling at a nonzero cursor starts a new
retained range without reconstructing previous output. Eleven new tests and
the fourteen existing conversion-history tests pass in
`common-mixer-history-c1.log`: **25 passed**, zero failures or ignored tests,
compile 23.53 seconds, execution 0.08 seconds.

AC022 adds direct physical source duration lookup and playback elapsed without
synthetic converter descriptors. Native ahead-limit comparison preserves six
fraction terms and compares before rounding; coarse guards prevent signed
limit subtraction overflow. Fifteen new tests include same-floor native
progress and limit crossings, huge denominators, signed limits and direct
history boundaries. `direct-source-time-c1.log` passes **48 tests** (15 new,
18 interpolation and 15 playback mapping), zero failures or ignored tests,
compile 15.94 seconds, execution 0.90 seconds. Bounded source inspection found
no concrete defect. Its test inspection identified missing maximum six-term
denominator coverage, now fixed by one supplemental test with independently
calculated Less/Greater expectations and identical timestamp projections.
All six proper fractions are nonzero and their denominator product reaches
414 bits. AC022 now has **16 new tests**; AC021 has 11.

Final `common-source-core-full-c2.log` passes **690 core tests**, zero failures
or ignored tests, with 90 result lines including empty examples; cached build
11.32 seconds. Earlier `common-source-core-full-c1.log` passed 689 before that
supplement, with build 3 minutes 35 seconds. Unchanged platform source tested
against the new common Mixer in `common-source-platform-full-c1.log` passes
**182 tests**, zero failures or ignored tests, including the installed ALSA
null diagnostic; build 22.38 seconds, execution 0.40 seconds. These checks do
not establish native publication or authority integration; final task review
and QA remain pending. CPU costs and foreign native execution remain unproven.

AC023 extracts the existing bounded atomic source history into a crate-private
payload reusable by native output adapters. Converted ALSA encloses all payload
publication and selection in its existing revision; no separate clock authority
or coherence protocol is added. Eleven direct payload tests cover actual Mixer
transitions, exact boundaries, fractional future refusal, allocation failure,
corruption and permanent invalidation. Final scoped checks pass 11 payload
tests and 97 ALSA tests (one existing ignored diagnostic). The full platform
run includes that installed ALSA null diagnostic and passes **193 tests**, zero
failures or ignored tests, in `shared-source-platform-full-c2.log`. This remains
development verification; native adapter binding and final task QA are pending.

Known ceiling: the common payload alone provides no coherent snapshot. Its
caller must read inside the existing revision and recheck that revision before
accepting the facts; source history grants no native delivery or pause ACK.

AC024 adds opt-in source history to common scalar telemetry under its existing
checked revision. An exact bounded read returns scalar facts and the selected
source interval from that revision. History-only writes preserve the previous
report and independent status; missing history permanently closes only the
opted source query. Legacy scalar decoding remains unchanged, including clock
metadata. Source queries validate render flags and checked report extents,
without treating software prepared counts as native delivery evidence.
Thirteen new tests cover real Mixer history, exact boundaries, concurrent
publication, allocation refusal, corruption, permanent invalidation, three
bounded retry attempts and repeated frozen reports whose finite endpoint
precedes the current physical block. `common-playback-telemetry-c1.log` passes
**22 telemetry tests**. `common-playback-telemetry-platform-full-c1.log` passes
**206 platform tests**, zero failures or ignored tests, including actual ALSA
null output. The core is unchanged from its previous 690-test check. Native
adapter integration and final independent task review/QA remain pending.

Known ceiling: history-only publication tests verify the common helper contract;
they do not reproduce an actual native post-render failure or prove delivery.

AC025 binds optional history to every WASAPI worker publication and exposes
an additive source snapshot query. Cold allocation preserves the original
Mixer on refusal. Existing ReleaseBuffer success/report/counter updates remain
before the later clock read; SILENT fallback remains outside successful music
submission. The common public SourcePlaybackSnapshot preserves the existing
private representation and requires separate native admission qualification.

AC026 binds borrowed ASIO renderer history and copied callback events under
the existing outer revision. Preparation remains a software count, owner
priming supplies no callback event, and exhausted publication is permanently
unavailable before rendering mutates the owner. AC027 extends CoreAudio's
existing checked outer revision across actual rendering, source publication,
all output copies and native presentation. Failed rendering retains earlier
delivery facts; successful delivery end is a range bound requiring failure,
epoch and freshness qualification, not an audible prefix certificate.

Nine new SDK-free renderer tests pass in `native-source-asio-portable-c3.log`
(build 12.92 seconds, run 0.03 seconds). Full Linux platform library checks in
`native-source-platform-full-c3.log` pass **206 tests**, zero failures or
ignored tests, including installed ALSA null output (build 23.18 seconds,
run 0.47 seconds). Windows Rust/stub-C all-target checking passes in
`native-source-windows-c3.log` (41.78 seconds); macOS checking passes in
`native-source-macos-c4.log` (26.93 seconds). ASIO-gated Rust and its Windows
fixtures pass a separate metadata-only diagnostic check in
`native-source-asio-rust-only-c4.log` (61 seconds). That diagnostic enables
the Rust module cfg only, without enabling the Cargo SDK build; it does not
build C++, approve the GNU IASIO ABI, link or execute an ASIO build.

Thirty native-only tests are prepared and typechecked: eight WASAPI, six ASIO
and sixteen CoreAudio. A stale ASIO test expecting a nonfinite error from
valid large finite samples was corrected to verify actual Mixer clipping;
no unreachable post-PCM error was fabricated. The new tests' mutable-binding,
PCM byte-capacity and helper-shadowing mistakes were corrected before the
final checks. An inspection claim about CoreAudio's final timestamp oracle
was rejected after checking the separate loop index and attempt counter:
actual ticks 2, 3, 4 correctly require the existing expectation 4.

This is development verification. Native-only tests have not run on Windows
or macOS, actual device output is unverified, and final required review/QA is
pending. Ordinary ALSA compound binding and common app authority integration
are the next dependencies; the whole retained feature and WBS remain open.

### Known ceiling

Known ceiling: Windows actual execution and device validation are absent;
the new WASAPI error fixtures simulate scalar publications and do not invoke
actual COM ReleaseBuffer faults.

Known ceiling: native epoch and audible progress authentication still require
the subsequent app admission integration. ASIO SDK/MSVC ABI and C++ bridge
build are unverified; Rust-only metadata checking is preparation evidence.

Known ceiling: CoreAudio delivery end does not prove a continuous audible
region; admission must qualify callback failures, native epoch and freshness.
Post-render/copy fault fixtures are explicitly helper simulations where the
validated production path cannot reproduce that failure.

## AC028 ordinary ALSA binding

Ordinary ALSA now joins generated source history, this stream's positive
submission prefix and optional raw native timing under its existing timing
revision. Cold preparation captures the first unsent frame, so reopening a
retained pending tail does not claim submission before a new positive write.
The write guard finishes before the bounded device wait and is reacquired
before a subsequent native query. Waiting and errors retain real source and
submission facts without supplying a new clock observation. Public queries
retain those facts after stop or failure and remove timing outside Running.
Publication exhaustion permanently closes proof without stopping audio.

Nineteen new actual-pump tests pass in `alsa-source-focused-c1.log` (build
44.26 seconds, run 0.09 seconds). Full platform library verification passes
**225 tests**, zero failures or ignored tests, in
`alsa-source-platform-full-c1.log` (build 8.16 seconds, run 0.91 seconds),
including installed ALSA null output and existing Converted regressions.
Focused diff whitespace checking passes. Independent bounded development
inspection reports no concrete defect; it is not formal review or QA.

Known ceiling: scripted pump tests and ALSA null execution do not establish
physical device latency or audible delivery. The public stop/failure timing
filter was inspected; most new fixtures exercise the actual pump and private
compound reader. Native epoch, freshness, acquired-prefix and pause ACK
qualification still depend on the upcoming common app authority integration.
Core remains at the previously verified 690 tests without source changes or
an unnecessary repeat. WBS remains 91/193; final review and required ordered
CLI/browser/desktop QA remain pending.

## AC029 original coordinate authority metadata

The existing bounded audio authority queue now retains each original pair
with optional native units. Cold opt-in binds an immutable source frame,
source counter/frequency or target frame basis. Native-unit progress survives
identical rounded nanoseconds when HOST progresses; identical units retain
the original pair, revision and age. Mismatched domains, units, frequency,
pair conversion, regression, capacity and revision exhaustion refuse before
mutation. Scalar owners retain their original behavior. Opted scalar mapping
and descriptor-less epoch/priming operations remain unavailable; same-basis
restart retains the descriptor and gameplay watermarks.

Fifteen new coordinate fixtures pass in `authority-coordinate-focused-c1.log`
(build 186 seconds, run 0.10 seconds). The authority regression filter passes
**123 tests**, no failures or ignored tests, in
`authority-coordinate-regression-c2.log` (build 96 seconds, run 0.55 seconds).
Two calibrated allocation tests pass in `authority-coordinate-alloc-c2.log`
(build 148 seconds, run 0.01 seconds): actual hot admission/read/refusal
requests no heap allocation; failed cold reservation recovers cleanly.
Full app library testing with `--no-default-features` passes **1989 tests**,
zero failures and four ignored, in `authority-coordinate-app-full-c2.log`
(build 14.22 seconds, run 7.69 seconds).

The three ignored ALSA adapter diagnostics were then explicitly executed
with the installed null device. Initial `authority-coordinate-alsa-native-c2.log`
records two passes and one failure: a fixture demanded a bounded live report
read after independently checking submission, while null output continuously
published new revisions. The fixture now joins the real output worker before
its unchanged successful-report assertion and owner recovery. No production
behavior or assertion was weakened. `authority-coordinate-alsa-native-c3.log`
passes all three diagnostics (build 70 seconds, run 0.14 seconds). The fourth
ignored test needs QUIC development certificate/key environment, which is not
configured. Existing no-feature dead-code warnings remain; the new unused
mutable test binding was removed. Scoped diff checking passes. Bounded
independent source inspection finds no concrete defect, not formal review.

Known ceiling: this is metadata storage and consistency validation, not
generated/submitted/native presentation qualification or gameplay commitment.
The high-frequency cases are arithmetic fixtures. Existing watermark
preservation includes a labeled synthetic inherited-state fixture; real
prefix/restart and pending-input refusal are separately exercised. WASAPI
already recognizes progressing raw counters with flat rounded timestamps;
SuppliedPair admission still needs its later coordinate-aware seam. Source
history retirement, phase-qualified inputs and native pause ACK remain open.
WBS stays 91/193; full retained playback and ordered final review/QA are open.

Next: AC030 prepares acquired HOST input from the existing raw-coordinate
queue using exact interpolation and a frozen Unknown-quality mapper. It
will preserve the current acquisition, freshness and extrapolation policy,
including already covered input with a future right latency association.
Exact max-ahead comparison precedes flooring; the mathematical token grants
no Runtime operation until subsequent native source/phase qualification.

## AC030 exact acquired input preparation

Coordinate input preparation now interpolates original native units on the
existing retained HOST interval, then floors the raw coordinate once on its
original basis. Normalized chronology is separate from active playback
elapsed. Exact prediction limits reject fractional excess before rounding;
the frozen Unknown-quality mapper cannot bypass those bounds. Validation
rechecks the complete authority state, current acquisition and freshness
without consuming input. Existing scalar APIs remain unchanged and cannot
operate an opted coordinate authority. The input's historical physical point
does not replace the real scheduling audio_at used for keysounds.

Eighteen new private fixtures and two new external allocation fixtures were
authored. Initial focused checking passes 17 and fails one in
`authority-coordinate-input-focused-c1.log` (build 120 seconds, run 0.10
seconds): an overflow fixture tried to retain an endpoint whose normalized
time was already unrepresentable. Existing observation guards correctly
refused it. Both private and external overflow cases now use representable
endpoints at logical origin MAX-2 and allowed exact extrapolation past the
latest logical MAX. The preparation and mapper then reach their actual
overflow path; production checks were not weakened.

Final `authority-coordinate-input-regression-c2.log` passes **141 tests**,
zero failures or ignored tests (build 88 seconds, run 0.42 seconds), including
all 18 new cases and old native/browser/group authority regressions.
`authority-coordinate-input-alloc-c2.log` passes all **four** calibrated
allocation tests (build 103 seconds, run 0.01 seconds), preserving the prior
two and verifying actual hot preparation/query/map/validation/refusal makes
no allocation request. `authority-coordinate-input-app-full-c2.log` passes
**2007 tests**, zero failures and four ignored (build 15.12 seconds, run 8.11
seconds). The ignored native/null and TLS diagnostics are the same four as
AC029; its three ALSA null diagnostics were separately executed successfully.
No native adapter or output-loop semantics changed in AC030, so they were not
repeated. Scoped diff checking passes. Bounded independent source inspection
finds no concrete defect; this is not formal review or QA.

Known ceiling: these tokens provide mathematical preparation only. Native
generation/submission qualification, phase-aware Runtime commitment,
presentation advancement, pause ACK, rebind and safe history retirement
remain open. Heap tests make no CPU-cost or physical accuracy claim. Core
690/platform 225 evidence is unchanged without unnecessary repeats. WBS
remains 91/193 (47.15%); full product and task close remain unproven.

Ready next work is frame-aware native validation, an actual ordinary ALSA
playback-query port and shared group phase APIs. Those independent source
lanes can run in parallel before the common actual solo/cohort integration.
The ordinary retained Mixer already accepts selected speed, so a converter
is not required for the first end-to-end route. Converted playback publication
currently lacks same-revision native timing; that must be bound explicitly
before claiming converted native coherence. Formal review/security followed
by required CLI/browser/desktop QA still precedes close.

## Original frame validator development evidence (AC-031)

Native validation now retains the original source/target frame basis and frame
position alongside its existing single accepted record. Frame progress can
share an integer nanosecond; a duplicate frame does not refresh HOST age.
Scalar/frame modes and bases cannot switch within an epoch, and commit
reconstructs the original frame token before changing state.

`frame-validator-focused-c1.log` records **13 passed, zero failed/ignored**
(build 26.07s, execution 0.03s). The subsequent actual platform library run
with `--include-ignored` records **238 passed, zero failed/ignored** in
`frame-validator-platform-full-c1.log` (execution 1.16s). Both logs live under
`target/wf/retained-speed-development-12e0c/`.

Known ceiling: this is metadata and transaction regression evidence, not
hardware acquisition, final independent review or required browser/desktop QA.
The full retained-speed task and player Goal remain open.

## Portable playback packet and shared phase development evidence (AC-032/033)

The portable output port exposes checked exact source queries and preserves
the adapter's original native evidence without imposing Copy or Clone on it.
Ordinary ALSA uses its actual compound source-history publication and accepts
a new observation only when the complete original timing packet matches.
Source-only facts after a wait or stop do not manufacture a presentation clock.
The public Linux export now includes `AlsaSourcePlaybackSnapshot`.

`source-playback-app-focused-c3.log` records **17 passed, zero failed, one
explicit native diagnostic ignored** (build 2m25s, execution 0.08s).
The first actual app compile exposed an absent Linux reexport, a missing chart
type import and two unsupported exact-query equality assertions. The fixes
preserve the arithmetic contract; query assertions use exact frame comparison.

Group and Solo phase APIs reuse the existing transport, audio producer and
Runtime. Common phase checks precede lifecycle changes; the supplied raw
keysound scheduling point remains unchanged. `group-phase-focused-c1.log`
records **9 passed, one failed**: its intended unwind occurred during eager
judge construction. The fixture now creates the real interaction normally and
panics in its routed input callback inside OwnerGuard. All owner restoration,
poison and original-queue cleanup assertions remain. The actual corrected
`group-phase-focused-c2.log` records **10 passed, zero failed/ignored**
(build 1m37s, execution 0.12s). Logs use the same development directory.

The earlier nonzero source-counter fixture now uses 2/3s + 1/3s: its correct
1s result differs by 1ns from independently floored offsets. This strengthens
the literal regression oracle without changing production time arithmetic.
Bounded read-only inspection found no concrete packet/group defect; it is not
formal review or QA. Aggregate app and actual ALSA diagnostic evidence follow
separately when their running commands finish.

Known ceiling: common native-owner coordinate admission and actual phase-aware
input/frontier integration remain open. Checked mathematical packets do not
authenticate hardware. Whole task completion still requires independent formal
review/security followed by the required CLI, browser and desktop QA.

Next integration must use the existing NativeAudioPresentation owner, check
coordinate mode/basis/frequency before duplicate or coarse-HOST deferrals, and
require actual authority retention before committing validator progress.
Legacy scalar admission must refuse coordinate mode even when it would add no
pair. SuppliedPair frames use the new frame validator; original WASAPI/ASIO
metadata keeps its existing validation. ASIO's existing rounded-source
progress check still needs separate treatment for tiny blocks above 1GHz;
adding a common frame validator does not change that ASIO path implicitly.

The final actual app library run, after all fixture corrections, records
**2,034 passed, zero failed, five ignored** in `phase-packet-app-full-c2.log`
(build 12.98s, execution 6.89s). The explicit installed ALSA null run records
**four passed, zero failed/ignored** in `phase-packet-alsa-native-c2.log`
(build 1m37s, execution 0.14s). Its earlier c1 run had three passes and one
failure collecting a bounded packet during continuous null publication.
The corrected fixture waits for actual positive submitted frames, performs
real stop/join, and requires the stable compound packet and recovered source
history to agree. The positive-prefix and timing-absence assertions are
stronger; production retry limits remain unchanged. This proves software
lifecycle and retained facts, not physical audio latency or audible output.

AC-031/032/033 are development-verified; the next declared AC-034 integrates
original coordinate admission into the existing native owner. WBS remains
**91/193 (47.15%)**, since the complete native input/control feature and required
formal review/QA have not yet completed. The player Goal remains active.

## Existing native owner coordinate admission (AC-034)

`NativeAudioPresentation::admit_coordinate` now uses the same native validator
and audio authority. Descriptor kind, basis, frequency and creation epoch are
checked before duplicate/deferred observations. The authority must retain
progress before the native record and pinned basis change. Legacy scalar
entries explicitly refuse coordinate mode, including no-op candidates.

The committed validator frame sidecar is exposed by a read-only
`latest_frame_position()` getter. The presentation owner compares that original
frame with its authority history, so independently restarting/repopulating the
authority with an equal rounded pair cannot hide a different sub-nanosecond
coordinate. No duplicate coordinate field, clock, history queue or revision was
added. WASAPI and ASIO retain their original evidence validation.

Actual development execution:

- `native-owner-platform-c1.log`: **238 passed, zero failed/ignored**;
  build 25.40s, execution 0.65s.
- `native-owner-focused-c1.log`: compiler refusal for a private test import;
  corrected to the existing public ObservationAdmission enum.
- `native-owner-focused-c2.log`: **15 passed, two failed**; both used native
  counter zero, correctly refused by the original pre-presentation guard before
  reaching the owner. Fixture counters became positive while preserving the
  duplicate/frequency/state assertions; production guards stayed unchanged.
- `native-owner-focused-c3.log`: **17 passed, zero failed/ignored**;
  build 1m47s, execution 0.12s.
- `native-owner-app-full-c1.log`: **2,051 passed, zero failed, five ignored**;
  build 15.11s, execution 8.83s.

All logs are under `target/wf/retained-speed-development-12e0c/`. Scoped diff
checking passes. Bounded independent source inspection found no concrete
defect; it does not constitute formal review or QA. Native adapters were not
changed, so the preceding four explicit ALSA null diagnostics remain the
available software lifecycle evidence without a redundant rerun.

Known ceiling: this connects coordinate admission, not the actual native pump's
phase input/frontier/controls. The existing ASIO high-Hz rounded-source
restriction remains. Original-coordinate operation/presentation comparison and
commit/frontier APIs are required next: a fractional cut can turn two equal
rounded raw points into different elapsed values. The declared next core
comparison preserves full fractional order and uses a separate bounded wide
comparison while leaving existing seven-limb paths unchanged. Actual hardware,
foreign OS execution and formal review then CLI/browser/desktop QA remain
unproved. WBS stays 91/193; the full player Goal remains active.

## Exact query ordering prerequisite (AC-035)

`InterpolatedTargetTime::compare_interpolated_time` compares the actual signed
whole and six proper fractional terms. The new query-to-query comparison uses
nine fixed limbs for products up to 512 bits and bounded carry; existing
conversion, elapsed and ahead-limit paths retain seven limbs. It allocates no
memory and grants no native presentation authority.

`query-order-existing-c1.log` records **16 + 18 passed**, zero failed/ignored,
for the existing source-time and phase targets (build 25.56s, execution
0.05s + 0.40s). `query-order-focused-c1.log` records **12 passed**, zero
failed/ignored (build 11.79s, execution 0.21s). It checks independent rational
cross products, six wide denominators, accepted signed whole limits,
mathematical equivalence, reversal/transitivity and calibrated zero
alloc/realloc/dealloc in actual hot comparisons.

The actual `cargo test -p beatkernel --locked --no-fail-fast` execution records
**717 passed across 75 targets**, zero failed/ignored, in
`query-order-core-full-c1.log` (build 6m06s). That command includes doctests but
does not execute the example targets. `query-order-core-examples-c1.log`
separately records **17 example targets, one passed**, zero failed/ignored,
(build 55.25s). This explains why its count differs from the previous
690-test all-targets baseline, which included examples and excluded doctests.
All logs use the same development directory. Bounded independent source/test
inspection found no concrete defect; this is not formal review or QA.

Known ceiling: actual input/frontier commitment still requires same-authority
exact operation/presentation metadata and exact pre-origin classification.
AC-036 now owns that dependent application work. Affected app regressions run
once after its authors freeze, avoiding an app rebuild before that integration
uses the new primitive. Native pump controls/epoch/converted integration,
hardware and required final review/QA remain open. No WBS item was promoted.

## Exact coordinate commits and observed frontier (AC-036)

The existing audio authority now retains exact operation and presentation
watermarks alongside its compatibility ClockPoints. Input preparation classifies
pre-origin time before rounding, and input commits revalidate the exact query,
acquisition, freshness and semantic state. The observed frontier selects an
actual retained native observation covered by the drained input prefix; it never
uses HOST now as extrapolated presentation. Actual presentation may trail an
earlier predicted input without regressing the committed operation.

Bounded independent inspection found a semantic snapshot defect: distinct
sub-nanosecond prior watermarks could share rounded state and revision after
correlation restart. Both exact watermarks now belong to PreparationState, with
mathematical equality rather than representation equality. New cross-owner
regressions cover rejection without input consumption or prefix closure, and
acceptance of mathematically equivalent encodings. This inspection is not formal
review or QA.

Actual development execution under the same log directory:

- `coordinate-commit-focused-c1.log`: 25 passed, zero failed/ignored;
  build 2m13s, execution 0.12s.
- `coordinate-commit-allocations-c1.log`: two passed, zero failed/ignored;
  build 1m48s, execution 0.01s. Calibrated allocation, reallocation and
  deallocation counters observe zero requests in the measured hot paths.
- `coordinate-commit-app-full-c1.log`: 2,075 passed, one failed, five ignored.
  The existing synthetic restriction fixture assigned only the compatibility
  operation ClockPoint. It now also assigns the matching exact query, preserving
  the regression assertion and production guards.
- `coordinate-commit-app-full-c2.log`: 2,076 passed, zero failed, five ignored;
  build 1m09s, execution 5.34s. Includes the authority, native presentation,
  solo/cohort and source packet regressions after the core comparison change.

Actual Runtime/native pump wiring, controls, foreign hardware and final task
review/QA remain open. WBS remains 91/193; the full Goal stays active.

## Actual source output owner connection (AC-037)

The existing GameplayOutputOwner now acquires one compound source playback
observation through its backend, checks the published epoch/basis and requested
source history identity, and admits the original coordinate through the existing
NativeAudioPresentation. Its report cache uses that packet's render report;
there is no separate report acquisition. Exact source queries remain read-only
and check the returned query mathematically. RemixedOutputBackend forwards the
original coordinate and source packet with the unchanged associated evidence
type, without Clone/Copy bounds or dynamic dispatch.

Bounded inspection found a missing post-acquisition identity check on readonly
queries. The owner now checks actual output epoch/basis after a Some packet as
well as before acquisition, because an immutable trait receiver does not forbid
interior mutation. Independent bounded rereading found no further source defect;
it is not formal review or native pump completion.

Execution in the existing development log directory:

- `source-owner-focused-c1.log`: 28 passed, zero failed, one ignored;
  build 1m56s, execution 0.15s. Includes 11 new owner/remix tests and the
  existing packet/ALSA helper tests; the explicit native diagnostic is ignored
  by this command.
- After that pass, one independent same-basis/different-history-identity
  regression was added for both query and observation with already committed
  report/history. The final fixture has ten owner and two remix tests. It uses
  actual Mixer rendering before basis capture to make both identities valid
  mathematically; only the requested identity is accepted by the owner.
- `source-owner-app-full-c1.log`: 2,088 passed, zero failed, five ignored;
  build 1m17s, execution 5.36s. Includes all twelve final new tests.
- `source-owner-allocations-c1.log`: two passed, zero failed/ignored;
  build 1m09s, execution 0.01s. Existing coordinate-commit allocation regression
  remains green; it does not measure the new output owner or hardware latency.

No native backend IO implementation changed in this step. Software null device
evidence remains the preceding explicit diagnostics; these DI owner tests do
not prove physical output or foreign OS execution. WBS stays 91/193.

Read-only launcher inspection confirms remaining integration dependencies:
selected speed currently bypasses retained practice, startup uses scalar
authority and rounded inverse associations, converted output is not yet opted
into phase history, and fresh practice attempts construct normal-rate scalar
transports. Removing the selected-speed exclusion alone would preserve a wrong
timeline. Device phase queries, exact-coordinate startup, and acknowledged
fresh-attempt phase replacement must precede removal of that exclusion.

## Portable playback mapping and device connection (AC-038)

NativePlaybackMapping transfers portable source or converted facts while native
evidence remains with the existing generic output adapter. It verifies epoch,
creation descriptor, requested history identity and exact query, then computes
active elapsed through the original source playback interval and conversion
correspondence. Normalized chronology is checked separately from elapsed; raw
nanosecond differences do not replace the exact source cut. Converted segments
use a half-open transition boundary while the positively submitted latest
generated endpoint can remain inclusive.

GameplayDevice and NativeGameplayDevice expose an additive readonly query hook
with explicit unsupported defaults and static bridge forwarding. Actual Linux
solo/cohort source hooks use the existing output owner. Converted device queries
remain unsupported until their actual compound target port is coherent. This
does not activate selected-speed retained playback before startup and control
integration. Bounded independent source inspection found no concrete defect and
is not formal review or QA.

`playback-mapping-app-full-c2.log` records **2,104 passed, zero failed, five
ignored**, including all ten new mapping/device fixtures and six ASIO app
fixtures (build 59.17s, execution 4.89s). `playback-mapping-allocations-c1.log`
records two passed, zero failed/ignored (build 1m16s, execution 0.02s), with
calibrated zero alloc/realloc/dealloc requests in source and converted hot
mapping/phase/refusal probes. That external target also compiles the actual
Linux binary hooks; it does not execute native playback.

`playback-mapping-windows-typecheck-c1.log` and
`playback-mapping-macos-typecheck-c1.log` both complete successfully for the app
with `--all-targets --no-default-features` (1m21s and 1m17s). The existing xcheck
script uses stub C compilers: this proves Rust target typing only, not foreign
linking, ABI, SDK support or OS execution. Existing target-specific warnings
remain; no warning was emitted for the new mapping module.

## ASIO original-frame progress and scalar owner atomicity (AC-039)

The native validator now permits equal floored source timestamps when original
ASIO blocks strictly progress without overlap. It still checks the original
rate, basis, epoch, block extent, evidence and actual HOST progress. Duplicates
and coarse HOST deferral retain their existing behavior.

The newly admitted ASIO flat-nanosecond case exposed two scalar consumption gaps:
app scalar admission and legacy platform discipline committed native validation
after a scalar Unchanged result. App scalar ASIO now requires Retained before
native commit; the platform discipline returns Unchanged before native commit
when its estimator does not progress.

The first shared app run passed 2,103, failed one and ignored five. Its existing
WASAPI test showed that a broad app Retained guard changed an intentional
contract: original counter progress updates native evidence while scalar history
and freshness remain unchanged. The guard was narrowed to ASIO; the target
entrypoint retains its existing behavior, including unsupported target ASIO.
The existing WASAPI test remains unchanged. Coordinate-mode admission still
requires exact Retained coupling. The final shared app run passed 2,104 with
zero failures and five ignored, without changing the existing WASAPI oracle.

`asio-coordinate-focused-c1.log` records six passed, zero failed/ignored
(build 24.43s, execution 0.03s). The initial platform full run passed 244 before
the legacy discipline correction. `asio-coordinate-platform-full-c2.log`
records the final seven new platform fixtures within **245 passed, zero
failed/ignored** (build 17.83s, execution 0.51s). The shared app execution covers
all six new app fixtures. No SDK, ASIO backend or license change was made;
portable evidence does not prove a Windows driver or physical timing.

AC-038 and AC-039 are development-verified, not full task completion. Exact
startup, coordinate pump/control integration, coherent converted native IO,
actual retained selected-speed launchers and ordered review/CLI/browser/desktop
QA remain open. WBS remains 91/193; the full player Goal stays active.

## Coordinate startup development verification (AC-040)

The startup contract now explicitly preserves original admitted units and
immutable epoch/descriptor identity, uses the earliest integer HOST point at or
after the requested exact coordinate, and allows only an explicit bounded
backward interval. The existing bounded input/BGM/startup loop must be shared.
Target-frame admission must retain authority progress before native commit;
target owner observation must check current output identity on both sides of IO.

The implementation shares the existing startup loop through a static generic
helper. Its seed keeps the original epoch, descriptor, observations and actual
readiness sample private. The inverse uses exact interpolation and a bounded
64-step lower-bound search, including the full signed HOST range. Native target
admission reuses the existing frame sidecar and checks both owners before
committing. Actual target output owner observation uses the original coordinate
port and reads its separate BGM report before admission.

All 32 new tests passed: 15 shared startup/inverse fixtures, nine target
presentation fixtures and eight target output owner fixtures. The startup
fixtures include coherent whole-owner replacement with a different epoch or a
different fractional creation basis sharing the same rounded origin. The first
owner run passed one and failed seven at fixture construction: a 2,000,000,000 Hz
mono Float32 format overflowed the checked native byte rate. Changing only that
fixture to 1,000,000,001 Hz preserves flat-nanosecond frame progress and stays
within the existing format limit. Production validation was not weakened.

Evidence in `target/wf/retained-speed-development-12e0c/`:

- `coordinate-startup-focused-c1.log`: 62 passed, zero failed/ignored; build
  1m34s, execution 0.12s. Includes new startup/target fixtures and existing
  coordinate/ASIO regressions.
- `coordinate-startup-owner-c2.log`: eight passed, zero failed/ignored; build
  58.07s, execution 0.06s.
- `coordinate-startup-app-full-c1.log`: 2,136 passed, zero failed, five existing
  ignored; build 7.88s, execution 5.80s.
- `coordinate-startup-allocations-c1.log`: eight passed across three external
  targets, zero failed/ignored; build 1m31s. The calibrated probes verify existing
  coordinate admission/preparation/commit and source/converted mapping paths
  still request no heap allocation after the shared interpolation change. This
  build also compiles the actual Linux binary; it does not execute playback.

Scoped diff whitespace checks passed. Independent bounded inspection found no
concrete source defect; it is not formal full-task review or QA. AC-040 is
development-verified. WBS remains 91/193; actual coordinate gameplay/control
integration and required ordered review/CLI/browser/desktop QA remain open.

Known ceiling: the high-rate fixtures use deterministic mathematical IO doubles,
not physical device support. A separate render report does not prove coherent
converted playback information; the compound port and actual player connection
are follow-up work.

## Explicit gameplay binding development verification (AC-041)

The additive `CoordinateGameplayConfig` carries the original source history
identity while preserving the existing audio config literal shape. Private
audio timing retains that explicit identity; all four existing scalar timing
constructors continue to pass None. The shared cold validator checks coupled
native/authority history and descriptor, actual HOST/raw/logical domains, source
rate and initial cuts, and the complete pristine phase Transport. It uses the
selected setup's original preroll and allows a different target device rate.
Attempt owner/generation are taken from the actual Transport and are not
inferred from the native epoch.

`coordinate-binding-focused-c1.log` records 16 passed, zero failed/ignored
(build 1m38s, execution 0.10s). `coordinate-binding-app-full-c1.log` records
2,152 passed, zero failed and five existing ignored (execution 5.52s).
The new fixtures cover exact fixed rates, odd preroll, differing target/source
rates, identical rounded origins with different original preroll, hidden no-op
command chronology, independent authority restart and refused scope changes
without mutation. Bounded independent source inspection found no concrete
defect; it is not formal review or QA. Scoped whitespace checks passed.

Two temporary dead-code warnings identify the unused timing identity getter and
field, pending the actual solo/cohort integration. They were not suppressed.
The existing unrelated WebTransport fixture warning remains.

Known ceiling: cold coordinate checks qualify scope and mathematical setup,
not native playback packets. The coordinate player entrypoints, actual input
and frontier processing, controls and selected retained launcher activation
still need integration. AC-041 is development-verified; WBS remains 91/193.

## Coordinate control dependencies confirmed during input integration

The actual pause observer and finite end observer currently compare rounded
output timestamps and invert their rounded associations. Attaching an exact
authority cutoff after their ACK is insufficient: an original coordinate below
a cut can share the cut's rounded timestamp. Exact controls must stage actual
source adoption or the original projected target boundary, qualify its native
crossing and earliest integer HOST cutoff, and only then commit the ACK and
actual phase control.

The coordinate input/frontier integration therefore blocks the existing
selected finite terminal fallback before its scalar Transport seek/advance.
It does not synthesize a phase endpoint or mark completion from a rounded
boundary. Legacy scalar paths retain their existing behavior. This is an
intermediate guard, not implementation of exact finite completion; NativeEnd
staging and qualification remain required before production launcher activation.

Known ceiling: coordinate pause/resume, exact finite native completion, fresh
practice attempt replacement and qualified source/converter history retirement
remain required. No retained selected-speed launcher is activated by the new
input entrypoints. The core histories currently have no retirement API; bounded
capacity refusal remains until original observations, acquired input,
extrapolation and pending control/replacement references can be qualified safely.

## Solo and cohort coordinate input development verification (AC-042/043)

The existing solo and cohort gameplay loops now expose explicit coordinate
entrypoints and process original acquired input and observed frontiers through
the actual shared phase Transport. Missing playback mapping holds the original
event; actual source/converted mappings use the current attempt phase basis.
Keysound scheduling retains its existing raw render timestamp. Genuine Runtime
operations commit before fallible report publication, and cohort partial
failure preserves completed member reports and their single authority commit.

The solo NORMAL route with no selected speed also checks the independent
Runtime endpoint and NativePause epoch, HOST domain and Running state. Its
new actual-entry regressions first prove the shared phase validator accepts
the configuration, then check each mismatched owner is refused without input,
device or gameplay side effects. The legacy scalar None-speed route is unchanged.

Development verification logs under
`target/wf/retained-speed-development-12e0c/`:

- `coordinate-pump-focused-c2.log`: 25 passed, zero failed/ignored; build 1m46s,
  execution 0.22s. Includes 17 solo and 8 cohort tests, original fractional
  coordinates, finite endpoint holds, genuine partial failure, canonical capture
  codec/rejudge and actual source Mixer keysound scheduling.
- `coordinate-pump-app-full-c1.log`: 2,177 passed, zero failed, 5 existing ignored;
  execution 7.76s.
- `coordinate-pump-external-alloc-c1.log`: 8 passed across the three existing
  coordinate allocation targets; build 1m52s, including actual Linux entrypoint
  compilation. These calibrated probes cover admission, input/frontier commits
  and playback mapping; they do not measure allocations of the complete pumps.

The first focused compile failed on four test-double API/error-conversion
mistakes. Only those fixtures were corrected. The cohort source-counter rig
now uses positive anchors 1@HOST104 and 2@HOST108; its literal fractional-query
and song-time assertions remain unchanged. Production counter-zero refusal was
preserved. A bounded independent read-only followup found no additional defect;
it is not formal task review or QA.

Known ceiling: these deterministic production-loop tests do not prove physical
device timing, converted native packet coherence, coordinate pause/resume,
finite native completion, fresh practice replacement or shipped launcher
acceptance. Required ordered code/security review and CLI/browser/desktop QA
remain outstanding. WBS stays 91/193 and the full player Goal remains active.

## Exact coordinate control authority development verification (AC-044)

The same AudioAuthority now prepares and validates exact control cutoffs using
the earliest integer HOST reached by original retained native coordinates.
The bounded inverse permits no extrapolation and requires the enclosing right
observation to be due and fresh. Pause drains input at or below the cutoff;
resume retains equality for the next execution. Accepted control commits only
the logical/exact operation watermark. Held presentation closure shares the
existing frontier validation and preserves both operation watermarks.

The 22 new authority/merger tests cover fractional source/counter/target cuts,
a 61-point rational ceil oracle, full signed HOST span and maximum counter
frequency, exact chronology despite equal rounded values, stale preparation,
pending inputs and held closure ahead of or behind an operation. Distinct
legal cuts sharing HOST/floor remain valid mathematical tokens; native adoption
must separately match the token's exact query to the actual cut.

Development logs under `target/wf/retained-speed-development-12e0c/`:

- `coordinate-control-focused-c1.log`: 20 passed, 2 failed. Both prediction
  fixtures requested HOST112 with acquired prefix110 and zero allowed output
  ahead. Only those rigs were corrected through public cold configuration and
  actual acquired prefix112, preserving their original assertions.
- `coordinate-control-focused-c2.log`: 22 passed, zero failed/ignored; build
  50.03s, execution 0.06s.
- `coordinate-control-app-full-c1.log`: 2,199 passed, zero failed, 5 existing
  ignored; build 7.07s, execution 4.77s.
- `coordinate-control-external-alloc-c1.log`: 8 passed across the existing
  coordinate admission, input/frontier commit and playback mapping allocation
  targets; build 1m08s, including Linux entrypoint compilation. These probes
  verify the affected existing helpers, not whole-control or device latency.

A bounded independent source inspection found no additional defect. It is not
formal task review or QA. An environment replacement removed the original
unfinished test-author handle; its existing draft was resumed by a new
independent author after checking actual agent state, without treating the
missing handle as a completion result.

Known ceiling: these control tokens grant mathematical correspondence, not a
native adoption or pause/end acknowledgment. Actual NativePause/NativeEnd
staging, coherent converted/native counter packets, fresh practice replacement,
history retirement and shipped retained launcher acceptance remain required.
Ordered formal review and CLI/browser/desktop QA remain outstanding. AC-044 is
development-verified; WBS stays 91/193 and the full player Goal remains active.

## Native coordinate pause and endpoint development verification (AC-045/046)

NativePause now binds the original epoch, coordinate descriptor and source
identity before requests. Read-only adoption preparations preserve the complete
owner state and actual Mixer pause/resume frames and converted projections.
Requests retain the original authority observation; an adoption before its
exact request coordinate is refused even when integer HOST and output agree.
The phase helper uses the frozen playback frame and current attempt origin,
keeping converted resume overshoot in subsequent playback mapping.

NativeEnd stages actual source endpoint reports and original target projections
on its existing owner. Checked observations can be retained without an ACK;
qualified completion requires the exact cut, matching current snapshot and
strict earlier input drain. The final phase comes from the absolute playback
endpoint and active attempt origin. Both native owners commit their ACK while
the authority control token remains valid, before authority cutoff commit.

Development logs under `target/wf/retained-speed-development-12e0c/`:

- `native-pause-coordinate-focused-c4.log`: 24 passed, zero failed/ignored.
- `native-end-coordinate-focused-c3.log`: 17 passed, zero failed/ignored;
  compilation 1m45s, execution 0.06s.
- `native-coordinate-controls-app-full-c1.log`: 2,240 passed, zero failed,
  5 existing ignored; execution 4.42s.
- `native-coordinate-controls-external-alloc-c1.log`: 8 passed across the
  existing authority admission, exact input/frontier commit and playback
  mapping allocation targets. The build also compiled native entrypoints.
  These existing probes do not establish allocation behavior of the new
  native ACK APIs or physical latency.

Earlier attempts are not passing evidence. The first pause compile encountered
fixture-only private imports/method calls; authors used existing public
underlying coordinate methods and the direct coordinate module without widening
production visibility. Its next run passed 23/24: the remaining converted
fixture had replaced the original bound basis with the converter's moving
frontier. One line now reads the original `pause.target_basis()`, preserving
all projected-cut, frozen-phase and subsequent-overshoot assertions. A later
command used an incorrect Cargo package name and executed no tests. For End,
one filter selected zero tests and one command omitted the host C compiler
environment; the corrected command used the actual module filter and toolchain.

A bounded independent inspection identified and rechecked the creation-origin
and exact-request defects in NativePause. This was development inspection,
not formal full-task review or QA.

Known ceiling: these APIs and deterministic tests are not yet connected to
actual production coordinate pause/end pumps. Coherent converted/native counter
publication, fresh practice replacement, history retirement, native stop and
terminal drain, capture/result completion and shipped launcher acceptance
remain required. No physical or foreign-device timing is established. Required
ordered formal review and CLI/browser/desktop QA remain outstanding. WBS stays
91/193 and the full player Goal remains active.

## Original source control publication port (AC-047)

`CoordinateControlFacts` preserves the same admitted source observation's
original coordinate, epoch, immutable descriptor, history identity, exact query
and complete software publication facts. Validation checks the actual current
NativeAudioPresentation and AudioAuthority records. The existing output owner's
`observe_native_control` acquires through `observe_native_playback` once and
returns these facts without a separate report read, new cache or ACK. Static
device and AudioTiming ports expose this for subsequent production pump work.
Converted control remains explicitly unsupported pending coherent publication.

The first focused run exposed a source defect: the new validator called the
scalar-only presentation logical mapper, which refuses coordinate mode. It
now uses existing checked `AudioAuthorityEpoch::logical_output` arithmetic
after validating original coordinate provenance. The corrected facts focus
passed all 11 tests. The first owner focus passed 6/7; its scalar compatibility
fixture had a fresh estimator on the wrong output epoch. It now binds the
estimator to the actual owner epoch through the existing API, preserving every
cache and read-count assertion. Subsequent verification is recorded below.

Development logs under `target/wf/retained-speed-development-12e0c/`:

- `source-control-facts-focused-c2.log`: 11 passed, zero failed/ignored.
- `source-control-owner-focused-c2.log`: 7 passed, zero failed/ignored.
- `source-control-app-full-c1.log`: 2,258 passed, zero failed,
  5 existing ignored; execution 5.03s.
- `source-control-external-alloc-c1.log`: existing helper allocation targets
  passed 8 tests, zero failed/ignored, including host entrypoint compilation.
  This does not measure allocation of the new control port or foreign OS
  execution, physical timing or end-to-end latency.

At the AC-047 checkpoint, `PumpTiming::coordinate_control` was unused pending
loop integration and its dead-code warning was retained. AC-048/049 below now
connect it to the existing loops.

## Actual solo and shared-cohort control integration (AC-048/049)

The existing coordinate gameplay loops now use the common original source
publication for actual pause/resume and endpoint staging. Required native
bindings are prepared on clones and installed after cold validation. Scalar
pause warming and finite-end seeks are bypassed in coordinate mode. Qualified
original cuts drain input in order; actual phase Runtime operations precede
native ACK, authority cutoff commit and fallible publication. Cohorts preserve
genuine nonempty partial reports and commit once; empty failures grant no ACK
or authority commit. Held closure preserves operation watermarks.

Pause equality belongs to the paused keyboard state. It does not advance the
gameplay input watermark. Resume reconciliation preserves the original device,
sequence and paused clock point while using the real resume output phase and
frozen song position. Absolute native endpoint minus active attempt origin
determines final elapsed phase, including nonzero section starts. Native ACK
does not bypass existing stop settlement, input drain, member terminal or
capture/result completion gates.

Independent authors added 11 solo and 10 cohort tests using the actual public
coordinate entries, Mixer reports, source histories and Runtime/capture. The
cohort nonempty/empty failure tests use the same private control service called
by its public loop. Cases include two/four members, four positive rates, odd
preroll, exact cutoff equality, missing/stale reports, delayed adoption,
publication failure, finite completion and canonical capture rejudge.

Development logs under `target/wf/retained-speed-development-12e0c/`:

- `native-controls-pumps-focused-c1.log`: fixture call arity errors; no tests
  executed. Existing private service calls were updated without changing their
  old assertions; unused new copied fixture helpers were removed.
- `native-controls-pumps-focused-c2.log`: 31 passed, 12 failed out of 43,
  including 22 existing authority tests. Fixtures were corrected to preserve
  original native evidence, use supported positive counter observations and
  configure the actual finite pause endpoint. Existing cold native evidence
  still permits a genuine frontier when a new control packet is unavailable;
  that case now checks the literal frontier instead of demanding no reports.
- `native-controls-pumps-focused-c3.log`: 39 passed, 4 failed. All 11 solo
  tests passed. The cohort equality path exposed a source defect, fixed in
  both loops. Delayed-release fixture checks now distinguish original paused
  input from legitimate reconciliation at the actual resume boundary. The
  cohort finite fixture now configures its original absolute endpoint.
- `native-controls-pumps-focused-c4.log`: 43 passed, zero failed/ignored,
  including all 21 new tests and 22 existing authority tests; execution 0.22s.
- `native-controls-pumps-app-full-c1.log`: 2,279 passed, zero failed,
  5 existing ignored; execution 4.47s.
- `native-controls-pumps-external-alloc-c1.log`: 8 existing helper allocation
  tests passed, zero failed/ignored, including host entrypoint compilation.
  This does not measure whole control-loop allocation or physical latency.

A bounded independent inspection found the solo cold-binding ordering and
legacy warming gate defects, and cohort staged-adoption retention/input hold
gaps. These were corrected and covered by actual-loop regressions. Its cohort
legacy-gate finding was already handled in the current source and required no
additional edit. This inspection is not formal full-task review or QA.

Known ceiling: these software control loops require real adapters to select
the compound control port. Coherent converted/native counter publication,
actual native startup/launcher selection, fresh retained practice replacement,
history retirement, physical timing and foreign OS execution remain required.
No production launcher permission or whole-player completion is claimed.
Required ordered formal code/security review and CLI/browser/desktop QA remain
outstanding. WBS remains 91/193 and the full player Goal stays active.

Known ceiling: common solo/cohort coordinate loops now consume the control port,
but the actual Linux launchers still select converted Target output without
the required coherent Target control publication or cold history opt-in.
Source forwarding alone does not activate retained coordinate play in those
launchers. Original facts still grant no freshness, input-drain, adoption ACK,
Runtime operation or game completion by themselves. Required full-task formal
reviews and CLI/browser/desktop QA remain outstanding. WBS remains 91/193;
the full player Goal stays active.

### Source startup and Linux port development verification

Linux solo and cohort devices now forward source control and source startup
observations through the existing owner once. Target output explicitly refuses
that source-only port. The new `prime_source_coordinate_native_audio` shares
the existing bounded startup loop, receives the actual history identity, and
feeds BGM only from the compound publication's original report. Its captured
two associations remain immutable while subsequent publications continue BGM
progress during readiness waits. It does not prime or acknowledge NativeEnd.

Independent authors added 18 tests using actual Mixer histories, presentation
admission and source packets. Root verification logs under
`target/wf/retained-speed-development-12e0c/`:

- `source-startup-focused-c1.log`: compilation failed on fixture error
  conversion; no tests ran. A contextual `map_err` was added without changing
  test inputs or assertions.
- `source-startup-focused-c2.log`: all 18 new tests passed, zero failed or
  ignored; execution 0.11s.
- `source-startup-app-full-c1.log`: all app library tests with
  `--no-default-features --lib`: 2,297 passed, zero failed, 5 existing ignored;
  execution 5.11s. Existing startup compatibility tests are included. Desktop
  feature and external integration suites are not part of this command.
- `source-startup-linux-bin-c1.log`: actual `linux_bms` binary compiled;
  35 tests passed, zero failed, 3 existing device-dependent tests ignored;
  execution 0.06s.

Known ceiling: Source port forwarding does not change selected launchers.
Both actual Linux launchers create Target output. Converted compound native
timing and original source identity validation are development-verified for
the ALSA publication below. App-side Target packet/control/startup integration,
history opt-in and safe retirement, retained practice replacement and
foreign/physical execution remain required.
These are development checks, not formal full-task review or QA. WBS remains
91/193 and the full player Goal remains active.

### Converted ALSA compound native publication

Opted-in converted output now uses the existing worker TimingShared publication
around generation, positive native target submission and the original timing
query. `ConvertedAlsaStream::native_playback_at` accepts the original source
history identity and returns a checked compound snapshot after at most three
outer attempts. It refuses mismatched identities, crossed revisions, absent
submission, exact queries beyond the submitted prefix and stopped streams.
An absent native timing query retains only software facts and grants no native
observation or ACK. Conversion phase origin no longer substitutes for original
source history creation in this additive reader. The legacy reader is preserved.

Independent fixture authors added 18 tests through actual converted worker
logic and scripted PCM operations. Root logs in the same development directory:

- `converted-native-focused-c1.log`: fixture temporary-array lifetime error;
  no tests ran. A local array binding fixed it without changing assertions.
- `converted-native-focused-c2.log`: required documentation missing on four
  public fields; no tests ran. Field documentation was added.
- `converted-native-focused-c3.log`: 18 new tests passed, zero failed/ignored;
  execution 0.04s. Existing allocation instrumentation confirms no heap calls
  in successful software-only reads and odd/exhausted refusals.
- `converted-native-platform-full-c1.log`: all platform library tests:
  262 passed, zero failed, 1 existing ignored; execution 0.45s.
- `converted-native-app-linux-c1.log`: app library tests with
  `--no-default-features`: 2,297 passed, zero failed, 5 existing ignored, 5.06s;
  actual Linux binary compiled and its tests passed 35/35, with 3 existing
  device-dependent tests ignored, 0.07s.

Known ceiling: the app Target packet, ALSA adapter and output owner now consume
this publication through the development-verified path below. Common Target
control, pumps, startup and selected launchers still require connection to that
same packet for mapping and pause/end staging. Source submitted frames must not
be fabricated from fractional Target positions. Target native counters on other
OSes, safe history retirement,
retained practice replacement, physical timing and ordered full-task
code/security review plus CLI/browser/desktop QA remain outstanding. WBS stays
91/193; no whole-player completion or native timing accuracy is claimed.

### Target packet, actual ALSA adapter and output owner

The app now carries platform-neutral converted playback facts in immutable
`NativeTargetPlaybackQuery` and `NativeTargetPlaybackObservation` values.
They retain the original Target basis, explicit source identity, exact query,
source/converted reports, boundary projections, native evidence and distinct
outer/inner revisions. The existing exact `NativePlaybackMapping` performs
mapping validation; the packet does not invent a Source submission count.

The actual ALSA adapter consumes `ConvertedAlsaStream::native_playback_at`.
An initial native snapshot chooses an exact query; the returned compound
packet must contain the identical complete native snapshot before an
observation is formed. Missing or changed timing yields no observation. The
existing output owner acquires this packet once, checks immutable identity,
admits its original coordinate, then updates only its existing caches from
that packet. Failed or absent admission leaves those caches untouched. The
query-only path grants no admission or ACK. Source and Target rates may differ.

Three disjoint implementation/test pairs added 57 tests: 24 packet, 17 ALSA
adapter and 16 owner. A bounded independent inspection found missing generated
upper-bound validation for Target boundaries. Core projection rejects offsets
past generated output; the packet now rejects those boundary times exactly,
while preserving generated boundaries beyond submitted/native queries and
historical boundaries before replacement stream start. The regression covers
startup, pause, resume and end. This inspection is not formal full-task review.

Development logs in `target/wf/retained-speed-development-12e0c/`:

- `target-packet-integration-focused-c1.log`: no tests; existing converted
  snapshot types lacked exports from the public Linux facade. The existing
  types are now externally nameable without changing their data or behavior.
- `target-packet-integration-focused-c2.log`: no tests; making those types
  public required field documentation under the platform crate's lint. Added
  documentation distinguishes generated/submitted output and the inner revision.
- `target-packet-integration-focused-c3.log`: 56 passed, one failed of 57.
  Checked Target duration overflow was refused as `InvalidReport`; it now
  returns the explicit `Overflow` error. The test assertion was preserved.
- `target-packet-integration-focused-c4.log`: all 57 passed, zero failed or
  ignored; execution 0.17s.
- `target-packet-integration-app-linux-c1.log`: app library tests with
  `--no-default-features`: 2,354 passed, zero failed, 5 existing ignored, 5.91s.
  The actual Linux binary compiled; 35 tests passed, zero failed, 3 existing
  device-dependent tests ignored, 0.07s. Existing mapping/startup/control tests
  are included in the library regression scope.

Known ceiling: common Target control facts, gameplay/startup ports, actual
launcher history opt-in, retained practice replacement and safe history
retirement remain required. Target counters on other OSes and physical timing
are not established by these tests. Required full-task code/security review and
CLI/browser/desktop QA still precede eventual close. WBS remains 91/193 and the
full player Goal remains active.
### Admitted Target control facts (AC056)

`TargetCoordinateControlFacts` now preserves the compound Target packet and
requires the same original coordinate, native record and pair in the existing
presentation before new control preparation. Exact fractional queries remain
distinct even when their nanosecond floors match. Source and Target common
accessors retain their actual reports and converted boundaries without inventing
a Source submitted prefix. Existing Source APIs remain compatible.

Independent source/test writers produced 16 new tests. Focused verification
passed all 27 new and existing Source tests; the app library passed 2,370 tests
with zero failures and five existing ignored tests. Logs:
`target/wf/retained-speed-development-12e0c/target-control-focused-c1.log` and
`target/wf/retained-speed-development-12e0c/target-control-app-c1.log`.
Bounded independent inspection found no concrete defect; it is not a formal
full-task review or QA verdict.

The common device/pump and solo/cohort consumption gap recorded at AC056 is
resolved by AC057–060 below. Native acknowledgement, freshness and input-drain
gates remain separate. This is not physical audio or complete player verification,
and it does not promote a whole WBS item to completed.

### Common device dispatch and Linux Target wiring (AC057/058)

The new static control port carries Source or Target facts through both device
traits and the bound audio timing adapter. Existing Source implementations use
their original single acquisition; explicit Target overrides survive blanket
dispatch. Linux solo/cohort adapters now obtain Target control facts from one
admitted owner packet and query playback mapping through the exact Target packet.

Independent tests passed all 16 dispatch cases with real Source packets and 3→7
converted histories. The app library passed 2,386 tests with zero failures and
five existing ignored tests. The actual Linux binary compiled and passed 35
tests with zero failures and three existing device-dependent ignored tests.
Evidence: `target/wf/retained-speed-development-12e0c/coordinate-device-focused-c1.log`,
`coordinate-device-app-c1.log` and `coordinate-device-linux-c1.log` in that directory.

The Source-only loop and solo Target rejection recorded at AC057/058 are resolved
by AC059/060 below. Known ceiling: Target startup, history retirement and fresh
retained practice remain required before selected launcher enablement. These
checks do not establish physical hardware behavior or formal full-task review/QA.

### Actual solo/cohort Target control loops (AC059/060)

The existing Runtime loops now handle both Source and Target compound control
facts. Actual source adoption reports and converted projections stage pause,
resume and finite end without fabricating a Source submitted prefix. Target
output enters explicit held rendering after genuine pause acknowledgement and
Runtime commit; accepted resume releases hold so the same Mixer adopts commands.
Already committed hold survives a later publication failure. Source behavior and
all existing Source test oracles remain intact.

Bounded independent inspection found an inherited ordering bug: solo could seal
Runtime audio from a stale finite packet before validating its provenance.
Both loops now validate the compound packet immediately after acquisition and
before any report or endpoint side effect. A regression uses a real finite
marker and confirms refusal leaves Runtime unsealed.

Independent test authors added 12 solo and 7 cohort tests, reusing the original
Runtime command queues and moving their actual Mixers into one 3→7 converter.
Tests cover four playback rates, two/four members, pause/resume, held callback
cursor/position preservation, native end, captures, replay equality, original
input provenance, missing/stale reports and publication failure. All 19 pass.
The full app library passes 2,405 tests, zero failures and five existing ignored
tests. The actual Linux binary compiles and passes 35 tests, zero failures and
three existing device-dependent ignored tests.

Logs under `target/wf/retained-speed-development-12e0c/`: focused-c1 failed before
tests because the new cohort test device omitted required `observe_end`;
`target-loops-focused-c2.log` had 11 passes/8 failures from solo fixture settings.
Real Source scheduling was then enabled. Two missing-observation cases correctly
assert the unchanged default age-bound refusal for their 1-second callbacks;
production freshness and the native clock were not relaxed.
`target-loops-focused-c3.log` passes all 19; `target-loops-app-c1.log` and
`target-loops-linux-c1.log` contain the full regression results.

Known ceiling: common Target startup, actual cold history opt-in, safe history
retirement and fresh retained practice remain required before selected launcher
enablement. This is deterministic production-loop evidence with injected native
observations, not physical audio, foreign-OS, browser or whole-loop allocation
proof. Formal full-task code/security reviews and required CLI/browser/desktop QA
remain pending; the full Goal stays active and WBS remains 91/193.

### Coherent Target startup and actual Linux seed ports (AC061/062)

The Target coordinate startup API now uses the existing bounded observation loop
and one compound packet per acquisition. It validates original Target provenance
and explicit Source identity before BGM receives the actual source report. Source
and Target rates remain independent. The original two associations are retained
while waiting for future HOST readiness; missing reports have no independent
fallback. Linux solo/cohort seed ports forward the same static control port.
Existing Source/scalar startup APIs and read cadence remain intact.

Independent author verification added 17 tests with real 3→7 conversion, original
Source creation `(1,1)` distinct from converter creation `2/3`, actual Target
admission and command queue. All 50 startup tests pass, including the existing
33 Source/coordinate cases. The full app library passes 2,422 tests with zero
failures and five existing ignored tests. The actual Linux binary compiles and
passes 35 tests with zero failures and three existing device-dependent ignores.
Logs under `target/wf/retained-speed-development-12e0c/`:
`target-startup-focused-c3.log`, `target-startup-app-c1.log` and
`target-startup-linux-c1.log`.

Earlier focused attempts documented in that directory: c1 failed before tests
from an Option/Result typo in the new fixture; c2 had 47 passes/2 failures from
invalid fixture identity construction and a cold unpinned grid assumption.
The corrected tests distinguish epoch refusal before IO from actual grid
discovery at the first Target admission. Production guards were unchanged;
original BGM, acknowledgement and presentation-state assertions remain.

Known ceiling: selected launchers still use their prior startup call sites and
have not opted into histories. The new API and actual seed ports must be selected
only after safe history retirement and fresh retained practice are connected.
These are deterministic DI observations, not hardware/foreign-OS/browser or
startup held/cached-zero execution evidence. Formal full-task review/QA remains
pending; the full Goal is active and WBS remains 91/193.

Retirement investigation confirmed that deleting history prefix alone would
invalidate existing append-only publication. The next change must preserve cold
identity separately from retained lower bounds, refuse queries before retained
spans, and republish bounded remaining slots under existing revisions. Callback
owners must apply requests before capacity preflight while preserving authority,
pending control/input/practice and unsubmitted PCM pins. No production capacity
increase or unconditional truncation is a substitute.


### Pure owner history retirement (AC063/064)

Source and converted histories now retire only a closed prefix before the
caller-qualified exact pin, retaining its covering span and the last span. Cold
creation identity, pulled/generated endpoints and all remaining descriptors stay
unchanged. Queries before the actual retained lower bound are refused. A checked
mathematical revision changes only on positive removal; it supplies no native
acknowledgement or authority to release consumer pins. Mixer/ConvertedMixer
delegates operate on their actual owned histories.

All 15 new focused tests pass: eight Source and seven converted. They exercise
real fixed-capacity pause/resume cycles, exact fractional coordinates, remaining
mapping, invalid/no-op requests and zero heap calls during retirement. Log:
`target/wf/retained-speed-development-12e0c/history-retirement-focused-c1.log`.
Bounded independent source inspection found no concrete defect; it was not a
formal full-task review. Revision overflow is checked before mutation in source
but not executed by public tests; converted render calls were outside allocator
measurement, so these results do not prove whole-loop allocation freedom.

Full `beatkernel --lib --tests` regression passes 716 tests across 76 suites
with zero failures or ignored tests. Log: `history-retirement-core-c1.log` in
that directory.

Known ceiling: native publication still
assumes append-only history, and actual callbacks must not invoke retirement
until bounded republishing and all authority/control/input/practice/pending PCM
pins are connected. Selected launchers remain unchanged. Formal full-task
review and CLI/browser/desktop QA remain pending; the Goal stays active.

### Source publication after owner retirement (AC065)

The Source publication now detects genuine owner retirement and republishes the
remaining bounded slots under its existing caller revision. It preserves the
original five-word identity header and rejects queries before the actual retained
first span. Same-revision append limits and permanent corruption refusals remain
intact. Retirement and rendering that restore the old count still trigger a
publication; no additional native clock or seqlock was introduced.

All 20 publication tests pass, including nine independently authored retirement
tests and the existing 11. The platform library passes 271 tests, zero failures
and one existing ignored test. The new tests exercise actual Mixer transitions,
24 fixed-capacity cycles against reference PCM/reports, fractional retained
bounds, unchanged original identity, stale owner revision and corrupt headers,
and heap-free retire/publish operations. Logs: `source-retirement-publication-focused-c1.log` and `source-retirement-publication-platform-c1.log` under
`target/wf/retained-speed-development-12e0c/`.

Known ceiling: converted phase publication still assumes append-only history.
Native callback retirement, qualified consumer pins and fresh retained practice
remain unconnected, so selected launchers are not enabled by this change. The current app library
regression passes 2,877 tests, zero failures and seven explicit environment-
dependent ignored tests (`history-retirement-app-c1.log`). These are current
worktree totals, not additional tests attributed to this change. Formal full-task
reviews and required browser/native QA
remain pending; mathematical fixtures are not hardware verification.

### Complete owner and converted publication retirement (AC066/067)

The complete output owner now protects pending PCM's original Target start and
Source sampling floor during retirement, including held/cached output and partial
admission. Source retirement also always protects the converter's next sampling
floor: full PCM admission can leave lookahead samples needed by the next block.
The low-level operation preserves PCM, positive admission, original reports and
boundaries; the caller must still qualify external live consumer pins.

Converted phase publication now recognizes genuine owner retirement, validates
all remaining spans before bounded slot replacement, and publishes phase and
Source facts under the same existing generation. Cold identities, stream origin
and submitted/generated frontiers remain unchanged. Queries before the actual
retained lower bound are refused; same-nonce count/frontier corruption still
permanently closes publication. A recovered retired owner does not grant a newly
seeded stream prior positive admission.

Independent authors added 20 tests across the complete owner and actual converted
telemetry. Initial focused verification passed 28 tests (19 new plus nine prior
Source publication cases). A bounded cache-floor follow-up found a genuine
lookahead defect: real 3→7 single-frame output followed by Source pause and full
admission could delete the Running predecessor still needed by cached output.
`converted-retirement-cache-red-c1.log` records the expected refusal returning
`Ok(1)` before the fix. The next-sampling guard repairs it;
`converted-retirement-focused-c3.log` passes all 29 retirement tests. The new
regression additionally checks actual cached output and exact same-owner lookup.
These logs are under `target/wf/retained-speed-development-12e0c/`.

An earlier fixture compilation typo (`TargetTime` intentionally has no
`PartialOrd`) was corrected with an exact comparator; production guards were not
relaxed. Tests exercise unequal rates, fixed-capacity real cycles against PCM and
report references, exact fractional boundaries, corruption/nonce refusals,
partial native admission, compound publication and bounded concurrent reads.
They do not prove every possible thread interleaving.

Final platform regression passes 291 tests with zero failures and one existing
ignored test (`converted-retirement-platform-c2.log`); the app library passes
2,877 tests with zero failures and seven existing environment-dependent ignores
(`converted-retirement-app-c1.log`). The strengthened actual cached lookup
regression passes in the final platform run.

Known ceiling: actual callback
request transport, external authority/control/input/practice receipt pins,
atomic combined retirement requests and fresh retained practice remain required
before launcher activation. The new crate-scoped owner methods are temporarily
unused in production until that callback connection. Formal full-task reviews
and required CLI/browser/desktop QA remain pending; the Goal stays active.

### Exact future-input minimum and paired retirement (AC068–070)

The authority exposes a read-only exact lower bound for future input using the
original coordinate bracket at the closed HOST prefix. It leaves its retained
observations, revisions and prepared tokens unchanged. Before-first bounded
extrapolation conservatively retains descriptor creation; before-origin inputs
keep their existing discard-only path. Outstanding control/practice references
still need separate pins.

Source and phase histories now share their retirement range/count/checked-nonce
logic with read-only prevalidation. The complete output owner checks both
histories and its existing pending PCM/next-sampling protections before retiring
either. One bad request leaves both histories unchanged. The exclusive owner's
commits mutate separate histories, so neither invalidates the other validation.

Independent authors added seven core validation tests and twelve paired owner
tests. Full core verification passes 723 tests across 77 suites with no failures
or ignores (`retention-prevalidation-core-c1.log`); platform passes 303 tests
with no failures and one existing ignore (`paired-retirement-platform-c1.log`).
Tests use actual pause/resume and Active/Held owners, exact fractional bounds,
invalid requests in both directions, fixed-capacity reference PCM/report
comparisons and allocation tracking. No existing oracle was changed. Logs are
under `target/wf/retained-speed-development-12e0c/`.

The authority adds eleven unit tests and two separate integration tests with a
calibrated TLS allocator, avoiding unsafe code in the app library. Full app
regression passes 2,888 tests with zero failures and seven existing environment
ignores; both allocation integration tests pass, including 128 repeated reads
with zero heap requests. Log: `authority-retention-app-c1.log` in that directory.
All 32 new tests across AC068–070 pass. A bounded independent read-only
inspection found no concrete defect; it was not formal review or QA. Checked
nonce overflow is source-verified but not exercised at u64 maximum.

Known ceiling: native callback request delivery, combined live consumer pins and
fresh retained practice remain required before selected launcher activation.
The new owner API is temporarily unused in production until that connection.
Formal full-task review and required browser/CLI/desktop QA remain pending.

## AC071–072: bounded request delivery in the actual converted output loop

The stream-bound mailbox stores one coherent exact Source/Target pin pair in
four inline atomic words. Original Source creation identity and the immutable
stream Target basis are checked before accepting a request. Producer and reader
each have three attempts; contention returns retry without waiting or allocation.
The existing ConvertedTelemetry generation qualifies both current retained and
generated ranges. A queued revision is mathematics, not native acknowledgement.

The actual shared ALSA output loop now calls a default no-op hook inside its
existing timing update, before pending PCM encoding or fresh generation. The
converted hook protects the next Source sampling floor and original pending
Source/Target starts, validates both histories before retiring either, and
republishes only positive removal. A clamped request remains pending for later
iterations; the existing guard still drops before EAGAIN native wait.

Independent authors added fourteen mailbox and eleven production worker tests.
Focused retirement verification passes 71 tests, including all 25 new tests;
full platform verification passes 328 with zero failures and one existing ignore.
Logs are `retirement-delivery-focused-c1.log` and
`retirement-delivery-platform-c1.log` under
`target/wf/retained-speed-development-12e0c/`. Tests exercise real complete
owners through the production loop, capacity-three repeated held and genuine
Source pause/resume cycles against reference PCM/reports, partial/EAGAIN
deferral, subnanosecond exact pins, original identities, checked request sequence
exhaustion, concurrent coherent pairs and calibrated zero heap requests.
A bounded independent read-only inspection found no concrete defect; it is not
formal review or QA. App regression passes 2,888 library tests and 35 actual
`linux_bms` binary tests with no failures; seven and three existing environment
ignores remain respectively (`retirement-delivery-app-c2.log`). The initial
command used nonexistent binary name `beatkernel-linux-bms` and was rejected
before compilation (`retirement-delivery-app-c1.log`); the corrected actual
target was verified without source changes. Scoped formatting and diff checks
passed. WBS remains 91/193 verified (47.15%).

Known ceiling: current generated range qualification is now connected, and
native callback request delivery supersedes the preceding temporary-unused
owner API limitation. The business port must still aggregate all actual live
input/control/practice references and qualify the current presentation/epoch
before submitting requests. Selected launchers remain unchanged. Actual ALSA
device/acoustic evidence and formal full-task code/security review followed by
browser/CLI/desktop QA remain pending; whole WBS completion is not claimed.

## AC073–074: current-presentation qualified output retirement port

The statically dispatched output backend now exposes a separate opt-in history
retirement trait. The existing GameplayOutputOwner validates the current
presentation and exact query through its existing Target playback packet path,
then requests that same packet's original covering Source physical span start
and exact Target phase start. It rechecks current output epoch/basis before
dispatch. The actual ConvertedAlsaReplacementBackend checks those expected
values again before forwarding to its existing stream mailbox. No separate
native observation, nanosecond inverse, cache admission or presentation/authority
change occurs. Missing output/packet queues nothing; busy and errors retain their
existing meanings. Returned revisions prove only queued mathematics.

Thirteen independently authored tests use actual complete owners and generated
Source/phase histories with a static effect backend. They cover Source creation
before converter construction, covering spans distinct from next sampling
position, exact subnanosecond and equivalent queries, real Held spans, recovered
stream start after the covering phase start, refusal before dispatch, busy/error,
unchanged caches/presentation and continued validity of an actual prepared
input token. Focused verification passes all thirteen
(`target-retirement-port-focused-c1.log`). A subsequent unused fixture evidence
field was removed; final-source full regression passes all thirteen again within
2,901 app library tests and 35 actual `linux_bms` binary tests, zero failures,
seven and three existing environment ignores respectively
(`target-retirement-port-app-c1.log`). Logs are under
`target/wf/retained-speed-development-12e0c/`. Scoped formatting/diff checks pass;
no old oracle or warning suppression changed.

Known ceiling: the business loop must still combine every live input/control/
practice reference before invoking this port. Selected launchers are unchanged.
Actual ALSA acoustic/hardware execution and an allocation measurement for this
new owner-port path are absent; the older mailbox/callback measurements do not
prove this entire path. Formal full-task code/security review and subsequent
browser/CLI/desktop QA remain pending. WBS stays 91/193 verified (47.15%), and
the full BMS player Goal remains active.

## AC075–077: queued/held practice and live pause references

The actual fixed practice receipt queue now exposes read-only original Source
and projected Target minima through both audio endpoint and controller. Raw
lookahead retains Source without inventing a Target projection; projected slots
retain their exact Target boundary. The shared scan does not consume receipts,
alter credits or allocate, and is bounded by configured receipt capacity. The
complete converter owner guards both histories against these actual queue
references; the actual callback clamps requests and leaves them pending while
these references limit retirement. Existing PCM/cache protections remain.

The app separately exposes its queued references and its first app-held
projected receipt. Both getters preserve exact fractions without popping the
queue; actual application releases the held pin. NativePause exposes its exact
original request only during coordinate Pausing/Resuming. Real ACK commits clear
that reference; Running/Paused and scalar transitions expose no pending pin.
Staged adoption/end cuts still need separate preservation by the caller.

Independent authors added 24 tests: six core queue tests, six complete-owner
guard tests, six pause tests and six app practice tests. Core focused tests pass
6/6 (`practice-queue-retention-core-c1.log`); full platform passes 334 tests with
zero failures and one existing ignore (`practice-queue-retention-platform-c1.log`).
Final-source app regression passes 2,913 library tests and 35 actual `linux_bms`
tests with zero failures, seven and three existing environment ignores
respectively (`practice-pause-retention-app-c3.log`). All 24 new tests pass.
Logs are under `target/wf/retained-speed-development-12e0c/`.

The initial app compile rejected fixture use of a private basis method and
structural equality on exact query types without PartialEq (c1). The next run
found five new-fixture assumptions incorrect (c2): ACK clears the original
reference, a control cutoff alone does not close acquired input, and the Mixer
basis must be captured before rendering advances its cursor. Authors corrected
fixtures through public coordinate methods, semantic exact comparisons, actual
held-frontier commit and original cold basis capture. No production guard, old
oracle, test count or ignore was weakened. Scoped formatting/diff checks pass.
Calibrated core/controller and complete-owner queue scans allocate no heap;
this does not prove the entire app/native request path allocation-free.

Known ceiling: a bounded independent read-only inspection found no concrete
source defect; it is not formal review or QA. The new callback practice clamp
has owner tests and source inspection, without a new practice-specific
production-worker fixture. Queue-to-held transfer must still be proved in the
actual business loop against earlier queued requests, including raw Source
references visible before projection. Future integration must combine queued
and held practice pins, future input and all live staged/requested control cuts,
and forward the raw Source minimum alongside the exact Target query. Selected
launchers remain unchanged. Hardware/acoustic evidence and full-task ordered
code/security review then browser/CLI/desktop QA remain pending. Full Goal stays
active, WBS stays 91/193 verified (47.15%).

## AC078–081: shared gameplay loop retirement dispatch

The current solo and cohort Target-coordinate loops now combine the future
input lower bound with pending pause/resume requests, staged pause/end cuts,
queued practice references and app-held practice references before acquiring
more input. Each shared iteration makes one static port call, including one
call for the whole multiplayer cohort. Queue pins are read before receipt pop;
held pins are read afterward. Raw Source references are forwarded separately
because an unprojected receipt has no Target query. The actual Linux Target
adapters forward to the current output owner; Source/legacy paths opt out.
The owner retains its exact packet, epoch, basis and original identity checks.
A Source hint earlier than the immutable original Source start is rejected;
otherwise it can only lower the actual covering Source span pin.

Missing latest control facts allow a read-only request using the previously
admitted presentation, with the existing current-owner packet qualification.
An absent mathematical packet causes no queue effect. A stale observed packet
fails before dispatch. Neither a queued revision nor a busy response creates
native observation or pause/end acknowledgment.

The first executed `desktop,webtransport` app regression compiled and passed
2,967 tests, with two failures in new fixture assumptions and seven existing
ignores (`loop-retirement-dispatch-app-c1.log`). Linux tests were not reached.
The solo fixture originally requested pause on its first lap, so its acquired
input was still fenced when the next lap failed. Its repair first commits a
real unpaused lap, then requests pause and injects an error before ACK. The
cohort fixture originally expected the future-input boundary to advance past
an unresolved pause request. The actual shared loop returns before frontier
closure while that request is pending; ACK clears it before held-frontier
advancement. The corrected case checks this real stall, exact retained query
and native progress before genuine ACK. Standalone authority/dispatch tests
retain strict comparison coverage for independently constructed legal bounds.
Existing test oracles and production guards are unchanged. Final verification
in `loop-retirement-dispatch-app-c2.log` exits 0: 2,969 app library tests and 35
actual `linux_bms` tests pass, with zero failures and seven/three existing
environment ignores respectively. All 27 new tests pass (four Source-hint
owner cases, eight common dispatch cases, eight solo and seven cohort cases).
The run uses `--features desktop,webtransport --lib --bin linux_bms`; its broader
feature set has more existing tests than the earlier desktop-only checkpoint.
Scoped formatting and diff checks pass. A bounded independent source inspection
found no concrete ordering defect; it is not the full formal review/QA gate.

Known ceiling: the loop fixtures record mathematical dispatch and run real
Runtime/capture/replay paths; they do not execute the actual native mailbox
retirement during queue-to-app ownership transfer. That interleaving still
needs actual ConvertedPump/mailbox evidence against prior accepted requests,
including projection after scan, raw lookahead and ring reuse. A platform
fixture can hold a genuine controller receipt to test mathematical protection,
but cannot prove app `PracticePlayback::apply_boundary` or fresh presentation/
correlation/capture/competition transitions. Selected launcher activation,
fresh retained Target practice, hardware and full-task ordered review/QA remain
unfinished. Full Goal stays active; WBS remains 91/193.

## AC082: actual callback ownership-transfer interleavings

Seven new tests run the production ConvertedPump render/admission/retirement
hook with the real telemetry mailbox, Mixer practice queue and converter.
They keep a conservative original request queued while a genuine projected
receipt is popped and held, then execute the hook before the next aggregate
request. Direct Source and compound Target lookups remain available. Other
cases cover equal-request revision reuse with partial PCM, raw Source-only
lookahead during held output and its later real projection, projection after
the previous scan, sixteen ring-reuse cycles against a separate reference
owner, full receipt credits, and later retirement after older references are
released. Aggressive-request clamping is tested separately while references
remain in the native queue.

Focused `practice-retirement-interleaving-platform-c3.log` passes all seven
cases; `practice-retirement-interleaving-platform-full-c1.log` passes 341
platform tests with zero failures and one existing environment ignore. Both
exit 0. Scoped formatting and diff checks pass. The first two attempts failed
before retirement: the fixture incorrectly supplied the current unsubmitted
buffer basis instead of the immutable stream creation basis. Diagnostics
showed genuine Source/phase lookups remained valid. Capturing the actual cold
Target basis before generation fixed the fixture; production identity guards
and all substantive assertions were preserved.

Known ceiling: these are scripted chronological interleavings through the
actual production hook, not a physical driver or scheduler stress run. Holding
a genuine receipt value models mathematical app-held protection; it does not
execute app PracticePlayback.apply_boundary, new attempt installation or
capture/competition transitions. The existing worker fixtures cover outer
timing publication. Fresh Target practice and selected launchers still need
their dependent production connections and verification. Full Goal and task
remain active; WBS remains 91/193.

## AC083: exact Target practice boundary qualification

TargetFrame practice now qualifies the original projected TargetTime against
retained native coordinate observations and uses the same exact coordinate
resume cutoff for readiness and commit. The prepared boundary binds its
original coordinate descriptor, epoch, Mixer basis and projection. Its exact
query getter returns a coordinate only for a genuinely Target-qualified token.
Rounded raw/logical points remain display/Runtime projections. Equal-cut input
stays queued for the fresh attempt; commit preserves native history, acquisition,
closed input chronology and epoch. Scalar qualification is unchanged; Source
coordinate practice is explicitly unsupported by this seam.

The seven new fixtures obtain receipts from the actual practice queue and
converter, then supply original native metadata through the DI admission port.
They cover crossing waits, non-divisor Target/HOST fractions, strictly earlier
input drain with equal input retained, exact commit and unchanged chronology,
malformed copied projection, old qualification expiry and token identity.
The first app execution compiled and passed 2,975 cases, with one new expiry
fixture failure and seven existing ignores (`practice-target-boundary-app-c1.log`).
The fixture attempted bounded native-history eviction without a closed prefix.
It now commits a genuine acquired-prefix/coordinate held frontier before the
next original observation; no operation advance or private state fabrication
was introduced. Earlier source inspection also corrected fixture ordering to
admit the actual native creation observation before pinning the original Mixer
basis. Production guards and old test oracles are preserved. Final verification
in `practice-target-boundary-app-c2.log` exits 0: all seven new cases pass,
alongside 2,976 app library and 35 actual Linux binary tests, with zero failures
and seven/three existing environment ignores. Scoped formatting/diff checks pass.

Known ceiling at the AC083 checkpoint (superseded by the subsequent fresh-phase
development below): qualification/control commit did not install a fresh
selected-speed attempt. At that checkpoint solo/cohort practice advanced
the retired attempt with scalar advance_to and installs Transport::new at
Rate::NORMAL. The RuntimeGroup replacement seam also requires NORMAL rate.
Next work must reuse exact playback mapping/output phase for the old cut,
install the new phase transport at the receipt's actual playback-frame origin
and generation, preserve selected speed, and exercise capture/gauge/competition
and equal-cut input through real solo/cohort entries. Same-output practice must
preserve original correlation and closed input rather than restart it. Selected
Linux launchers still need coordinate history/startup/initial transport wiring
after these dependencies. Bounded inspection found no concrete production
defect in this seam; it is not full formal review/QA. Physical timing and
Source-coordinate practice remain separate scopes. Full Goal stays active;
WBS remains 91/193.


## Fresh same-output selected-speed practice (2026-10-13, development)

The new explicit RuntimeGroup/SoloRuntime phase replacement retains the selected
positive speed and original output owner/grid while installing the genuine
receipt generation and playback-frame origin. Solo/cohort native practice
advance the old attempt through the exact original mapping and preflight the
fresh phase before archival. Same-output transitions preserve original native
history, correlation epoch and acquired/closed input chronology. Scalar APIs
retain their inherited normal-rate behavior. External archive and UI failure
semantics remain terminal; this is not a rollback transaction across ports.

`fresh-practice-phase-app-c1.log` stopped at two missing-trait-method fixture
compile errors. The methods were supplied without a scalar fallback.
`fresh-practice-phase-app-c2.log` compiled and ran 2,982 PASS/8 FAIL/7 ignored:
a rejection fixture constructed a wrong-domain candidate, solo fixtures confused
the advancing next-generated Target cursor with the cold stream descriptor, and
the no-pause/no-fixed-end cohort fixture refused the actual loop's original
native observation port. New fixtures now build coherent candidates, verify
immutable phase/authority descriptors and admit genuine facts from the same
converted owner. Assertions and production guards were preserved.
`fresh-practice-phase-app-c3.log` has 2,989 PASS/1 FAIL/7 ignored. All new Runtime
and cohort cases pass, as do solo loop/missing-mapping/observer-refusal cases;
canonical replay/current judge comparison was added for actual solo captures.
The remaining scrub fractional assertion used queue minima which may be
masked by an older Started receipt; actual applied-cut evidence is required.
No Linux binary result is inferred after the library failure.

Known ceiling: this development checkpoint has one unresolved new fractional
cut fixture and no formal full-task review or QA PASS. Selected Linux launchers
still need original Source/phase history, coordinate startup/initial transport
and coordinate entry wiring; Source-coordinate practice and other native
backend/browser/physical acceptance remain separate required scopes. WBS91/193
and the full player Goal stay active.


The stronger actual-cut fixtures use genuine odd Source cuts. Solo's scrub
renders an actual seven-frame Target block after its cold eight-frame block;
the next real request cuts at Source11 and projects to Target17, preserving
Source11+1/3. The native crossing HOST input belongs to the fresh capture and
its song time derives from the actual recorded mapping and selected 3/2 rate.
Cohort's 31ms looping section at 3/2 cuts at Source21 and first crosses at
Target32, preserving Source21+1/3. Its original continuous Source span remains
at creation 0/0: a fresh transport origin is not a rebased audio-history origin.
Each player's input at the crossing HOST must be absent from the old archive
and present in the fresh capture with the original metadata and expected
positive song time. The public timing contract floors relative active Source
duration once before applying the selected rate: 333333ns at 3/2 gives
499999ns; the fixture does not demand a different rounding contract.

`fresh-practice-phase-app-c4.log` has 2,990 PASS/one new FAIL/seven existing
ignores. All original fourteen new cases pass, including the strengthened solo
scrub and canonical judge comparison. The added cohort case first incorrectly
expected the continuous Source span to rebase at frame21. Its oracle now checks
the original Source creation identity and unchanged 0/0 span, exact cut coverage
and physical21-to-playback21 correspondence, preserving all fractional/equal-cut
capture/replay assertions. Production history and guards were not changed.
Known ceiling: enabled competition state is not exercised by these new native
practice fixtures; their Noop competition ports do not prove opponent network
or ghost behavior. Selected launcher activation, Source-coordinate practice,
foreign-backend/browser/physical coverage and formal whole-task review/QA remain
open. The c5 re-execution is pending; no final PASS is inferred from c4.


`fresh-practice-phase-app-c5.log` retained 2,990 PASS/one new FAIL/seven ignores.
The strengthened cohort case reached its correct fractional cut and replay hash
checks, then detected two same-device input32 records instead of one. Inspection
of the actual matching-member Runtime routing and bound-input capture contract
rejected a cross-player fanout explanation. The test device synthesized inputs
on every acquire call; a genuine practice commit continues the loop without a
wait, so it acquired the same frame again and regenerated an already consumed
event. The new fixture now emits a physical batch once per original native
frame while preserving repeated completed-prefix reports. Its own-input oracle
uses full DeviceId plus sequence, still requires exactly one event, and keeps
HOST provenance, positive fractional song time, old archive exclusion and
canonical current replay equality. No production sequence policy, binding
routing or capture semantics changed. The c6 execution is pending.


Final bounded fresh-phase development verification (2026-10-13):
`target/wf/retained-speed-development-12e0c/fresh-practice-phase-app-c6.log`
exits 0 with 2,991 app library and 35 Linux binary tests passing, zero failures,
and seven/three existing environment ignores. All 15 new Runtime/solo/cohort
cases pass. Actual solo odd-Source scrub and two/four-member odd-Source loop
prove fractional original correspondence, positive first-fresh-input timing,
equal-cut archive exclusion/new capture attribution and canonical replay/current
judge equality. Missing mapping waits without archival/replacement/ACK;
post-install observer refusal preserves coherent fresh identity. Original
Source history creation and continuous playback spans remain unchanged.

This supersedes the pending c3/c4/c5 development results above. It is bounded
software evidence from genuine Mixer/practice queue/Converter with native facts
supplied through DI, not physical/native-device acceptance or formal full-task
review/QA. Enabled competition is still a fixture coverage gap. Selected Linux
launcher history/startup/initial output phase/coordinate entry activation,
Source-frame/counter practice, other native backends and browser acceptance
remain required. WBS stays 91/193 and the full player Goal stays active.


## Linux selected retained coordinate launch (2026-10-13, development)

Local nonnetwork solo and cohort playback now prepare the retained speed-aware
audio program and PracticePlayback setup, including original start/end and
whole preroll. The coordinate cold factory enables Source playback history
before converter ownership moves and phase history before generation; its
identity uses actual Source creation descriptors. Both launchers share exact
cold Target query, original Source phase initialization and bounded history
capacity helpers. The same output owner and input collector survive startup
and gameplay. Existing scalar network committed startup and Legacy contracts
remain separate. A selected policy/score bridge dispatches actual coordinate
practice, preserving pristine state, original roster, gauge/capture/member
competition identities and refusing malformed initial scope before UI effects.
An explicit coordinate cohort activation accepts a fresh positive phase while
the original activation API preserves its NORMAL rate guard.

Root inspection found a real startup error before execution: the coordinate
branches called NativeAudioPresentation.logical_output, whose scalar mapping
guard correctly rejects coordinate mode. The launchers now use the epoch's
checked pure mathematical projection only after genuine exact HOST qualification.
That method is public across the library/binary crate boundary, grants no
native evidence/submission/admission/ACK and changes no authority state. The
scalar guard remains intact. The cold query excludes preroll because whole
preroll is already represented by the phase Transport.

The first compiler command, linux-coordinate-launch-app-c1.log, exited101 at
four type/API errors: epoch projection was pub(crate), one new fixture called
nonexistent policy.clone, and another used TargetTime > without PartialOrd.
The existing checked projection was exposed with its mathematical-only contract;
the fixture rebuilds identical original-source policies with explicit metadata
equality and uses exact typed Target comparison. No test execution results
are inferred from that compile failure. The c2 execution is pending.

Known ceiling: the new binary fixtures prove real cold software composition,
not actual ALSA/HID acquisition timing, native HOST startup qualification or
GUI operation. New selected bridge tests exercise actual generic cohort
practice; ambient solo UI, enabled competition and custom gauge are not proved
by those fixtures. Bounded histories can reject when protected old references
outlive their fixed budget; no runtime growth or unlimited-retention claim.
Source-coordinate practice, other native/backend/browser acceptance and formal
whole-task review followed by browser/CLI/desktop QA remain required. Full
Goal stays active, WBS91/193 unchanged.


`linux-coordinate-launch-app-c2.log` compiled the app and binary targets, then
ran 2,997 app PASS/two new FAIL/seven existing ignores; binary tests were not
reached. The two positive selected cohort cases found real duplicate policy
preparation: default-gauge wrappers called prepare_play_policies, and the
selected-speed pump called prepare_policies through ResolvedGameplayHost again.
The single-preparation assertions remain unchanged. Four actual selected
wrapper guards now prepare default policies early only when speed is absent;
selected-speed or custom-gauge pumps own their one preparation. This fixes
the same verified root cause in solo/cohort practice and normal selected
policy/score entry siblings, preserving plain scalar default preparation. No
idempotency latch, duplicated state or relaxed fixture was added. The c3
execution is pending; one newly introduced unused fixture import was removed.


Final bounded Linux coordinate activation development verification:
`linux-coordinate-launch-app-c3.log` proves 2,999 app PASS/zero failures/seven
existing ignores; its binary target initially failed one new completion oracle.
Finite diagnostic sections intentionally lack a full-song completion owner.
`linux-coordinate-launch-bin-c4.log` verifies the corrected finite contract and
an added genuine full-song selected 3/2 completion case for two/four members:
46 binary PASS/zero failures/three existing ignores. App sources were unchanged
between c3 and the binary-only c4 execution. All 19 new cases pass (eight
selected-policy bridge, six cold solo factory/phase and five cohort composition).
Scoped formatting and source/doc diff checks pass. These results supersede
pending c1/c2/c3 checkpoints; first failure logs remain available.

Actual cold factory tests preserve nonzero original Source physical8/playback5,
1000-to-1500 grids, original commands/PCM, partial submission truth, immutable
cold query and whole-preroll NORMAL/3/2 phase setup. Cohort composition preserves
sparse original two/four-player rosters, gauge/capture metadata and one shared
phase/scope; malformed IDs, section, axes, missing phase/generation and old
scalar nonnormal activation refuse. Actual selected cohort scrub/loop entries
produce canonical replay/current judge equality, reject malformed initial
scores/rosters/policy/capture/source/end/scope before effects and prepare selected
UI policy once. New native binary launch source routes local nonnetwork playback
through those coordinate factories/startup/phase/selected practice wrappers.

Known ceiling: fixture proof excludes actual ALSA/HID clock startup qualification,
ambient UI and physical timing. New bridge fixtures do not directly execute the
solo private score helper, enabled competition or custom gauge. Bounded history
can refuse when old protected pins outlive its fixed budget. Scalar network
committed startup, Source-frame/counter practice, foreign backend/browser
acceptance and full-task ordered independent review then QA remain open.
WBS stays91/193; the full player Goal remains active.


Source-coordinate retained practice development verification (AC090–092):
`target/wf/retained-speed-development-12e0c/source-coordinate-practice-app-c8.log`
records app 3,015 PASS/zero failures/seven existing ignores and Linux 46 PASS/
zero failures/three existing ignores. All 16 new cases pass. Exact Source
receipt frame/Hz qualification supports SourceFrame and SourceCounter without
using native counter frequency as Source sample rate. Genuine direct projected
and plain receipts retain their original creation grid. Same-nanosecond but
unequal rational projection times now refuse before mutation; the earlier
reported exact-projection identity ceiling is resolved by its malformed test.
Solo NORMAL/3:2 loops/scrub and sparse 2/4-player SourceFrame loops/scrub use the
shared phase replacement and preserve archive/fresh capture identity. Solo uses
canonical section reconstruction; cohort compares per-member ReplaySession
judge hashes. The missing solo packet case explicitly reaches the actual
Source30 loop query and then waits without archive, scope or success ACK.
Counter startup uses an actual gate at physical1 with original history0/0 and
positive native observations; physical/playback differences remain explicit.
First failure logs c1–c5/c7 are retained; final c8 supersedes those checkpoints.
Scoped source formatting and source/doc whitespace checks pass.

Known ceiling: these are software DI observations and genuine rendered queue
receipts, with empty sample banks; they do not prove audible PCM, physical
latency, real devices, ambient GUI or browser execution. Cohort SourceCounter,
nonzero original creation identities, preroll, enabled competition/custom gauge
and an explicit numeric fractional-counter song-time oracle remain outside
these fixtures. The tests preserve pending equal-cut input; they do not prove
retroactive reassignment of inputs already committed before a later request.
Source history retirement remains opt-out, so no unlimited-history guarantee
is made. Windows/macOS actual launch adapters and startup still require Source
packet forwarding; WASAPI needs actual native frequency discovery and a start
cut covered by retained positive counter observations, while ASIO needs an
immutable coherent playback query seam. Full-task independent code/security
review followed by browser/CLI/desktop QA remains open. WBS91/193 and the full
BMS player Goal remain unchanged and active.

Foreign Source adapter development checkpoint (2026-10-13, AC093–095):
WASAPI now forwards actual submitted Source packets and same-snapshot native
observations. CoreAudio uses successfully delivered callback bounds rather
than assuming pulled frames were delivered. ASIO has an immutable coherent
Source view that leaves SDK diagnostics and clock anchors untouched; mutable
observation retains the existing real SDK checks. Exact Source queries retain
original creation frames and independent u64 native frequency. Publication
races wait, while malformed identity/revision/range refuses before admission.

Development verification in `target/wf/retained-speed-development-12e0c/`:
`foreign-source-adapters-app-c2.log` records app3,024/Linux46 PASS, zero failures,
seven/three existing ignores. All nine new portable cases pass. Windows GNU
and macOS all-targets desktop,webtransport type checks passed in the respective
`foreign-source-adapters-windows-c1.log` and `foreign-source-adapters-macos-c1.log`.
Scoped formatting of the eleven changed source/test files passed. These are
development checks, not formal independent review or QA verdicts.
The host platform library suite also passed 341 tests with zero failures and
one existing ignore (`foreign-source-adapters-platform-c1.log`). Executed
app/Linux/platform total: 3,411 passing tests; prepared foreign cases are excluded.

Known ceiling: CoreAudio's ten new native-gated fixtures were typechecked,
not executed. ASIO's eleven new fixtures and SDK-specific code still require
authentic SDK/MSVC typing and execution. Actual foreign output devices,
audible PCM and latency remain unverified. The ASIO immutable-query seam is
now implemented, but Windows owned-enum forwarding and all four foreign
solo/local launcher integrations remain pending. WASAPI startup must use
actual native frequency and a start cut covered by positive observations.
Source history retirement and full ordered code/security review followed by
browser/CLI/desktop QA remain open. WBS91/193 and full Goal remain unchanged.

Windows Source owner composition (AC096–098, 2026-10-13): library WASAPI
native/coordinate/Source methods now share borrowed-stream routes; ASIO has
the borrowed immutable Source route and reusable original callback qualifier.
The Windows output enum and replacement backend forward existing coordinate
and Source ports, preserving full original native evidence, exact query and
history identity. Checked packet reconstruction preserves all original facts.
Mutable report caches use the same returned coordinate/compound render;
immutable reads and accepted pause/end caches remain unchanged. ASIO's HWND,
fresh SDK diagnostics/configuration and anchor checks remain in acquisition.

Development evidence: `windows-source-enum-xcheck-c1.log` under
`target/wf/retained-speed-development-12e0c/` records successful Windows GNU
all-targets desktop,webtransport Rust typing. Four changed source files passed
scoped formatting and whitespace checks. Bounded independent reading found no
concrete defect; it was not a formal review or QA verdict. The native evidence
carrier produces a new dead-field warning while direct launcher consumption
remains pending.
`windows-source-enum-app-c1.log` records app3,024/Linux46 PASS, zero failures,
seven/three existing ignores: 3,070 executed host regression cases, excluding
foreign native execution.

Known ceiling: the type check uses C/archive stubs and excludes ASIO SDK.
Actual Windows execution, SDK/MSVC compatibility and hardware latency remain
unproven. Cold Source preparation/startup and four foreign solo/local launchers
still need integration; positive-speed exclusion is still present there.
Full-task independent code/security review then browser/CLI/desktop QA,
Source history retirement and the full player scope remain open. WBS91/193
is unchanged.

macOS retained Source launch (AC099–102, 2026-10-13): platform-neutral cold
helpers preserve actual Source history identity, exact creation query and
initial logical Source phase. Both macOS local solo/cohort now retain original
PCM/program, whole preroll and selected fixed speed through common coordinate
startup and practice pumps. Original input collector, policies, gauges,
captures and roster remain shared; cohort full-song completion is prepared
for every selected member and original sounds are restored. Network admission
uses its existing scalar path. Help now describes local retained fixed speed.

Development evidence in `target/wf/retained-speed-development-12e0c/`:
`macos-source-launch-app-c2.log` records app3,024/Linux46/newhelper7 PASS, zero
failures, seven/three existing ignores (3,077 executed host cases).
`macos-source-launch-xcheck-c1.log` records successful macOS all-targets
desktop,webtransport Rust typing. First c1's single fixture failure was an
incorrect addition of independently rounded preroll origin/advance; c2 uses
explicit whole signed-phase expectations before and after preroll. Production
timing and guards were not changed. Bounded reading found no concrete defect;
it was not a formal review/QA verdict.

Known ceiling: C/archive stubs provide Rust typing, not actual macOS execution
or native C compilation. Host tests exercise real Mixer PCM and pure startup
math, not IOHID/CoreAudio devices or audible/physical timing. Windows positive
counter startup and two launchers still need integration. Source retirement,
actual foreign device acceptance and full ordered independent reviews then
browser/CLI/desktop QA remain open. WBS stays91/193, full Goal active.

Local Source gate startup (AC103–104, 2026-10-13): a fresh real closed queue
is primed from original Source observations, then its producer schedules one
future frame from coherent frozen render facts and actual native buffer size.
Pause/end observers are staged first. Startup returns only after the actual
applied frame equals that cut and positive original observations bracket its
exact Source time. The returned HOST is the minimum integer point reaching
that cut, with zero backward allowance. A private resource watchdog bounds
missing-publication/frozen-HOST waiting without supplying gameplay time.
Existing public priming retains its previous HOST contract.

Development evidence under `target/wf/retained-speed-development-12e0c/`:
`local-source-gate-app-c3.log` records app3,034/Linux46 PASS, zero failures and
seven/three existing ignores. All ten new gate cases pass, including actual
PCM onset, 12GHz/nondivisible u64 counter frequency, nonzero physical creation,
exact HOST ceil, stale/invalid inputs, cancellation and missing-publication
timeouts. Core library12 plus queue/start-gate16 passed in the corresponding
`local-source-gate-core-c1.log` and `local-source-gate-core-queue-c1.log`.
Windows GNU/macOS all-targets desktop,webtransport Rust checks passed in
`local-source-gate-{windows,macos}-c1.log`. Initial c1/c2 errors came from
fixture PCM byte limits and incorrect instantaneous interpolation arguments;
production admission was not weakened. Scoped format/whitespace checks pass.

Known ceiling: native timestamps and submitted prefixes are supplied DI; real
Mixer PCM/history/queue evidence does not prove device latency or SDK execution.
Frozen HOST after arm, pause/end staging failure and real gate-admission races
are outside these new fixtures. A zero-length playback without an actual applied
frame cannot claim startup success. Port calls must return for the watchdog to
run. Windows frequency discovery and both actual launcher connections remain
pending; full independent reviews then browser/CLI/desktop QA, Source retirement
and the full player Goal remain open. WBS stays91/193.

### Windows launcher integration — 2026-10-13

Windows solo/local retained play now forwards all positive fixed speeds to the
shared Source-coordinate practice path. The real closed queue/history is
installed before native open; actual WASAPI counter frequency or ASIO frame
basis describes startup, and the common gate helper uses the applied native
buffer and existing input collector. Qualified original gate coordinates seed
the shared Transport without reopening output. Network startup remains on its
existing admission path.

`windows-source-launch-xcheck-c3.log` records successful Windows GNU all-targets
desktop,webtransport Rust typing. `windows-source-launch-app-c2.log` records
3155 host passes: app3034, Linux46, portable Windows59, actual CLI9, cold Source7;
ten existing ignores are not passes. c1/c2 type errors were incorrect caller
accessors, resolved by the already-captured public OutputFrameBasis. Host c1
also exposed a stale finite-end oracle expecting i64::MAX plus preroll to fit;
the test now keeps 20h/week/near-limit successes and asserts overflow refusal.
Production chronology/admission guards remain strict. Scoped format/diff checks
passed. The bounded read-only integration inspection is not formal review.

Known ceiling: foreign typing uses stubs, authentic ASIO SDK/native execution
is not covered, and host Windows tests do not execute WASAPI. Physical
input/audio, Source-history retirement/lifetime, tighter cold reservation,
retained-specific AC009 CLI/lineage coverage, independent DEEP reviews followed
by browser/CLI/desktop QA and full player acceptance remain open. WBS91/193 and
the original Goal are unchanged.

### Retained invocation and recording-lineage tests — 2026-10-13

Added the two originally planned AC009 external test files. Host c2
`retained-invocation-c2.log` passed six tests: real shipped help, twelve Linux
fixed-speed solo/cohort standalone/primary cold invocations with original chart
and WAV, foreign parser/platform refusal ordering, and composed F5/bookmark/loop
lineage through actual Runtime input, canonical capture and replay reconstruction.
Member IDs7/u32::MAX preserve roster and separate original-base recording paths;
selected6/4 remains pinned and capture normalizes3/2. Refusals preserve active
owners, and loop selection rebases after intervening retry ordinals. The cold
invocation's100ms preroll is distinct from the warm attempt's zero preroll.
Initial CLI failure was a diagnostic-format oracle (structured NotFound/code2
instead of Display OS error2); production behavior did not change.
Both new test targets passed Windows GNU/macOS Rust typing in
`retained-invocation-{windows,macos}-c1.log`; scoped format checks passed.
Bounded independent inspection found a weak event-count oracle; final lineage
assertions require original object, Instant Hit, expected delta/time and input
metadata. `retained-invocation-c3.log` passed all six tests again.
Fresh final Windows GNU/macOS typing passed in
`retained-invocation-{windows,macos}-c2.log`; final scoped format/whitespace passed.

Known ceiling: ALSA null plus missing evdev proves cold preparation/refusal,
not physical playback. Supplied output-phase points prove Runtime/replay math,
not native presentation. Actual GUI control delivery, full AC009 acceptance,
Source retirement/cold allocation work and ordered full reviews/QA remain open.
Windows source publication initializes roughly24MiB for current conservative
capacity before actual native buffer metadata exists; no immediate-loading
claim follows from Mixer Vec reservation alone. WBS91/193/fullGoal remain.

### Source retirement request implementation — 2026-10-13

Added an inline Source-only mailbox, typed backend request port, qualified owner
dispatch and optional shared telemetry range qualification. Requests preserve
original identity/basis, use exact covering Source spans and raw consumer
minima, coalesce monotonically with checked revision, and do not mutate render,
presentation or pause/end facts. Actual compaction remains callback-owned.
`source-retirement-platform-c1.log` passed361 (new mailbox12/telemetry8);
`source-retirement-app-c1.log` passed3043 app (new owner9), Linux46, portable
Windows59, retained CLI3/lineage3 and cold Source7. Total3522 host passes,11
existing ignores excluded. Bounded inspection found no extra defect; it is
not formal review/QA.

Known ceiling: native factories, callbacks, backend wrappers and Source pump
dispatch still need connection. The new private APIs are unused and produce
dead-code warnings; no production Source lifetime/native ACK proof follows from
the29 new request-layer tests. Native SDK/hardware, actual consumer-pin clamp/
retry, cold allocation optimization and whole-task reviews/QA remain open.
Full Goal/WBS91/193 are unchanged.

### Production Source callback connection — 2026-10-13

Source retirement now runs through the actual WASAPI/CoreAudio/ASIO factories,
callback owners, static adapters, Windows remix owner, native device ports and
solo/cohort presentation dispatch. The previous request-only checkpoint is
historical. Shared application clamps outstanding Mixer practice pins and
retries after release without inventing native acknowledgements. CoreAudio's
post-enqueue result race was fixed and has a deterministic callback fixture.
Platform regression passed367 with one existing ignore; final Windows GNU and
macOS all-target typing passed in `source-retirement-callback-windows-c2.log`
and `source-retirement-callback-macos-c1.log`. Windows c1 exposed missing remix
trait forwarding, corrected before the final check. All logs remain under
`target/wf/retained-speed-development-12e0c/`.

Known ceiling: foreign typing uses stubs; SDK/native device execution and
physical synchronization remain unverified. The roughly24MiB Windows cold
publication, actual retained desktop F5 connection, final app regression and
ordered full review/browser/CLI/desktop QA remain unfinished. WBS91/193 and
the full player Goal remain active.

After source freeze, `source-retirement-callback-app-c1.log` passed3045 app,
Linux46, portable Windows59, retained CLI3/lineage3 and cold Source7 tests:
3163 executed passes,10 existing ignores. Platform plus app totals3530 host
passes with11 ignores. This supersedes the pending-app status immediately
above; genuine SDK/device and whole-task review/QA acceptance remain open.

### Retained F5 keeps the active output — 2026-10-13

F5 now uses the existing retained Scrub transaction at the immutable original
start, preserving the output/worker, selected speed and pinned section end.
Pending/refused/paused/output-changing requests preserve live state; an applied
reply clears repetition. Finished joined sessions keep fresh retry behavior.
Focused control7 passed; full desktop372 passed with13 existing ignores in
`retained-f5-desktop-full-c2.log`. Initial full c1 found one old cancellation
oracle, replaced by actual same-worker Mixer/converter/pump F5 application
and then explicit cancellation for the original cleanup/conservation checks.
The genuine pump verifies one output open, real retry ordinal/capture generation
and unchanged PCM/input/replay invariants. Pure key/channel tests also cover
nonzero pinned versus current/bookmark start, selected6/4 and refused/pending
edits without cancelled output.

Known ceiling: direct Desktop key dispatch and supplied native-shaped pump
observations are not physical OS keyboard or acoustic/device proof. Full ordered
review and browser/CLI/desktop QA remain open; Windows cold geometry is the next
implementation under verification. WBS91/193/full Goal remain unchanged.

### Windows history follows actual native buffer geometry — 2026-10-13

Retained Windows launchers no longer preallocate history from the maximum
render ceiling. WASAPI uses validated GetBufferSize on its existing worker;
ASIO uses the actual staged native frame count. Publication precedes prefill,
and cold identity is captured without allocation. Existing history, recoverable
Mixer ownership and legacy allocation-refusal behavior are preserved. Platform371
and app/CLI/cold3165 tests passed with11 total existing ignores; Windows all-target
stub typing passed. Logs are `native-history-geometry-{platform,app,windows}-c1.log`.
At supplied64-frame geometry, actual publication storage is106032 bytes
instead of the former ceiling's25270320 bytes; elapsed loading speed was not
measured. No additional native open, thread or callback allocation was added.

Known ceiling: authentic ASIO SDK and native device/acoustic execution remain
unverified. Full code review found one separate selected-speed overlap budgeting
bug in all six launchers, now under remediation. Independent security review
reported no concrete finding; full code PASS and ordered browser/CLI/desktop QA
are still required. WBS91/193 and the full Goal remain active.

### Selected overlap remediation and duplicate-control review — 2026-10-13

All six launchers reserve BGM overlap using the selected speed and the same
discrete voice extent as core practice rendering. Focused core20 and external42
tests passed; app/lib/main/Linux/portable Windows3522 passed with23 existing
ignores. The external tests include real BMS/WAV loading and12 Linux
standalone/primary solo/cohort initialization routes. Windows all-target stub
typing passed. Failed test attempts retain their logs: incorrect ChannelPolicy
import, mixed-Hz legacy count and NORMAL feedback expectations were corrected.
No native playback success is inferred from missing-input refusal.

The second full independent code review verified the BGM fix but found another
P1: unchanged native coordinates with later HOST metadata pass admission while
preserving old accepted evidence, then fail exact control-facts construction.
Source and Linux converted Target control acquisition require disposition-aware
waiting for unaccepted candidates. Exact repeated accepted evidence remains
usable for newer paused/render reports. A bounded existing-owner correction
and real Source/Target regressions are implemented; strict evidence validation
and accepted age remain unchanged. The final full app suite passed3,568 with23
existing ignores in `duplicate-control-app-full-c2.log`, including all four
new regressions. The earlier focused run45PASS/1FAIL found an invalid new
Counter oracle; unchanged HOST with changed QPC now correctly asserts native
monotonicity refusal, preserving valid later-HOST duplicate waiting.

Known ceiling: the prior full code review failed on that duplicate-control bug;
the final frozen correction passed the third full DEEP code review with zero
findings, and the second independent security review also passed with zero
findings. Subsequent independent QA and current limitations are recorded below;
physical foreign SDK/device/acoustic evidence and full task/WBS completion remain
unproved.

### Local commit checkpoint and actual QA — 2026-10-13

The implementation is saved in coherent core, platform and application commits,
with accompanying tests. Independent CLI QA passed 4,763 Rust tests with zero
failures and 25 ignores, plus 24 direct CLI checks. Strict browser QA returned
FAIL after 10/14 flows: delayed touch stopped gameplay and audio WASM fetch
recorded `net::ERR_ABORTED`. Later unchanged diagnostics passed 14/14 and 13/14;
the latter reproduced the network error. Two controlled actual-main.js
diagnostics reproduce the delayed-touch and in-flight prefix races; no source
fix for them is included in these checkpoints.

Desktop QA returned BLOCKED_ENV. Actual same-window speed/Practice/backstack,
resize, startup refusal and joined-error F5 checks passed, but `/dev/input`
and `/dev/snd` are absent. Logs/screenshots reside in the three
`target/wf/retained-speed-qa-*-12e0c-c2/` directories; diagnostic artifacts
are in `target/wf/retained-speed-delayed-touch-unit-c2/`. These ignored files
are local evidence and are not shipped with the commits.

Known ceiling: active native playback, qualified retained controls and drain,
authentic foreign SDK/device execution, acoustic timing and final task-bound
QA attestation remain unavailable or incomplete. Checkpoint commits do not
assert acceptance. WBS remains 91/193; development stays paused at the user's
request. No push, history rewrite, `mise.toml` or `doc/HANDOFF.md` inclusion.
