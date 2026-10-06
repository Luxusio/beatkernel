# Prepare replay hazards before sound identity

Recorded replay validation now compiles the selected source and prepares its
source-aware pristine hazard judge before calculating input/mine sound identity.
Invalid mine resolution therefore retains PlaybackError::Hazards rather than
being misclassified as InputSounds by identity preparation. Existing canonical
codec, version, rules, section, setup/profile and sound identity checks remain.
No second judge or alternate replay algorithm is introduced.

The existing source-aware replay/practice/offline regression failed before the
fix. All five MinePlan tests passed afterward without changing assertions.
All six replay-validation tests passed. Full library execution reports 1467
passed / 96 failed, reducing the previous failure count by one; all remaining
failures are named in the prior baseline. Workspace all-targets WebTransport
compilation and whitespace checks exited zero with only existing warnings.
Native playback/acoustic acceptance, remaining defects/drift and full independent
review/QA stay outstanding.
