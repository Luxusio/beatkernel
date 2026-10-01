# Graphical saved-record selection

Settings now has a Records chooser for the selected chart: edit a directory,
scan direct replay files, inspect a selected recorded prefix and attach it as
an own or other opponent. Preview uses the shared bounded replay reader and
actual logical reconstruction with the current chart, profile and practice
start. It shows recorded operations/time and actual hit/miss/combo counts;
it does not infer full-song completion. Enumeration and replay processing run
on the existing serialized metadata worker, with pending work fencing edits
and navigation. Attach/Clear change the settings draft, and Apply remains the
step that commits next-session options. The native gameplay owner still
rechecks actual recording compatibility at session start.

## Known ceiling

Known ceiling: the chooser inspects at most 4096 direct directory entries,
retains 256 paths and uses 64 MiB/one-million-operation replay caps — add
incremental browsing/configuration when record directories or valid recordings
routinely exceed these limits.

Known ceiling: regular-file and symlink checks do not protect against hostile
concurrent path replacement — add host-specific handle opening when untrusted
concurrent filesystem mutation becomes a supported threat model.

Source compilation and fixture authoring are separate from GUI/file/gameplay
execution acceptance and independent review/QA, which remain user-deferred.
The full player Goal remains active.
