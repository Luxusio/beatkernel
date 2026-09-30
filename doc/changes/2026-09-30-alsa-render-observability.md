# ALSA mixer execution observations

ALSA now retains the last successful core Mixer render report through the existing
bounded atomic telemetry publisher. Callers can inspect cumulative consumed,
applied, late and rejected commands, active voices, pending commands and the output
block independently of native submission counters. Before a successful render or
when publication is unavailable, the method returns None. Stop and native failures
retain the last successful render for diagnostics without claiming current output
or presentation. Linux BMS and native inspector diagnostics print this report after
join on normal and error cleanup. Queue admission, rendering, native submission
and audible output remain distinct. Test and native execution, formal review and
QA remain deferred; the full original Goal remains active.

Rust 1.98.1 locked workspace all-target and Windows-target platform/sample
all-target compile checks passed. The authored pure Mixer/ALSA facade fixture
was compiled without running tests or opening native devices.
