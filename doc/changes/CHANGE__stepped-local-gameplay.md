# Nonblocking shared local gameplay

Add StepLocalGameplay over the existing RuntimeGroup and common local member
preparation. Its private shared controller uses the same StepGameplay BGM,
output clock, command batch/ACK, finite output fence and completion machinery.
One original bank/output queue/transport serves every member. Independent
member score and capture state retain original physical input and core reports.

Unknown sources are ignored without freezing setup. Common deadline advances
retain stable member order; core, score or capture failure retains every
already committed member report and fences the whole owner. Export each real
capture once after stop/failure. Unlimited completion requires all actual
member interactions finished; finite completion requires every member at the
logical endpoint plus the existing real output/ACK/presentation barriers.

Six independently authored deferred groups cover legacy solo report/PCM/capture
parity, three/four-member judging while ACK waits, projected contact section
captures with actual StepReplay PCM, finite endpoint and real output barriers,
committed core/capture failure prefixes, and setup/clock/export/partial ACK
boundaries. They use actual preparation, RuntimeGroup, Mixer and replay codecs;
authored source does not establish that their assertions passed. Test and
runtime execution remain deferred.

Scoped Rust formatting and staged whitespace inspection completed cleanly.
Four compile-only configurations completed with exit zero: workspace all
targets, headless app all targets, wasm32 browser library and wasm32 audio
library. Existing three platform cadence dead-code warnings remain on WASM.
No assertions/tests, JavaScript parser, binding generation, build/link,
app/browser/device/audio/network run, formal review or QA was executed.

This is the portable nonblocking owner. Browser binding, device assignment,
multi-field rendering and full runtime/device acceptance remain required work.
Required ordered reviews and browser/CLI/desktop QA remain pending.
