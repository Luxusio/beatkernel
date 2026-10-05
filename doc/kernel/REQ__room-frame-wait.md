# Common incomplete room frame waiting

RoomFrameWaitState uses RoomDeadline for a fixed 1 ms..120 s bound per incomplete
frame. Idle has no deadline. The first incomplete prefix observation starts one
bound; later fragments never renew it. Negative/regressing observations refuse.
An active deadline is checked before a complete-frame observation can clear it.
Failures seal state; only a timely complete frame allows a subsequent independent
frame to start a new bound. State owns no clocks, timers, IO or allocations and
cannot be cloned/reset to renew authority. Wait returns whole remaining time.

The split-operation driver configures once while decoder empty; it checks the
existing deadline before accepting any later prefix into the decoder, using the
supplied original processing observation. Late data cannot advance session,
revision or peer-token state. Successful prefix processing updates waiting from
real decoder state; idle queries and malformed bytes do not create deadlines.
RoomPlayError::FrameWait retains typed first refusal; actual driver lifetime
failure guards prevent retry success. Close releases wait state. A complete
prefix supplied before expiry is not a claim that parsing/callbacks finish before
expiry; native IO has its existing separate processing observation available.

RoomPlayIo uses the same state, checking its existing polled/captured/processing
observations before IO or protocol dispatch, without extra clock samples. Setup
configures the decoder while empty. Native RoomNetworkOptions gains frame_timeout,
default 10 s with existing 1 ms..120 s bounds. The native room actor configures IO
before stream servicing. Actual app composition forwards io_stall_timeout as
frame_timeout; partial fragments cannot keep a native connection alive forever.
Existing IO failure precedence, actual completion credits and cleanup stay owned
by the original ports; no timer/loop/dyn allocation is added to a gameplay hot path.

The WASM facade configures bounded frame waiting and returns -1n idle or positive
nanoseconds pending. Typed expiration maps to timeout/frame diagnostics. Worker
frame timers wake Rust rather than declaring timeout. Receive entry guards reject
expired final fragments even if a timer has not run. Timely completion clears the
timer; later frames have independent bounds; leave/close/abort/drain clears pending
waiting. Actual capture times, write IDs and complete-write receipts remain intact.
The Window gains no domain or rendering work.

## Known ceiling

Existing transport read/connect timeouts remain IO bounds. Frame policy measures
provided observation points, not acoustic accuracy or CPU completion time. Native
and browser orchestration, full BMS features and measured performance remain work.
Pure/native/adapter fixtures are authored for later execution; no test assertions,
JS parsing/generation/browser, real network/hardware timing, formal review or QA
acceptance is inferred from compile-only checks. Full Goal remains unproven.
