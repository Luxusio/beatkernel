# Native live group competition

The native LiveCompetition application path uses one actual
GroupMultiplayer owner for its sole stable local PlayerId. QUIC and
WebTransport share the existing Session, clock/start and final-ACK cleanup.
Ordinary/final publications carry actual MemberProgress rather than scalar
protocol frames. Saved opponents remain independent of peer reports.

The accepted remote roster freezes the sole local member's ordinal-zero peer.
Only that exact member from a complete validated matching prefix may appear
in the native comparison HUD and terminal report; preserve original member
identity/song time and never aggregate or choose another row after failure.

Four independent fixture groups were authored for later execution: exact
ordinal-zero identity at 1/3/64 members, invalid later-row/roster atomic refusal,
canonical latest/final selection and genuine offline Runtime/capture invariance.
No assertions were executed. Scoped rustfmt and whitespace checks completed.
Rust 1.98.1 completed all four authorized Cargo check configurations with exit
code 0: workspace all-targets, runtime no-default-features all-targets, and
wasm32 library checks for browser and browser-audio. Existing wasm32 platform
cadence dead-code warnings remain; compilation is not runtime or QA evidence. Native local cohorts still need one shared
network owner and per-member presentation. Actual browser/native transport,
physical acquisition/audio and performance, multi-host rooms, independent
review/security and required QA remain unfinished. The full player Goal and
Harness task remain active/open, with no completion claim.
