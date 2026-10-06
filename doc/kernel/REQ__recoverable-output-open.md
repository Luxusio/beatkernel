# Retain mixer ownership when output opening fails

Expose a core MixerOpenFailure<E> containing the original backend error and
optional unique Mixer. Borrow the error and available mixer without consuming
either; into_parts moves both without cloning assets or command ownership.
Absence explicitly means recovery is unavailable, not a replacement empty
mixer. Public diagnostics need not format PCM or command contents. Construction
and transfer are cold control-thread operations with no native IO.

ALSA exposes open_recoverable(request, mixer), alongside the compatible existing
open wrapper. Preflight/ABI/format failures return the original unchanged mixer.
Worker spawn failure retains the mixer on the control owner until worker claim;
ordinary native setup failure joins the worker and returns its exact mixer with
the original setup error. Receive failure likewise joins and retains software
ownership only when a normal terminal worker return proves it is available.
Panic does not synthesize recovery or hide the unavailable mixer. Successful
opening retains actual configuration and captured frame basis as before.

Use a small private statically injected worker-launch seam for independent
memory tests, used by the actual native open path. A cold ownership slot may
allocate and lock during thread launch/claim; drop the worker's lock and slot
reference before native setup and render processing. The control-side slot
ends with launch. No additional lock, allocation or dynamic dispatch enters
the real-time render loop. Startup failure recovery always waits for worker
retirement. Existing open returns the original error and intentionally discards
the optional recovered mixer to preserve its original signature/semantics.

This preserves software state at failure, not pre-open cursor state after an
adapter has rendered, native buffer delivery, voice rewinding or automatic live
handoff. WASAPI/CoreAudio/ASIO recoverable open integration remains subsequent
work. A worker panic can lose the mixer; callers must distinguish that ceiling.

Author core ownership and actual ALSA preflight/launch/normal-join/panic fixtures
with advanced/paused mixer state, queued commands and original PCM; no device
calls in deferred memory cases. Assertions, spawned fixture threads, physical
devices and formal review/QA remain deferred. Scoped Rustfmt and four existing
sequential compile-only commands run after both writers stop. Full BMS player
Goal remains active and incomplete.
