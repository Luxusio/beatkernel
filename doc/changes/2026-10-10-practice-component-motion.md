# Practice component motion — verification in progress

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
the explicitly enabled X11 window test remains unrun. Initial IME fixtures
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
the audio package is unchanged. Logs and disposable execution evidence live
under `target/wf/practice-component-motion-01a12429/` on this machine. Source and
test writers are frozen; independent code review and CLI/browser/desktop QA have
not yet run for this child. These development results do not establish child
completion, all-screen animations, physical input/audio latency or hardware
performance. WBS10.16/10.17 remain unfinished, and the broader player Goal stays
active.
