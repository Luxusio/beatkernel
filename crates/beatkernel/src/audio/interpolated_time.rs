//! Exact signed native-counter interpolation. Mathematical coordinates alone
//! grant no permission to extrapolate or admit a native presentation clock.
use super::{AudioError, ConvertedPhaseError, SourcePosition, TargetTime};
use crate::time::{ClockPoint, Timestamp};
use core::cmp::Ordering;

const NANOS: u128 = 1_000_000_000;

#[derive(Clone, Copy, Debug)]
struct Fraction {
    numerator: u128,
    denominator: u128,
    negative: bool,
}
impl Fraction {
    const ZERO: Self = Self {
        numerator: 0,
        denominator: 1,
        negative: false,
    };
    fn target(time: TargetTime, negative: bool) -> Self {
        Self {
            numerator: time.numerator().into(),
            denominator: time.denominator().into(),
            negative,
        }
    }
}

/// Exact signed target duration interpolated from original native counters.
/// This validates arithmetic only, not an observed interval, epoch, submitted
/// range, freshness, acoustic time, or permission to extrapolate.
#[derive(Clone, Copy, Debug)]
pub struct InterpolatedTargetTime {
    whole: i128,
    fractions: [Fraction; 3],
}
impl InterpolatedTargetTime {
    /// `base + left/frequency + (right-left)*host_offset/(frequency*host_span)`.
    /// Equal counters are valid. Signed offsets may lie outside the interval;
    /// callers must apply their observation and extrapolation policy separately.
    pub fn new(
        base: TargetTime,
        left: u64,
        right: u64,
        frequency: u64,
        host_offset: i128,
        host_span: u64,
    ) -> Result<Self, AudioError> {
        if frequency == 0 || host_span == 0 || right < left {
            return Err(AudioError::InvalidFormat);
        }
        let offset = host_offset.unsigned_abs();
        if offset > u128::from(u64::MAX) {
            return Err(AudioError::Overflow);
        }
        let denominator = u128::from(frequency) * u128::from(host_span);
        let drift = u128::from(right - left) * offset;
        let positive = u128::from(base.seconds()) + u128::from(left / frequency);
        let quotient = drift / denominator;
        // Cancel while unsigned: a drift quotient may exceed i128::MAX even
        // though the resulting signed whole (including MIN) is representable.
        let whole = if host_offset < 0 {
            signed_difference(positive, quotient)
        } else {
            positive
                .checked_add(quotient)
                .and_then(|v| i128::try_from(v).ok())
        }
        .ok_or(AudioError::Overflow)?;
        Ok(Self {
            whole,
            fractions: [
                Fraction::target(base, false),
                Fraction {
                    numerator: u128::from(left % frequency),
                    denominator: frequency.into(),
                    negative: false,
                },
                Fraction {
                    numerator: drift % denominator,
                    denominator,
                    negative: host_offset < 0,
                },
            ],
        })
    }

    /// Interpolates original signed native counters without rounding either
    /// anchor. Negative anchors grant no permission to extrapolate or admit a
    /// presentation clock. The caller retains the actual observed interval.
    pub fn from_signed_native(
        base: TargetTime,
        left: i64,
        right: i64,
        frequency: u64,
        host_offset: i128,
        host_span: u64,
    ) -> Result<Self, AudioError> {
        if frequency == 0 || host_span == 0 || right < left {
            return Err(AudioError::InvalidFormat);
        }
        let offset = host_offset.unsigned_abs();
        if offset > u128::from(u64::MAX) {
            return Err(AudioError::Overflow);
        }
        // The distance between two ordered i64 counters fits u64, including
        // MIN..MAX. Its product with the bounded offset fits u128.
        let distance = (i128::from(right) - i128::from(left)) as u128;
        let denominator = u128::from(frequency) * u128::from(host_span);
        let drift = distance * offset;
        let frequency = i128::from(frequency);
        let anchor = i128::from(base.seconds()) + i128::from(left).div_euclid(frequency);
        let quotient = drift / denominator;
        let whole = if host_offset < 0 {
            anchor.checked_sub_unsigned(quotient)
        } else {
            anchor.checked_add_unsigned(quotient)
        }
        .ok_or(AudioError::Overflow)?;
        Ok(Self {
            whole,
            fractions: [
                Fraction::target(base, false),
                Fraction {
                    numerator: i128::from(left).rem_euclid(frequency) as u128,
                    denominator: frequency as u128,
                    negative: false,
                },
                Fraction {
                    numerator: drift % denominator,
                    denominator,
                    negative: host_offset < 0,
                },
            ],
        })
    }

    /// Exact ordering without narrowing the fractional denominator.
    pub fn compare_target_time(self, other: TargetTime) -> Ordering {
        let other_whole = i128::from(other.seconds());
        // Three proper fractions contribute strictly between -1 and 3.
        // Compare coarse wholes first, avoiding subtraction at signed limits.
        if self.whole < other_whole - 3 {
            return Ordering::Less;
        }
        if self.whole > other_whole + 3 {
            return Ordering::Greater;
        }
        let mut terms = [Fraction::ZERO; 5];
        terms[..3].copy_from_slice(&self.fractions);
        terms[3] = Fraction::target(other, true);
        compare_sum::<7>(&terms, self.whole - other_whole)
    }

    /// Exact ordering of two signed interpolated durations, without rounding
    /// either query. Mathematical ordering grants no native clock authority.
    pub fn compare_interpolated_time(self, other: Self) -> Ordering {
        // Six proper signed fractions cannot cancel a whole difference above
        // six. abs_diff also handles opposite signed limits without overflow.
        if self.whole.abs_diff(other.whole) > 6 {
            return self.whole.cmp(&other.whole);
        }
        let mut terms = [Fraction::ZERO; 6];
        terms[..3].copy_from_slice(&self.fractions);
        for (term, other) in terms[3..].iter_mut().zip(other.fractions) {
            *term = Fraction {
                negative: !other.negative,
                ..other
            };
        }
        compare_sum::<9>(&terms, self.whole - other.whole)
    }

    /// Orders an exact physical source duration against an integer source
    /// frame, without rounding or authenticating its native provenance.
    pub fn compare_source_time_frame(
        self,
        frame: u64,
        source_hz: u32,
    ) -> Result<Ordering, ConvertedPhaseError> {
        let boundary = TargetTime::from_frames(frame, source_hz)
            .map_err(|_| ConvertedPhaseError::InvalidDescriptor)?;
        Ok(self.compare_target_time(boundary))
    }

    /// Compares against `base + ticks/frequency + offset_ns/1e9` exactly.
    /// Signed offsets validate arithmetic only; they grant no extrapolation
    /// permission or native observation/admission qualification.
    pub fn compare_native_time_with_offset(
        self,
        base: TargetTime,
        ticks: u64,
        frequency: u64,
        offset_ns: i64,
    ) -> Result<Ordering, AudioError> {
        if frequency == 0 {
            return Err(AudioError::InvalidFormat);
        }
        let offset = i128::from(offset_ns);
        let other_whole = i128::from(base.seconds())
            + i128::from(ticks / frequency)
            + offset.div_euclid(NANOS as i128);
        // Six proper signed fractions cannot cancel a whole beyond ±6.
        // The bounded other whole makes these guards safe even for MIN queries;
        // subtract only after the guards have bounded the difference.
        if self.whole < other_whole - 6 {
            return Ok(Ordering::Less);
        }
        if self.whole > other_whole + 6 {
            return Ok(Ordering::Greater);
        }
        let mut terms = [Fraction::ZERO; 6];
        terms[..3].copy_from_slice(&self.fractions);
        terms[3] = Fraction::target(base, true);
        terms[4] = Fraction {
            numerator: u128::from(ticks % frequency),
            denominator: frequency.into(),
            negative: true,
        };
        terms[5] = Fraction {
            numerator: offset.rem_euclid(NANOS as i128) as u128,
            denominator: NANOS,
            negative: true,
        };
        Ok(compare_sum::<7>(&terms, self.whole - other_whole))
    }

    /// Floors once, adds the clock origin, then checks the final i64 timestamp.
    pub fn point(self, origin: ClockPoint) -> Result<ClockPoint, AudioError> {
        let nanos = floor_nanos(self.whole, &self.fractions).ok_or(AudioError::Overflow)?;
        let timestamp = nanos
            .checked_add(i128::from(origin.timestamp.as_nanos()))
            .and_then(|v| i64::try_from(v).ok())
            .ok_or(AudioError::Overflow)?;
        Ok(ClockPoint {
            domain: origin.domain,
            timestamp: Timestamp::from_nanos(timestamp),
        })
    }

    /// Floors this physical duration minus an integer source cut once.
    /// This does not remove mixer pauses or authenticate any native clock.
    pub fn elapsed_nanos_since_source_frame(
        self,
        cut: u64,
        source_hz: u32,
    ) -> Result<i128, ConvertedPhaseError> {
        self.elapsed_nanos_since_source_cut(i128::from(cut), source_hz)
    }

    pub(super) fn elapsed_nanos_since_source_cut(
        self,
        cut: i128,
        source_hz: u32,
    ) -> Result<i128, ConvertedPhaseError> {
        if source_hz == 0 {
            return Err(ConvertedPhaseError::InvalidDescriptor);
        }
        let rate = i128::from(source_hz);
        // Callers supply a u64 cut or physical_start + active_cut - playback_start,
        // whose magnitude is bounded by twice u64::MAX.
        let frames = -cut;
        let mut terms = [Fraction::ZERO; 5];
        terms[..3].copy_from_slice(&self.fractions);
        terms[3] = Fraction {
            numerator: frames.rem_euclid(rate) as u128,
            denominator: source_hz.into(),
            negative: false,
        };
        elapsed_finish(self.whole, frames.div_euclid(rate), 0, &terms)
    }

    pub(super) fn elapsed_at_source(
        self,
        source: SourcePosition,
        source_hz: u32,
        start: TargetTime,
        active: bool,
        cut: u64,
    ) -> Result<i128, ConvertedPhaseError> {
        self.elapsed_at_source_cut(source, source_hz, start, active, i128::from(cut))
    }

    pub(super) fn elapsed_at_source_cut(
        self,
        source: SourcePosition,
        source_hz: u32,
        start: TargetTime,
        active: bool,
        cut: i128,
    ) -> Result<i128, ConvertedPhaseError> {
        let (anchor_whole, anchor) = source_anchor(source, source_hz, cut);
        let mut terms = [Fraction::ZERO; 5];
        terms[0] = anchor;
        if active {
            terms[1..4].copy_from_slice(&self.fractions);
            terms[4] = Fraction::target(start, true);
            elapsed_finish(
                self.whole,
                anchor_whole,
                -i128::from(start.seconds()),
                &terms,
            )
        } else {
            elapsed_finish(0, anchor_whole, 0, &terms)
        }
    }

    pub(super) fn compare_source_frame(
        self,
        source: SourcePosition,
        source_hz: u32,
        start: TargetTime,
        active: bool,
        frame: u64,
    ) -> Result<Ordering, ConvertedPhaseError> {
        let (anchor_whole, anchor) = source_anchor(source, source_hz, i128::from(frame));
        let mut terms = [Fraction::ZERO; 5];
        terms[0] = anchor;
        let whole = if active {
            terms[1..4].copy_from_slice(&self.fractions);
            terms[4] = Fraction::target(start, true);
            combined_whole(&[self.whole, anchor_whole, -i128::from(start.seconds())])
                .ok_or(ConvertedPhaseError::Overflow)?
        } else {
            anchor_whole
        };
        // Five proper signed fractions cannot cancel a whole outside this
        // window. Bound the multiplier before using the private wide sums.
        Ok(if whole < -5 {
            Ordering::Less
        } else if whole > 5 {
            Ordering::Greater
        } else {
            compare_sum::<7>(&terms, whole)
        })
    }
}

fn source_anchor(source: SourcePosition, source_hz: u32, cut: i128) -> (i128, Fraction) {
    // Callers pass a u64 cut or physical_start + active_cut - playback_start,
    // bounded by twice u64::MAX. Frame subtraction therefore fits i128.
    let rate = i128::from(source_hz);
    let frames = i128::from(source.frame) - cut;
    let denominator = u128::from(source.denominator) * u128::from(source_hz);
    let numerator = frames.rem_euclid(rate) as u128 * u128::from(source.denominator)
        + u128::from(source.numerator);
    (
        frames.div_euclid(rate),
        Fraction {
            numerator,
            denominator,
            negative: false,
        },
    )
}

fn signed_difference(positive: u128, negative: u128) -> Option<i128> {
    if positive >= negative {
        i128::try_from(positive - negative).ok()
    } else {
        let magnitude = negative - positive;
        if magnitude == 1u128 << 127 {
            Some(i128::MIN)
        } else {
            i128::try_from(magnitude).ok().map(|v| -v)
        }
    }
}
fn combined_whole(parts: &[i128]) -> Option<i128> {
    let mut positive = 0u128;
    let mut negative = 0u128;
    for &part in parts {
        if part < 0 {
            negative = negative.checked_add(part.unsigned_abs())?;
        } else {
            positive = positive.checked_add(part as u128)?;
        }
    }
    signed_difference(positive, negative)
}

// Normalize all small nanosecond quotients and their exact residual carry
// BEFORE scaling the whole seconds, so boundary cancellation is retained.
fn normalized(terms: &[Fraction]) -> (i128, u128) {
    let mut small = 0i128;
    let mut residuals = [Fraction::ZERO; 5];
    for (index, term) in terms.iter().enumerate() {
        let (quotient, remainder) = scaled_fraction(*term);
        small += if term.negative {
            -(quotient as i128)
        } else {
            quotient as i128
        };
        residuals[index] = Fraction {
            numerator: remainder,
            ..*term
        };
    }
    small += fraction_floor(&residuals);
    (
        small.div_euclid(NANOS as i128),
        small.rem_euclid(NANOS as i128) as u128,
    )
}
fn floor_nanos(whole: i128, terms: &[Fraction]) -> Option<i128> {
    let (carry, remainder) = normalized(terms);
    finish(combined_whole(&[whole, carry])?, remainder)
}
fn elapsed_finish(
    query: i128,
    anchor: i128,
    start: i128,
    terms: &[Fraction],
) -> Result<i128, ConvertedPhaseError> {
    let (carry, remainder) = normalized(terms);
    let whole =
        combined_whole(&[query, anchor, start, carry]).ok_or(ConvertedPhaseError::Overflow)?;
    let nanos = finish(whole, remainder).ok_or(ConvertedPhaseError::Overflow)?;
    if nanos < 0 {
        Err(ConvertedPhaseError::NegativeElapsed)
    } else {
        Ok(nanos)
    }
}
fn finish(whole: i128, remainder: u128) -> Option<i128> {
    let magnitude = whole.unsigned_abs().checked_mul(NANOS)?;
    if whole < 0 {
        signed_difference(remainder, magnitude)
    } else {
        magnitude
            .checked_add(remainder)
            .and_then(|v| i128::try_from(v).ok())
    }
}

// A native proper residual has at most 128 bits; scaled by 1e9 it needs
// at most 158. The usual path uses u128. The exceptional quotient is still
// below 1e9, so binary search requires at most 30 fixed-width comparisons.
fn scaled_fraction(term: Fraction) -> (u128, u128) {
    if let Some(scaled) = term.numerator.checked_mul(NANOS) {
        return (scaled / term.denominator, scaled % term.denominator);
    }
    let scaled = Wide::<3>::from_u128(term.numerator).mul_u128(NANOS);
    let denominator = Wide::<3>::from_u128(term.denominator);
    let mut low = 0u128;
    let mut high = NANOS;
    while low + 1 < high {
        let middle = (low + high) / 2;
        if denominator.mul_u128(middle).compare(scaled).is_gt() {
            high = middle;
        } else {
            low = middle;
        }
    }
    let residual = scaled.sub(denominator.mul_u128(low));
    debug_assert_eq!(residual.0[2], 0);
    (
        low,
        u128::from(residual.0[0]) | (u128::from(residual.0[1]) << 64),
    )
}

fn sums<const N: usize>(terms: &[Fraction]) -> (Wide<N>, Wide<N>, Wide<N>) {
    let mut denominator = Wide::from_u128(1);
    for term in terms {
        denominator = denominator.mul_u128(term.denominator);
    }
    let mut positive = Wide::from_u128(0);
    let mut negative = Wide::from_u128(0);
    for (index, term) in terms.iter().enumerate() {
        let mut product = Wide::from_u128(term.numerator);
        for (other, factor) in terms.iter().enumerate() {
            if other != index {
                product = product.mul_u128(factor.denominator);
            }
        }
        if term.negative {
            negative = negative.add(product);
        } else {
            positive = positive.add(product);
        }
    }
    (positive, negative, denominator)
}
fn compare_sum<const N: usize>(terms: &[Fraction], whole: i128) -> Ordering {
    let (mut positive, mut negative, denominator) = sums::<N>(terms);
    if whole < 0 {
        negative = negative.add(denominator.mul_u128(whole.unsigned_abs()));
    } else {
        positive = positive.add(denominator.mul_u128(whole as u128));
    }
    positive.compare(negative)
}
fn fraction_floor(terms: &[Fraction]) -> i128 {
    let (positive, negative, denominator) = sums::<7>(terms);
    if positive.compare(negative).is_lt() {
        let difference = negative.sub(positive);
        let mut magnitude = 1u128;
        while denominator.mul_u128(magnitude).compare(difference).is_lt() {
            magnitude += 1;
        }
        -(magnitude as i128)
    } else {
        let difference = positive.sub(negative);
        let mut quotient = 0u128;
        while !denominator
            .mul_u128(quotient + 1)
            .compare(difference)
            .is_gt()
        {
            quotient += 1;
        }
        quotient as i128
    }
}

// Private bounded products only. Source anchor denominator <=96 bits, and
// query/base/left/drift/start denominators <=64/64/128/64: common products
// <=416 bits and carry sums <=419 bits, fitting seven limbs. No allocation.
// Native ahead-limit comparison has six denominators <=64/64/128/64/64/30:
// products <=414 bits and bounded whole/carry sums <=418 bits fit the same limbs.
// Comparing two queries uses <=64/64/128/64/64/128-bit denominators:
// products <=512 bits and bounded whole/carry sums <=516 bits fit nine limbs.
#[derive(Clone, Copy)]
struct Wide<const N: usize>([u64; N]);
impl<const N: usize> Wide<N> {
    fn from_u128(value: u128) -> Self {
        let mut words = [0; N];
        words[0] = value as u64;
        words[1] = (value >> 64) as u64;
        Self(words)
    }
    fn mul_u128(self, factor: u128) -> Self {
        let low = self.mul_u64(factor as u64);
        let high = self.mul_u64((factor >> 64) as u64);
        let mut shifted = [0; N];
        shifted[1..].copy_from_slice(&high.0[..N - 1]);
        debug_assert_eq!(high.0[N - 1], 0);
        low.add(Self(shifted))
    }
    fn mul_u64(self, factor: u64) -> Self {
        let mut result = [0; N];
        let mut carry = 0u128;
        for (index, limb) in self.0.into_iter().enumerate() {
            let product = u128::from(limb) * u128::from(factor) + carry;
            result[index] = product as u64;
            carry = product >> 64;
        }
        debug_assert_eq!(carry, 0);
        Self(result)
    }
    fn add(self, other: Self) -> Self {
        let mut result = [0; N];
        let mut carry = 0u128;
        for (index, limb) in self.0.into_iter().enumerate() {
            let sum = u128::from(limb) + u128::from(other.0[index]) + carry;
            result[index] = sum as u64;
            carry = sum >> 64;
        }
        debug_assert_eq!(carry, 0);
        Self(result)
    }
    fn sub(self, other: Self) -> Self {
        debug_assert!(!self.compare(other).is_lt());
        let mut result = [0; N];
        let mut borrow = false;
        for (index, limb) in self.0.into_iter().enumerate() {
            let (value, first) = limb.overflowing_sub(other.0[index]);
            let (value, second) = value.overflowing_sub(u64::from(borrow));
            result[index] = value;
            borrow = first || second;
        }
        debug_assert!(!borrow);
        Self(result)
    }
    fn compare(self, other: Self) -> Ordering {
        self.0.iter().rev().cmp(other.0.iter().rev())
    }
}
