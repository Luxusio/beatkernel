# Explicit output clock epochs

The estimator rebind contract below remains available to explicit legacy
HOST/correction consumers. Current BMS audio-authoritative sessions distinguish
raw stream origins from a stable logical output timeline. Held-output publication
stages two original associations, validates a newer physical epoch and preserves
committed acquisition/input/operation/presentation watermarks before ownership
swaps and lease release. Same-stream ordinary resume retains continuous
correlation. These connected paths follow [audio authority](REQ__audio-authority.md);
the earlier pending-integration statements below describe the reusable helper's
scope. Cross-backend/rate matrices, actual SDK/hardware execution and independent
QA remain unproven; no gapless or acoustic-accuracy guarantee follows.

PresentationEstimator and its platform discipline expose an explicit output
observation epoch, initially zero for existing constructors. A caller can rebind
to a strictly newer u64 epoch with an explicit new stream origin, playback origin
and applied song origin. The host domain and validated discipline configuration
stay fixed. Reject equal/older epochs and inconsistent playback domains/origins
before changing any state. Checked maximum epoch never wraps.

Successful rebind clears the old observation ring/history, latest pair and update
watermark while retaining the originally reserved capacity. It supplies no clock
sample or successful startup/completion evidence. Transport history, judged input,
score, replay and actual audio state are untouched. New observations must warm up
again before continuous drift/phase correction. Do not instant-seek a live judge
or assume old latency/drift statistics apply to a reopened output stream.

Epoch-tagged pair/proven-progress admissions reject a mismatched epoch before
any observation/freshness mutation. Platform-tagged WASAPI/supplied-pair/ASIO
admissions likewise reject old stream tokens, preserving original evidence and
existing counter/frequency/rate checks. Rebind resets platform source identity
only after core validation succeeds, allowing a new backend/rate to establish
new observations. Assign the token when the stream/observation is created, never
relabel a delayed old observation with the current epoch at admission time.

Existing untagged public APIs remain compatible and assume the current stream;
they cannot reject an old same-domain observation by themselves. Actual callers
must still fence/drain old callbacks and use tagged paths to get epoch isolation.
GameplayPresentationPort exposes the cold rebind and optional epoch getter, with
explicit unsupported refusal defaults for existing custom implementations. Core
and native discipline adapters implement the same contract through static DI.
No new dynamic dispatch, IO, lock, timer, thread, crate or per-note allocation.

This establishes the reusable clock transition layer, not a complete live device
hot swap. Device stop/reopen, sample-grid/PCM conversion, command/voice transfer,
pause/output fence, actual first-playback anchor, runtime integration and physical
latency measurements remain required. Buffer-length variation on an unchanged
stream stays distinct from a backend/latency discontinuity. Acoustic accuracy and
gapless or instant synchronization are not guaranteed by this helper.
Native stopped-output ownership recovery now follows
[stopped mixer recovery](REQ__stopped-mixer-recovery.md), retaining the original
software state after retirement. Presentation fencing, new-device origin mapping
and live application transfer remain separate required integrations.
Fresh ALSA/WASAPI counters now use the captured original mixer grid through
[output frame basis](REQ__output-frame-basis.md). This supplies counter mapping;
actual stream fencing and automatic runtime transitions remain pending.
Ordinary pause/resume preserves the current epoch through
[resume clock identity](REQ__resume-clock-identity.md), rather than silently
reconstructing epoch zero. It does not increment the output stream token.
Paused frame evidence now has its own explicit output-epoch rebind under
[pause output rebind](REQ__pause-output-rebind.md), preserving acknowledged
playback/gap state while resetting the old source interpolation and kind.
Runtime owners must coordinate these epochs and real callback retirement.

Author independent core storage/atomicity/epoch/warmup/continuous-history cases,
platform source-reset/tagged-refusal cases and actual generic port delegation/
unsupported cases. Assertions/runtime/hardware/formal review/QA remain deferred;
scoped formatting and four sequential compile-only checks run after both paired
writers stop. Full BMS player Goal remains active and unproven.

Audio-authoritative held replacement preserves the original native evidence
kind. Finite ASIO endpoint rebind validates the immutable frame grid, held render
and old HOST chronology using the original upper endpoint, then seeds a staged
endpoint owner through the full ASIO observation. A midpoint pair cannot replace
that interval. Malformed, reached or regressing candidates leave the active
endpoint owner unchanged. The point rebind API retains its existing behavior.
