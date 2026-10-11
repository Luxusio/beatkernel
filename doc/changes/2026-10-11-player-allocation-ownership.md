# Player allocation and retained ownership cleanup

This task preserves the full five-part cleanup scope: shared UI suspension,
Records restoration, measured report/publication copies, gauge staging and all
five remaining native draft/view owners. It follows the loading task's
baseline-proven Records failure rather than excluding that test indefinitely.
The full BMS player Goal and WBS completion remain separate.

Planning independently traced the Records failure to detail packet construction
at zero extent and reactive packet validation before suspended composition.
The shared primitive now has an explicit absent drawable source for suspended
layout packets, preserving dependencies and latest memo values. Public Scene
snapshot validity is unchanged. Four independently authored primitive fixtures
cover initial zero extents, hidden updates, restoration and refusal behavior;
their focused execution passed as recorded below.

The gauge source now exposes a private staged Copy snapshot and commit seam,
with the public observation algorithm unchanged. Custom profile storage and
fallible deep-copy association remain owned. The native report caller now
stages Copy snapshots and reuses its retained legacy history buffer. Before/after
measurement remains pending, so the source change alone is not an established
allocation improvement.

The native owner audit identified a concrete missing LiveAudio scope
synchronization during application suspend/resume. Other owners already pass
through the common synchronization helper, and route retirement already
releases LiveAudio. The missing helper call is corrected and six connected
ownership regressions are written; execution remains pending. Split draft/view
fields alone are not treated as defects.

The first focused development command exited 101 before tests because cc-rs
could not find `cc`. No test pass or source failure is inferred from that run.
The repository already contains the real host compiler wrapper; the retry uses
the previous QA configuration with explicit CC/AR, one build job and the existing
test target directory. Its actual compilation and test results remain pending.
Both attempts are retained separately under
`target/wf/player-cleanup-allocation-ownership/retained-development-{1,2}.log`.

The corrected focused primitive execution subsequently exited 0: 14 retained
tests passed, zero failed or ignored, including all four new regressions. The
test-library build took 3m09s; two existing warnings remain. This development
result makes the primitive available to the dependent Records implementation;
it does not replace full menu/Records or independent CLI/GUI acceptance.

The actual focused gauge execution then passed 80 tests, zero failed or ignored.
It includes gauge policy/dynamics, native and browser gauge/pressed fixtures,
actual replay, archive and stepped gameplay paths. Allocation integration and
the native report caller still require measurement and implementation; these
80 passing behavioral tests do not establish an allocation reduction.

The existing Records suite subsequently passed all 32 tests without exclusions,
including the unchanged
`cached_grade_and_comparison_details_suspend_update_and_restore_original_data_and_actions`.
Its historical zero-viewport failure is repaired by the shared primitive; no
extra Records production patch is justified by that original case alone.
Extended initial-zero and changed-hidden-detail coverage remains part of AC002,
alongside actual UI recovery and the other four cleanup acceptance criteria.

The actual gauge allocation integration passed all four tests, with no filters
or ignored cases, after a 4m31s build. Observing the named reports requested zero
allocations for default policies and every custom grade count from 0 through
64, with 0/1/128/129 judgments and the separately tested hazard/refusal paths.
Cold owned profile copying retains its actual allocation cost: one request of
16 bytes per configured grade for nonempty profiles, up to 1,024 bytes at 64
grades. These are public-API measurements, not a before/after observe speedup;
at this measurement stage the native caller retained its old profile staging copy.
The retained raw output is `gauge-allocations-development-1.log` (exit 0).

Expanded Records verification passed all 35 tests, with no failures, ignored
cases or prior-failure exclusion. All three new initial-zero and hidden-update
fixtures passed alongside the unchanged original recovery test. No Records
production change was needed beyond the shared primitive correction.

Before measuring the native report caller, exact byte comparison confirmed
`app/src/player.rs` still matches base `14f5362`; its SHA256 and adjacent gauge
and score source hashes are retained in `pre-player-optimization-source.json`.
The baseline is explicitly before the caller optimization, rather than a claim
that the whole working tree equals the historical base. The new public meter
separates observed publication from throttled reports and forced delivery.

The first report-meter execution actually failed (2 passed, 1 failed). The
live cold/warm matrix completed, but the new custom-gauge replay fixture used
`ReplayVisual::new`, which deliberately rejects a policy requiring a
policy-aware consumer. Existing gauge replay fixtures use `new_section`; the
author corrected only that constructor choice without changing records,
measurement or assertions. `player-publication-before-1.log` retains the failed
attempt; no full baseline PASS or caller optimization was claimed from it.

The second attempt also failed (2 passed, 1 failed), now at compiled replay
identity validation: the fixture's builtin judge rules differed from its
declared contact-capable input mode. The author replaced only cold judge
construction with the existing public `mine_plan::prepare_judge`, using the
same input mode for preparation and capture. Strict replay identity checks were
preserved. The first two live matrices remain actual partial evidence; neither
failed attempt is counted as complete replay or baseline acceptance.

The third baseline attempt failed at native asset registration because the new
fixture supplied a nonexistent chart path (2 passed, 1 failed). The live matrix
again completed. A real owned temporary chart/asset fixture is being prepared;
production file loading and strict replay validation remain unchanged. The
failed run is retained in `player-publication-before-3.log` and is not counted
as a complete baseline.

The first Desktop ownership command targeted the library and selected zero
tests (exit 0); it provides no ownership acceptance. Desktop belongs to the
native binary, so the six connected tests require the binary target and the
registered `desktop::panel_ownership_fixtures` module filter.

The corrected fourth report baseline passed all three integration tests and all
48 named observations: 40 live cold/warm solo/two-player cases and eight
registered custom-policy replay cases. `complete-before-source.json` records
the corrected fixture hash and byte equality of `player.rs` to `14f5362` before
optimization. Solo custom hazard-only staging requested 1,024 bytes once;
two-player staging requested 2,048 bytes twice. These identified profile copies
justify private Copy snapshot staging; forced owned delivery remains a distinct
measurement and retains unavoidable public snapshot allocations.

The actual Desktop binary ownership suite compiled and exercised all six
tests: five passed, while the LiveAudio reopen fixture failed at line398 with
an absent draft. This failure is under investigation; neither the helper fix
nor five passing siblings establishes complete all-five ownership acceptance.
Its retained evidence is `desktop-ownership-development-2.log` (exit101).

Caller optimization must also preserve an existing accepted compatibility
path: a previously prepared custom member policy can be replaced by an incoming
default replay policy when existing validation permits it. Matched-profile
replay can use a Copy snapshot; rare policy replacement requires staged owned
replacement, discarded on any later refusal. No new public rejection is added.

The LiveAudio fixture failure was traced to the channel's queued default
startup snapshot overwriting the fixture's acknowledged Playing/Paused state
during actual `collect_game()`. The factory now drains that startup snapshot
before installing its acknowledgement; all suspend/reopen assertions remain
unchanged. Actual binary rerun remains required.

Report caller changes are frozen for verification: solo/local and matched-policy
replay stage `GaugeSnapshot`; the local array holds Copy candidates; accepted
replay policy replacement retains its rare owned candidate. All late validation
still precedes state commit. Five private regressions cover transactionality,
contention, owned delivery and the accepted replacement compatibility path.
The public meter is byte-identical to the complete baseline fixture.

The after-optimization meter passed all three tests and 48 named observations
using that identical fixture (`player-publication-after-1.log`, exit0). Matched
custom hazard-only staging now requests zero allocations, down from 1,024 bytes
for solo and 2,048 for two players. Warm custom solo and replay 128-judgment
staging drops from 2 requests/1,168 bytes to 1 request/144 bytes; their separate
forced delivery drops from 10 requests/56,400 bytes to 9 requests/38,992 bytes,
saving one 17,408-byte history copy. Two-player owned delivery remains unchanged.
These are allocation request measurements for named work, not latency rankings
or claims of an allocation-free player. At this stage private atomicity and
Desktop fixture reruns still awaited execution; their later results follow.
Formal review and independent CLI/browser/desktop QA remain required.

The five private report regressions subsequently passed with zero failures or
ignored tests (`player-atomic-development-1.log`, exit0): atomic cohort refusal,
late solo refusal, custom replay failure/recovery, contended mailbox and owned
snapshot independence, plus accepted replay profile replacement and later live
observation. Changed-file scoped formatting also passes after formatting the
new retained/Records fixtures and sorting the new Desktop module declaration.

The corrected Desktop binary ownership suite passed all six tests with zero
failures or ignored cases (`desktop-ownership-development-3.log`, exit0).
Settings, Players, Devices, Records and LiveAudio now have connected portable
coverage for retained parents, pending completion refusal/acceptance, fresh
identity and Closing. The repaired LiveAudio suspension permit passed. These
are development fixture results; actual native/browser journeys and independent
formal review/QA are still required for task acceptance.

Independent DEEP formal review at `088f865` returned FAIL with one verified
LiveAudio defect: a reply completed while suspended was consumed before
Navigator presentation admission, leaving the retained draft's request and
Applying message stale after resume. The earlier six passing owner fixtures
did not cover completion during suspension. Repair must defer only reply
consumption in the existing mailbox, keep native collection/cleanup running,
and exercise both accepted and refused replies through suspension and resume.

The reviewed repair now leaves output replies in the existing channel while
Navigator is Suspended. It gates only reply consumption, preserving practice,
snapshot collection, native worker cleanup and Closing draining. The existing
real-channel fixture now delivers success and refusal during suspension, polls
twice while hidden, checks exact retained identity/request/editor/values/message,
then resumes and checks one-time presentation and cleared pending ownership.
The expanded suite passed all six tests (`desktop-ownership-development-4.log`,
exit0), including both suspended-completion outcomes. Fresh formal review and
required independent QA remain pending; the earlier formal FAIL is preserved.

Known ceiling: focused primitive, Records and gauge development checks passed,
including the complete before/after report meter and private atomicity;
all-five owner development fixtures passed; independent formal review and actual
CLI/browser/desktop QA remain required. No
world-fastest, whole-player zero-allocation or WBS promotion is claimed.

Independent final DEEP code review and documentation review passed at `f8fc87d`
(production source `073a449`). Fresh CLI QA executed the complete app suite
without the old Records exclusion: 41 targets, 3,187 passed, zero failed,
38 environment-dependent ignored tests and zero filters. The fresh allocation
meters passed seven tests and 48 named observations. Twelve actual CLI paths
passed on rebuilt binary `be0a6591ebf97116b0296660313711d261d1b0ce257ef0cd91171f857c9cade0`;
Linux and Windows all-target typing subsequently passed with exit0. macOS
typing and complete CLI lens acceptance remain pending; foreign stub checks
establish Rust typing, not native linking or execution.

After that suite passed, the task resource clause was amended to allow one
exclusive compiler and one exclusive GUI lease. Native functional GUI checks
used a hash-verified frozen binary while metadata-only checks ran; GUI lenses
never overlap and these observations establish no latency ranking.

Independent desktop QA returned BLOCKED_ENV with successful actual X11 checks
for Settings, Players, Devices and Records. Records detail pixels matched after
800x600 -> 1x1 -> 960x720 restoration; hidden page changes, retained parents and
fresh reopening were exercised. The actual LiveAudio entry requires native
input initialization, but this host has no `/dev/input`, `/dev/uinput` or sysfs
uinput device; `/dev/null` genuinely refuses the evdev ioctl. No mock driver was
substituted. Connected six-test success/refusal coverage remains distinct from
this missing shipped-GUI acceptance. Both owned applications exited through
product controls with exit0 and left no owned windows/processes.

Known ceiling: actual LiveAudio GUI acceptance requires an accessible real or
virtual evdev device; this environment cannot currently supply one. Complete
browser QA and remaining typing still proceed. Full task and player completion
remain unproven; the four successful native panels do not replace that gap.
