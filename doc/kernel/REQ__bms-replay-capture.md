# Native BMS replay capture

The separate `beatkernel-bms-runtime` crate can capture the accepted operations
of its actual live Runtime through the core ReplayRecorder. Each report is
recorded once, including its successful bound-input prefix after a judge error,
unchanged physical provenance, unoffset song time and successful explicit
advances. Audio queue failures do not erase accepted judge operations.

Capture starts before the first judge operation. Its versioned application
identity includes the pristine JudgeEngine stable hash, covering compiled chart,
rules and profile state. This noncryptographic identity does not authenticate
source files, assets or device configuration. A replay consumer must reconstruct
and check the same judge setup before replaying through ReplaySession. The
fingerprint cannot reconstruct the setup: the consumer still needs the matching
chart and rule implementations supplied separately. The options field preserves
the profile as `bms-judge-profile/v1:` followed by LE i64 input offset, u64 window
count, then each caller-ordered window's u32 grade and i64 early/late bounds.
These settings remain subject to normal JudgeProfile validation when loading.

`--record-replay PATH` enables recording. Record and encoded-byte bounds are
explicit, defaulting to 1,000,000 operations and 64 MiB. Nested physical input
codec limits apply before payload cloning. Capacity, domain and chronology
failures reject the complete report and stop the application; the prior valid
recording remains a prefix, never a claim of a complete session.

Recording performs control-thread allocations and bounded codec work; it does
not execute a second judge or perform filesystem IO in the gameplay loop.
After native cleanup attempts, the application encodes and writes the log using
exclusive file creation. Existing output files are never overwritten. Saving
failure is reported, and a newly created file can be partial after write failure.
Earlier gameplay or cleanup errors retain precedence over saving errors.

Known ceiling: finite recording caps can stop long sessions unless explicitly
increased. Encoded-byte limits bound durable data, not total process memory;
temporary encoding and in-memory recording consume additional memory. A capture
contains judgment inputs and advances, not PCM or a reproduction of physical
audio timing. Execution tests and native recording checks remain deferred.
