# Gapless practice on retained output

This is the implementation contract for retained-owner loop/scrub. It is not
evidence that the feature is implemented or that WBS09.11 is complete. The
[fresh-owner restart API](REQ__section-restart.md) retains its existing contract.

## Original data and output ownership

Prepare the BGM program from original decoded PCM and original-song cues before
native output starts. Loop/scrub must retain the same Mixer, bank, consumer,
device converter and native endpoint. Each repetition must not copy a new PCM
suffix, reset converter history/phase, discard unread converted samples or reopen
the device. Use existing explicit Ceil source-frame selection and report the
requested/applied source time and correction; never select from a previous
rounded/sliced restart. Chart object times and asset samples stay immutable.

Retained preparation keeps the full original bank, chart, keysound bindings and
invisible/hazard plans. Only the initial judge source is section-selected.
All original lanes require valid bindings because a later backward transition
can restore an initially excluded lane. The recurring program owns finite song
regions; the retained Mixer itself has no immutable finite endpoint. Its legacy
BGM feeder is empty to prevent duplicated cues after delayed owner-thread ticks.

Fresh practice excludes earlier heads and crossing holds, matching existing
section selection. Each attempt prepares coherent judge/rules, class/gauge,
score/failure, end, capture and publication state. Use the existing Judge/replay
engine; keep the original recording base and increasing attempt ordinal with
the correct section header. Watch and network sessions cannot loop.

Initial native solo setup publishes the section-selected source with its matching
compiled chart. Initial competition and replay capture use that same selected
source and existing section/replay identity rules. Section selection removes
earlier visible heads while retaining invisible and mine metadata under the
existing source-selection contract; do not drop that metadata to make setup pass.
Keep the original source, PCM bank and full sound plans for later retained
transitions; resolve gauge/timing policy from original data.

The graphical session's unprojected launch lineage advances at every committed
fresh attempt. Local member capture paths remain projections of that lineage;
the first member's projected filename cannot replace the global recording base.
An F5, bookmark or fresh-loop retry requested during retained playback must
derive its recording ordinal from the last committed attempt after the previous
worker joins, while preserving the requested section. A UI snapshot taken before
cancellation does not freeze the audio-side repetition ordinal.

Once fresh Runtime/capture state is actually installed, its generation and
recording/retry identity must advance independently of fallible screen or reply
publication. Observer failure or cancellation must archive that fresh capture
under its fresh path, never the already-retired attempt's path. Final joined
retry uses this actual applied identity even when the visual snapshot could not
be published; borrowed score state also follows actual attempt application.

The practice pump archives retired attempts only. Its final live capture stays
owned by the session on completion, cancellation and technical failure, so the
existing native finisher can save that capture and derive the completed-result
sidecar from the same evidence and current attempt path. Finalization preserves
the original gameplay/publication error if replay or sidecar saving also fails.

A successful retained scrub starts a nonrepeating attempt and clears the UI
loop-enabled indicator when its correlated applied reply arrives. Pending or
refused scrub leaves the previous loop indicator unchanged. The next loop
toggle after a successful scrub enables the marked region rather than sending
another disable request.

The UI consumes a reply together with its originating request action in one
mailbox operation. Lock contention must retain both pending request and reply;
it cannot consume a success while losing the action needed to update loop state.

## Clock coordinates and boundary ordering

Stop-command admission evidence follows the retained output lifetime, matching
the Mixer's cumulative execution counters. A fresh attempt resets its own
failure/stop latch and completion barrier, but must retain prior admitted Stop
evidence. Final advances at a cohort boundary contribute to the same evidence.
Failed-gauge loop/scrub regressions must execute legitimate Stops for inactive
voices on the same Mixer and continue into fresh attempts without weakening
unknown-stop validation or resetting Mixer counters.

Converted output must qualify a practice boundary at the first actual target
sample at or after its source boundary, using the conversion block's exact
source positions and target time. Source lookahead is not target consumption.
Held target silence must remain part of physical target time without consuming
source PCM. Raw source receipts cannot be consumed as native-qualified target
receipts. Projection evidence must remain ordered and bounded across worker
stalls, cached PCM, conversion retargeting and output-state moves.

Finite logical sections retain the original half-open interval `[start,end)`.
Their Runtime, capture and playback configuration must agree on that exact end.
Audio transition occurs at the actual Ceil output sample boundary. Inputs at or
after logical end but strictly before the presented transition update original
acquisition order and physical key state without judgement, keysound or replay
Input records, using the existing finite Runtime disposition. Old-attempt
Advance is clamped to the exact logical end. Original input equal to the actual
presented transition belongs to the fresh attempt; neither its timestamp nor
the audio boundary is relabelled or moved earlier.

Physical output frames stay monotonic. Playback frames exclude inserted pause
silence. Logical audio time remains on the retained output epoch. Original song
time may return to a requested section anchor. Program generation is a separate
identity from native device/output epoch; no host-clock fallback supplies missing
audio evidence. Converted source/target rates use their existing exact frame
basis rather than restarting conversion at the practice boundary.

Repetition runs on an audio-side prepared cursor, even when UI/gameplay ticks
stall. At a boundary, retire old-scoped voices/future commands, start the new
program/BGM heads, then execute new-attempt keysounds at that frame. A late old
Runtime command cannot acquire the new generation merely because the callback
advanced. Legacy AudioCommand::Seek and immutable finite endpoints keep their
documented semantics; retained practice is a distinct mechanism.

Ordered bounded receipts preserve rendered physical/playback boundary,
iteration/generation and requested/applied original target. A rendered receipt
does not prove native presentation. Drain every previous acquired input prefix
using the existing authority before committing the corresponding fresh attempt.
Several audio iterations cannot be collapsed to the latest render report.
Receipt overflow or expired native mapping must be explicit, never overwritten
or replaced with a wall-clock observation.

Native boundary preparation exposes the original HOST cutoff before input drain.
Acquisition readiness is a separate check: drain strictly earlier original input
against the old attempt and retain input equal to the cutoff for the new one.
Pin the original mixer source-frame basis even with a target-rate converter;
source frame indices are not native target frame indices. A native epoch change
invalidates prepared boundary tokens and requires explicit basis admission.

## Real-time and failure rules

Cold preparation validates program/sample/region/gain/capacity and checked frame
arithmetic. Render performs no allocation, lock, decoding, chart compilation,
PCM suffix copying or final heap-owner drop. Program queries and receipt work
are bounded by explicit caller-selected limits.

The program reserves `max_overlap` mixer voice slots for BGM; gameplay needs
additional slots in the same mixer. Cold overlap admission must account for
output-grid quantization and mixed source rates for every exact section anchor.
An original-time nonoverlap does not imply nonoverlapping rendered voices.
Insufficient capacity is a cold refusal, never a callback panic or dropped BGM.
The original PCM extent remains the basis for section source selection.

Cold admission failure leaves current owners untouched. After an actual audio
boundary, capture/publication failure retains its applied generation and boundary
and stops technically; it cannot claim rollback or successful completion.
Cancellation/closing revokes pending transition authority. Pause/resume/output
replacement and practice must share one explicit admission precedence; a pending
transition cannot race replacement. Recovery retains the complete program,
receipt and converter state or refuses before retiring the current owner.

## UI/native integration and verification

Supported live F8/F11 routes send bounded PlayerViewer practice requests through
the common native pump. Normal accepted transitions do not cancel, join,
spawn_game or reopen output. Unsupported capability and refused/stale/pending
requests remain visible. F5 ordinary pinned retry stays a separate lifecycle.
Shared local cohorts use one audio boundary and independent member state/records.
Platforms supply output and observation adapters, not different gameplay policy.

Verify the actual desktop request -> pump -> real Mixer/ConvertedMixer -> injected
native-owner path with endpoint open count one and no normal reopen, independent
PCM/time/recording oracles, worker stalls, many loops and queue/receipt/pause/error
cases. Core helper tests or fabricated counters alone cannot complete the feature.
Preserve existing finite, Seek, pause, replay and output-recovery regressions.
Current executed commands, results and limitations are recorded in the
[implementation evidence](../changes/2026-10-10-gapless-practice-output-timeline.md).

## Known ceiling

Portable PCM and injected native-owner evidence do not prove physical acoustic
sync, actual ASIO SDK/driver operation, hardware latency, browser practice parity
or world-leading performance. These remain in the full player WBS, including
09.12 and the OS/device/browser matrix; no component-only result completes09.11.
