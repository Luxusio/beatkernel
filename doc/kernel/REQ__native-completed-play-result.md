# Native completed live results

Native solo and local common pumps classify CompletedPlayResult only after their
actual successful finite/unlimited gameplay/output completion guards. Reuse the
shared model with original config song_origin/end_song and each actual owner's
gauge. Do not re-observe events, aggregate local gauges, alter captures/hashes,
relax output evidence or infer whole-song clearance from practice completion.

Preserve existing run_gameplay/run_cohort signatures as wrappers. Add explicit
run_gameplay_with_result and run_cohort_with_results entry points returning an
optional result or complete per-player result table. Genuine completion returns
Some; cancellation, closed input/device and optional diagnostic time cutoff return
None. Errors remain errors. This makes headless completion evidence available
without depending on a UI publisher. No session-field or device-trait changes,
platform-specific policy or OS-launcher duplication is needed.

Publish completed results through explicit business host callbacks in the
actual completion paths. Default callbacks are no-ops for existing injected
hosts. Only the native compatibility host calls the player adapter; common
gameplay policy cannot reach a player global/thread-local. Add result-returning
fully injected entry points alongside the old unit-returning wrappers.
Publication refusal after proven completion is a technical error carrying the
exact immutable result/table and original cause, not a rollback of completion.
Check cancellation again before final classification/marking/publication. The retained PlayerSnapshot stores one bounded result table
for its exact registered roster; LocalPlayerSnapshot's existing shape stays
unchanged. Solo uses the sole registered identity, local uses original member IDs.
Validate all rows, unique identities, roster coverage, gauge snapshot agreement,
and one common scope before mutation; stage allocation before replacing data.
Unknown/missing/duplicate/later-invalid rows must not publish a partial prefix.
New sessions start without results. Repeating the exact table is idempotent;
changed results cannot replace an attached first completion table.

On first commitment, force one nonblocking handoff attempt through the existing
retained snapshot channel so report cadence does not defer results until cleanup.
An occupied channel may still defer visibility; final cleanup handoff retains
the snapshot. An exact repeat does not trigger another completion handoff.
Publish once using the existing retained snapshot channel, with no new render
loop, native/device access, business-policy copy or per-frame result calculation.
Keep original score/history/pressed state, room results and lifecycle status.
Actual cleanup/join may still fail after output completion: retain completed
results as historical evidence while displaying the separate technical status.
No result is manufactured by with_publisher's Finished status or cleanup success.
Unattached publication is a no-op; typed owner completion still works headless.
Cancellation already observed before completion prevents creation/publication.

Independent deferred fixtures cover actual solo/local pumps and real portable
Mixer/output guards with below-threshold/default failure, healthy survivors and
all-failed cohorts, full versus finite/nonzero-start practice, cancellation/cutoff/
closed input and technical error with no result, headless versus attached behavior,
atomic whole-roster validation, duplicate and changed tables, and cleanup failure
preserving completed evidence. Existing public fixture call shapes remain intact.

Result rendering/browser export, archive persistence, recorded-prefix scope,
configurable gauge identity and high-level mine admission remain unfinished.
Architecture package boundaries, assertion coverage, actual native/browser/device
behavior and performance are not proven by source/compile checks. Execution and
formal review/QA remain deferred; full player Goal remains active.
