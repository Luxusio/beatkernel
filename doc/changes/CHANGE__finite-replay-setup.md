# Finite recorded section setup

Section-aware capture and logical reconstruction preserve a strictly later
original-song end in canonical BMS profile v4 metadata. Unlimited capture keeps
byte-identical v1/v2/v3 metadata. Finite recording and reconstruction reject
operations beyond the end and inputs at its exclusive boundary, while retaining
valid prefixes and explicit advances at the end. Existing unlimited consumers
refuse v4 rather than silently discard the endpoint.

This prerequisite does not yet connect finite live/replay owners or page
controls. Genuine capture/codec/reconstruction fixtures are authored for later
execution; compiler checks are not runtime acceptance. Full Goal active, task
open/PENDING; formal review and QA remain deferred.
