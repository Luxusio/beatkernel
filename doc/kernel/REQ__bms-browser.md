# Browser host and shared selected-file preparation

The browser host belongs to the existing `beatkernel-bms-runtime` application
crate. The BMS adapter and kernel remain independent of DOM and browser APIs.
React is not used. Native and browser preparation must call the same chart,
audio and image algorithms through an asset source boundary.

## Selected resources

The user selects a folder or a set of files. Folder relative paths are retained;
flat selection can resolve only the resource names it actually supplies. Capture
the selected relative path into an explicit message field before transferring
File objects to the Worker; do not depend on browser extension properties
surviving structured cloning. Read
original bytes, not browser-decoded text, so the common UTF-8/Shift-JIS policy
remains authoritative. Resolve assets relative to the selected chart's parent.
Never use chart resource names as network URLs or native filesystem paths.

Memory keys normalize slash/backslash and dot components while preserving
Unicode spelling and case. Empty, absolute, parent, drive-qualified and NUL
paths are rejected. Duplicate normalized keys and file/directory collisions
are rejected, rather than replaced. Audio lookup shares the native literal-first
finite extension-variant policy; images use exact lookup. Native canonical
containment and symlink behavior stay in the filesystem adapter.

The default selected tree permits 32,768 files, 64 MiB per file, 256 MiB total
encoded bytes and 4,096 UTF-8 bytes per path. These are configurable source
limits. The supplied host preflights metadata before sequential file reads;
Rust independently checks admission. Chart parsing, PCM and decoded-image
budgets additionally apply. Replay identity validation precedes asset reads.
Mixed source sample rates remain supported by the existing SampleBank/Mixer
policy; preparation preserves each source rate and adds no pre-resampling pass.
The selected mix format does not imply an actual browser output device.
Image aliases share existing raw/cropped
resources and preserve existing unavailable-image behavior.

## Ownership and lifecycle

Main-thread DOM owns selection, controls, text status and canvas layout. A
dedicated module Worker owns the selected tree, common preparation, prepared
chart/PCM/images, and the wgpu renderer on a transferred OffscreenCanvas. This
avoids running preparation or GPU submission on the main UI thread. It does
not establish a browser audio or gameplay thread.

Imports use generations; late work must not replace a newer selection. One
import pump retains at most one active candidate/read plus the latest queued
metadata request. A superseded unabortable file read must settle before the
candidate is released and the latest request starts. Read files sequentially.
GPU initialization and
mutating WASM calls are serialized. Chart replacement must either publish the
prepared chart and its metadata together or retain the old selection clearly.
A completed file import is staged until the main thread accepts its current
catalog generation. An ignored proposal must not replace the accepted library;
a failed later import retains the same accepted library on both sides.
Render only after a relevant change or bounded surface retry, with at most one
pending presentation callback. Size checks precede GPU configuration; zero
extent suspends drawing. Retain the canvas for recoverable surface recreation.
On page lifecycle teardown, terminate the Worker, cancel pending redraw and
disconnect layout observers; restoration creates a fresh canvas and owner.

Resource names and chart metadata enter DOM through text nodes, never HTML.
Unsupported Worker, OffscreenCanvas, secure context or WebGPU configurations
produce an explicit error. The pinned renderer supplies WebGPU; this host does
not promise a WebGL fallback.

## Preview and remaining player work

The first host displays the actual compiled chart and prepared static BGA at
an explicit original-song time. Nanoseconds cross JavaScript as BigInt or
validated decimal text, preserving signed 64-bit values. Animation callback
timestamps are presentation scheduling only, never an audio clock. The shared
visible-note query, retained GPU note cache and BGA opacity rules are reused.

Browser audio output/presentation evidence, input acquisition/clock mapping,
actual Runtime judging/capture/replay, local-device limitations and browser
network transport remain required follow-on adapters. The preview is an
intermediate implementation, not completion of the requested browser player.

## Known ceiling

Selected-file limits bound retained encoded data, not the entire process.
Binding copies, decoder scratch, decoded PCM/images, GPU allocation and overlap
between the current and staged replacement library consume additional memory.
Arbitrary binding callers can allocate before Rust admission; metadata preflight
belongs to the supplied host. Files not selected by the user are unavailable.

Source compilation does not prove generated bindings, WASM linking, Worker
ordering, browser file access, canvas output, playback or physical latency.
Fixtures may be authored and compiled; execution, browser/native acceptance and
formal review remain deferred under the user's verification sequencing.
Deferred Node fixtures cover the actual host metadata/time helpers and Worker
generation/cancellation path with mocked WASM ownership, without requiring a GPU.
