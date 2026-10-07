# First-failure AudioWorklet grid evidence

Status: implemented candidate; independent review and final QA remain pending.

Audio failures now preserve bounded numeric first-cause facts instead of only
a status code. The preallocated Worklet payload records operation origin,
processor phase, actual callback frame and extent when known, retained native
expected/start words, and the actual successful arm frame. Presence flags
distinguish unknown values from valid zero. Successful callbacks add no
diagnostic polling, object/view allocation or BigInt conversion.

Host, command and sample owners validate the snapshot and retain it on their
original errors. Rejected ACKs preserve their existing order, sequence and
admitted prefix while carrying the same first snapshot. Healthy cleanup keeps
its existing success behavior; distinct cleanup failures remain distinct from
the original processor error. Legacy status-only messages stay compatible.

Focused actual-source Node fixtures pass all 114 tests, including high words,
absence, hostile getters, reentrancy, first-cause and cleanup behavior. These
checks do not establish actual browser delivery or a timing repair. A separate
owned-browser probe is being prepared to exercise the unchanged native frame
chronology validator through a declared test-only skipped callback.

The original sporadic status6, pre-play domain choice and delayed-input frontier
issues remain unresolved and unwaived. Diagnostic fields never authorize
presentation, judging, output advancement or replay completion. No timestamp,
frontier, buffer/backend, native chronology or broader player policy changes.
