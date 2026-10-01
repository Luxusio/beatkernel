# Precise practice section editor

F6 Practice now edits an exact start and optional end independently using seconds, M:SS or H:MM:SS with up to nine fractional digits. Tab or a click selects the input. Empty end means through song end; a configured end must follow start. Done validates and atomically updates both parent draft fields, refreshing the selected raw settings editor. Invalid time/range or final byte capacity keeps the draft unchanged. The candidate releases previous endpoint bytes before setting replacements, so a full draft can reuse their capacity. Full Song resets start and clears end; Through End clears only end; Back discards and Settings Apply remains separate. Retained dependencies repaint only changed fields, focus, controls or errors, with existing scope/backstack lifecycle preserved.

Regression fixtures are prepared for exact one-nanosecond/20-hour/week/MAX positions, cross-host option preservation, invalid ranges, second-field failure atomicity, full-capacity replacement, Done/Back/focus/reset boundaries, independent retained repaint, hit geometry/restoration and disposal. Source compilation does not establish native GUI or acoustic behavior; execution remains deferred.

## Known ceiling

Known ceiling: fixed 960x720 logical viewport and whole composed geometry uploads remain — upgrade when responsive layout and measured upload costs require it.
Known ceiling: native ASIO/network finite sections remain unsupported — upgrade when their presentation/frontier protocols are implemented.
