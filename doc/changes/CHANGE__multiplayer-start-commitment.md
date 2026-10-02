# Committed multiplayer software start

Both participants now finish clock sampling before admitting a common software
start. Wireversion5 adds clock-ready(tag8, empty), host proposal(tag9, i64 elapsed
host nanoseconds), join acceptance(tag10, exact proposal echo) and host
commit(tag11, same echo). Versions1..4 are incompatible. The host publishes a
schedule only after the complete commit frame is written; the join participant
publishes one after receiving that exact commit. Ordinary and terminal progress
admission requires a retained schedule. The socket worker performs negotiation;
the existing native startup owner continues input draining and cancellation until
the committed local target, then releases the existing audio-start call.

The checked scalar model rejects wrong roles/order, duplicates, wrong echoes,
negative or regressing observations, stale/future estimates, excessive interval
width, overflow and insufficient remaining lead. Join deadline conversion retains
the sampled offset interval, checks its earliest endpoint and chooses its midpoint
for release. Both schedules retain the actual selected interval width. Failed
transitions preserve the previous model state. An in-flight frame must finish
before a following transition is admitted; partial writes cannot commit a start.

Defaults are2000ms lead,100ms minimum remaining lead,5000ms maximum clock estimate
age,100ms maximum uncertainty width and25ms maximum gate release lateness.
The library exposes nanosecond policies; native CLI extraction accepts
`--mp-start-lead-ms`, `--mp-start-min-lead-ms`, `--mp-clock-max-age-ms`,
`--mp-clock-max-uncertainty-ms` and `--mp-start-max-lateness-ms`. Timing flags require
a network role, reject duplicates and check conversion/policy bounds. Zero
uncertainty and zero permitted lateness are valid strict policies. The existing
overall startup timeout still applies, including the target wait; callers must
allow time for their selected lead. Offline and ghost-only startup skips this wait.

Inline fixtures prepare later verification of signed/asymmetric two-peer clock
offsets, full-write barriers, exact echoes, transition rollback, chronology,
estimate freshness, deadline margins, integer boundaries, fragmented wire frames,
owner retention, configurable flags and release boundaries. Execution and formal
acceptance remain deferred under the existing user instruction.

Compile-only evidence: workspace all-target `cargo check --locked`, runtime
all-target checks for `x86_64-pc-windows-gnu` and `x86_64-apple-darwin`, runtime
all-target `--no-default-features`, and the `wasm32-unknown-unknown` runtime library
with `--no-default-features --features graphics` all exited0. Scoped rustfmt and
`git diff --check` completed. Existing macOS block0.1.6 future-compatibility and
WASM audio-cadence dead-code warnings remain. No tests, socket/native device
execution, formal review, QA or acceptance gates were executed.

## Known ceiling

Known ceiling: A committed software start does not establish hardware output-zero
or sample-accurate song synchronization. Preroll differences, backend start latency,
device clock drift and OS scheduling still require explicit alignment and runtime
measurement. Gate lateness is measured before returning to the caller; additional
scheduling before the audio-start call is unbounded. Host completion of a commit
write cannot prove the peer received it; a disconnect can leave asymmetric starts.
No authentication, distributed atomicity or physical timing guarantee is claimed.
