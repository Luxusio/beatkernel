# Competition connection acquisition

Competition connection requests own existing identity/player vectors and options
and borrow the requested role. A generic factory consumes an admitted request
once, preserving original ordered PlayerIds, vector allocations and opaque
factory errors without extra trait bounds. Policy imports no concrete player,
native endpoint, room owner, UI implementation or filesystem reader.

Policy rejection guarantees zero factory calls. Once admitted, factory failure
may follow partial native acquisition; cleanup belongs to that adapter. This is
not a resource transaction or proof that a refused connection had no IO. A
successful factory value proves acquisition only, not remote identity agreement,
readiness, physical audio synchronization or gameplay completion.

Complete preflight precedes factory effects: explicit room availability, bounded
identity, positive unique roster of 1..64, common timeout/queue/start/preroll
options, bilateral group-setup wire size, role-specific credentials/address or
WebTransport destination/Origin/CA metadata. Room identity retains its existing
size rule independently of the bilateral group envelope. Credential bytes and
TLS correctness remain adapter checks, not metadata policy claims.

Common MultiplayerOptions and NetworkRole values belong to policy, with old
import paths retained through re-exports. Existing common option and group wire
size validators are reused rather than introducing divergent limits. Validation
may strengthen early refusal in the outer competition constructor; invalid
requests acquire no certificate, socket, thread or room owner.

NativeCompetitionNetwork::new supplies actual graphical room availability from
the outer composition boundary, then uses the generic acquisition path with a
native factory. The factory keeps actual bilateral/room constructors and resource
ownership. This exposes acquisition to deterministic fake factories; concrete
owner internals, waits/ACKs and full live owner construction remain further work.

Independent fixtures cover effect-free refusal, wire-size/roster/options bounds,
actual role metadata, original ownership and opaque factory failures. Assertions,
real native IO/TLS/network/graphics, benchmarks and formal review/QA remain
deferred. Compile checks do not establish whole-player completion or reliability.

Eight cfg-dependent groups are authored and the four compile-only configurations
exited zero after both writers stopped. See
[the evidence scope](../changes/CHANGE__competition-connection-factory.md).
