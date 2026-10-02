# Miss-triggered Poor background

Native solo/local/replay display now projects the current original-song
BMP00/channel06 Poor selection for a default 500ms after a known matching new
miss transition. During the interval it replaces normal Base/Layer using raw
pixels; normal composition resumes exactly at expiry. With no selected Poor
resource normal composition continues. A selected undefined/missing resource
uses the existing explicit blank-resource path. The portable policy accepts
0..10seconds; zero disables. Desktop currently uses the default.

The [BMS command memo author's Poor notes](https://hitkey.nekokan.dyndns.info/cmds.htm)
describe BMP00 until channel06 changes its selection and temporary replacement
on mistakes. The 500ms lifetime is BeatKernel's explicit policy; historical
timing and extended POORBGA modes/header parsing are not implied.

Exact-chart NoteProgress retains a constant-size latest effective miss time
from the complete admitted prefix, independent of the recent128 HUD history.
Hits do not clear it; unknown/custom/mismatched stages and repeated completed
transitions do not retrigger. Each local member uses its own progress and song
clock. Projection uses i128 subtraction without allocation or wall-clock time;
pause freezes reported time. Future latest misses do not activate early.
Seeking requires rebuilding the admitted prefix; rewinding the query clock
alone does not rewind gameplay. Fresh progress inherits no previous miss.

Authored pure boundary/identity/dense-prefix/local fixtures and actual Runtime
timeout, capture/codec/reconstruction, live/replay native publication fixtures
are prepared for later execution. Source compilation alone cannot prove native
GPU correctness or performance. GPU selection uploads can still stall or fail
their separate budget. Video, extended display modes and full platform/runtime
acceptance remain unfinished.

Allowed source checks succeeded for host workspace/all-targets, Windows GNU
and macOS all-targets, no-default-features/all-targets and WASM graphics/library.
Scoped rustfmt/diff checks succeeded. An initial desktop binary/library module
path mismatch was corrected before final native compilation. Existing WASM
cadence and macOS block future-compatibility warnings remain. Tests, native
apps/GPU/device execution and formal review/QA gates have not run.
