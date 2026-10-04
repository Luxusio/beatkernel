# Browser coalesced mouse and pen movement

Pointer movement acquisition preserves original mouse/pen history rather than
only the final dispatched position. Window collects keyboard, touch, mouse/pen,
HID and Gamepad inputs; Worker retains mapping, judgment, replay and rendering.
Nonempty native coalesced movement replaces the dispatched sample, while absent
or empty history preserves the single-sample fallback. Complete history and
aggregate button transitions are bounded and validated before publication.
Main forwards the original batch through its existing bounded 256-event pump.

Independent component fixtures add three groups (seven total), and actual Host
fixtures add two groups (116 total), prepared for deferred execution. They cover
original fields, masks, atomic refusal, ownership, 256/1024 boundaries and the
existing intermediate-null/final-watermark pump. Runtime tests, browser/device
acceptance, performance measurements and
formal review/QA are deferred under the user's verification instruction. This
change does not close the active player task.

## Known ceiling

Coordinates assume an untransformed canvas. History behind an already committed
global input frontier is refused. Relative pointer lock, predicted samples,
pressure and tilt remain future work. Browser/device execution and latency are
not verified by source inspection.
