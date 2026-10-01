# Shared local-cohort pause

Linux ALSA local play with 2..64 explicitly assigned keyboards now connects F9
pause to the same native presentation-boundary coordinator as solo play. Every
member shares the mixer, Transport and acknowledged pause/resume times. Judges,
scores, key levels and captures retain their player/device ownership, including
sparse positive player IDs and u32::MAX.

Fair acquisition still drains every source before committing a shared boundary.
The existing InputMerger preserves source chronology and global timestamp order.
Pause drains pre-boundary gameplay, parks later key levels and advances every
member once at the acknowledged pause boundary. Paused idle periods generate
no judge advances or capture records. Resume retains the configured input lag:
all source queues must drain and the merge frontier must reach the resume
boundary before synthetic releases and original post-boundary events can score.
Native keysounds stay on the original playback grid, and discipline uses the
cumulative paused-frame gap. Input loss or partial group failure still stops
the whole cohort with its committed report/capture prefixes and native cleanup.

A portable composition fixture covers 2/3/4/64 members using actual command
queues, Mixer, RuntimeGroup, NativePause, PauseKeyboard, InputMerger, capture and
replay reconstruction. It reverses acquisition order, includes sparse/max IDs,
checks shared pause silence and logical time, preserves release provenance,
compares mixed keysound samples and reconstructs each independent judge state.
The fixture is authored and compiled only; runtime/device/GUI acceptance remains
deferred. Windows/macOS local-cohort pause, ASIO presentation support, network and
replay pause policies, loops and browser integration remain required work.
