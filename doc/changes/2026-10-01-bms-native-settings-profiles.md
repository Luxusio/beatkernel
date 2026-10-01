# Native settings profiles

The player now loads explicit `--profile` files and exposes a profile path plus
Load/Save in Settings. A versioned, host-tagged UTF-8 codec preserves bounded
native option values and repeat ordering; CLI overrides replace matching flag
groups. GUI file operations use one worker while redraw continues, with draft
and game mutations fenced until collection. Load replaces the draft, Save
persists its syntax-validated values and Apply separately changes the next
game. Publication uses a synced owned sibling, create-only hard links for new
files and rename for replacement; malformed/foreign profile targets are
refused. Known ceiling: operations drain on close, new-file publication needs
hard-link support, concurrent replacements need external coordination and
directory crash durability is not promised. Profiles exclude chart selection
and GPU/UI settings. File scenarios are authored and compiled, not executed;
actual file/GUI/native acceptance and independent reviews/QA remain deferred.
