# ASIO live driver selection

The paused ASIO capability now advertises a trusted-driver CLSID alongside
buffer, channel order and matrix. Apply explicitly loads the selected installed
driver; the field states this trust obligation and that existing clock/error
estimates remain. Driver code loading still uses the native trusted-control API.

Canonical nonzero UUID/CLSID parsing is shared by SDK-free policy tests and the
Windows registration decoder. Edited identities are resolved against exactly
one installed entry in the existing registry view before replacement is queued.
Missing, malformed and ambiguous identities refuse without retiring current
output. Native enumeration does not load the DLL; recoverable opening occurs
only after the common owner retires the old stream and its callback routing.

The selected registration reaches the existing reopen configuration. Source
Mixer format/rate, channel/matrix checks, QPC/error settings, epoch/frame basis,
pending stream/attempt HWND and cleanup-error ownership remain unchanged. Native
preparation checks the new driver's actual channel/buffer/rate constraints.
Portable tests cover canonical identities, unique installed selection and
capability trust text; retained UI covers Apply/reply correlation and scope.

Independent code/security reviews passed, followed by scoped QA: platform 206
passed / 1 ignored, runtime library 1,588 passed / 2 ignored, main 239 passed,
Windows binary 37 passed, and actual ALSA settings diagnostic 1 passed, zero
failures. New canonical identity, unique-registration and UI trust/scope tests
executed. Workspace, WASM and Windows normal/isolated SDK Rust source checks
exited 0; CLI help exited 0 and invalid mode returned expected exit 1.

Registry enumeration is not a transaction or DLL-integrity proof; native code
trust remains the existing explicit-selection model. Caller error estimates are assumptions, not new
driver accuracy measurements. Actual SDK bridge/link/MSVC/driver switching and
physical output require Windows and remain unverified on Linux. Cross-backend
selection, full-rate conversion and complete player acceptance remain pending.
