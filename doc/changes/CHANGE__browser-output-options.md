# Browser output context options

AC-207 connects retained latency and requested sample-rate controls to the
actual AudioHost context constructor. Both live and replay capture one immutable
output selection before audio setup. AudioHost independently validates and
clones the optional constructor preferences. Existing callers keep interactive
latency and automatic rate defaults. Invalid selections and unsupported browser
requests fail without a silent default retry.

Actual opened context rate remains the preparation and output clock source.
Replay retains its recorded judging and section. These settings last for the
page lifetime, lock during busy operations and remain after failure or Stop.
The [Web Audio options](https://www.w3.org/TR/webaudio/#dictdef-audiocontextoptions)
express browser preferences; no fixed callback size, native backend control or
measured acoustic latency is claimed.

Independent fixtures are authored for bounded admission, immutable Window
snapshots and the actual AudioHost constructor boundary. JavaScript parsing,
tests, browser/audio execution, generated bindings, formal review and QA remain
deferred. Rust is unchanged; previous compiler checks do not validate this JavaScript.
The full Goal remains active and the task open/PENDING.
