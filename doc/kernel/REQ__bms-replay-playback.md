# BMS durable replay reconstruction

The separate BMS runtime reconstructs captured logical gameplay through the same
builtin JudgeEngine and core ReplaySession used by live play. It requires the
matching BMS source and bounded canonical replay file; PCM assets and native
devices are unnecessary for logical inspection.

Before applying any record, loading validates codec byte/count/header/input
limits, chronology and normalized domains, exact runtime version, versioned BMS
rule identity, judge-rule seed zero, and strict profile options. The profile preserves its
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

## Durable BMS branch seed

Chart branch provenance is separate from `ReplayHeader.seed`, which remains the zero builtin-judge rule seed. Existing capture APIs continue using chart seed zero with byte-identical v1 (whole song) / v2 (positive section start) profile metadata. `new_at_with_chart_seed` accepts the explicit original chart seed. A nonzero seed uses `bms-judge-profile/v3:` followed by little-endian u64 chart seed, nonnegative little-endian i64 original-song section start, then the existing profile body (signed i64 offset, u64 window count, 20-byte windows). V3 seed zero is noncanonical and rejects; zero callers use v1/v2. All nonzero u64 values are representable, and section starts include zero through i64 MAX. Original timestamps and offsets never shift during seed restoration.

`decode_chart_setup` returns profile/start/chart seed while existing `decode_setup` and `decode_profile` APIs retain their return shapes. V1/v2 imply chart seed zero; malformed/truncated/negative-start/noncanonical/trailing/cap-exceeding metadata rejects. Validation regenerates exact canonical metadata using the stored seed and compares the pristine compiled judge setup. Arbitrary supplied BmsChart provenance is not cryptographically authenticated; differing compiled setups reject.

Logical, offline-render and native-replay entrypoints extract the recorded chart seed and resolve the source with the shared seeded BMS parser. Replay-aware preparation validates source/log compatibility before opening assets, then prepares only selected references. Practice section transformation stays in the existing shared path. The retained common settings field supplies the source seed through native live preparation and capture; absent/empty configuration resolves zero. Tests for exact legacy/new bytes, boundaries, actual file/codec/source/PCM restoration and failures are authored for later execution; source checks do not establish runtime acceptance.

## Live source seed configuration

All native live entrypoints accept `--chart-seed` as nonempty unsigned decimal u64 (including zero/MAX, excluding signs and overflow); absent means zero. The retained common settings field treats empty as default zero, persists explicit values through existing profiles/overlays and defers incomplete draft semantic validation to apply/launch. A shared local session resolves the chart once with that seed, and all independent player captures/competition setup headers receive the same provenance. Practice retry/bookmark/finite-loop invocations preserve the pinned seed rather than choosing new branches.

Saved record preview restores the recorded chart seed before logical reconstruction and requires the current comparison draft seed/profile/start to match. Saved competition restores the local recording's seed. Replay launch projection excludes live chart-seed overrides because the recording owns the branch selection. Existing generic settings editing/profile UI displays the new field without a new widget lifecycle. Library discovery continues using zero for stable catalog metadata; actual gameplay projections come from the seeded prepared chart. No cryptographic seed provenance, random seed generation or platform-accepted playback is claimed. Meaningful profile/launch/native/competition/record fixtures are authored and compiled for later execution.


## Section-aware finite reconstruction

`decode_section_setup` retains profile, start, chart seed and optional end.
Finite v4 requires a nonnegative start, strictly later end and exact metadata
extent before window allocation. `validate_section_setup` and
`reconstruct_section` reconstruct the actual pristine builtin judge and compare
the canonical section header before applying any recorded operation. Records
beyond the end and input records at the end reject; advances at the end and
valid prefixes remain supported. Original times and input offset apply once.

Legacy tuple decoders and reconstruction entrypoints refuse finite metadata,
so an end cannot be silently dropped by an unlimited consumer. Native/offline paths,
record catalogs and multiplayer callers still use those legacy entrypoints
until their explicit finite owner integration is connected. Portable stepped
replay and its Rust browser binding use the explicit section-aware APIs;
Window/Worker forward the recorded endpoint into finite audio setup after
independent validation against the actual output rate. Finite live gameplay
and end controls remain pending; actual browser acceptance remains unverified.

## Lossless contact replay reconstruction

Typed RecordedSetup carries BmsInputMode. Section-aware decoding, validation and
reconstruction restore contact mode and construct actual matching BMS rules.
Validation checks mode-specific rule identity and original setup/hash metadata.
Legacy tuple decoders refuse v5 even without an end because they cannot retain
its mode. Reject noncanonical mode 0, unknown mode/end tags, invalid extents and
profile windows. Finite competition identity retains mode in the base header
and an external endpoint; a header already carrying an end is refused there.
## Invisible input-sound identity

Canonical setup comparison uses the source-aware chart identity extension in
REQ__bms-replay-capture.md. Recompile actual invisible selections and WAV gain
from the original selected source, preserving selections earlier than section
start, before comparing headers or applying operations. A legacy judge-only
header cannot authorize a nonempty invisible timeline. A source with no
invisible selections retains its exact legacy header; injected, removed or
changed selections and changed invisible gain must reject as setup mismatches.
This validation establishes compatibility metadata, not PCM content identity.
Actual invisible sound planning remains a separate pending integration.
