# Shared judge hazard processing

The shared JudgeEngine gains optional one-shot timed hazards over its actual
button and enabled touch-contact ownership. Unique full-width identities,
signed marker times and opaque application values remain separate from scored
chart objects. Setup sorts markers by time with declaration order retained for
ties and reserves the operation report up front. A cursor consumes each marker
once; only controls referenced by hazards carry occupancy counts.

Input first resolves earlier markers with old ownership, then commits actual
ownership changes, then resolves equal-time markers. Advance resolves through
its effective time. The first successful operation at a boundary determines
the result. Duplicate Down, Repeat, unrelated controls and orphan release do
not change counts; each real owner/contact releases independently. Reports
retain marker time and original EventMeta only for an exact input boundary.
The existing judge offset policy applies to input and advance consistently.

Immutable configuration, cursor, occupancy and the previous operation report
participate in complete snapshot/hash state. Different hazard configurations
cannot restore into each other. Failed operations/configuration preserve owned
state. An engine without configured hazards retains its previous canonical
encoding and ordinary JudgeEvent API. The full contract is
[shared judge hazards](../kernel/REQ__judge-hazards.md).

Six independent deferred groups in `crates/beatkernel/tests/hazards.rs` cover
caller budgets/full-width IDs/stable ties, previous and updated boundary
ownership, multiple button/contact owners, normal judgments and rejection
atomicity, configuration/reusable snapshots/provenance hashes, and signed long
times/input offsets/overflow. These fixtures are authored for later execution.

After both writers returned actual terminal STOPPED, scoped Rust formatting and
whitespace checks completed. Four locked compile-only configurations exited 0:
workspace/all targets with WebTransport, no-default WebTransport/all targets,
WASM browser/lib and WASM browser-audio/lib. Existing WASM cadence unused-code
warnings remain. No test or runtime execution was performed.

## Known ceiling

This source component is installed in JudgeEngine. The shared BMS preparation
guard still refuses mine gameplay until RuntimeReport and live/local/replay/
practice/offline owners consume outcomes and implement BMS gauge/death, WAV00,
completion and rendering. No test execution, browser/device run, performance
acceptance, formal review, QA or task close is claimed by this change.
