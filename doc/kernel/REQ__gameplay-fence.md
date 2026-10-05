# Committed gameplay fence

The portable Runtime must allow an application owner to fence gameplay at its
latest actual committed song frontier. Runtime::fence_gameplay returns that
Timestamp, latches it once and is idempotent. With no committed operation it
returns None and leaves setup untouched. gameplay_fence reports the optional
latched frontier. The kernel does not interpret BMS gauge values or decide why
a player failed.

After fencing, ordinary input/advance operations still validate original host,
song mapping, sequence and output-domain evidence and commit acquisition
chronology. Reports retain the frozen logical song position. Input reports keep
the actual normalized physical event, but perform no touch routing, binding,
judge push/deadline advancement, normal/press/hazard sound admission or fabricated
release. Keep judge state, hash, pending notes/hazards and held contacts unchanged.
No stale hazard report is republished. Ordinary unconfigured execution preserves
its existing behavior. The raw configured song-end observation remains separate
from the frozen frontier and does not prove presentation/audio drain.

Fencing must not flush/retry queued audio, replace a producer, pause output,
mark completion or change Transport. Explicit control audio remains available
for later owner-managed cleanup. Wrong-domain, regressed host/sequence or
reverse song mapping errors still reject atomically. Repeated fencing after
validated later acquisition does not move the original frontier.

Explicit paired gameplay restoration clears the fence together with chronology,
sequences and endpoint setup. The restoring application must reconstruct its
gauge/failure policy and refence a failed prefix when applicable. Mere clock
correction, transport access or producer exchange does not release the fence.

RuntimeGroup::fence_player selects one registered member, returning its optional
committed frontier without poisoning or fencing surviving members. Unknown IDs
reject before mutation. It does not clear an existing technical group poison;
committed evidence remains fenceable after a partial technical failure. Expose
player_gameplay_fence for observation and SoloRuntime::fence_gameplay /
gameplay_fence as direct solo facades. Preserve shared queue ownership and full
device/source identity. A failed local player can retain its acquired source
without aborting surviving players or fabricating new input.

Independent deferred fixtures must cover actual hold/hazard/button/touch state,
queued audio preservation, post-fence input and advancement, chronology errors,
idempotence, explicit restoration, and independent local source/member behavior.

Known ceiling: this component is explicit owner control. The automatic stepped
and native default-gauge calls are described in [BMS gauge ownership](REQ__bms-gauge.md).
The separate [scheduled gameplay sound stop](REQ__gameplay-sound-stop.md) adds
explicit per-runtime/shared-member audio control and the stepped failure hookup.
Broader replay failure policy, native audio-stop wiring, completion outcomes and
actual output-drain integration still need implementation. The fence
does not itself make mine-containing files playable. Keep the high-level mine
admission guard. Hardware/browser/performance acceptance requires later execution.
