# Original native presentation metadata validation

Status: validator extraction and combined player owner implemented and portably
tested; production selection is being integrated. Final review/QA pending.
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

The application owns both validators in one native presentation owner. Its
original snapshot includes the published output epoch, exact frame basis and
original native evidence. Cold construction rejects pre-admitted owners rather
than stitching independently accumulated histories. Construction and every
admission reject inconsistent raw origins, HOST domains or epochs. Pin the exact
frame basis on the first accepted progress; subsequent snapshots must match it,
including supplied-pair evidence. Failed or deferred observations do not change
this identity. Embedded native bases must equal the snapshot basis. Stage native validation, admit its
progressing correlation pair to audio authority, then immediately commit native
metadata under the same exclusive borrow without a callback between steps.
An authority refusal leaves native metadata unchanged. Native counter progress
may commit while quantized output is unchanged; this cannot refresh authority
age. ASIO coarse-HOST deferral likewise retains previous authority history.
The owner reads no clock, changes no Transport and adds no observation or input
queue. Full ASIO brackets remain accessible to pause/end consumers.

Pure owner fixtures must prove identity/basis refusal, pinned-history admission
failure without native mutation, duplicate and quantized-output behavior,
coarse ASIO deferral and original bracket preservation. An additive owner alone
does not establish production pump, replacement or resume migration.

Pause and completion consumers use original interval evidence: ASIO midpoint
cannot replace a bracket or its upper endpoint. Recheck current epoch/basis
before handing evidence to those consumers. Retired backend observations cannot
gain authority. Source metadata validation does not establish acoustic accuracy.

Verify memory-only original snapshots, native frequency/position/QPC and basis
identity, source mixing, high-frequency quantization, duplicate/stale/token
atomicity, ASIO rates/overlap/output extents and bracket preservation, coarse
HOST deferral, epoch replacement and unchanged legacy estimator behavior.
Existing fixture assertions remain intact; Rust tests are not hardware/SDK QA.

## Actual output observation port

An explicitly implemented static backend port supplies an optional original
native snapshot through the existing output owner. It does not call the legacy
estimator. Remix and backend switching forward the exact snapshot and typed
errors. Keep native readiness, unavailable evidence, terminal stream faults,
configuration changes and original host-anchor refresh rules intact.

Before acquisition or admission, the output owner checks its published creation
epoch and exact frame basis against the presentation owner. Acquire the snapshot
and render evidence before committing application state. Refuse a mismatched
snapshot, backend read error or invalid presentation without updating accepted
presentation or the owner's cached report. Native IO itself is not rolled back.
Unavailable evidence grants no presentation progress. A missing render report
does not erase the last cached report.
An already pinned presentation basis must match before IO or lifecycle access.
Prefer the original ASIO snapshot's render evidence, or the WASAPI snapshot's
available render, after the backend read succeeds. Do not populate a legacy
accepted-observation cache to make the audio-authoritative path obtain a report.

Pause and end read the combined owner's accepted original record. ASIO uses its
full bracket/render evidence, while point sources use the accepted pair. Recheck
published identity before this access; held replacement may retain previously
committed pause evidence but cannot sample a retired source. Static fake-IO tests
must prove the legacy estimator entry point is unused and cover both switching
branches, remix forwarding, read failures and interval preservation. This port
does not by itself select audio authority in the production gameplay pumps or
implement held-output publication.
