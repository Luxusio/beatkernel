# Optional ASIO control and exact configuration

Added SDK-free buffer/rate request validation and an optional Windows MSVC ASIO
SDK control bridge. Exact buffer requests follow reported bounds and granularity
without rounding; external clock is explicit. The control owner opens the selected
registration on one COM thread and exposes capabilities, channel metadata, explicit
rate changes and control-panel calls. Default builds require no SDK/C++ compiler
and stay under the MIT distribution policy; SDK-combined artifacts follow GPLv3.
Standard CI/verification commands now select the SDK-free build, with enabled
Windows SDK checks requiring an actual supplied SDK and compatible compiler.
Host default/all-feature and Windows GNU default all-target checks passed; nine
portable fixtures compiled without execution. SDK-enabled Windows compilation,
native control acceptance and ASIO stream/callback/mixer output remain pending.
No tests, reviews, QA or native playback ran in this slice. The full-plan Goal
remains active.
