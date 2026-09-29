BeatKernel now has a two-crate Rust workspace and an OS-independent integer time
and transport foundation in place of the mistakenly introduced React scaffold.
The transport supports rational positive/zero/negative rates, pause/resume, seek,
historical timestamp lookup, checked overflow and atomic command errors. Public
examples and tests verify behavior, including four independent property sequences
of 1,200 rate changes. Native I/O is not implemented in this slice; see the
[time/transport contract](../kernel/REQ__time-transport.md) for its exact boundaries.
