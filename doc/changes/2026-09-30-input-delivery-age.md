# Explicit input delivery age observations

The core now supplies a bounded pure observer for event timestamp age at an
explicit same-domain receipt point. It reuses the existing duration ring and
retained-tail nearest-rank summary without acquiring clocks, changing events,
assuming a device period or modifying gameplay. Full signed timestamp spans use
checked wide subtraction into u64; domain, future-event and receipt-regression
errors preserve observer state. Native BMS composition roots supply actual fresh
post-acquisition clock points and print labeled counts/percentiles after cleanup
on normal/error paths. Windows observes QPC receipt-to-runtime software age;
Linux and macOS retain their kernel/IOHID event timestamp boundary. Processing
time, delivery age and physical input-to-sound timing remain distinct. Native
execution, fixture execution and formal review/QA remain deferred; the full
original Goal is active.

After all writers stopped, locked Rust 1.98.1 compile-only checks passed for the
workspace/all targets on the Linux host, and platform/BMS runtime all targets on
x86_64-apple-darwin and x86_64-pc-windows-gnu. Eight independent core fixtures
were authored and compiled, not executed: literal nearest ranks, retained tail,
zero retention, full signed spans, atomic rejection, cross-device event order,
capacity limits and clone independence. These results establish compilation;
native linking/playback, actual age distributions, physical timing and Apple
ARM64 compilation remain unestablished.
