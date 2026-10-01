# ALSA output metadata discovery

`beatkernel_platform::linux::alsa_output_devices(max_devices, max_text_bytes)`
queries PCM name hints without opening or changing a stream. Its separate dynamic
symbol table keeps existing PCM stream opening independent of discovery symbols.
Use it outside game acquisition and audio callbacks; the BMS settings worker is
one consumer.

The [official ALSA Name Hint Interface](https://www.alsa-project.org/alsa-doc/alsa-lib/group___hint.html)
defines `snd_device_name_hint(-1, "pcm", &hints)`, `snd_device_name_get_hint`
and `snd_device_name_free_hint`. Signatures follow the
[official header](https://github.com/alsa-project/alsa-lib/blob/master/include/control.h).
Matching deallocation follows the
[ALSA implementation](https://github.com/alsa-project/alsa-lib/blob/master/src/control/namehint.c):
free each extracted string with libc `free`, then release the hint list with
`snd_device_name_free_hint` while the dynamically loaded library remains owned.
Rust owners release both resources on success and error.

Include explicit Output and unspecified duplex hints, skip Input, and reject
unknown directions. Return exact nonempty UTF-8 NAME IDs and optional UTF-8 DESC
metadata. Do not select a default or certify availability, channel layout, sample
rate, period or buffer support. Exact duplicate NAME entries retain the first
output/duplex description in native order.

Limits must be 1..4096 scanned hints and 1..16384 bytes per extracted text field.
Count input-only and duplicate hints too. Reject overflow, malformed text and
control characters in IDs without truncating the result. Descriptions may contain
newlines; the application catalog flattens controls for display and preserves IDs.
The BMS picker requests 1024 hints and 4096 bytes per field, then admits metadata
through its 4 MiB aggregate catalog ceiling.

ALSA builds the entire native hint list and allocates extracted strings before
Rust admission checks. These limits bound Rust traversal and retained metadata;
they do not bound ALSA's initial allocation or wall time. Discovery can fail or
stall independently of stream playback. UI close drains the settings worker.

Fixtures were authored and compiled only. Native discovery, resource cleanup
under real failures and UI lifecycle execution remain deferred by the user's
verification instruction; compilation does not establish device availability.
