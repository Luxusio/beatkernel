# Competition presentation policy and ports

Competition presentation payloads belong to the business layer, not the player
UI implementation. Move GhostSnapshot, NetworkStatus, NetworkSnapshot and
CompetitionSnapshot to a pure presentation module; preserve player::* paths as
re-exports and their exact field shapes/traits. Keep peer progress explicitly
self-reported and display-only; publication never enters judgment or completion.

Pure policy defines a narrow host contract for attachment, an explicit monotonic
publication clock and saved/solo/group publication. A retained cadence uses
one stable publication-clock origin; callers must not switch unrelated host
origins while reusing it. This clock has no input/output or judgment authority.
Actual native competition
publication delegates to that policy. Native UI lookup, Instant acquisition and
publication effects belong in a separate bridge. Expose explicit host-injected
publication/observation paths where needed; native compatibility entry points
select the bridge. Do not hide concrete UI access inside a supposedly pure port.

Preserve the 50ms non-forced publication cadence, forced status transitions and
unattached/room suppression, whole-group atomic publication, last remote prefix
on disconnect, remote roster/order validation and bounded sanitized ghost labels.
Do not allocate/build snapshots on a suppressed publication. Advance cadence
only after successful publication, anchored to the post-effect clock read as
in the existing native implementation. A pre-effect timestamp cannot make a
slow publication shorten the following interval. If the post-effect clock
fails/regresses, preserve the prior cadence and report the failure; the already
performed publication cannot be rolled back or represented as unperformed.
Use checked explicit monotonic values, reject
clock regressions without fabricating a progress timestamp or updating cadence.
A failed publication preserves already observed local competition progress and
original errors. Preserve native cleanup error precedence and old assertions.

Independent deferred memory fixtures use fake hosts and original payload values:
exact cadence boundary, forced/unattached suppression, publication refusal and
retry, backwards clock, basename sanitization, whole-row validation, retained
peer prefixes and preserved player ID ordering. No OS clocks/network/UI/renderers
are needed to test policy. Existing native compatibility tests remain unchanged.
No extra crate/dependency or per-note dispatch is required.

This increment separates competition display policy and effects. Native network
ownership, preparation/storage, diagnostic terminal output and setup waiting still
need further separation; injecting display alone does not make native competition
owners pure. Assertions, formal review/QA, drivers and benchmarks remain deferred.
