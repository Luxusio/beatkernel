# Native software output continuity

Status: implementation candidates present; combined review and QA pending.

The existing recovery carriers now transfer a complete statically typed output
owner. Ordinary Mixer constructors retain their original inference behavior;
additive state constructors support complete ownership. NativeOutputState keeps
the Mixer, prepared equal-rate converter and generated target PCM together with
its positively admitted prefix. Reopening uses the first unsubmitted frame as
its native basis, and incompatible pending interpretations refuse without loss.

The actual ALSA pump carries that state through stop and failures, replays a
bounded suffix before pulling new PCM and advances admission before fallible
telemetry. It publishes reports only for fresh source rendering. Native setup
uses nonmutating preflight validation and commits preparation after native
acquisition succeeds. Static PCM, clock and encoder seams exercise the same
production loop in deterministic fixtures.

Application replacement uses the existing controller with a generic software
owner. Exact authorized first-tail basis is separate from the unchanged pause
frontier. Earlier replay observations and stale reports cannot supply new pause
or native-authority anchors. Active tails under a held replacement refuse while
retaining ownership. Native and application source/test pairs developed in
parallel after the common API checks passed.

Development evidence: core ownership checks passed 12 tests; platform audio
checks initially passed 55 tests. After ALSA integration and a nonmutating
preflight regression, the platform library passed 106 tests, zero failures and
one ignored. Logs: target/wf/native-output-continuity/helper-core.log,
helper-platform.log and native-platform-development.log. These scoped results
do not establish full-task acceptance. The combined application lib/bin run
then passed 2,303 tests, zero failures and two ignored; evidence is
target/wf/native-output-continuity/app-development-remediation.log.

Independent review found one invalid-domain refusal regression: freshness was
checked before noncommitting native validation. Validation now runs first;
valid earlier-tail evidence still waits before any commit or anchor collection.
The unchanged invalid-domain refusal fixture and new full-owner early/fresh
publication fixtures pass. Final independent review and required QA remain open.

The first independent QA passed all functional checks: 3,072 workspace
unit/integration tests and 20 doc-tests, both production WASM libraries,
Windows/Darwin Rust typing, an independent public owner driver and an actual
ALSA null-plugin diagnostic. Null-plugin execution is not physical presentation
proof. QA also identified four newly introduced formatting drifts and a
non-Linux unused test-helper reexport. Those four files were formatted and the
helper export now matches its Linux test consumers; final review and QA are
being refreshed after this bounded correction.

Broad repository formatting and strict clippy remain non-green. QA compared
task-base source and formatting, distinguishing old drift from the four new
format issues. The four reported clippy source files are unchanged from the
task base. These broader health limits are not an all-green release claim.

## Known ceiling

Known ceiling: native hardware and acoustic continuity remain unverified —
upgrade when physical verification is available.

Positively unsubmitted software PCM is distinct from already admitted but
unheard device output. Unequal native rates, admitted/presented recovery,
other backend migration, measured performance, complete BMS/UI/network behavior
and full player release acceptance remain mandatory Goal work.
