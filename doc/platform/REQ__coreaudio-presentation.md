# CoreAudio presentation-to-clock conversion

`macos::presentation::coreaudio_presentation_pair` converts an actual
CoreAudioPresentation plus exact CoreAudioApplied configuration into an explicit
output/host ClockPair. It is pure setup/control-thread arithmetic and reads no
native clock. The caller supplies the destination host domain and native-to-host
ClockMapper, normally the same MachClock used for input and native output.

The source is the exact logical first-frame grid: output_origin plus
floor(first_frame * 1e9 / applied sample rate). Checked i128 arithmetic adds the
origin before narrowing to signed nanoseconds. The provided output_grid must
match this derived timestamp and the declared output domain. Frames must be
positive and no greater than the applied buffer; request/applied format and
buffer must match exactly, device must be explicit/nonzero, and the stored
native buffer layout must match channel count. Layout length is checked as
1..32 before scanning any entries, bounding malformed public-config validation.

Host-valid flag 2 is required for an association. Missing host validity, native
point or output grid returns None, except that provided contradictory fields
return explicit invalid-observation/domain errors. Native point domain must
match applied native_clock. Sample-valid flag 1 must agree with native sample
frame presence, and a provided sample frame must be finite. Native sample frame
is diagnostic floating point only; it is never used as song/output time.
Other native timestamp flags are retained without inventing meaning.

The target is mapper.map(actual native point, explicit host domain). Unmapped
host relations reject. Mapping takes no implicit receipt-time sample. Actual
CoreAudio output presentation time may be in the future relative to callback
receipt/current host polling; the converter accepts that relation and never
clamps it to now, substitutes receipt time or creates a fake zero-origin epoch.
Output frame zero is allowed when actually observed. Caller discipline and
Transport enforce freshness/chronology separately.

Public snapshots and caller-owned mappers cannot be authenticated by this
helper. It validates consistency of provided metadata but cannot reconstruct or
verify raw mach ticks against the native timebase without the supplied MachClock
API. It does not claim exact acoustic latency or hardware synchronization.
ClockPair carries no accuracy bound; the bounded generic presentation discipline
retains Unknown quality even when a supplied mapper reports an exact conversion.

Public errors distinguish checked overflow, declared-domain mismatch, invalid
applied configuration, inconsistent observation and unmapped host. Authored
inline fixtures use only explicit synthetic mappers for grid/mapping, future
native association, absence/flags/nonfinite metadata, domain/configuration
errors and full-width arithmetic. No MachClock construction, OS calls, callback
execution, linking, tests, QA or review are run during the user's verification
deferral. Only scoped formatting and Apple-target compilation are performed.
