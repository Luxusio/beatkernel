# BMS durable replay reconstruction

The separate BMS runtime reconstructs captured logical gameplay through the same
builtin JudgeEngine and core ReplaySession used by live play. It requires the
matching BMS source and bounded canonical replay file; PCM assets and native
devices are unnecessary for logical inspection.

Before applying any record, loading validates codec byte/count/header/input
limits, chronology and normalized domains, exact runtime version, versioned BMS
rule identity, seed zero, and strict profile options. The profile preserves its
signed offset and caller-ordered grade/early/late windows; normal JudgeProfile
validation rejects invalid bounds or duplicate grades. Recompiled pristine judge
setup identity must match the captured noncryptographic fingerprint. Different
charts, rules or profiles reject rather than silently reinterpret the log.

The reader bounds bytes as it reads, independent of file metadata, and rejects
oversized/growing streams and read failures. Profile metadata must have its exact
versioned prefix, nonzero representable count and exact checked extent; truncated,
overflowing or trailing metadata fails before window allocation. Existing codec
validation remains authoritative for the durable replay envelope.

`replay_bms --chart PATH --replay PATH` reconstructs only recorded operations.
Optional `--cursor N` or `--song-ns N` requests core exact-cursor or song-time seek;
they are mutually exclusive. Time seek may add the core's explicit boundary
advance, including new timeout results at that selected time. Reverse inspection
restores snapshots and replays forward through recorded inputs. No input binding,
clock remapping, extra offset or synthetic end-of-chart advance occurs during
loading. Empty recordings and valid failed-session prefixes remain limited to
their recorded extent. The CLI prints logical results and actual engine hash.

Known ceiling: this is logical replay reconstruction, not native audio playback.
The setup hash is not content authentication. Exact runtime-version compatibility
is required; migrating old recordings needs an explicit future migration policy.
Source and replay resource caps are explicit; process memory also includes judge
state, retained results, input records and checkpoints. Execution, native replay
and deterministic comparison tests remain deferred by user instruction.
