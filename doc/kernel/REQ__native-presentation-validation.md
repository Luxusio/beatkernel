# Original native presentation metadata validation

Status: selected extraction contract; implementation/verification pending.
Preserve native metadata checks independently of the generic presentation
estimator so BMS audio-authoritative playback need not enter a correction ring.

The platform validator owns one accepted original evidence/pair record, its
output epoch/origin and HOST domain. It reads no clock, retains no fit/history,
allocates no storage and changes no Transport. WASAPI evidence includes original
snapshot/basis, frequency, native position and QPC chronology. ASIO retains the
complete render block, rate/origin and both original HOST bracket endpoints;
the checked midpoint supplies correlation only. Supplied pairs remain a distinct
source kind. Do not mix source kinds, bases, changed frequencies or malformed
epochs/counters silently.

Preparation validates without mutation. Commit binds exact prior semantic state
and rejects stale or cross-owner mismatches; identical semantic state may accept.
Return explicit Progress, Unchanged or AwaitingHostProgress. Duplicates do not
refresh native metadata. Preserve genuine WASAPI native counter progress even
when integer nanosecond output quantizes to the same value. ASIO equal coarse
HOST midpoints await progress and keep the full previous accepted evidence.

The BMS transaction is native preparation, shared AudioAuthority admission, then
native metadata commit. Platform code imports no application type. The legacy
PresentationDiscipline performs the same preparation around its existing core
estimator and commits metadata only after successful admission. Preserve its
public API, errors/error precedence, retention and Transport results; extraction
must not relax legacy behavior. Rebind also stages both validations before
committing, with rejected operations retaining the previous evidence.

Pause and completion consumers use original interval evidence: ASIO midpoint
cannot replace a bracket or its upper endpoint. Recheck current epoch/basis
before handing evidence to those consumers. Retired backend observations cannot
gain authority. Source metadata validation does not establish acoustic accuracy.

Verify memory-only original snapshots, native frequency/position/QPC and basis
identity, source mixing, high-frequency quantization, duplicate/stale/token
atomicity, ASIO rates/overlap/output extents and bracket preservation, coarse
HOST deferral, epoch replacement and unchanged legacy estimator behavior.
Existing fixture assertions remain intact; Rust tests are not hardware/SDK QA.
