# Rolling BMS background command admission

The separate BMS runtime crate supplies an off-thread BgmFeeder shared by native
composition roots. The prepared BGM schedule stays in memory; this is command
admission, not streamed PCM or another decoder. Total BGM count is independent of
the bounded queue and mixer pending capacities. Parser and asset bounds still
apply.

BgmConfig declares output origin/domain, sample rate, nonnegative preroll,
positive lookahead and maximum outstanding BGM commands. Only finite-gain Play
commands are accepted. Song timestamps map to origin+song+preroll with checked
wide arithmetic before narrowing. Target frames use integer ceiling, matching
Mixer. Pre-origin targets reject. Stable sorting retains original equal-time
order, voice/sample identities and gains.

`BgmFeeder::from_output_commands` also accepts already mapped Play timestamps.
It requires zero additional preroll and preserves their absolute output times;
wide subtraction from the explicit output origin determines frames, including
negative origins and the full i64 time span. The same credit, lookahead, ordering
and exact failure semantics apply to native replay keysounds and BGM together.

The caller supplies an actual completed-render frame cursor and finite admission
budget. Credits retire only for admitted targets strictly before that cursor;
targets equal to the render end have not executed yet. The feeder admits commands
within an inclusive cursor+ceil(lookahead*sample_rate/1e9) frame horizon while
credit/budget remains. Lookahead is quantized upward by less than one output
frame. Deferred eligible work stays pending in the feeder and is reported.
A failed queue admission returns its exact
command/reason and preserves the successful prefix. An unadmitted cue whose target
is already behind the cursor fails explicitly; it is not retimestamped, skipped
or retried automatically. Cursor regressions and arithmetic errors are explicit.

Native compositions use one real Mixer queue with 65536 command/pending slots,
64512 maximum outstanding BGM commands and 1024 nominal live-command slack.
Slack is not hard isolation from arbitrary input bursts. Initial admission occurs
before opening output, which may prefill immediately. Startup calibration and
the live loop continue feeding actual rendered progress; normal loop admission
is bounded to 256 commands. Commands enter the same producer through Runtime
after it owns the queue. Default lookahead is 3 seconds, controlled explicitly
by --bgm-lookahead-ns. A long output buffer, stalled control loop or dense burst
can exceed this finite horizon/capacity; the session fails with evidence and
requires an explicit larger horizon or restart, without a physical latency claim.

Feeder diagnostics expose configuration, admitted/remaining/outstanding counts
and deferral. Native render counters remain the source for actual mixer execution
rejections; admission is not proof of native delivery or acoustic output. Existing
device-loss and stop/close cleanup contracts remain in force. Pure fixtures are
authored and compiled only while tests/native playback/review/QA remain deferred.
