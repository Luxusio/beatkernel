//! Native system effects selected only by the outer gameplay composition bridge.

use crate::{native_gameplay::NativeGameplayResult, native_pump_control::NativePumpControl};
use std::time::{Duration, Instant};

pub(crate) struct SystemControl;

impl NativePumpControl for SystemControl {
    type Moment = Instant;

    fn now(&mut self) -> NativeGameplayResult<Instant> {
        Ok(Instant::now())
    }

    fn checked_add(moment: Instant, duration: Duration) -> Option<Instant> {
        moment.checked_add(duration)
    }

    fn wait(&mut self, duration: Duration) -> NativeGameplayResult<()> {
        std::thread::sleep(duration);
        Ok(())
    }
}
