# Common physical browser input API

Add explicit physical-binding setup beside compatible keyboard constructors.
The common core BindingMap retains Any/exact device selection and full
HID/native/vendor control identities. Configured encoded/payload budgets bound
canonical physical event decoding before any runtime operation. Unsupported
clock domains or negative acquisition times are refused rather than replaced
with Worker receipt time.

The existing StepGameplay owner still performs binding, judgment, capture and
output accounting. Reuse PressedKeys with actual admitted bound-input reports
for device/control/lane ownership; endpoint-suppressed input clears presentation.
An observation failure after a committed report cannot undo judgment; retain
that report's progress and fail the owner explicitly.

This API is preparatory integration. Raw HID still needs its device report
adapter, touch still needs deliberate interaction mapping, and hardware
permission/acquisition plus application controls remain pending. Accepting a
physical event does not prove a playable device adapter.

Source and six independent deferred fixture groups are authored. Both writers
reported STOPPED before scoped formatting and compile-only integration. Workspace/headless and both WASM feature cargo checks exited 0. WASM retains
three existing platform cadence dead-code warnings. No runtime, binding
generation, test or browser acceptance is claimed. Required independent reviews
and QA precede eventual close; task remains PENDING and broad Goal active.
