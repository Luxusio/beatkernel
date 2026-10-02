# Native physical-frame startup foundation

The kernel now provides an initially held command queue for silent device
calibration before playback. A producer arms one immutable physical start frame;
the mixer leaves preceding frames silent and starts playback frame0 at that exact
frame, including inside a render block. Physical frames advance while commands,
voices, playback time, rate and seek state remain held. A missed frame rejects
rather than silently moving playback. The producer can observe the actual first
positive playback frame. Default queues retain their existing behavior; ordinary
pause remains independent. A pause intersecting an armed first-playback interval
rejects explicitly rather than moving the start. Invalid and empty render
preflight do not consume commands or adopt the start.
Arming is independent of command-ring capacity. Producer admission checks the
consumer's published physical frontier; the callback checks again to reject a
publication race that missed the target. Disconnect before unarmed release is
an explicit failure. Applied-start evidence is published once only after positive
playback; a zero-length finite endpoint does not acknowledge a start.

The application adds checked session/native-host bracketing and nominal host-to-
output physical-frame projection. Capturing a native host observation between
session clock reads preserves that bracket when converting a future target.
A committed schedule's uncertainty width expands its midpoint conservatively,
including odd integer widths. Projection preserves both endpoint frames, rounds
up to the sample grid, selects the latest endpoint and requires the earliest to
remain beyond the caller's render-ahead margin. Domain mismatch, stale brackets,
chronology, zero rate, late deadlines and arithmetic overflow reject explicitly.

These are bottom-up components for the next native startup integration. The
current Linux/macOS/Windows startup still waits for the software-call commitment.
The intended integration starts a held device, calibrates during silence, obtains
a future committed target, projects it into the native output grid, safely arms
the gate and anchors the transport to observed applied playback. BGM commands
remain on the playback grid; judgment timestamps cannot be moved independently
of actual PCM output. No platform API/device format choice or dependency changes.

Authored fixtures prepare real PCM crossing-block and partition checks, immutable
end fences, retained control commands, admission/disconnect, invalid/empty renders,
missed starts, domain/age/margin/quantization and long-span/overflow boundaries.
Tests and native/socket execution remain deferred under the user's instruction.

Compile-only evidence: workspace all-target check, runtime all-target checks for
WindowsGNU/macOS, runtime all-target without default features, and the WASM
graphics library check all recorded exit0. Initial native checks found a test
helper using u64 instead of ClockDomainId's u32; the helper was corrected and
the four affected native configurations were rechecked. WASM library source was
unaffected by that cfg(test)-only correction. Scoped rustfmt/diff checks completed.
Existing macOS block0.1.6 future-compatibility and WASM cadence dead-code warnings
remain. No fixture execution, native/socket use, formal review/QA or acceptance
gate was performed.

## Known ceiling

Known ceiling: A ClockPair alone supplies neither a drift nor an error bound.
Session-to-host and host-to-output conversion here assumes a nominal unit slope;
the retained software bracket is not a proof of physical clock accuracy. Sample
quantization is represented, but hardware latency and later drift are unmeasured.
Native callers are not yet switched to held calibration or physical-frame arming;
reported gate application is render evidence, not native/acoustic presentation.
Backend integration and later runtime/formal acceptance are still required.
