# Native profile publication

This contract owns the file I/O guarantees of `settings_profile::save_profile`,
`save_player_profile`, `load_profile` and `load_player_profile`. The existing
[player settings contract](../kernel/REQ__bms-player.md) retains Save/Load/Apply,
operation-worker ownership and versioned codec behavior. Implementation and
independent verification of the strengthening below are in progress.

Pure models, host/schema validation and codecs remain in `settings_profile.rs`.
Production filesystem operations belong to its private `native_settings_profile`
adapter. Only the four original save/load functions are reexported for public
compatibility, including on WASM; encode/decode never call storage. The adapter
uses pure decoders to validate actual file contents and the shared publisher to
perform staging and commit. Injectable filesystem fixtures exercise that native
adapter, while codec tests remain independently executable.

## Validation and compatibility

Saving encodes and validates the complete original v1/v2 model before file I/O.
Encoding errors retain precedence. A missing filename is refused next, followed
by the shared publication namespace check, before target metadata or reads.
The exact hexadecimal 8.3 staging namespace and conservative case/space/period/
stream-base aliases defined by the
[individual publisher contract](REQ__completed-result-archive.md#complete-individual-native-publication)
are reserved; requests in that namespace return `InvalidInput` without file I/O,
including when a parent is missing or an existing reserved file is malformed.
Other native filenames are preserved without lossy rewriting.

Existing targets must be regular, nonsymlink, bounded, structurally valid
same-host profiles. Native-only v1 saving refuses existing v2. Player v2 saving
may replace valid v1/v2. Unknown/malformed/foreign/oversized targets remain
untouched. Existing codec bytes, public signatures and the 72 KiB file bound
remain unchanged; loading retains checks on both the leaf and opened file.

## Staging and commit

Saving uses the shared exclusively owned sibling stage and namespace. It writes
all bytes, flushes, synchronizes and closes the writer before publication.
Interrupted/short writes follow `Write::write_all`; write/flush/sync refusal
cannot invoke commit or expose a partially written accepted final destination.

When the initial target check finds absence, commit exclusively hard-links the
complete stage. A concurrently created target remains untouched. When the
initial check accepts an existing profile, commit revalidates that actual target
immediately before rename. Changed malformed/foreign/symlink targets are refused
and preserved. Successful rename explicitly retires ownership of the old stage
pathname before subsequent cleanup; a later entry at that released name must
never be removed by the old owner.

Precommit failure closes the handle and cleans only the owned stage, retaining
the original error. Cleanup refusal cannot mask a validation or native I/O
error. After successful linking, best-effort cleanup cannot change success or
delete the complete final. Unsupported links fail without a direct-final or
clobbering fallback. Save is create-or-replace: two ordinary concurrent public
calls can both succeed if one observes the other's complete file as replacement.
Exactly-one-new-winner verification must synchronize after both actual target
checks selected absence, before their commits.

## Verification cues and limits

Real public save/load tests must reload canonical v1/v2 data and preserve refused
targets. Internal tests use actual files and the production profile classification
and commit closure for write/sync/commit/cleanup faults and deterministic races.
Verify moved-owner retirement by creating a foreign entry at the released stage
pathname and observing that it survives owner drop. The original self-alias
defect requires an actual public reproduction before the source correction.

Known ceiling: caller-owned directories must remain stable; separate path checks
do not guarantee hostile ancestor containment or interprocess replacement locks.
Known ceiling: file synchronization does not establish directory-entry power-loss
durability; cleanup is best effort and interrupted processes may leave stages.
Injected capacity refusal is not actual disk exhaustion, and foreign Rust type
checks are not Windows/macOS execution. Public profile WASM trapping is not a
confirmed bug: existing metadata refusal may precede process-ID lookup. Helper
WASM execution and app WASM typing must not be presented as direct public profile
runtime acceptance. Whole WBS09.13, interruption/recovery and the full player Goal
remain unfinished.
