# CoreAudio mixer execution observations

CoreAudio now exposes the last successful Mixer render outcome and cumulative
command execution counters using the existing bounded atomic publisher. Callback
ownership supplies the publication version; no callback allocation, locks, native
queries or logging are added. Stop retains the report after callback/listener
quiescence before releasing context. Failed stop continues to own live callback
state, and failed render does not invent a replacement outcome. The native example
prints retained reports during cleanup. Core execution remains distinct from
buffer delivery, native presentation timestamps and audible output. Native linking,
playback, tests and formal review/QA remain deferred; the full Goal remains active.

Rust 1.98.1 locked workspace all-target, Apple-target platform/sample all-target
and Windows-target platform all-target compile checks passed. Apple checks used
the installed x86_64-apple-darwin standard library; this does not prove ARM64
compilation or native callback execution. Existing portable publisher/Mixer
fixtures remain authored coverage, with no new test execution claimed.
