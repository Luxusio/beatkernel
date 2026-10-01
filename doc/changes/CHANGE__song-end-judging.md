# Immutable original-song judging boundary

Runtime can install one nonnegative song_end before its first committed
operation. The optional limit defaults to None, locks after successful setup,
and is cleared by session restoration. Exchanging shared transport/audio owners
preserves the limit. RuntimeGroup validates setup before configuring every
private member with the same boundary; SoloRuntime uses that same path.

Clock normalization, acquisition sequence, raw host/song chronology and reverse
segment checks precede fencing. Input strictly before the boundary retains its
ordinary binding, judge and keysound behavior. Input at or after it commits
validated acquisition without binding and advances the original judge only to
the end. Timers use the same cap. Capture records the actual Advance operation
at original-song time, without a new wire format or fabricated note completion.
Later operations cannot duplicate results. Logical song_end_reached does not
override judge errors or prove native presentation completion.

The logical end and Mixer playback-end frame are separate grids. Audio maps the
original endpoint to an upward-rounded frame; logical input admission uses the
exact original timestamp. Neither changes Transport history, pause or discipline.
Real Mixer/queue/RuntimeGroup/capture/ReplaySession fixture source covers
1/2/3/4/64 members with sparse and maximum player IDs, a pre-end hit, exact-end
exclusion, silent PCM suffix, repeated late operations and retained-prefix hash
equality. Core fixtures also cover clock/sequence/reverse errors, restoration,
configuration lifetime and authoritative judge failure. Fixtures are authored
and compiled only; no execution or native acceptance is claimed.

Known ceiling: native owners have not yet connected these setup options to
practice-region intent, physical endpoint presentation, input frontier draining
and terminal cleanup. UI loops still restart from observed position and can
overshoot or leave a reopen gap. Exact native/gapless looping, ASIO presentation,
network pause policy, browser integration, full controls and native/GUI/replay
acceptance remain required. Execution and formal close gates remain user-deferred.
