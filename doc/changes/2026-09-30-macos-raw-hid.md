# macOS raw HID acquisition and explicit framing

Scope inspection found that the macOS input backend acquired only scalar values.
Added an explicit timestamped raw-report mode for vendor device adapters while
keeping default typed value acquisition separate. A runtime-resolved native
callback API avoids a new unconditional deployment-version link requirement;
unavailable timestamped registration fails explicitly. Raw envelopes retain native
report type/ID/bytes, arrival mach ticks, canonical device identity and clock
provenance. Finite queue/byte bounds and native failures remain observable.

A portable conversion helper takes an explicit separate-ID or leading-ID layout.
It preserves all metadata, rejects integer ID overflow and incorrect/missing
prefixes, and bounds a fallible canonical copy. Payload bytes beginning with an ID
are not guessed to be framing. Authored fixtures cover these ambiguities and
capacity/representation boundaries without requiring Apple execution.

Apple-target all-targets and portable HID conversion compile checks passed.
The native example selects value/raw input explicitly and prints original raw
envelopes beside explicitly normalized reports. It does not invent a vendor
decoder or claim successful physical acquisition. Formal reviews, QA, native
link/execution and fixtures remain deferred; the full Goal remains incomplete.

Also authored the original plan's chart parser/compiler mutation corpus: bounded
byte/text mutations, valid generated BPM/STOP/measure/hold documents, independently
computed timing, subdivision/scaling/permutation equivalence, and extreme checked
arithmetic cases. Focused corpus and workspace all-targets compile checks passed.
The corpus has not executed and is not coverage-guided fuzzing evidence.
