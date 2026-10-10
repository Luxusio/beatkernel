# Practice component motion — desktop adapter verification blocked

Practice now exposes the retained control-node, component-composition and inverse
hit APIs used by Selection and Display. Browser and native owners connect the
existing explicit motion requests to Practice without introducing Virtual DOM,
a second scheduler or per-frame geometry reconstruction. Native IME candidate
anchors on motion-owned screens project staged editor bounds through the last
accepted immutable pose and frame extent, including fractional scale/offset,
fixed parent clipping, opacity and outward physical rounding. Intended behavior
is specified in [the motion contract](../ui/REQ__ui-motion.md).

Shared foundation development checks pass 118 Practice tests and five new
visible-rectangle projection tests. A test-filter invocation matching zero tests
does not contribute evidence. Two initial Practice fixture failures were traced
independently to buttons moved outside their fixed parent-row clip; corrected
horizontal probes preserve exact edge refusal and geometry identity assertions.
Browser owner fixtures pass all five after correcting a Settings setup that
omitted a mandatory draft field. All 29 browser-menu fixtures and ten shared
component scene fixtures pass. Native fixtures pass all seven ordinary tests;
the X11 window test is ignored in the ordinary run and was explicitly executed
later by independent desktop QA. Initial IME fixtures
changed the target after drawing, correctly invalidating the painted state;
initialization now precedes drawing. Same-key repaint deliberately retains its
component ID, so the repaint fixture verifies stable identity, actual admitted
preedit, increased geometry revision, restored transform and accepted anchors.
The 49 existing native-related regression tests pass; two window tests remain
ignored in that regression result. Overlapping filters are separate runs, not
additional unique tests.

The current browser release build and pinned wasm-bindgen regeneration succeed.
Main WASM SHA256 is
`197cadc2fe3339d537d0e8460186b391c6137dec08eba5cece719a477d5ccefe`;
the audio package is unchanged. Independent DEEP code review passes with zero
findings. Independent CLI verification passes 2,796 app library/native binary
tests, with 16 explicitly ignored cases, plus 67 Node boundary tests, scoped
Rust2021 formatting and Windows/macOS app binary stub typing. Stub typing does
not prove linking or execution on those operating systems. The first compiler
guard and omitted Node VM flag failures remain preserved beside successful
cached/corrected invocations.

Independent browser QA passes against the current WASM and exact production
renderer Worker: actual old/new pointer positions, 39 animation frames, editor
repaint, opacity refusal, Back/fresh reentry and disposal. Six screenshots were
visually inspected, with 6,664 changed action pixels and no console/page/Worker
errors. Owned browser, display, server, profile and cache were cleaned up.

Independent native execution passes seven ordinary owner fixtures and the real
Practice X11/llvmpipe Vulkan window fixture. The final window run presents 21
animation frames, preserves geometry epoch/identity and refuses the old hit
while accepting moved control71. Its directly captured Practice PNG was visually
inspected. Verification depth is `window-rendered`: typed host requests and real
presentation/hit behavior were checked, while the fixture does not pump OS
keyboard or physical IME events. Local native development also needed the
existing `target/toolchain/native-ui-runtime/usr/lib/x86_64-linux-gnu` libraries
prepended to the toolchain `LD_LIBRARY_PATH`; no system installation was needed.

The independent desktop lens returns **BLOCKED_ENV**: runtime X11 MCP
`window_list` and screenshots did not match the actual Practice window/display,
even during the bounded live-observation remediation. Empty lists and black
MCP screenshots are not application rendering evidence. The existing managed
`:99` server was retained; owned test processes were terminated. An earlier
vacant-display ownership refusal and an incorrect ten-color screenshot heuristic
are preserved separately; bitmap Practice output legitimately contains eight
palette colors. Neither is relabeled as a product defect or successful MCP
binding.

The child remains unfinished until the desktop adapter can enumerate and capture
the same visible Practice window and the required independent GUI gate passes.
Logs, screenshots and reports live under
`target/wf/practice-component-motion-01a12429/{qa-cli,qa-browser,qa-desktop}/` on
this machine. Source review, functional test outcomes and Harness closure are
separate. WBS10.16/10.17 remain unfinished; all-screen animation, physical
input/audio latency, hardware performance and the full player Goal are not
established by this scope.
