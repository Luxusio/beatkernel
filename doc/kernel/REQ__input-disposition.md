# Accepted input disposition

The generic core exposes transient facts from the actual judge input transition.
These facts prepare BMS empty-POOR policy without inferring a penalty from an
empty result vector, inventing an ObjectId or changing existing scoring.
Default judge/runtime/replay APIs retain their behavior, outputs and state hashes.

## Judge evidence

`JudgeEngine::push_input_report` returns original JudgeEvents and a copied
InputDisposition. A private shared transition serves normal and reporting modes;
the legacy mode statically erases reporting-only work, allocations and dispatch.
Observations are not stored in engine state or snapshot/codec bytes.

InputFreshness is computed before ownership commit. A new enabled button/contact
Down is FreshPress; an already owned enabled Down is HeldDown. Button Repeat is
ExplicitRepeat whether held or not. Other input, including disabled-contact
Down, is Other. Contact ownership includes source, physical/logical destination
and contact ID exactly as existing admission.

The disposition retains actual precommit candidate count and resolver selection.
Nonempty candidates with a declined selection are not unmatched. Selection alone
is not proof of callback dispatch. Count dispatch only immediately before actual
on_input, after Completed and accepts_input filters. Accepted active callbacks
with zero emitted results still match input.

Passive result count comes from expiry before input callbacks. Input result count
comes from subsequently stamped callback results. Unrelated passive misses do
not change whether a new press matched a note. Input hazard count is newly
consumed inclusive-phase markers, including Avoided and other controls; earlier
passive markers are excluded. It is an independent phase fact, not proof of
control-local causality or a rule that suppresses a penalty.

`unmatched_fresh_press()` means FreshPress with no candidates, no dispatched
callbacks and no input callback results. Hazards remain separately inspectable.
No grade, gauge, combo, EX, empty-POOR penalty or synthetic JudgeEvent follows
automatically. Time/resolver admission errors return the existing Err before
engine-owned mutation and produce no accepted disposition. Existing custom
policy effects and callback panics remain outside atomic-error guarantees.

## Runtime evidence

`Runtime::process_input_report` and `process_input_at_report` return a wrapper
containing the original RuntimeReport, accepted dispositions and optional first
RefusedJudgeInput (normalized GameControlId, EventMeta and actual JudgeError).
RuntimeReport and legacy API layout/behavior remain unchanged. One disposition
aligns with each successfully appended bound input, in binding order.

Fence, song end, unbound and ignored routing have no invented accepted bound
dispositions. A song-end advance error is not a refused input. Normalization or
preflight errors retain existing RuntimeError. Successful fanout prefix, audio,
hazards and observations survive later judge/queue failure; this is not a batch
atomicity promise. Caller policy decides how to handle that accepted prefix.
Original physical provenance and one-time input offset application remain.

## Replay evidence

`ReplaySession::push_input_report` records the unchanged input while returning
the same judge's facts. `from_records_observed` streams each accepted input's
InputDisposition with its original ReplayRecord through a static callback.
The callback runs only after results, record and cursor commit; advances and
rejected operations generate no input callback. Prior accepted-prefix callback
effects remain visible if a later record makes reconstruction return Err.
Callback panic happens after commitment and is not rolled back.

Legacy reconstruction, advances, checkpoints, seek/reverse/fork, record wire
format and stable hashes stay unchanged. No observation history is retained by
the session. Default paths use static no-op observation and do not construct
extra reporting vectors or rich reports merely to discard them.

## Verification and remaining policy

Verify real engine, Runtime fanout and ReplaySession entry points, not mock
classification or result-vector inference. Cover repeats, contacts, declined
and invalid resolvers, active zero-result callbacks, unrelated expiry, passive
versus inclusive hazards, original normalization, refusal prefixes, endpoint
suppression, snapshot/hash/records and default/reporting parity. Compare the
unchanged v1 software workload before/after without a performance ranking or
threshold claim. Reported-path metadata allocation is explicit; the old path
uses a private static no-op adapter.

WBS08.10 covers these facts. WBS08.11 still requires an explicit BMS dialect for
candidate/repeat/gauge/combo/capture meaning, including replay/network identity.
This generic evidence does not choose historical BMS behavior or complete the
full-player, physical device or performance objective.
