# Native settings editor

The graphical player now composes a bounded UTF-8 line editor and GPU text
fields into a paginated native settings screen. Device IDs, native buffers and
periods, timing windows, key bindings and competition options can be edited
before a game owner starts. Apply reuses the existing platform/competition
parsers without opening resources, then supplies exact flag/value pairs to the
next game; Back discards the draft. Existing repeated options and paths with
spaces remain intact, chart selection supplies its path exactly once, and
unfocused fields borrow text without per-frame editor allocation. Known ceiling:
option validation does not certify device/file availability; persistent profiles,
enumerated selectors, clipboard and IME composition remain absent and text uses
the current ASCII glyph fallback. Source compilation and authored fixtures do
not establish GUI or native execution; execution/review/QA remain deferred.
