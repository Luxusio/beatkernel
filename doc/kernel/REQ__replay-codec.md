# Durable replay encoding

The replay codec persists normalized bound inputs and explicit advances from the
same ReplayRecorder/ReplaySession log. Its envelope includes schema version,
runtime version, chart/rules identities, options, seed, normalized clock domain
and optional opaque calibration metadata. The application must check identities
against the intended chart/rules before loading into ReplaySession; a codec
cannot prove content authenticity or the correctness of caller-provided metadata.

Wire format is deterministic little endian with magic BKREPLAY, version 1,
explicit operation/option tags and u64 byte lengths/counts. Input operations embed
the complete bounded physical-input codec blob and logical control ID. All
native provenance, raw payloads and floating IEEE bits are preserved. Encoding
and decoding both reject wrong header versions, nonzero/nonsequential initial
ordinals, descending song times and mismatched normalized event domains.

Limits explicitly bound encoded file bytes, operation count, combined header
payload bytes and nested input/payload sizes. Checked extents and remaining-file
checks precede decoded allocations; allocation failures are returned. Truncation,
unknown versions/tags, invalid UTF-8 runtime version and trailing bytes fail
without a partial replay result. Decode does not run the judge or access files.
The separate finite example owns bounded file reading and a new output path.

In-memory checkpoints remain owned logical snapshots, not serialized trait
objects. A durable log reconstructs its checkpoints through the actual engine
and application-supplied rules. No callback IO, cryptographic integrity or native
physical sync guarantee is introduced. Tests and round-trip example execution
remain deferred under the user's verification instruction.
