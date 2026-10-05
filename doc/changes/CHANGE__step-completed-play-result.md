# Completed stepped play result model

StepGameplay now retains its first CompletedPlayResult only after successful
finite/unlimited output completion. StepLocalGameplay retains one result per
actual member after shared cohort completion, using each member's gauge rather
than the control owner's unrelated gauge. New owners start without a result.

The read-only Copy/Eq model preserves FullSong versus PracticeSection start/end,
the exact gauge snapshot and Cleared/BelowClearThreshold/Failed(reason) outcome.
Latched numeric failure takes precedence over level. whole_song_clear rejects
practice even when its completed gauge reaches the clear threshold. Getters
distinguish unknown local members from members awaiting completion and remain
readable after later technical errors.

The original output validation/drain logic is unchanged. Duplicate completion
does not rewrite the first result, and errors before completion cannot create one.
No judge, gauge, score, capture, hash, frontier, replay-format or competition-prefix
mutation was introduced by classification.

Native publication, result UI/browser exports, archive integration and recorded
prefix result scope remain unfinished, as do configurable profile identity and
high-level mine admission. Actual execution and formal review/QA remain deferred;
the full player/engine Goal and task stay active.

Architecture status remains qualified: common stepped/native owners still import
the platform crate's pure PresentationDiscipline types. Native device calls use
the existing adapter boundary, but complete package dependency isolation and
layer-wide assertion coverage have not been established. Authored fixtures and
compile-only checks are not proof of complete layer separation or tested behavior.

Independent deferred fixtures add six groups: two result-model groups, two actual
solo completion groups and two actual local completion groups. They cover threshold
boundaries/failure precedence, whole/practice scope, real fatal operations and
ACK/output completion, per-member results, pending/partial ACK and invalid output,
and result history with preserved capture/hash/frontier. Existing fixtures are
unchanged. Both writers reported actual terminal Writes STOPPED before root
formatted only six Rust paths and checked whitespace. Assertions, parsers,
applications, generated bindings, hardware and formal review/QA were not executed.
All four authorized locked compile-only checks exited zero: workspace/all-targets
with webtransport, runtime no-default webtransport/all-targets, WASM browser and
WASM browser-audio. Existing unused strict-wrapper/playfield and WASM cadence
warnings remain visible; Windows/macOS compilation remains deferred.
