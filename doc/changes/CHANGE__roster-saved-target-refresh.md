# Refresh saved opponent target choices when player count changes

Changing the local player count rebuilt source selectors but left saved opponent
target selectors on the old roster. A selected removed player stayed displayed
as an ordinary current player instead of an explicitly retired choice.

After updating the roster/source UI, rebuild saved opponent choices through the
existing showOpponentSelection function. Preserve the selected original ID; show
Removed player for a retired target and let existing launch preflight refuse it.
Do not silently reassign it. Existing pending/live-play fences prevent count
changes during gameplay, and labels remain text-only DOM values.

The regression checks changing three players to two while a saved opponent targets
player three, the retained invalid choice and no audio acquisition on invalid
launch. Source assignment, replay compatibility and gameplay logic are unchanged.

Independent scope review-code PASS (DEEP bounded formal-only) and review-security
PASS reported no findings. Both independently executed the retired-target case.
The code reviewer additionally confirmed current frame-arm/PCM/source contracts;
security checked literal DOM label handling and frozen launch selections. Their
results cover this production change and associated fixture repair scope, not
whole-task or real browser/WASM/network acceptance. Required formal QA remains.

Verification (2026-10-06): full Node web suite with experimental VM modules and
test concurrency two reports 508 passed, 0 failed, exit 0, versus the initial
368/140 baseline. Actual Chromium is available locally; real-browser verification
requires generating the presently absent WASM package and remains separate.
