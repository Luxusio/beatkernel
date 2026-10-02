# Multiplayer session clock sampling

Network playback now waits for a software clock estimate as well as bilateral
native preparation. Each socket worker performs eight sequential ping/pong
exchanges using elapsed monotonic session nanoseconds. Wire version4 adds tags6
and7 with fixed16-byte request and32-byte response payloads, including sequence,
echo and receive/send timestamps. Versions1/2/3 are incompatible. Audio callbacks
and the main UI do not perform this exchange; the existing startup owner keeps
servicing native input and cancellation before audio starts.

The checked scalar model retains the remote-minus-local interval
`[t2 - t3, t1 - t0]`, assuming nonnegative directional transport delays and a
constant offset during a sample. Its width is the local round trip minus remote
processing. It chooses the smallest corrected round trip, preferring the newest
equal-delay sample. This interval follows from the four-timestamp relationships
described by [RFC5905 section8](https://www.rfc-editor.org/rfc/rfc5905.html#section-8);
it is our inference under those assumptions. BeatKernel rejects negative
corrected delay rather than adopting RFC delay clamping. This is a session clock
model, not an NTP client. Integer arithmetic uses i128 intermediates and checked
nonnegative i64 timestamps. Remote deadline conversion preserves both endpoints,
rejects stale or future observations, and requires the earliest possible deadline
to remain strictly after the local present.

Authored fixtures cover fragmented frames, simultaneous probes, eight-sample
selection, malformed/unsolicited/mismatched responses, retry state, retained owner
estimates, asymmetric delays, processing time, signed offsets, chronology,
observation age,20-hour/week spans and integer boundaries. Fixtures are prepared
for later execution; compile checks do not establish timing accuracy.

Compile-only evidence: `cargo check --workspace --all-targets --locked`, runtime
all-target checks for `x86_64-pc-windows-gnu` and `x86_64-apple-darwin`, the
runtime all-target check with `--no-default-features`, and the
`wasm32-unknown-unknown` runtime library with `--no-default-features --features
graphics` all exited0. Scoped rustfmt and `git diff --check` also completed.
The existing macOS block0.1.6 future-compatibility warning and WASM audio cadence
dead-code warnings remain. No fixture, socket, application, device or formal
review/QA execution was performed for this change.

## Known ceiling

Known ceiling: Software encoding/parsing timestamps include scheduling and
buffering. No drift bound, hardware timestamp accuracy, clock discipline,
authenticated timing or measured physical audio synchronization is established.
Common-start commitment, preroll agreement and native audio deadline scheduling
remain future work. Socket/device execution and formal acceptance stay deferred
under the user's existing instruction.
