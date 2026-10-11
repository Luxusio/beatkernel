# Rational pitch-changing replay output

Status: planner, renderer and CLI implementation have coordinator development evidence
in the [change record](../changes/2026-10-12-replay-render-playback-speed.md).
Independent DEEP code/security reviews pass; QA is pending. This implements part of original WBS05.13 without marking
its remaining real-time/native/browser acceptance complete.

The replay sound planner and offline renderer accept an explicit positive core
`Rate`. New `plan_audio_with_rate` and `plan_section_audio_with_rate` APIs append
the rate to the existing planner arguments. `render_replay_with_rate` accepts it
before the output writer. Existing APIs delegate with `Rate::NORMAL`, preserving
their signatures, normal PCM and command counts. No required report field or
recording format changes. Zero/reverse playback rates are rejected.

Selected original song time maps to output origin plus output preroll plus
`trunc_toward_zero((song - section_start) * denominator / numerator)` nanoseconds.
The stored profile offset is removed once before this mapping. Preroll remains
an output duration; device sample Hz and requested output duration remain separate
from speed. Use checked quotient/remainder arithmetic to avoid a needless wide
intermediate overflow, then reject unrepresentable timestamps rather than clamp.
Nanosecond quantization precedes the existing upward output-frame rounding;
fractional nanoseconds or arbitrary exact fractional-frame positions are not
promised. There is no accumulated floating-point timing or host-clock mapping.

Original replay operations, song timestamps, header/profile/section identity,
judge events and final judge hash remain unchanged. Asset preparation and section
cropping remain in the song domain. Actual BGM, keysound, input-selected, mine and
gauge-stop commands use the shared original sound-selection logic and rate-aware
timing. Non-normal plans prepend one actual `SetRate` at output origin before all
equal-time sounds; original BGM-before-gameplay and admission ordering remain.
The Mixer applies that control before its first sample, changing actual sample
readheads, including long BGM. It counts as an admitted command and grants no
Stop allowance or extra queue capacity. Normal plans add no control.

The section-aware planner uses the same inverse mapping and frame rounding for
its exclusive endpoint. Every command, including SetRate, at or beyond that
endpoint is excluded. A positive section may quantize to a zero-frame fence at
an extreme speed; it then has no admitted plan commands, without a minimum-frame
clamp. Legacy offline rendering and CLI still reject finite replay metadata;
this change does not silently expand that existing admission rule.
Other policy-aware metadata retains its existing legacy refusal too. Tests of
nondefault gauge policies use the existing section-aware consumer; changing
output speed does not authorize a new recording-policy consumer.

The actual renderer uses the existing queue, Mixer, PCM writer and error evidence.
Requested frames describe output extent, not song extent. Zero output validates
but admits, writes and flushes nothing. Capacity or writer failures preserve
original actual prefix/RenderReport evidence and do not produce success. Original
resource limits remain; speed never silently raises them. All mapped targets are
checked before the first sink write, including targets excluded by output cutoff.

`render_replay_bms` adds unique `--speed NUM/DEN`, default `1/1`. Each component is
nonempty unsigned ASCII decimal; numerator is1..i64::MAX, denominator1..u64::MAX.
Fractions are reduced by the core Rate constructor. Invalid, signed, zero,
overflowing, malformed or duplicate values fail before file access. `--rate`
continues to mean device sample Hz. Help and successful output expose normalized
speed and pitch-changing behavior. Output uses existing create_new/no-overwrite
policy; later planning or rendering failure may leave an empty or partial new
file. Speed validation cannot replace original chart/asset/replay checks.

Independent tests must compare literal planner times and actual nonconstant
keysound/BGM PCM at1/2,1,2 and3/2, with unchanged logical replay evidence. Cover
normal byte parity, normalized ratios, explicit output preroll, signed offsets,
section starts, frame quantization/ties, finite zero fence, queue refusal from
the added control, zero/cutoff/block-size independence and writer failure.
Use original owned-Stop regressions and actual optimized CLI output artifacts,
invalid paths and output collisions. Fixture helpers supply setup, never timing
or PCM expectations. Tests/builds alone do not replace direct CLI execution.

## Known ceiling

This is offline pitch-changing output of selected recorded sounds. It does not
reproduce original native scheduling or past dropped audio, implement real-time
speed switching, expose native/browser speed controls, provide reverse playback
UI or pitch-preserving DSP, or prove physical performance. Full WBS05.13 and
the player Goal retain their original outstanding requirements. The existing
[replay audio contract](REQ__bms-replay-audio.md) remains the baseline for legacy
and native consumers.
