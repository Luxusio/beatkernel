# Runtime-driven logical rendering composition

Source inventory found that the existing visual example projected a fixed
timestamp without gameplay events. The new `runtime_visual` example connects
canonical virtual button/pointer input, bindings, actual JudgeEngine/Runtime,
the audio queue/Mixer and VisualProjector. Twelve finite snapshots project the
actual RuntimeReport song time and emitted judge events into reused frame storage.
Four lane fixtures include instant/hold outcomes, alongside a pointer-tracking
path. The external SVG renderer reads logical states and immutable geometry.

Output uses a new optional positional SVG path, create_new and a 256-KiB bound.
`--help` performs no fixture/file work. This remains a software-only example,
without native acquisition, audio device output or physical timing claims.
The existing projection-only example remains available for other geometry.

Rust 1.98.1 locked workspace all-target compilation passed. No examples/tests,
independent reviews, renderer checks or QA were executed under the user's
verification deferral. Updated source inventory now reflects implemented live
ASIO composition while retaining SDK/native acceptance as outstanding. The full
runtime Goal remains active; no phase or full completion follows from compilation.
