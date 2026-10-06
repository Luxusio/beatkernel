# Recover software mixer ownership after output retirement

Core audio defines a statically injected StoppedMixerSource port with associated
error and take_stopped_mixer()->Result<Option<Mixer>,Error>. Actual ALSA, WASAPI,
CoreAudio and ASIO streams implement it through their existing owning-thread or
callback retirement boundaries. No new crate, lock or per-note allocation.

Recovery never requests stop, waits, joins, starts, reopens or clones a mixer.
Before confirmed worker join/callback retirement it refuses without consuming
state. After confirmed retirement it moves the original Mixer once; repeated
take returns None. Preserve its unique command consumer, PCM bank, voice IDs,
rational heads, gains, rates, pause/finite-end state, playback/physical cursors,
pending commands and execution counters. Existing telemetry/cadence/report getters
and stop error ordering remain available. No detached worker or callback may
retain an alias to the recovered object.

Thread streams return owned mixer state after normal worker termination, including
ordinary rendering/device errors, and retain it in the joined stream. Panic can
destroy worker-local state and must not claim recovery. Initial open failure may
still consume the mixer under existing APIs; transactional failed-open rollback
is subsequent work. WASAPI preserves original worker failure/stop/wake errors.
CoreAudio recovers only after successful stop/listener removal/IOProc destruction
and in-flight callback drain. Failed unregister/drain retains context and refuses
recovery. ASIO requires actual successful control close/detach/drain, separate
from render diagnostics/fault status; an uncertain close never authorizes take.

ASIO planar renderer may yield its contained Mixer once through a cold mutable
take operation, retaining format/configuration/last-report data. Rendering after
take refuses explicitly and leaves every destination unchanged. Existing valid
rendering remains preallocated without new callback allocation or locking.

This returns software state, not a physical presentation fence. Rendered-but-
unheard device buffers can be lost on stop; callers must establish a real pause/
output fence and account for retained/unsubmitted audio before seamless transfer.
Reopening with a nonzero mixer cursor also needs explicit device-clock origin and
presentation-epoch mapping. Different format/rate/render budgets require separate
PCM/grid/capacity work. Runtime/UI hot-swap wiring, browser-worklet transfer and
physical latency/gapless acceptance remain pending. Do not infer them from take.

Author generic recovery-port PCM/command/state equivalence, actual planar renderer
take/refusal and actual ALSA stop/join/take paths using worker-only memory fixtures
without native audio devices. Assertions, native threads in fixtures, hardware,
formal review/QA remain deferred. Four sequential compile-only checks and scoped
formatting follow both paired writer terminal stops. Host checks do not compile
Windows/macOS/SDK-only branches; text integration is not platform acceptance.
Full BMS player Goal remains active and unproven.
