# Common interval-aware native startup

SDK-enabled ASIO network sessions now use the same committed startup owner as
the other native backends. ASIO observations retain their original host bounds,
render report and output grid. The owner checks calibration consistency against
an assessed rate band, projects the full future band, arms a frame beyond the
rendered frontier plus the actual buffer, and waits for the first crossing's
upper host bound. The nominal transport anchor remains separate from uncertainty.
Point backends retain their existing projection.

The Windows adapter uses the actual prepared buffer size and configured sample
rate, and seeds finite completion and playback discipline from typed native
evidence. Committed ASIO gameplay uses the existing common logical render grid
for keysounds, excluding startup silence from command positions. Cancellation,
input retention, BGM replenishment and cleanup ownership stay in their existing
shared/native owners. No crate, dependency or licensing change is introduced.

## Prepared regression coverage

Interval fixtures cover coarse host timers, future rate variation after an exact
past average, source-grid rounding, nonzero origins, invalid domains/chronology,
inconsistent bounds, frame margin, long spans and overflow. Common-owner fixtures
use the actual gated Mixer with synthetic ASIO presentation observations to
cover first-crossing evidence retention, upper-host arrival, exact sound onset,
logical scheduling, finite and empty playback, end seeding and cancellation.
The fixtures are prepared for later execution, compiled but not run.

## Compile evidence

Linux workspace/all targets, Windows GNU app/all targets, macOS app/all targets,
headless app/all targets and WASM graphics library cargo checks completed with
exit zero. The first Windows check found one remaining tuple destructure after
the typed observation change; adapting that WASAPI gameplay caller fixed the
second Windows check. Other configurations passed their first checks. Evidence
is in target/ac154-host.exit, ac154-windows-2.exit, ac154-macos.exit,
ac154-headless.exit and ac154-wasm.exit. Scoped formatting and diff checks passed;
existing WASM cadence warnings and the macOS block future-compatibility warning
remain. No runtime or formal review/QA acceptance is claimed.

## Known ceiling

Rate/error bounds are caller assessments, not measured physical accuracy.
Manual ASIO pause remains unsupported. SDK/MSVC compilation, driver callbacks,
actual output/network timing, runtime tests and formal acceptance remain deferred
by the user. Ordinary Windows GNU checks cannot cover the enabled SDK branch.
Broader player/browser functionality and native resource composition remain open.
