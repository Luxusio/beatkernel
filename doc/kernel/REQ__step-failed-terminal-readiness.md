# Numeric-failed stepped gameplay terminal readiness

An actually failed and committed-fenced player has finished gameplay even when
its judge retains unplayed notes, hazards and held state. Never synthesize misses,
purge those states or advance its frozen frontier merely to finish output. A
manual/control fence or a recoverable empty gauge alone is not numeric failure:
require both the member's actual gauge failure and its actual Runtime fence.
Technical owner failure still prevents successful completion.

Solo terminal readiness may use that numeric-fenced state instead of the normal
pending-note/hazard deadline. Local readiness checks each actual member: numeric
failure plus member fence, or the existing healthy criteria. Finite healthy
members must reach their configured end; unlimited healthy members must pass
the prepared judge deadline and complete every retained note and hazard. Healthy
members cannot be skipped because another member failed, including a failed
primary. All-failed cohorts must be able to finish with frozen song frontiers.

Add small internal SongCompletion helpers: judge_until() returns its immutable
prepared threshold; observe_terminal_ready(ready, bgm, rendered, presented) reuses
the existing drain and BGM-empty checks with caller-proven logical readiness.
Keep the ordinary public observe path, healthy solo readiness and its exact
deadline/hazard semantics unchanged. Do not create a second drain algorithm or
make a Stop ACK into physical presentation. Local caller proof includes every
healthy member's prepared threshold and note/hazard states.

Use this terminal path only after the stepped shared owner validates raw output
with actual acknowledged Stop evidence. Both finite and unlimited completion
still require no outstanding batch, an empty internal command queue, all BGM
fed/credits retired, and existing output evidence. Unlimited output needs a
subsequent idle render and presentation past its fixed drain target. Finite
output needs the immutable endpoint, exact acknowledged consumed/applied total,
empty pending output and actual endpoint presentation; numeric-fenced readiness
may replace the frozen song==end check only when every member is ready.
Keep producer/drain resets and invalid evidence rejection before frontier adoption.

Completion here means terminal gameplay plus resolved output according to existing
evidence, not successful chart clearance. Preserve failed gauge, raw judge/hash,
score, source provenance, actual capture prefix and frozen timestamps. Final
clear/fail result classification remains separate; the high-level mine file
admission guard stays until native owners and final results are connected.

Independent deferred fixtures cover actual solo and local fatal operations with
future pending notes/hazards, exact remote Stop ACK and real Mixer/presentation;
all-failed and failed-primary/healthy-survivor cohorts, future BGM and outstanding
batches/credits, early/equal/later idle barriers, finite endpoints/counts and
frozen frontier preservation; technical/invalid output and recoverable/no-mine
regression. Existing ACK fixture groups retain their evidence assertions, aligning
only readiness assumptions which this behavior intentionally changes. Assertions,
browser/device/performance and formal QA/review remain deferred.
