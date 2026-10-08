# Dedicated native acquisition and ordered drain evidence

Status: all three source integrations passed available development checks;
independent code review and final QA remain in progress.

The shared collector constructs, services, closes and drops its native source
on one worker. Original canonical events and completed-drain observations share
a bounded FIFO. A cut becomes visible only after its preceding events are
consumed. Cancellation and terminal failures remain observable when transport
is full; source objects need not be Send. Entry and queued-byte limits include
owned payload capacity. Channel slot reservation is separately bounded.

Shared solo/cohort pumps use collector cuts for acquired progress, pause/resume
and completion. Consumer host time remains receipt/output-correlation evidence.
An empty transport queue never proves native acquisition through that time.
Delayed native timestamps remain unchanged; existing lag and committed-frontier
checks stay with InputMerger. Startup retain/discard and pre-origin policies
are preserved rather than selecting a new preplay domain rule.

Linux uses bounded fair evdev sweeps and publishes the initial pre-drain sample
only after every selected source reaches WouldBlock. Windows owns its HWND,
Raw Input registration and message draining on the worker; foreground cleanup
runs exactly once before decode or publication errors propagate. Selection
metadata crosses only a bounded cold channel. Thread-local UI attachment is
sampled by the caller; the worker does not read another thread's attachment.

Development evidence: initially 13 collector tests; after the cleanup precedence
remediation, 14 pass, including four close/destructor failure combinations.
The earlier close failure survives later destructor panic, while an acquisition
error remains primary with that cleanup cause. Full app library 1881 passed, zero
failed, two existing ignored; Linux binary 33 passed; Windows host-portable
binary 46 passed. Windows Rust-target typing passed through the inspected
`target/toolchain/xcheck.sh`, which uses C stubs and does not link or execute
native code. Evidence is under `target/wf/native-input-collector/`.

macOS must not infer HID queue emptiness from a runloop timeout. The new path
uses the public checked queue interface, distinguishing native underrun from
dequeue/conversion failure and reusing the existing canonical value conversion.
Its contract is defined by Apple's [IOHIDDevicePlugIn.h](https://github.com/apple-oss-distributions/IOKitUser/blob/main/hid.subproj/IOHIDDevicePlugIn.h).
Legacy callback APIs remain compatible. macOS host-portable tests passed 28;
the actual Darwin application/platform test branches passed Rust typing through
the C-stub check. Thirteen platform fixtures are compiled but not executed on
Darwin here. Native execution and final independent QA remain unproved.
Underlying kernel/driver losses are not completely observable.

Physical-device behavior, supported-host Windows/macOS execution, acoustic
latency, universal scheduler reliability and full-player completion remain
unproven. Portable facade tests and foreign target typing are distinct evidence.
