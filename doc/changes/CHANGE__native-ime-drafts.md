# Screen-owned native IME drafts

Search, settings values and profile paths accept native IME commits. Preedit
text uses a bounded UTF-8 preview at the committed caret, without changing
settings arguments, search results or profile data. Exact byte cursor endpoints
are validated; malformed ranges, controls and capacity failures preserve the
committed editor. Commit uses the existing transactional editing paths.

The active screen instance and selected field own the composition. Field/route
changes, hidden/inactive UI and pending profile work discard preview and native
enable acknowledgement. Fresh enabled notification is required for commits.
Composition consumes ordinary keyboard shortcuts/text, so Enter cannot apply
settings or start play while preedit is active. Pointer presses discard active
composition. Retained presentation borrows the preview only for its field and
does not clone unchanged editors just to draw.

Fixtures are prepared for Unicode middle insertion, UTF-8 byte boundaries,
selection cursor ranges, controls/capacity, base editor preservation, visual-only
search/settings previews, one commit, field/profile switching, inactive and
occluded admission, disabled events and shortcut ownership. They are compiled
only; actual IME, GUI and hardware execution remain deferred.

Known ceiling: the native event API has no composition generation identifier;
ordered enable/disable and current-target admission do not prove every possible
late-event behavior. Native event-order acceptance remains required.
Known ceiling: bitmap glyph fallback, caret-only preview (including a visible
fallback for absent native caret), no selection underline or shaping and
OS-owned candidate placement remain; multilingual presentation is unfinished.
