# Browser local network Worker

The actual live-play Worker connects its local cohort through one negotiated
group Session and shared start. It validates member canonical identities,
publishes actual ordered member snapshots, binds accepted remote roster members
to local peer HUDs and retains final words before releasing the local game.
Optional explicit peer targets are bounded and frozen; the default pairs frozen
roster positions without substituting an unmatched member's score. Peer HUD
failure stays per member while the common transport and saved comparisons
continue. Periodic member score presentation remains on Worker.

Seven deferred groups were added to the actual Worker-module fixtures, bringing
the total to 75 while preserving all 68 existing groups. They cover capability
and target admission, shared identity/start, remote mapping, HUD isolation,
pre-disposal snapshots, write/ACK cleanup and cancelled/stale owners. The old
unsupported local-network fixture now tests invalid local peer-target refusal.
Scoped whitespace checks completed. No JS parsing, assertions or runtime was
executed, and Cargo checks were not repeated for this JS-only change.

Known ceiling: Page launch still refuses the local/network combination and
needs separate source integration. Native group callers, multi-host rooms,
generated bindings, actual devices/audio/transports, browser performance and
formal review/QA acceptance remain unfinished. Fixtures are authored for later
execution under the standing verification deferral; no full-player completion
is claimed.
