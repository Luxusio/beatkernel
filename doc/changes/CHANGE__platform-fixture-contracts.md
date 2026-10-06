# Align platform error and amplitude fixtures with existing contracts

Two worker-launch fixtures wrapped Box<ConcreteError> directly in io::Error.
That constructor stored the box itself as its concrete error payload, so a
subsequent downcast to ConcreteError failed before checking launch preservation.
Explicitly coerce to Box<dyn Error + Send + Sync> first. The original payload
pointer, value, IO kind, unique recovered mixer and no-work assertions remain.
The launch implementation and error forwarding behavior are unchanged.

The ASIO renderer fixture expected finite f32::MAX times finite gain to become
nonfinite PCM. The core mixer deliberately sums in f64 and clamps once to [-1,1]
(REQ__audio.md), so the expected nonfinite path was unreachable from its valid
inputs. The fixture now verifies actual wide-sum/clamp output: 0.5 as Int16Lsb,
1.0 as Float32Msb, exact render report/counters and next-frame silence.
Existing low-level ASIO PCM tests still inject NaN/infinity directly and verify
rejection before writing output planes. The renderer scratch guard remains
unchanged; this fixture does not claim to exercise that defensive branch.

Full `cargo test -p beatkernel-platform --locked`: exit 0, 190 passed,
0 failed, including 4 doctests. The previous baseline had 187 passes and the
3 failures corrected here.

These are pure platform tests. No ASIO SDK, driver or physical native-output
execution is established. The broad Harness task remains open.

Workspace all-target webtransport check: exit 0; existing library warnings remain.
