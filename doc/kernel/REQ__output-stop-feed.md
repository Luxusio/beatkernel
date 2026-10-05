# Mapped output Stop command feeding

The existing BgmFeeder is also the bounded supply path for native/Worker replay
audio plans. Its song-time BGM constructor continues to accept only finite-gain
Play commands. Its from_output_commands constructor accepts finite-gain Play
and Stop, preserving exact full-width voices and already mapped output-domain
timestamps. Reject SetRate/Seek and every command before the declared output
origin, including Stop. Require zero extra preroll for mapped output; never add
song start, output origin or preroll a second time.

Sort mapped Play/Stop cues by timestamp and original ordinal. Equal-time order
is the caller's actual intended order, so an appended Stop follows that voice's
Play without a special command-kind sort. Apply the existing ceiling-to-frame
mapping, horizon, pending credits, admission budget, late rejection and cursor
chronology checks to both kinds. Stop receives no priority bypass or fabricated
render cursor. Queue refusal retains the accepted prefix and exact rejected
command, including a rejected Stop, with no implicit retry or rewind.

Expose BgmFeeder::admitted_stops() as a constant-time count of Stop callbacks
that returned success. Start at zero, increment only after actual callback
success, leave a rejected Stop uncounted, and retain the count after credit
retirement and empty/repeated feeds. Keep BgmFeedReport's existing public shape.
The count is callback-admission evidence: a native queue callback can prove queue
admission, while a Worker prepared-batch callback does not prove Worklet
acknowledgement. Never substitute the count for mixer execution, acoustic
silence, presentation, complete drain or successful clear/fail outcomes.

Independent deferred fixtures use actual command queues and Mixer output to
cover equal-time order, inactive Stop diagnostics, partial refusal/explicit retry
without duplicated accepted commands, sparse full-width voices, negative origin,
subframe ceiling and config/chronology/command-kind rejection. Default BGM stays
Play-only, and the strict generic render validators remain unchanged.

The [replay gauge-failure audio plan](REQ__replay-gauge-sound-stop.md) uses this
component for its mapped Stops. The
[offline owner](REQ__offline-gauge-sound-stop.md) tracks actual local admission;
the [stepped replay ACK owner](REQ__step-replay-stop-ack.md) distinguishes actual
remote Stop prefixes from prepared feeder callbacks. The
[stepped live/local owner](REQ__step-live-stop-ack.md) uses that same ACK component
for its shared remote output. The
[native recorded player](REQ__native-replay-stop-evidence.md) uses actual producer
callback admission with explicit owned cursor validation; the
[native live/local terminal path](REQ__native-failed-terminal-readiness.md)
uses its own actual producer evidence and numeric-fenced readiness. Final clear/fail
and actual full-player acceptance still require integration and verification.
Mine admission stays guarded.
