# Browser live judge timing

AC-201 replaces the Worker host's fixed 50 ms early/late and zero offset with
retained Window timing drafts. Live Play snapshots the settings once before
asynchronous setup and preserves the original synchronous audio resume gesture.
Existing busy owners lock editing; failed setup and stopped sessions retain
the draft values for retry.

The existing play-model component parses bounded decimal millisecond text with
at most six fractional digits through BigInt into exact nanoseconds. Early/late
are nonnegative signed-i64 values and offset admits the full signed-i64 range.
No fractional rounding, exponent or whitespace coercion is introduced. An absent
Worker configuration retains 50/50/0 ms defaults; malformed supplied fields
fail preflight before preparation.

The Worker forwards independently validated BigInts to the actual BrowserGame
constructor. Constructor/setup failures use the existing cleanup owner. Replay
ignores live settings and uses the recorded judge; saved-opponent and multiplayer
compatibility derive from the genuine resulting profile. No Rust, codec, storage
schema, protocol, dependency or clock source changes are made.

Six independent source fixture groups are authored: two exact numeric boundary
groups, two actual Worker routing groups and two Window lifecycle groups. All
existing groups are retained. Binding instrumentation covers routing, rather
than replacing the existing genuine Rust judge fixtures. Scoped whitespace is
the only verification performed in this slice. No JavaScript parsing, assertions,
tests, generated bindings, application/browser/audio execution, formal review or
QA was performed. Goal remains active and task remains open/PENDING.
