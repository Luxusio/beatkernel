# BMS captured replay reconstruction

The separate BMS runtime loads bounded captured replay logs and checks exact
runtime compatibility, versioned builtin rule identity, seed, profile options
and recompiled pristine judge setup before applying recorded operations through
the same JudgeEngine/ReplaySession. The `replay_bms` CLI reads a matching BMS chart
without PCM preparation and prints logical results, operation position and the
actual judge hash. Optional exact-cursor or song-time seeks reuse core snapshot
restoration, preserving recorded provenance and applying the profile offset once.
Empty and failed-session prefix logs do not invent a final chart advance.

## Known ceiling

This path reconstructs logical gameplay; native audio replay remains outstanding.
The setup hash is noncryptographic and runtime versions must match exactly.
Encoded-file caps do not bound all judge/result/checkpoint memory. Source and
replay fixtures are authored and compiled only; examples, deterministic comparison
execution, native playback, independent reviews and QA remain deferred.
