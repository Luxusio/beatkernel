# Native BMS replay capture

The portable finite stepped live owner must install the existing core logical
endpoint before processing. Capture uses its immutable original start/end and
chart seed, recording actual capped reports in the section-aware format. Input
at/after the end is validated by core acquisition but not bound; the resulting
actual advance report remains at the end. Output frame rounding never replaces
this logical boundary. Unlimited captures retain legacy bytes. Native capture
adapters and browser live callers require explicit integration before claiming
finite recording support across hosts.

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
The [BMS replay inspector](REQ__bms-replay-playback.md) performs those identity
checks and reconstructs recorded operations through the core ReplaySession.

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


## Finite recorded sections

The section-aware capture API preserves an optional original-song end. A finite
section uses `bms-judge-profile/v4:` followed by little-endian u64 chart seed,
i64 start and i64 end, then the existing offset/count/windows body. Start is
nonnegative and end strictly follows start. V4 accepts chart seed zero; absent
end keeps the exact prior v1/v2/v3 bytes. The core replay version and rule seed
remain unchanged. The additional bytes count toward normal header/file limits.

Finite capture rejects accepted operations beyond the end and bound inputs at
the exclusive end before recording mutation. An explicit advance at the end is
valid. Preroll operations and valid failed-session prefixes remain recordable;
no synthetic terminal advance is added. Existing capture entrypoints still
produce unlimited metadata until their finite owner integration is connected.

## Button/contact setup metadata

Explicit contact capture uses bms-judge-profile/v5: followed by mode byte 1,
little-endian u64 chart seed, nonnegative i64 section start, end tag 0 or 1,
optional exclusive i64 end greater than start, then the existing profile body.
Its rules identity is beatkernel-bms/press-judge/v1. Unknown modes/tags fail.
ButtonOnly keeps exact v1-v4 bytes and builtin rule identity. Header/file budgets
include all new bytes before allocation. Record actual bound physical variants,
original times and provenance; never translate contacts to keyboard events.
## Invisible input-sound identity extension

input_sounds::InputSoundIdentity::from_source(&BmsChart)
-> Result<Option<InputSoundIdentity>,String> returns None for no invisible
selections, preserving legacy setup exactly. For a nonempty timeline it validates
actual compile_invisible and wav_gain and fingerprints its semantic selections.
Opaque Copy/Eq identity exposes fingerprint()->u64. Canonical bytes are the
ASCII domain beatkernel-bms/input-sounds/v1, u64 marker count, u32 f32 gain bits,
then ascending (logical control, song timestamp) records of u32 control, signed
i64 nanoseconds and full u64 sample, all little-endian. Duplicate control/time
positions reject. Streaming FNV-1a64 uses offset 14695981039346656037 and prime
1099511628211 with defined wrapping arithmetic. Source row order, ordinal, line,
asset filenames, voice remapping, native devices and output clocks are excluded.
This is a noncryptographic compatibility fingerprint like the existing judge
identity; it is not proof of asset contents or a security/authentication digest.

replay_capture::setup_input_sound_header takes the existing setup_input_header
arguments followed by Option<InputSoundIdentity>. None returns the exact legacy
header. Some emits chart_identity=bms-judge-setup/v2: followed by the existing
pristine judge hash u64 and input-sound fingerprint u64. Options/rules/header
envelope versions are unchanged. Full header/file limits still validate the
additional bytes; no unbounded data or encoded marker table goes into metadata.
LiveReplayCapture::new_with_input_sounds takes the existing new_with_input_mode
arguments followed by that identity, with the same atomic setup and capture
behavior. Existing constructors keep their legacy output.

Actual StepGameplay and its shared StepLocalGameplay controller retain the
identity computed from the selected source before moving preparation fields.
Solo and member competition_header and configure_capture use the new APIs.
Replay setup validation computes the same identity from source_at's source and
compares the complete canonical header before replay operations. A changed,
missing or injected invisible selection or changed WAV gain must mismatch;
legacy headers cannot authorize an invisible source. Practice retains selections
before the start so they remain part of the identity and active-key selection.

This does not enable invisible audio. Asset/runtime installation and replay
command selection are pending; the shared preparation admission guard remains.
