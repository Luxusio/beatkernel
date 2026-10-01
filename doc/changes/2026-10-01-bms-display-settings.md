# Display settings and player profiles

The graphical BMS player adds a Display draft for GPU backend, presentation
mode, FPS and note lookahead using the existing bounded text fields and modal
commands. Done keeps the display draft; Back discards it. Main settings Apply
validates both native and display options before committing between sessions.
FPS, lookahead and supported presentation mode update on the main UI owner;
GPU backend changes require saving and restarting with that profile (or an
explicit backend CLI argument). Gameplay timing and owners stay
independent of display settings.

Player profiles now save both native and display configuration in version 2.
The existing version 1 format loads with display defaults. Explicit CLI values
override matching stored values. Load and Save remain serialized settings-worker
operations, and Apply is separate. Invalid or foreign profiles reject the whole
draft replacement. Native-only version 1 APIs refuse version 2 replacement.

Known ceiling: backend switching requires app restart. Profile creation still
requires hard-link support; concurrent writers need external coordination and
directory crash durability is not promised. Clipboard, IME and multilingual
glyph shaping remain pending. File, GUI, GPU and native execution acceptance,
independent formal review and QA are still deferred by the user. Source
compilation alone does not prove runtime behavior or the complete player Goal.
