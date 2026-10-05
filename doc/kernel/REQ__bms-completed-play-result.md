# Completed BMS live play results

A clear/fail result requires the actual owner's successful terminal gameplay and
output completion evidence. Loading, gauge.can_clear(), a numeric fence, Stop ACK,
cancellation, explicit diagnostic wall cutoff, cleanup success or a technical
error cannot create a completed result. Keep the gauge's threshold predicate
separate from successful output completion.

Build a small application result model before UI/archive integration. Retain the
exact gauge snapshot and distinguish FullSong (start zero, no explicit endpoint)
from PracticeSection with the original start and optional endpoint. Nonzero-start
unbounded practice is still practice. Preserve actual numeric failure reason;
otherwise classify Cleared at or above the configured threshold and
BelowClearThreshold below it. A practice threshold classification cannot claim a
whole-chart clear. Expose an explicit whole-song-clear predicate.

Only internal owner completion paths construct the model; expose read-only result
data and getters without an arbitrary public completed/result setter. Do not
observe judge events again, mutate score, append synthetic misses, alter gauge,
rewrite captures or modify hashes/frontiers to classify the result. Existing
replay formats and competition score prefixes remain unchanged.

StepGameplay stores no result until observe_completion returns successful true
from existing finite/unlimited output evidence. StepLocalGameplay then stores
each member's own gauge result only after the shared cohort completion succeeds;
never substitute control-primary gauge or a cohort aggregate. Preserve the first
result on duplicate successful completion observations. If a later output or
owner error occurs, preserve the already completed historical result as evidence,
while returning the original technical error; an earlier failure cannot create it.
New owners/restoration do not inherit another session's result.

Independent deferred fixtures cover below/equal/above threshold and latched failure
precedence, whole versus finite/nonzero-start practice, actual solo/local fatal
operations and output drain, healthy survivors and all-failed cohorts, early/idle/
presentation/ACK barriers, invalid output/partial ACK before completion and
duplicate observation preserving gauge/hash/capture/frontier/result identity.

The [native result contract](REQ__native-completed-play-result.md) connects
actual live/cohort completion to typed results and atomic retained publication
through injected host callbacks. Cancellation, device closure and diagnostic
cutoffs do not create a result. Publication refusal after completion carries
immutable result evidence with the original technical cause. The full feature requires
retained result UI/browser export and durable archive integration. Recorded-prefix
replay completion is a distinct scope and must never infer whole-chart clearance
from the end of a captured prefix. Configurable gauge profile identity, legacy
capture compatibility and high-level mine admission remain unfinished. No source
or compile check proves hardware/browser/performance or final player acceptance;
actual execution and formal review/QA remain deferred. Goal stays active.
