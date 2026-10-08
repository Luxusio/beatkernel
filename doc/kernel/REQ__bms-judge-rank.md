# BMS judge rank declarations

The separate MIT BMS adapter retains RANK and DEFEXRANK declarations and exposes
a checked, copied metadata snapshot. This is preparation policy with no native
IO or dynamic dispatch. It does not select timing windows or change existing
caller-controlled live/replay judging.

## Values and parser admission

`BmsRank` represents codes 0 through 4 as VeryHard, Hard, Normal, Easy and
VeryEasy. Its checked parser accepts an ASCII decimal integer, optional leading
plus and leading zeros, with at most 18 total digits; other syntax or values
refuse. Existing `parse()` admission of descriptive RANK text remains unchanged.
The explicit typed accessor validates that text before it can become a rank.

Selected case-insensitive DEFEXRANK headers are newly accepted by the actual
parser. They use the existing exact nonnegative decimal grammar: optional plus,
at most one dot, at least one digit, and at most 18 total ASCII digits including
leading/fractional zeros. Zero is valid metadata, not an invented playable
zero-width preset. Exponents, negative values, NaN and infinity refuse. Numeric
percentage is retained as a reduced i128 numerator and positive denominator;
no floating-point conversion or silent truncation occurs. For example, 120.125
is 961/8 percentage points. Private fields preserve validated invariants.

Accepted raw text remains in the original metadata map. Existing Reject or
explicit LastWins duplicate policy applies independently to each header.
Selected malformed DEFEXRANK errors identify its original physical source line.
Inactive malformed payload follows existing RANDOM/SWITCH selection and is not
interpreted, while physical input caps and structural validation still apply.
An oversized decimal reports the existing decimal-precision Limit, not an
unreachable arithmetic Overflow under that grammar.

## Explicit resolution

`BmsChart::judge_rank_metadata()` validates both present raw declarations,
including mutable/fabricated metadata, without modifying the chart. Accessor
errors use line 0 as whole-document validation, consistent with existing typed
metadata accessors. A malformed lower-priority value is still an error.

`BmsRankMetadata::resolve()` requires `BmsRankPrecedence::RankFirst` or
`DefExRankFirst`; neither the precedence nor a missing declaration has a
default. When the preferred header is absent, the other valid declared header
may be selected. When both are absent, the result is None. The resolved
`BmsJudgeDifficulty` preserves whether its value came from RANK or DEFEXRANK.
This API does not claim last-physical-header precedence; the raw map has no
cross-header source ordering.

Existing BmsChart layout, source objects, tempo/STOP timing, rules and default
judge profiles remain unchanged. Metadata-only comparisons must keep gameplay
physical lines fixed because those lines already participate in provenance.
Inserting lines is not promised to preserve replay identity.

## Compatibility limits and evidence

The engine author's [beatoraja extension document](https://raw.githubusercontent.com/exch-bms2/beatoraja/master/manual/extension.txt)
documents RANK4, DEFEXRANK priority and an EASY100 baseline. The format memo
author's [DEFEXRANK documentation](https://bms.ms/~hitkey/cmdsJP.htm)
describes other baseline/order conventions. These are engine-specific evidence,
not one universal timing table. Callers must choose their own validated timing
profile and meaning of the percentage baseline separately.

Historical key/scratch/LN-end timing presets, dynamic EXRANKxx/channel A0,
engine defaults and app profile wiring remain required work under WBS08.07 and
08.09. This metadata foundation does not establish those features or full-player
acceptance. Verify actual parser/typed API, exact decimal bounds, duplicates,
conditional branches, fabricated metadata, explicit precedence and unchanged
source/compiled behavior in independent adapter tests before completion.
