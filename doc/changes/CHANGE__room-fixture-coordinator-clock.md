# Keep the shared room coordinator clock monotonic in protocol fixtures

Twenty-one room driver/publication/snapshot/actor tests shared an invalid setup:
the first participant's ClockReady receive advanced the coordinator to 10803,
then the second participant requested its ClockReady at 10800. The proposal loop
similarly received participant one's Accept at 11090 before requesting participant
two's proposal at 11001. The coordinator correctly refused both global regressions.

Stage the genuine protocol exchanges: collect both clients' decoded ClockReady
messages after their actual writes, then receive them at 10803. Collect both
written Accept messages, then receive them at 11090. Existing proposal times,
client timestamps, clock estimates, commit writes and one-shot permission checks
are retained. The single server reference clock is monotonic across participants.
Two start assertions now distinguish the server estimate's 17 ns width from
the client's actual 23 ns corrected round trip: client ping at t0, remote receive
at t0+7, remote pong construction at t0+13 and local receive at t0+29 yield
29-(13-7)=23. StartSchedule retains that client estimate; it is not copied from
the server's independently measured exchange.

The final upload/drain case uses timed relay write admission during its real
aggregate-final-ACK delivery loop. This retains the actual server capture floor
required by subsequent DrainReady. The corresponding DrainReady receive also retains its original server read
capture at 19003. Untimed polling/reception intentionally cannot establish
coordinated readiness. All final write/ACK and terminal success assertions remain.

No production clock or protocol guard is relaxed, and no start/ack success is
fabricated; every message still passes the actual codecs and client/server owners.

This changes only a shared test setup. Subsequent start, final write/ack, drain,
projection, actor/cache, and ownership assertions are executed against the real
common protocol implementations. It is not network socket, QUIC/WebTransport,
TLS or physical clock synchronization acceptance. The broad task remains open
with formal QA and other functional work pending.

Verification (2026-10-06): full runtime library with webtransport reports
1556 passed, 12 failed versus the preceding 1535/33 baseline. All twenty-one
shared room driver/publication/snapshot/actor failures resolve, with no new
failing names. Subsequent assertion fixes preserve distinct client uncertainty
and timed relay admission/reception. The full command still exits 101 for the
remaining failures; this is not full-task or native network QA PASS.
