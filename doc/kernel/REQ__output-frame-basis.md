# Fresh native stream counters on an existing mixer grid

Core OutputFrameBasis retains the original mixer grid origin/domain, positive
sample rate and exact physical next-frame offset at stream creation. Mixer can
produce this value without changing cursor, voices, queue or format. Capture it
before the new backend primes/renders anything; playback cursor is distinct and
must not substitute for the physical offset. New native submission counters stay
stream-relative and truthful, never prefilled with previously rendered frames.

Convert stream-relative frame counts by adding the checked original frame offset
before nanosecond division. Convert arbitrary native position/frequency by adding
the exact frame/rate and position/frequency rationals before final floor. Use
bounded quotient/remainder carry arithmetic, not a giant overflowing common
numerator or two independently rounded timestamps. Preserve signed origin,
full-width u64 offsets/counters, exact domains and checked timestamp overflow.
Zero/invalid rates/frequencies reject. Existing zero-offset conversions remain
byte/value compatible. The value reads no hardware or clock and allocates nothing.

Actual ALSA and WASAPI streams capture/expose the mixer basis at open. ALSA's
new basis-aware presentation converter keeps existing native timestamp/query/
submitted-minus-delay checks, then maps played frames into the original mixer grid.
Shared Linux gameplay and replay output adapters call it with the actual stream
basis, covering local and solo compositions without OS-specific business rules.

WASAPI basis-aware observation and two-snapshot calibration keep original native
counter/frequency/QPC/quality evidence. Output-origin identity is the basis point
at native counter zero; retain basis identity within an observation epoch so
changing the offset/rate silently is refused. Platform discipline exposes tagged
basis-aware admission with existing output-epoch rejection before interpretation.
Live calibration/observe/resume paths use the actual stream basis. Legacy public
conversion/constructors stay compatible and retain zero-offset behavior.

CoreAudio and ASIO already use absolute RenderReport frame identity for their
logical output points; do not add the stream offset a second time. This change
does not alter PCM/mixer sample grids or reset transport/judgment history. It does
not itself fence unheard device buffers, preserve failed-open rollback, switch
runtime owners, convert formats/rates or establish physical acoustic accuracy.
Complete live handoff and platform/device acceptance remain subsequent work.

Author independent rational-carry/frame/time bounds and actual advanced/paused
Mixer basis tests, memory WASAPI/ALSA converter/source-identity cases, and native
ALSA open-basis helpers/retirement fixtures without device execution. Migrate
only affected old stream literal defaults. Assertions/threads/runtime/formal
review/QA remain deferred; scoped formatting and four sequential compile-only
checks follow both paired writer terminal stops. Full BMS Goal stays active.
