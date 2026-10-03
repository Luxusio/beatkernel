# Browser finite replay forwarding

The Worker reads the actual Rust replay end/frame getters and sends finite
metadata only when configured. Both Worker and Window validate its exact frame
mapping with the prepared original-song start, 100 ms preroll and actual output
rate. The Window retains this setup snapshot before sample transfers and passes
the frame fence into AudioHost.finish. Unlimited playback retains its old wire
shape and argument-free finish; invalid finite setup has no unlimited fallback.

Independent protocol/lifecycle fixtures are authored for later execution.
This JavaScript-only change does not establish generated-binding, browser,
acoustic or full-player acceptance. Finite live ownership and end controls are
still pending. Full Goal remains active and the task open/PENDING; formal review
and required browser/CLI/desktop QA remain deferred, required before close.
