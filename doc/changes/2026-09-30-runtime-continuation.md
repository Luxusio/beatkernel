# Runtime implementation continuation

Implemented the next source slices under the user's explicit verification
deferral: integrated Runtime with explicit clock mappings and scalar audio
publication; indexed Lane/Point/Path projection; complete logical replay,
live operation recording and snapshot configuration compatibility; generic
Repeated/Composite/Axis/Contact/Pointer/Pose interactions and fixtures; native
Linux evdev/hidraw/ALSA and macOS IOHID/CoreAudio; bounded BMS parser and a
separate offline core/platform/adapter composition executable.

Section restart prepares original-PCM frame selection with explicit rounding,
reports the applied song position, builds fresh Mixer/queue owners, and replaces
runtime judge/transport/producer ownership together. Native output reset and the
first sample's observed host/presentation clock relation remain explicit host
responsibilities. No absolute physical synchronization guarantee is claimed.

Source formatting and build checks passed for the portable workspace, Windows
runtime composition and native Apple target. Examples/tests were authored;
the newly added tests, CPU benchmark and native audio paths were not executed.
Review-code, review-security, QA and hardware timing remain deferred and PENDING.
Earlier Phase 0–7 evidence is not evidence for these new changes.

The C host SDK condition has not been activated without a concrete embedding
host. ASIO licensing and backend implementation remain outstanding. BMS support
is the documented UTF-8 subset; unsupported commands fail explicitly. Linux
native ABI support and macOS float32 output ceilings are documented in their
platform contracts. The full Goal remains active and incomplete.
