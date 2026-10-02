# Static chart WAV gain

BMS audio preparation now applies selected `#VOLWAV` percentages to keysounds
(including long-note heads) and scheduled BGM through the existing finite
command gain. Missing headers use100%; 0% mutes and200% doubles amplitude.
Decoded PCM stays intact and gain is applied once by the shared mixer, which
already sums in f64 and clamps output to[-1,1]. No PCM copy, device change or
new callback work is introduced. Native, offline, custom-decoder and replay-aware
loaders share preparation; replay audio uses the current selected chart header,
which is not independently stored in replay metadata. Audio gain does not change
judge identity, time, grading or stored input when physical gameplay lines stay fixed.

The [format memo author's VOLWAV description](https://hitkey.nekokan.dyndns.info/cmds.htm#VOLWAV)
defines percentage of original sounds, default100 and an amplification example.
BeatKernel accepts the existing nonnegative plain decimal grammar, optional
leading plus and18 total digits. Conversion to finite f32 gain happens during
preparation, using a checked adapter accessor also for fabricated metadata.
Duplicate and seeded branch policies remain unchanged. The precision limit is
an explicit BeatKernel admission rule, not a historical universal format limit.

## Known ceiling

Existing18-digit/plain-decimal grammar remains; dynamic BGM/key volume channels
97/98 are unsupported. Extreme amplification can clip via the existing mixer.
Author fixtures cover parser boundaries, seeded branches, duplicate policies,
identity preservation, unchanged PCM, mute/fraction/amplification through actual
offline Runtime/Mixer, hold bindings, and replay-aware/custom-decoder loading.
Fixtures are source-compiled only under the user's validation deferral.
Native audio, hardware behavior and historical-player conformance are unverified.
Formal review/security/QA and task close remain deferred; this is an implementation
candidate within the still-active full BMS player goal.
