use crate::{BmsError, BmsErrorKind};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Ratio {
    pub n: i128,
    pub d: i128,
}
pub(crate) fn gcd(mut a: i128, mut b: i128) -> i128 {
    while b != 0 {
        let next = a % b;
        a = b;
        b = next;
    }
    a
}
impl Ratio {
    pub const ZERO: Self = Self { n: 0, d: 1 };
    pub const ONE: Self = Self { n: 1, d: 1 };
    pub(crate) fn new(n: i128, d: i128) -> Result<Self, BmsErrorKind> {
        if n < 0 || d <= 0 {
            return Err(BmsErrorKind::Syntax("nonnegative rational required"));
        }
        let divisor = gcd(n, d);
        Ok(Self {
            n: n / divisor,
            d: d / divisor,
        })
    }
    pub(crate) fn add(self, rhs: Self) -> Result<Self, BmsErrorKind> {
        let common = gcd(self.d, rhs.d);
        let n = self
            .n
            .checked_mul(rhs.d / common)
            .and_then(|left| {
                rhs.n
                    .checked_mul(self.d / common)
                    .and_then(|right| left.checked_add(right))
            })
            .ok_or(BmsErrorKind::Overflow)?;
        let d = self
            .d
            .checked_mul(rhs.d / common)
            .ok_or(BmsErrorKind::Overflow)?;
        Self::new(n, d)
    }
    pub(crate) fn mul(self, rhs: Self) -> Result<Self, BmsErrorKind> {
        let first = gcd(self.n, rhs.d);
        let second = gcd(rhs.n, self.d);
        let n = (self.n / first)
            .checked_mul(rhs.n / second)
            .ok_or(BmsErrorKind::Overflow)?;
        let d = (self.d / second)
            .checked_mul(rhs.d / first)
            .ok_or(BmsErrorKind::Overflow)?;
        Self::new(n, d)
    }
    pub(crate) fn ticks(self, resolution: u32) -> Result<i64, BmsErrorKind> {
        if i128::from(resolution) % self.d != 0 {
            return Err(BmsErrorKind::Resolution);
        }
        let ticks = self
            .n
            .checked_mul(i128::from(resolution) / self.d)
            .ok_or(BmsErrorKind::Overflow)?;
        i64::try_from(ticks).map_err(|_| BmsErrorKind::Overflow)
    }
}
pub(crate) fn decimal(text: &str, line: usize) -> Result<Ratio, BmsError> {
    let text = text.strip_prefix('+').unwrap_or(text);
    let mut n = 0i128;
    let mut d = 1i128;
    let mut dotted = false;
    let mut digits = 0usize;
    for byte in text.bytes() {
        if byte == b'.' && !dotted {
            dotted = true;
            continue;
        }
        if !byte.is_ascii_digit() {
            return Err(BmsError::new(
                line,
                BmsErrorKind::Syntax("plain decimal required; no exponent/sign suffix"),
            ));
        }
        digits += 1;
        if digits > 18 {
            return Err(BmsError::new(
                line,
                BmsErrorKind::Limit("decimal precision"),
            ));
        }
        n = n
            .checked_mul(10)
            .and_then(|v| v.checked_add(i128::from(byte - b'0')))
            .ok_or_else(|| BmsError::new(line, BmsErrorKind::Overflow))?;
        if dotted {
            d = d
                .checked_mul(10)
                .ok_or_else(|| BmsError::new(line, BmsErrorKind::Overflow))?;
        }
    }
    if digits == 0 {
        return Err(BmsError::new(line, BmsErrorKind::Syntax("empty decimal")));
    }
    Ratio::new(n, d).map_err(|kind| BmsError::new(line, kind))
}
