# Explicit gameplay host boundary

Shared solo/cohort policy must receive cancellation, pause requests, publication
and diagnostics through an explicit business-owned host interface. A generic
entry point supplies device, clock/wait control and host independently. Headless
and deterministic tests use the same actual loop with an explicit host, without
thread-local player service lookup, stdout/stderr or implicit UI attachment.

Move the shared pause state value out of the player implementation and preserve
the public `player::PauseState` path as a re-export. Publication receives actual
borrowed reports. Cancellation and pause intent are read-only command inputs;
pause acknowledgement and section-end signals retain their existing chronology.
Diagnostics carry typed borrowed business evidence, with formatting and terminal
I/O owned by the bridge. No strings, allocation or additional per-note traversal
are required merely to send diagnostics to a no-op host.

Keep existing public native and control-only call shapes as compatibility
exports backed by an outer bridge that selects the real system control and
legacy player publisher. The fully injected implementation cannot choose these
concrete adapters. Existing private test helper call shapes may retain explicit
compatibility wrappers under `cfg(test)`; old fixture assertions remain unchanged.
Host publication failure remains a technical observation failure and preserves
the original committed report, gauge/capture/competition observer results and
actual Stop admission evidence. Do not skip fencing or later report observation
because the UI refused a publication. Local publication retains the existing
whole-batch call and ordering; no new per-member partial UI publication.

Cancellation/closure/diagnostic expiration do not become completed results.
No hidden default host inside fully injected entry points. No new crate, thread,
render loop or audio callback effect. Native launchers keep their current public
entry calls through compatibility composition.

Deferred tests inject explicit cancellation, pause requests and acknowledgements,
solo/local publication refusal and no-op diagnostics. Use real portable queue,
Mixer and capture paths with runtime profiling disabled and virtual waits.
Compare repeated reports, PCM, captures and hashes independently of an ambient
publisher, and retain partial/fatal report prefixes across publication failure.
This is a boundary increment; platform presentation types and network owner
coupling still require separate work. Test execution and formal QA remain deferred.

The legacy native `LiveCompetition` and `NativeGroupCompetition` implementations
still include network operations, presentation publication and native timing
internally. Injecting a host alone cannot make those concrete adapters pure.
The host-boundary fixtures use absent competition owners; the subsequent
[competition-port boundary](REQ__gameplay-competition-ports.md) also supports
generic sessions containing populated deterministic observers. The final
architecture still requires separation inside the comparison/IO adapters and
of platform presentation types.
