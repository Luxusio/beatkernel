# Acknowledged terminal competition prefix

The shared solo competition owner now attempts to deliver its exact last
observed score prefix after native audio/input cleanup. It bypasses the50ms
periodic publication throttle, admits an ordered terminal frame and waits
boundedly for a matching peer acknowledgement before stopping the owned socket
worker. Failure is reported explicitly; local judgment and replay capture
remain independent of peer reports. An unobserved session has no fabricated
terminal progress. Linux ALSA, Windows WASAPI/optional ASIO and macOS CoreAudio
already invoke shared finish after native cleanup, so the wait adds no work to
audio callbacks or the input/advance path.

The repository-owned BKMP wire protocol is version2. Terminal progress shares
the existing cumulative validation and sequence space; its acknowledgement
identifies the exact terminal sequence. Separate peer-terminal storage retains
that distinction after ordinary progress. A peer can still be playing when the
local session ends; receipt means that endpoint accepted this self-reported
prefix, not that both players completed the chart. Cancellation and errors may
also end at a valid prefix. The worker prioritizes a single pending peer ack
between complete frames and writes an already-pending peer ack before exposing
local acknowledgement success. It never interleaves a partial frame.

## Known ceiling

Version1 peers are incompatible with this wire version. Delivery can fail on
queue pressure, protocol errors, disconnect, cancellation or the configured
I/O stall timeout. A peer acknowledgement is not authenticated score verification,
a ranked result, persistent storage or a guarantee of receiving a peer's later
terminal prefix after local shutdown. Peers still start independently; synchronized
start, network pause, local network groups and automatic network practice loops
remain unfinished. Transport-state/framing/lifecycle and actual Runtime terminal
prefix fixtures are authored for later execution. Compilation alone does not
establish socket, device or formal acceptance; these remain deferred by the user.
