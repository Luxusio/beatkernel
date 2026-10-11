# Recorded BMS audio reconstruction

The separate BMS runtime validates a captured log against the matching chart and
stored profile through its replay loader, then plans sound commands from the
actual JudgeEngine results. Live runtime and replay use the same generic
SoundBinding hit-stage selection. Misses remain silent; normal prepared BMS
bindings sound Instant and HoldHead, while any additional stage needs an explicit
binding. The logical judge runs the stored operations, not synthetic autoplay.

Judge results carry effective time. Legacy audio removes the stored profile offset once
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

## Explicit pitch-changing output speed

The opt-in rate-aware planner/renderer entry points use a positive core `Rate`;
existing entry points retain normal1x delegation. Their original song-to-output
delta is inverse-scaled with checked rational arithmetic, then added to output
origin and unscaled output preroll. Nanoseconds truncate toward zero before the
existing upward frame rounding. Recording operations, original timestamps,
profile/section identity and logical judge results/hash remain unchanged.
Device sample Hz and requested output seconds remain separate from speed.

Non-normal plans put one actual SetRate at output origin before equal-time sound
commands. It changes Mixer sample readheads, counts as an ordinary queue admission,
and grants no Stop allowance or extra capacity. The section-aware exclusive fence
also applies to that control; a section quantized to zero output frames contains
no commands. Default/native consumers and the strict offline finite-metadata
refusal retain their existing admission contracts. CLI speed is explicit
`--speed NUM/DEN`, defaults1/1, normalizes positive checked decimal components
and rejects malformed values before file access.

The [scoped speed contract](REQ__replay-render-playback-speed.md) owns exact
API/rounding/queue/error semantics and verification. Its linked change record
contains independent DEEP code/security PASS and actual CLI QA:40 related tests,
54 optimized command paths, unchanged recording/hash and literal PCM. Final
documentation review and task close follow that pre-close snapshot.
No native/browser real-time rate control or pitch-preserving
DSP is implied by offline output speed.

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

## Real-time fixed replay speed integration

The [real-time replay speed contract](REQ__realtime-replay-speed.md) defines
the ongoing common/native/browser integration. Selected positive speed affects
both actual planner/Mixer readheads and genuine presentation-to-original-song
mapping. Preroll and acknowledged pause gaps remain output durations. A finite
original end caps wide song time before narrowing; it cannot hide an uncapped
numeric error. Legacy constructors remain normal-speed wrappers.

Quantization can leave presented song time just short of the original end,
including a zero output-frame fence. Only exact executed/admitted command
accounting, completed feeder/ACK retirement and genuine source presentation or
original converted target crossing authorize processing remaining original
records up to that original end. This adds no advance beyond the validated
original end, fabricated record completion or host-clock substitute. The
records-independent `FiniteReplayCompletion::output_finished` exposes that
existing output authority to native integration; final completion still
requires actual finished records. Full real-time integration and independent
runtime acceptance remain pending.
