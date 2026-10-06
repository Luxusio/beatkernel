# ASIO live buffer and matrix replacement

Windows nonnetwork solo and local-cohort ASIO sessions now advertise live
buffer/channel-matrix controls through the existing common paused replacement
service. A static typed Windows request enum selects native opening; the shared
owner/controller still owns pause holds, epochs, Mixer recovery and publication.

Initial output captures the already trusted canonical driver registration,
registry view, selected channel order and explicit clock/error settings. Reopen
creates a same-thread host HWND and reuses that registration. Native preparation
is recoverable; failures preserve the original Mixer or the pending stream and
its HWND, including cleanup errors. Applied buffer metadata is optional until
an actual primed render report validates it; no placeholder count is advertised.

Portable ASIO mapping accepts driver-preferred/default or positive bounded
exact frame counts, preserves matrices during buffer-only changes and supports
explicit exact reset. It rejects duration buffers, period fields, another driver
and matrices whose target width differs from the fixed selected channel list.
Changing driver, channel selection or backend remains pending.

Capability UI now renders only explicitly advertised fields. ASIO shows buffer
and matrix; it does not invent editable WASAPI device/period rows. Empty
capabilities refuse panel creation without indexing an empty field list. Pure
mapper fixtures and retained panel apply/reply/scope fixtures cover this policy;
the Linux matrix fixture now advertises its field explicitly like the real
ALSA adapter does.

Independent code and security reviews passed, followed by scoped CLI QA:
runtime library 1,584 passed / 2 ignored, main 231 passed, Windows standalone
binary 33 passed, and the explicit actual ALSA live-matrix diagnostic 1 passed,
all with zero failures. The new mapper and retained UI/empty-capability fixtures
executed successfully. Workspace, WASM and Windows SDK-included/excluded Rust
all-targets checks exited 0. CLI help exited 0; invalid input returned the
expected exit 1. The SDK Rust check uses target-only cfg and native stubs while
leaving the real Cargo SDK build gate unchanged.

Native driver execution, C++ SDK
bridge linking, MSVC ABI and physical timing require the actual Windows/SDK
environment. Rust-only cross-source checks cannot substitute for those gates.
The full player remains unfinished; network/Watch and macOS cohort output
controls, broad backend/driver/channel selection and rate conversion remain.
