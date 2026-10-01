# Graphical whole-song retry

The graphical player gains F5 and a Retry button while playing or viewing results.
The immutable SessionLaunch pins the accepted chart, native configuration, roster
and comparison options. A retry invocation is prepared and validated before
cancellation, then waits for old native cleanup and joining before a new channel
and game owner can start. A failed cleanup blocks automatic retry; explicit cancel,
focus loss, suspension and closing clear queued retries. Spawn failure retains the
previous joined results. Gameplay timestamps remain with native acquisition.

Recording attempts derive .retry<N>.bkr from the original file stem. For local
cohorts the existing native .p<ID>.bkr suffix follows that stem. Increasing retry
ordinals are checked; no recursive suffix, overwrite, existence query or inferred
filename fallback is introduced. Malformed/oversized invocations and exhausted
ordinals fail before cancellation. Original first-run filenames remain exact.

This action retries the entire song from its beginning. Arbitrary section starts,
hold reconstruction, PCM frame selection and native presentation synchronization
remain required work under the section-restart contract. A fresh calibrated
native composition alone does not prove acoustic repeatability.

Meaningful fixtures are authored and compiled only. Actual tests, GUI/GPU/input/
audio/device/file/network execution and formal review/security/QA/task close remain
user-deferred. Full player Goal stays active.
