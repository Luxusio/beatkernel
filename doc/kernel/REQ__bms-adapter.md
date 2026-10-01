# BMS text adapter contract

`beatkernel-bms` is a separate MIT crate depending only on the core `beatkernel` crate. It reads already-decoded UTF-8 BMS text, returns SourceChart plus adapter-owned lane/sample/audio mappings and BGM events, and creates existing builtin Instant/Hold rules. Platform acquisition, sample decoding, sound devices and judging remain outside parsing. The file-loading example bounds file reads and compiles actual parsed content; the parser performs no asset IO. Legacy Shift-JIS conversion belongs before this text boundary; the application shared decoder has an explicit UTF-8-first/strict-Shift-JIS fallback policy and explicit mode API.

Supported syntax is case-insensitive: base BPM (default 130), WAVxx base36 IDs/paths, direct hexadecimal BPM channel03, extended base36 BPMxx/channel08, STOPxx/channel09, decimal measure-length channel02, layered BGM01, visible player channels11..19/21..29, paired LNTYPE1 channels51..59/61..69, and LNOBJ endpoints on visible channels. Channel16/26 are scratch lanes; other channels keep their numeric lane identity rather than assuming a fixed key layout. LNTYPE1 holds pair successive nonzero markers per lane, including across measures; only the head sample is sounded, and the tail token is retained as metadata. Dangling/overlapping/malformed holds are rejected. LNTYPE2, mines, invisible notes, unsupported timing/scroll/warp extensions, and unrecognized commands/channels fail with line diagnostics. Listed descriptive headers are preserved. BMPxx resource definitions and Base04/Poor06/Layer07 selections are retained and compiled separately; unsupported BGA crop directives remain explicit visual warnings.

Measure duration is exactly four quarter beats times its rational length. Tokens divide that measure into equal rational positions; global quarter-beat positions use checked i128 arithmetic. The minimum integer beat-grid resolution is the LCM of reduced rational denominators, with an explicit caller cap and checked conversion to core u32/i64 ticks. Decimal values never pass through floats. STOP units are 1/48 of one quarter beat, independent of measure length; duration uses the tempo active at that beat, after a same-beat BPM change. STOP durations quantize to integer nanoseconds once. Notes/BGM at a STOP use its pre-STOP timestamp, matching the core chart compiler.

Repeated channel lines merge their nonzero entries at exact rational positions; zeros never delete existing entries. Overlapping non-BGM entries and repeated header/length definitions reject by default; an explicit LastWins policy selects the later source line for the same channel/position or definition. BGM lines always layer and preserve source acquisition ordinal, including simultaneous samples. Conflicting direct/extended BPMs at one beat and repeated STOPs at one beat reject regardless of duplicate policy. Long-note/visible-note overlap on the same lane rejects rather than inventing routing behavior.

Sample paths remain exact uninterpreted references; no extension fallback or filesystem traversal occurs during parsing. Sample indices map directly to SampleId/AudioBinding in this adapter. Lane controls/visual bindings map from canonical BMS visible channel codes; Instant and Hold interaction IDs are separate per lane. IDs and game conventions are defined here, never hardcoded into the core. Missing referenced samples/tempos/STOPs fail explicitly; long-note tail sample definitions are optional under this adapter's explicit mute-tail LNTYPE1/LNOBJ policy.

Limits bound input bytes, lines, individual line size, parsed nonzero events, final objects/timing markers, and exact beat-grid resolution. Diagnostics identify the original source line (0 denotes whole-document configuration/compile errors). No parse result is returned on failure. Compilation returns the real gameplay CompiledChart and separately timed BGM entries using the same core tempo/STOP compiler. Formal verification/tests remain deferred under the user's current instruction; authored fixtures are not PASS evidence.

Sources: the format creator Urao Yane's [original BMS format specification](https://bm98.yaneu.com/bm98/bmsformat.html) defines the original directive/channel model. Extension semantics are documented by the author of the [BMS command memo](https://hitkey.nekokan.dyndns.info/cmds.htm), including base36 references, extended BPM/STOP and LNTYPE1. The memo describes implementation differences; this adapter's choices above are its explicit compatibility contract. Community executable specifications are not represented as official standards.

## LNOBJ long notes

A case-insensitive LNOBJ header selects one nonzero two-digit base36 endpoint token in visible channels. After exact channel merging, each endpoint closes the immediately preceding non-endpoint visible object on its own lane, including across measures. Earlier ordinary notes and a final ordinary note remain instants. Orphan or consecutive endpoints reject at the original endpoint line. The marker becomes retained tail metadata, never a separate judged object; only the head requires a WAV definition and sounds a keysound. This explicit mute-tail policy differs from original RDM, which may play a defined LNOBJ marker as BGM. Layered channel01 remains ordinary BGM and requires its own WAV reference.

Disjoint LNOBJ and LNTYPE1 holds share existing Hold rules and exact BPM/STOP compilation. Same-lane overlapping or touching hold ranges and visible objects inside a hold reject. Duplicate LNOBJ headers follow existing Reject/LastWins policy; malformed/zero markers reject. Existing raw-event/final-item/grid bounds still apply. Source: extension creator [RDM long-note documentation](https://nvyu.net/rdm/jp/rby_ex.php). Parser/player/live-replay fixtures are prepared for later execution; compilation is not native acceptance.

## Deterministic conditional branches

`parse_seeded(text, options, seed)` resolves RANDOM/SETRANDOM, IF/ELSEIF/ELSE/ENDIF and ENDRANDOM before existing payload parsing. `parse` uses seed zero. Random choices use SplitMix64: add `0x9e3779b97f4a7c15` with u64 wrapping, mix `(z ^ (z >> 30)) * 0xbf58476d1ce4e5b9`, then `(z ^ (z >> 27)) * 0x94d049bb133111eb` with wrapping multiplication, then xor with `z >> 31`; select `((u128(z) * n) >> 64) + 1` in the inclusive positive u32 range; selection is preparation work, never gameplay work. Inactive RANDOM does not consume the stream. SETRANDOM selects its positive u32 value without consuming a draw. Commands are case-insensitive. First matching branch in each IF chain wins; inactive payload definitions, objects and unsupported directives do not affect the selected chart. Control syntax remains validated in inactive branches.

Nested random scopes restore the preceding choice on ENDRANDOM; closing a scope across an open IF rejects. IF chains must close explicitly, and directives requiring no operand reject extra operands. Remaining random scopes close implicitly at EOF for legacy files without ENDRANDOM. Combined random/IF nesting is capped at 128, including sequential unclosed RANDOM directives; files exceeding that cap require explicit scope endings. IF requires a current random scope, and branch operands are positive u32 values. Physical input byte/line limits apply even to discarded payload; selected-event limits and original source-line diagnostics remain unchanged. No filtered copy or asset IO occurs in the parser.

Seed-aware application loading/preparation APIs use the same parser, defaulting to zero. Resolved chart compilation and setup identity use existing shared logic. Replay profile v3 serializes nonzero source seeds separately from the unchanged zero judge-rule header seed; legacy v1/v2 imply source seed zero. Replay-aware file loading resolves the recorded seed before setup validation and asset IO. The common retained settings field persists `--chart-seed` and native solo/local sessions propagate it through preparation, capture and comparisons. Direct arbitrary-seed callers must pass the same resolved chart and explicit seed into capture. Fixtures are authored and compiled for later execution; this is not an acceptance claim.

## SWITCH branches

Case-insensitive SWITCH/SETSWITCH select positive u32 values using the same source-seed SplitMix64 stream as RANDOM; inactive scopes consume no draw. CASE has a positive u32 label. A SWITCH scope starts with inactive payload and becomes selected at the first matching CASE, or at final DEF if no case selected. Once selected, subsequent CASE/DEF payload falls through until an active SKIP or ENDSW. Inactive SKIP does not terminate a later matching case. A single optional DEF must be last; CASE after DEF and duplicate DEF reject. Labels before DEF may repeat or be unordered; no extra allocation is needed to enforce an unspecified ascending-label convention.

SKIP takes no operand and targets the nearest enclosing SWITCH, requiring a prior CASE/DEF. An active SKIP inside nested IF/RANDOM disables the enclosing SWITCH payload while all remaining nested controls are still structurally validated. Ancestor activation is respected, so ENDIF/ENDRANDOM or an inner ELSE cannot restore a skipped outer branch. CASE/DEF/ENDSW require the current top scope to be SWITCH; closing commands do not cross unclosed scopes. ENDSW is mandatory even for inactive switches and at EOF; only RANDOM scopes retain implicit EOF endings. All scopes share the 128 depth bound. Original physical caps and diagnostics apply to discarded payload; selected object/timing/sample rules remain unchanged. SWITCH choices do not replace the separate RANDOM choice used by IF.

The [format memo author's SWITCH extension documentation](https://saxxonpike.github.io/bms-command-memo/index.html#SWITCH) describes fallthrough/SKIP/default behavior and differing historical implementations. The bounded, structured policy above is BeatKernel's explicit dialect: permissive DEF-before-CASE and ignored structural mistakes are unsupported. Authored parser and real preparation/replay/PCM fixtures are compiled for later execution, not represented as native or format-conformance acceptance.

## Independent visual resources and timing

ImageId is a separate base36 BMP namespace, including BMP00 initial poor
resource. Definitions retain exact opaque nonempty paths; there is no parser
asset IO. BgaChannel Base/Poor/Layer correspond to04/06/07. Zero row tokens
are rests and do not clear previous selections. Undefined nonzero BMP
references remain admitted selections for application blank-resource handling;
missing WAV/BPM/STOP definitions retain their strict errors. Definitions and
per-channel duplicate positions obey the existing explicit duplicate policy
and source-seeded branch selection. Equal-time different channels remain
independent and use visual-only acquisition ordinals.

The independent bga_ticks_per_beat grid encompasses gameplay denominators
under the same configured resolution cap. Visual subdivisions never alter
SourceChart gameplay resolution or nonvisual acquisition ordinals. BGA uses
checked core BPM/STOP marker rescaling and pre-STOP scheduling through
compile_bga and CompiledBms.bga; visual-only markers are never judge objects.
Source/raw-token limits include visual events. Existing note metadata includes
original source lines, so moving gameplay lines still changes replay identity
as before; this work does not remove that behavior.

The application owns explicit bounded raster preparation and original-song
Base/Layer composition; the adapter does not interpret native clocks or upload
images. BMP00/Poor selections do not imply continuous poor display. Video,
crop/opacity and miss-triggered poor-overlay policies remain outside supported
visual semantics. Authored parser/timing/seed/limit fixtures are source-compiled
for later execution, not format conformance or native GPU acceptance.
