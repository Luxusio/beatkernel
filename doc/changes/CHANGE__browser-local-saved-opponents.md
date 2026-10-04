# Per-member saved competition in browser local play

Saved selections now carry an explicit stable player target for local play.
Target controls use the actual roster, and setup refuses missing/removed targets
before audio acquisition. Solo preserves its existing untargeted API and requires
local targets to be cleared explicitly. The Worker reads each selected file once
and calls the actual target member's comparison admission API.

BrowserLocalGame retains independent common SavedOpponents/HUD state per member
with whole-owner success-charged limits of eight records and 64 MiB. Admission
uses that member's pristine competition header; observations use its actual song
frontier. Failed comparisons produce independent null/error rows and never alter
live judges, optional captures or the one shared audio owner. Solo and local
bindings reuse the existing opponent serializer.

The page validates all member identities, counts, labels and Own/Other metadata
against frozen selections. Invalid individual results disable only their target
HUD; invalid roster ownership refuses the complete envelope. Periodic valid
counters stay on Worker; failure notices and final grouped results are event
driven on Window.

Comparison count reserves fixed space before touch configuration. Common field
geometry drives both the renderer and contact routing. A failed HUD retains its
space, so contacts cannot be shifted by disappearing comparison rows. Existing
native/solo composers preserve their default geometry.

Fixtures are prepared for later execution. Source compilation and whitespace
checks do not establish browser/device/audio or performance acceptance. Local
network competition, active contact page remapping and full runtime acceptance
remain pending; the full player Goal stays open.
