# Browser Worker HUD ownership

BrowserCanvas already draws local/live and replay score HUDs with the shared
scoreboard component on the graphics Worker. Remove the duplicate Window song
position and periodic score/status DOM writes on gameplay step and audio-report
responses. Preview/menu changes and final completion, stop and error summaries
remain event-driven Window work.

Keep correlated render/step responses, acquisition watermarks, queued input
pumping, actual output evidence, command ACKs and joined completion/cleanup.
This slice does not move the remaining audio control bridge or saved/network
opponent DOM presentation. Touch/pointer/HID adapters also remain pending under
the broader input-thread contract.

Implementation and two independent deferred fixture groups are authored. The
Window fixture file retains its prior 43 groups, with 45 total. Source-only
scoped whitespace checks found no diagnostics. This JS-only slice runs no Cargo
check; preceding Rust compilation is not evidence of JS correctness.
No browser, JS parsing, test or performance execution is authorized for this
source-only slice. Full Goal remains active and task PENDING; required reviews
and QA must precede eventual close.
