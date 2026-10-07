# Recorded BMS audio reconstruction

The separate BMS runtime validates a captured log against the matching chart and
stored profile through its replay loader, then plans sound commands from the
actual JudgeEngine results. Live runtime and replay use the same generic
SoundBinding hit-stage selection. Misses remain silent; normal prepared BMS
bindings sound Instant and HoldHead, while any additional stage needs an explicit
binding. The logical judge runs the stored operations, not synthetic autoplay.

Judge results carry effective time. Audio removes the stored profile offset once
to recover operation song time, then maps output origin + explicit nonnegative
preroll + song time with checked wide arithmetic. A command before output origin
or outside representable time fails rather than clamps. Background Play commands
are admitted only at/before the last recorded song time; empty logs have no BGM.
Equal-time BGM commands preserve prepared order and precede hit commands, whose
order follows actual judge results and sound bindings. Finite gains and referenced
assets are required. The plan exposes origin metadata, actual judge events and
final logical hash separately from audio execution.

Current audio-authoritative native captures normalize accepted Runtime inputs
and operations on the logical output timeline while retaining original HOST
acquisition provenance. Replay decodes each recording's own normalized domain
and setup; legacy records are not reinterpreted and this migration adds no wire
schema. A joined native fixture roundtrips two real hits through codec and
reconstruction with matching events, hash, gauge, EX and logical timestamps,
then verifies keysound identities/order under an explicit replay origin/preroll.
The 21 legacy capture/playback/native-feed integration regressions pass. These
development fixtures do not establish physical replay timing or final QA.

The offline renderer uses the actual core Mixer and shared PCM block writer.
Timestamp-to-frame selection rounds upward with integer arithmetic. It admits
only one target-frame group before rendering toward the next group; total notes
are independent of simultaneous queue and voice capacity. Capacity failures
return exact failed commands, and successful RenderReports remain separate from
admission. Blocks, bytes, frame extent and timestamp extent are checked. Frames at
or beyond the requested output extent are excluded. Zero frames validate the
recording but admit/write nothing. Logical counts/hash describe the full recording,
independently of output cutoff. Sample tails may continue beyond the recording's
last operation, but no later BGM or gameplay commands are invented.

`render_replay_bms` loads bounded WAV assets with an explicit sample rate/channel
layout, reads bounded replay data and writes newly created raw interleaved f32le.
Existing output paths are never overwritten. Render/write failure may leave a
partial newly created file. The renderer does not flush; the CLI flushes its sink.
Preroll defaults to three seconds and can be set to any nonnegative i64 duration.
Replay, block, command and voice limits are explicit.

Known ceiling: capture records logical judge operations, not original native
output scheduling points, PCM or failed audio admissions. This reconstruction
therefore reproduces selected song-time sounds, not original physical timing or
past dropped audio. BGM and PCM use the supplied prepared audio setup; the logical
judge fingerprint does not authenticate them. The
[native replay player](REQ__bms-native-replay.md) connects this plan to existing
output backends; actual native replay execution evidence remains pending.

Native Replay Watch pause keeps these planned command timestamps on the original
playback grid. Mixer silence advances only physical frames; rolling admission
uses completed playback frames and stops during pending/paused transitions.
Recorded operation progress stops at the acknowledged playback boundary and
resumes through the same ReplayVisual/JudgeEngine operations. No pause command
is added to the recording, and original live wall-pause history is not reproduced.
Assets are preloaded and
the finite command plan is allocated off-thread; source/log limits do not bound
all process memory. Executed development regressions provide the scoped evidence
described above. Actual native hardware execution, formal review and ordered
independent QA remain unproven; existing examples are not completion evidence.


## Explicit finite section planning

`prepare_section_replay`, `ReplayVisual::new_section` and `plan_section_audio`
accept validated finite metadata. Their legacy counterparts remain strict
unlimited consumers. Section preparation selects PCM directly from original
assets; scheduling subtracts original start and adds preroll once. Finite audio
planning excludes any command whose rounded output target is at or beyond the
exclusive configured frame fence, including a hit just before the logical end
that rounds onto that frame. Recorded judge results, input provenance and final
hash remain unchanged. Finite visual playback never adds a terminal advance.

The stepped owner requires actual render and presentation evidence at the fence,
finished records, complete command acknowledgements/feeder retirement and
actual consumed/applied counts equal to the admitted plan. Commands queued after
the retained fence cannot count as executed; accounting mismatch fails. This
is a finite fence rather than an idle-voice drain. Native/offline callers remain
strict until explicitly integrated; runtime acceptance is still pending.
