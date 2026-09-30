# Native BMS replay output integration

The separate BMS runtime's native replay player connects checked captured-gameplay
audio plans to existing WASAPI, ALSA and CoreAudio streams, with explicit devices,
formats, backend sizing/modes and configurable finite command/voice/lookahead
limits. Already mapped commands retain their exact output timestamps without
adding origin or preroll again. Initial commands are supplied before native
prefill; later admission follows completed successful Mixer reports. Execution,
queue and native failures stop through cleanup, and final diagnostics separate
admission from core rendering and native observations. A finite requested wall
duration includes preroll without inventing acoustic completion or recording new
physical input. Source control remains in the separate BMS runtime crate.

## Known ceiling

Native rendering can race admission, and finite credit/lookahead can fail for
dense schedules or control stalls. Assets/commands are preloaded. Original
physical output scheduling and dropped audio were not captured. ASIO remains
unavailable pending licensing/distribution resolution. Actual native sound and
timing, fixtures/examples, independent reviews and QA remain unexecuted; locked
cross-target checks establish source buildability only.
