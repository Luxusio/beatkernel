# ASIO finite clock anchor renewal

The standalone ASIO replacement adapter retained one finite multimedia-clock
anchor forever. Unlike the existing Windows live path, it did not renew before
the supplied horizon expired. This is a dependency of connecting the Windows
gameplay loop to common output ownership.

Both paths now use the SDK-free anchor's checked half-horizon renewal decision.
A fresh native timer reading is bracketed by the same QPC clock; renewal retains
the caller's age, measurement and drift bounds exactly. Wrong domains, reversed
host chronology and invalid acquisition brackets fail instead of resetting the
clock timeline. Odd horizons round the halfway threshold upward; a one-nanosecond
horizon does not request repeated acquisition at an unchanged host time.

Native sampling stays on the control thread. Callback processing, rendered-block
evidence, output epochs, Mixer ownership and driver lifetime are unchanged.
Portable fixtures cover boundary/regression/domain rejection, unchanged original
anchor on renewal failure, preserved error bounds, and a simulated week of
renewals crossing the wrapping native timer. These fixtures do not prove native
driver accuracy or physical synchronization.

Independent code and security reviews passed, then scoped CLI QA passed:
platform 204 tests, runtime library 1,584 tests, and application main 223 tests
(2,011 passed, zero failures, three explicitly ignored diagnostics). Workspace
all-target and WASM browser-library checks exited 0. The focused multimedia
clock suite executed seven passing fixtures.

Windows application all-target Rust source checking, including the optional
ASIO Rust paths, exited 0 using target-only cfg flags and C SDK stubs. This
leaves the actual Cargo SDK build gate unchanged and does not prove a native
C++ bridge build, linking, MSVC ABI compatibility or driver execution.

Windows common owner/UI composition is still pending. It must preserve the
existing ASIO HWND release order and sysref message pump. Native SDK compilation,
linking and driver execution require the actual SDK/MSVC/Windows environment;
an isolated SDK Rust source check cannot substitute for those gates.
