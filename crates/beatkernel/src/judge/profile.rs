use std::collections::HashSet;

use crate::time::Duration;

use super::JudgeError;

/// An opaque caller-defined grade identity, with no built-in score ordering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JudgeGrade(pub u32);

/// Inclusive asymmetric timing bounds for one grade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JudgeWindow {
    /// Grade returned when this is the first matching window.
    pub grade: JudgeGrade,
    /// Nonnegative extent before the target.
    pub early: Duration,
    /// Nonnegative extent after the target.
    pub late: Duration,
}

/// Validated nested windows and a signed input-time correction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JudgeProfile {
    windows: Vec<JudgeWindow>,
    input_offset: Duration,
}

impl JudgeProfile {
    /// Constructs nonempty narrow-to-wide windows with unique grade identities.
    ///
    /// Bounds must be nonnegative and cannot shrink on either side. Positive
    /// offset moves mapped input and advance time later; it is applied once.
    pub fn new(windows: Vec<JudgeWindow>, input_offset: Duration) -> Result<Self, JudgeError> {
        if windows.is_empty() {
            return Err(JudgeError::InvalidProfile);
        }
        let mut grades = HashSet::new();
        let mut early = Duration::ZERO;
        let mut late = Duration::ZERO;
        for window in &windows {
            if window.early < early || window.late < late || !grades.insert(window.grade) {
                return Err(JudgeError::InvalidProfile);
            }
            early = window.early;
            late = window.late;
        }
        Ok(Self {
            windows,
            input_offset,
        })
    }

    /// Borrows the validated caller-ordered windows.
    pub fn windows(&self) -> &[JudgeWindow] {
        &self.windows
    }

    /// Returns the first inclusive match for a wide signed nanosecond delta.
    pub fn grade(&self, delta: i128) -> Option<JudgeGrade> {
        self.windows.iter().find_map(|window| {
            (delta >= -i128::from(window.early.as_nanos())
                && delta <= i128::from(window.late.as_nanos()))
            .then_some(window.grade)
        })
    }

    /// Returns the widest early bound.
    pub fn max_early(&self) -> Duration {
        self.windows[self.windows.len() - 1].early
    }

    /// Returns the widest late bound.
    pub fn max_late(&self) -> Duration {
        self.windows[self.windows.len() - 1].late
    }

    /// Returns the signed correction applied to mapped song time.
    pub const fn input_offset(&self) -> Duration {
        self.input_offset
    }
}
