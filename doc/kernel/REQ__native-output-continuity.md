# Native output software continuity

Status: implementation in progress; independent review and QA pending.

An output owner retains its unique Mixer, prepared converter, generated target
PCM and positively admitted prefix together. Stop/recovery/reopen must transfer
that complete state after proven native retirement. A subset take that would
lose retained state refuses without consuming the owner. The original failure
and the latest cleanup diagnostic remain separate.

Generated, admitted and presented frames are distinct. Successful rendering
records pending PCM before encoding can fail. A validated positive native write
advances admission before later clock/timing/counter operations can fail.
Zero/no-progress writes never advance the prefix or rerender the pending block.
Same-interpretation reopen submits the exact suffix before fresh source pulls,
even when the new period is smaller or larger. Incompatible rate, channel,
matrix or encoding/layout changes refuse while preserving retryable state.

The new stream's frame basis identifies its first unsubmitted frame; native
counters start at zero. The advanced Mixer pull frontier cannot replace this
basis. Old reports do not become new callback, pause or endpoint evidence.

Paused replacement can drain retained output only when the actual stored report
proves paused silence, zero playback frames and the unchanged frozen playback
cursor on the supported equal-rate path. An active tail under a held replacement
refuses while retaining ownership. Existing pause frontier guards remain;
publication waits until the earlier tail reaches that frontier and fresh
nonempty paused rendering and actual native observations qualify the new stream.

The first integration uses the actual ALSA pump and application replacement
controller. Production static operation seams support scripted short writes,
encoding/wait/timing/clock/stop failures and exact PCM/frame oracles. Rendering
and pending replay use bounded prepared storage, static dispatch and no callback
allocation, destruction or locks. Native details stay in adapters; business
policy consumes software ownership and admission facts.

Implementations reuse existing ownership carriers and lifecycle policy. Keep
one retained state representation instead of per-exception orchestration or a
generic history/snapshot framework. Freeze shared API signatures first, then
develop independent native and application lanes in parallel with disjoint
source/test ownership. Run compiler/tests after relevant writers finish; review
and QA cover the combined result.

This software guarantee does not recover already admitted but unheard device
frames discarded by native stop. Acoustic gaplessness, unequal native rate
mapping, other backend migration, physical latency/performance measurements and
the complete BMS player remain required Goal work. Preserving refusal does not
mark those outcomes complete.
