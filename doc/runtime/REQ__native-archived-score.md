# Native completion score association

Native solo and local completed recording paths shall export the same exact
version-2 score/timing details as common Step completion. Keep original completion
proof, capture identity, gauge policy and PlayerId association. Score alone,
cancelled playback or a prefix cannot create an archive. No replay rerun, guessed
counts, gauge-derived score or UI/global state readback is allowed.

The common solo pump gains run_gameplay_with_result_and_score_and_ports with an
explicit borrowed ScoreSummary, caller-supplied control/device and host. Require
an initially default score before effects. Use NativeScoreHost::new(host, score)
as a static business observer around the existing host publication boundary:
accumulate each genuine committed RuntimeReport once via ScoreSummary.observe,
then attempt the original host publication even if scoring refuses. Preserve the
original score error first, or the original publication error when scoring succeeds.
Atomic score refusal leaves its prior prefix intact; the existing pump preserves
the original report in its observation error and cannot claim completion.

Delegate cancellation, pause, diagnostics, section and typed completion publication
without fabricated values. The solo score observer does not merge local reports;
local pumps already maintain independent member scores. Add no IO, clocks, locks,
UI dependency, trait-object dispatch, new crate or event copies in the observer.
Existing scoreless entry points stay compatible. Native compatibility composition
adds run_gameplay_with_result_and_score, reused by Linux, Windows and macOS roots.
The actual roots create default score before their pump, retain it through cleanup,
and pass it to finish_solo_with_result_and_score.

Add solo_archive_with_score and cohort_archive_with_scores alongside existing
scoreless APIs. Scored construction uses ResultArchive.from_completed_with_scores
with the whole original ID roster; missing, foreign, duplicate and invalid later
scores refuse the entire archive. Retain CompletedSoloPublicationError and
CompletedLocalPublicationError proof without replacing their original first error.
Local finalization obtains references to existing GameplayPlayerState.score.
Prepare archives before capture consumption, after cleanup and before save effects.
Always attempt replay saves before archive publication, preserving existing
outcome/cleanup/replay/archive error precedence and exclusive-create policy.
Original scoreless APIs continue emitting version 1.

## Evidence and known ceiling

Author pure host delegation, scoring/publication failure, exact opaque-grade and
timing fixtures plus genuine Runtime/portable-pump/Mixer completion and native
save association fixtures. Use injected effects, without actual audio/input/file/
network execution. Compile checks establish source compatibility only; Windows
and macOS adapter call-site edits require later target and hardware acceptance.
Runtime assertions, formal reviews, required QA, verify and close remain deferred.
Cold archive construction may allocate; first new grade uses existing score-map
insertion. No benchmark or globally allocation-free claim is established.
Native Records now retains associated stored final score details in a cached
subview under [the detail contract](REQ__native-record-details.md). Full
grade-table UI and saved comparison archival remain unfinished.
