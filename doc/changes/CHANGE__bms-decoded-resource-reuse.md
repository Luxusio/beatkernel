# Reuse decoded resources during BMS preparation

AC-209 reuses the first successfully prepared PCM for subsequent SampleIds
whose references resolve to the same resource key in one preparation call.
Every reference still resolves through the existing policy. Reused keys skip
another encoded read, codec decode and channel conversion. Distinct keys are
separate; the bounded map retains no encoded data and does not survive the call.

The std-only core offers a fallible setup-time PcmSample::try_clone with explicit
limits and independent owned storage. Each ID still charges its full PCM bytes
and count, keeping bank representation, transfer ownership, source rates and
gameplay/replay identities intact. No real-time cloning, resource freshness
guarantee, shared PCM storage or measured performance improvement is claimed.

Independent fixtures are authored for exact copies, admission and path reuse,
full per-ID quotas and genuine shared gameplay/Mixer/capture behavior. Compiler
checks compile fixture bodies; tests, assertions, product/audio/browser/device
execution, formal review and QA remain deferred. Full Goal active; task open/PENDING.
