# Windows gameplay output ownership

Windows solo and local-cohort gameplay now use the shared GameplayOutputOwner
and correlated paused output-request service. A statically injected composition
adapter wraps the existing native Output rather than discarding its ASIO host
HWND, driver-message servicing, native faults or refreshed multimedia-clock
anchor. Startup retains the exact raw native stream inside the common owner.
Input acquisition still preserves original attachment IDs, events and times.

Nonnetwork WASAPI play advertises device/buffer/period/channel-matrix fields.
Portable typed draft mapping preserves current mode, negotiation, encoding and
rate, validates dimensions before retirement, and preserves/reset matrices
against the original source width. Native opening uses recoverable WASAPI
methods and retains accepted matrix metadata; replies canonicalize coefficients.
Initial launch and saved profile schemas still reject the live-only matrix.

ASIO observations retain full rendered-block interval provenance and original
frame basis. End/pause use accepted interval evidence. ASIO manual output
replacement and cross-backend settings remain pending; those capabilities are
not advertised. Network output settings remain disabled. Shared ownership is
connected for Windows local play; macOS cohort and Watch controls remain pending.

Portable mapper fixtures run on Linux without constructing native streams.
Independent code and security reviews passed, followed by scoped CLI QA:
runtime library 1,584 passed / 2 ignored, main 227 passed, Windows standalone
binary 31 passed, and explicit actual ALSA live-matrix diagnostic 1 passed,
all with zero failures. The four mapper fixtures execute in both the standalone
binary and the main application's embedded Windows module.

Workspace all-targets, WASM browser-library, Windows GNU all-targets and isolated
ASIO Rust all-targets source checks exited 0. Shipped Windows binary help exited
0. Initial-launch matrix arguments returned the expected exit 1; actual native
playback on Linux also refused with its explicit Windows requirement.

Windows Rust source checks cannot prove actual
WASAPI/Raw Input/ASIO execution, C++ SDK bridge linking, MSVC ABI or acoustic
synchronization. The full player and native device acceptance remain unfinished.
