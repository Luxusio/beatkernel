//! Injectable diagnostic time and waiting, separate from gameplay clock evidence.

use crate::native_gameplay::NativeGameplayResult;
use std::time::Duration;

/// Effects owned by the outer native pump, never by judge or audio callbacks.
/// Control moments must not replace acquired input or presentation timestamps.
pub trait NativePumpControl {
    /// Ordered control-clock values with no gameplay clock-domain authority.
    type Moment: Copy + Ord;

    /// Reads the control clock used only for an optional diagnostic deadline.
    fn now(&mut self) -> NativeGameplayResult<Self::Moment>;

    /// Adds a nonnegative duration, refusing an unrepresentable deadline.
    fn checked_add(moment: Self::Moment, duration: Duration) -> Option<Self::Moment>;

    /// Waits outside gameplay processing; failure leaves cleanup to the caller.
    fn wait(&mut self, duration: Duration) -> NativeGameplayResult<()>;
}

/// A diagnostic cutoff cannot establish gameplay or output completion.
pub(crate) struct NativePumpDeadline<M> {
    deadline_and_last: Option<(M, M)>,
}

impl<M: Copy + Ord> NativePumpDeadline<M> {
    pub(crate) fn new<C: NativePumpControl<Moment = M>>(
        control: &mut C,
        seconds: Option<u64>,
        overflow_message: &'static str,
    ) -> NativeGameplayResult<Self> {
        let deadline_and_last = match seconds {
            Some(seconds) => {
                let now = control.now()?;
                let deadline =
                    C::checked_add(now, Duration::from_secs(seconds)).ok_or(overflow_message)?;
                Some((deadline, now))
            }
            None => None,
        };
        Ok(Self { deadline_and_last })
    }

    pub(crate) fn active<C: NativePumpControl<Moment = M>>(
        &mut self,
        control: &mut C,
    ) -> NativeGameplayResult<bool> {
        let Some((deadline, last)) = self.deadline_and_last else {
            return Ok(true);
        };
        let now = control.now()?;
        if now < last {
            return Err("native pump control clock regressed".into());
        }
        self.deadline_and_last = Some((deadline, now));
        Ok(now < deadline)
    }
}

#[cfg(test)]
mod fixtures {
    include!("native_pump_control_fixtures.rs");
}
