# Original-timeline fresh practice starts

The existing graphical native Settings editor gains PRACTICE START (NS), optional
--start-ns, strict nonnegative ASCII i64 and defaultzero. Windows WASAPI/optional
ASIO, Linux ALSA and macOS CoreAudio solo/local compositions prepare a fresh
section before starting output, then calibrate original start minus preroll to
observed output zero. F5 reuses that pinned start and existing cleanup lifecycle.

Preparation retains future original chart targets with all tempo/STOP/scroll
markers. Earlier heads, including crossing holds, are excluded; no successful
prior play or held state is invented. Automatic overlapping BGM selects ceiling
frames directly from original decoded PCM and reports source frame, applied
song time and correction. Independent suffix sample identities avoid aliasing
multiple instances of one asset. Future/retained commands stay in original song
time until a separate one-time output-relative mapping before preroll admission.

The core bank gains a consuming asset iterator for setup-time PCM transfer.
Original buffers move into the new bank; only retained BGM suffixes allocate.
Original-plus-suffix decoded storage remains within caller total bytes before
copying. At most4096 crossingcues and base samplecount plus4096 (corecap65536)
are admitted; default64MiB perasset/256MiB total stay unchanged. Capacity,
identity overflow, invalid positions and no remaining content fail explicitly.
Completion calibration remains conservative using original absolute extents.

Section replay captures use a filtered pristine judge identity. Standalone
loading/rendering and full-chart ghosts do not automatically infer that section;
matching preparation is required. Live scrubbing, bounded loops, pause/resume and
historical hold restoration remain work. Ceiling source selection and output
frame scheduling are bounded corrections, not physical synchronization proof.

Fixtures are authored and compiled only. Actual tests/native/device/GUI/GPU/
audio/file/network execution and independent formal review/security/QA/close
remain user-deferred. Full player Goal stays active.
