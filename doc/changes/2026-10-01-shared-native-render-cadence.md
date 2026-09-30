# Direct WASAPI and CoreAudio render timing

The ALSA prefix capture and frame-derived residual calculation now live in a
shared platform module, preserving Linux's existing public result/error names.
WASAPI captures actual pre-Mixer shared QPC during Running and excludes Ready
prefill; CoreAudio captures actual pre-Mixer mach time and transfers its capture
only after callback unregister/drain. Native examples print owner-side summaries
after cleanup. Diagnostic clock failure remains explicitly unavailable rather
than silently replacing audio output or fabricating a timestamp.

Five portable fixtures are authored, including missing timing and full timestamp
span arithmetic. Locked Rust 1.98.1 workspace and Windows GNU/macOS x86_64
platform all-target source checks passed. Tests, examples, linking, native device
measurements, independent review and QA were not executed. The
[measurement contract](../platform/REQ__native-render-cadence.md) preserves
startup/prefix scope and distinguishes software scheduling from acoustic timing.
