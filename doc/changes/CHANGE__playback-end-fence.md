# Exact audio playback-end foundation

MixerConfig can fix an exclusive playback_end_frame before Mixer construction.
Default mixers remain unlimited. A callback crossing the endpoint renders only
frames before it, then fills the physical suffix with silence. Playback cursor,
voices, rational heads, pending commands, rate and song anchor freeze at the
endpoint while physical output/counters continue. Queue resume cannot lift this
immutable fence. Empty/invalid renders do not adopt it; zero end consumes nothing.

RenderReport retains its fields: playback_frames is the actual active prefix and
paused is the applied block-end state, including an all-active block ending
exactly at the fence. NativePause uses the playback end for acknowledgement and
still waits for real native presentation crossing. Repeated identical coalesced
partial reports preserve the frozen prefix; fresh active-prefix reports cannot
restart an already paused grid. Replay feeder validation accepts valid prefixes.
PracticeLoop maps matching original start, explicit preroll and nonzero output
rate with checked integer ceiling arithmetic, including long charts and subframes.

Known ceiling: this is the exact audio component, not completed native looping.
Graphical loops still use observed-position fresh restarts. Native owners must
connect endpoint intent, Transport/input/judging/capture admission and lifecycle
before exact native-loop claims. Frame rounding cannot provide subframe acoustic
precision and reopening leaves a gap. ASIO presentation, network pause policy,
browser/full widget host/native controls and full native GUI replay acceptance
remain open. Literal PCM, partition/manual-pause/fractional/zero/invalid/future
command and RT allocator fixtures, native boundary/replay composition and checked
long endpoint mappings are authored/compiled only; execution/formal gates remain
user-deferred. No queue command, native telemetry field or dependency was added.
