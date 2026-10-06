# ASIO live channel selection

The ASIO paused output capability now includes ordered output-channel indices.
Launch and live parsing share the same bounded-list policy: 1..32 unique
unsigned ASCII-decimal indices fitting the signed SDK channel range (index zero
is valid). The actual driver still validates available channel indices during
recoverable preparation.

Same-count reordering retains the current source Mixer and matrix. Changing
count requires a matrix with the original source width and target rows matching
the new selection. Buffer-only changes preserve both selection and matrix.
Exact reset after resizing requires selecting the original source count too;
no source format is rebuilt or channel list silently truncated. The accepted
selection reaches the native reopen configuration and canonical reply.

WASAPI rejects this ASIO-only field before retiring its current stream. The
retained UI renders three advertised ASIO fields and submits channel/matrix
edits together without replacing the screen scope. Driver identity, registry
view, source rate, QPC/error settings and pending cleanup ownership are unchanged.

Portable mapping/launch-policy and retained UI fixtures cover reorder, resize,
preservation, reset and invalid lists. Independent code/security reviews passed,
followed by scoped QA: library 1,588 passed / 2 ignored, main 234 passed, Windows
binary 35 passed, and actual ALSA settings diagnostic 1 passed, all with zero
failures. Workspace, WASM and Windows normal/isolated SDK Rust all-targets checks
exited 0. Latest duplicate/out-of-range capability assertions executed in both
application entry points.

Direct Windows CLI help exited 0. Valid channel lists and the signed maximum
parsed before the expected Linux OS refusal; duplicate, oversized, negative,
non-ASCII, empty and 33-channel lists returned the expected channel errors.
Rust cross-source checks cannot establish actual SDK bridge linking,
MSVC ABI, Windows driver availability or physical channel routing. The full
player remains unfinished; driver/backend switching and native acceptance remain.
