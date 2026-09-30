//! Explicit checked mach absolute-time sampling and origin normalization.
use super::ffi;
use beatkernel::time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp};
use std::io;
/// Shared native mach clock and one explicit origin/domain mapping.
#[derive(Clone, Copy, Debug)]
pub struct MachClock {
    numer: u32,
    denom: u32,
    origin: u64,
    native: ClockDomainId,
    host: ClockDomainId,
}
/// One unmodified tick sample with absolute/native and normalized host points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MachSample {
    /// Raw mach_absolute_time counter, preserved without truncation.
    pub ticks: u64,
    /// Absolute mach nanoseconds in the declared native domain.
    pub native: ClockPoint,
    /// Elapsed nanoseconds from this clock's explicit initialization origin.
    pub normalized: ClockPoint,
}
impl MachClock {
    /// Caches the native timebase and samples one explicit origin.
    /// Native and host IDs must differ because their timestamp origins differ.
    pub fn new(native: ClockDomainId, host: ClockDomainId) -> io::Result<Self> {
        if native == host {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "mach absolute and origin-relative clocks require distinct domains",
            ));
        }
        let mut timebase = ffi::Timebase::default();
        // SAFETY: aligned writable mach_timebase_info_data_t; API retains no pointer.
        let status = unsafe { ffi::mach_timebase_info(&mut timebase) };
        if status != 0 || timebase.numer == 0 || timebase.denom == 0 {
            return Err(io::Error::other(format!(
                "mach_timebase_info failed: {status}"
            )));
        }
        // SAFETY: mach_absolute_time has no arguments or ownership requirements.
        let origin = unsafe { ffi::mach_absolute_time() };
        Ok(Self {
            numer: timebase.numer,
            denom: timebase.denom,
            origin,
            native,
            host,
        })
    }
    /// Samples the native mach counter exactly once.
    pub fn sample(&self) -> io::Result<MachSample> {
        // SAFETY: process-local read-only kernel time primitive.
        let ticks = unsafe { ffi::mach_absolute_time() };
        self.at_ticks(ticks)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "mach timestamp overflow"))
    }
    /// Samples without constructing an allocated error on conversion failure.
    /// None remains unavailable for callback diagnostics; it is not zero time.
    pub fn sample_realtime(&self) -> Option<MachSample> {
        // SAFETY: read-only native time primitive; no pointers or ownership.
        self.at_ticks(unsafe { ffi::mach_absolute_time() })
    }

    /// Converts saved native ticks, retaining their exact raw value.
    pub fn at_ticks(&self, ticks: u64) -> Option<MachSample> {
        let native = self.native_point(ticks)?;
        let timestamp = self.map(native, self.host)?;
        Some(MachSample {
            ticks,
            native,
            normalized: ClockPoint {
                domain: self.host,
                timestamp,
            },
        })
    }
    /// Absolute mach nanoseconds, independent of the initialization origin.
    pub fn native_point(&self, ticks: u64) -> Option<ClockPoint> {
        let nanos = u128::from(ticks) * u128::from(self.numer) / u128::from(self.denom);
        Some(ClockPoint {
            domain: self.native,
            timestamp: Timestamp::from_nanos(i64::try_from(nanos).ok()?),
        })
    }
    /// Explicit absolute mach domain.
    pub const fn native_domain(&self) -> ClockDomainId {
        self.native
    }
    /// Explicit origin-relative host domain.
    pub const fn host_domain(&self) -> ClockDomainId {
        self.host
    }
}
impl ClockMapper for MachClock {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        if from.domain == to {
            return Some(from.timestamp);
        }
        let origin = self.native_point(self.origin)?.timestamp.as_nanos();
        let nanos = if from.domain == self.native && to == self.host {
            i128::from(from.timestamp.as_nanos()) - i128::from(origin)
        } else if from.domain == self.host && to == self.native {
            i128::from(from.timestamp.as_nanos()) + i128::from(origin)
        } else {
            return None;
        };
        Some(Timestamp::from_nanos(i64::try_from(nanos).ok()?))
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_rational_timebase_preserves_ticks_and_quantized_endpoints() {
        let clock = MachClock {
            numer: 2,
            denom: 3,
            origin: 2,
            native: ClockDomainId(1),
            host: ClockDomainId(2),
        };
        let sample = clock.at_ticks(4).unwrap();
        assert_eq!(sample.ticks, 4);
        assert_eq!(sample.native.timestamp.as_nanos(), 2);
        assert_eq!(sample.normalized.timestamp.as_nanos(), 1);
        assert_eq!(sample.native.domain, ClockDomainId(1));
        assert_eq!(sample.normalized.domain, ClockDomainId(2));
        assert_eq!(
            clock.at_ticks(0).unwrap().normalized.timestamp.as_nanos(),
            -1
        );
        assert_eq!(
            clock.at_ticks(2).unwrap().normalized.timestamp.as_nanos(),
            0
        );
        assert_eq!(
            clock.map(sample.normalized, clock.native_domain()),
            Some(sample.native.timestamp)
        );
        assert_eq!(clock.map(sample.native, ClockDomainId(3)), None);
    }
    #[test]
    fn native_timestamp_overflow_never_becomes_zero_time() {
        let clock = MachClock {
            numer: 1,
            denom: 1,
            origin: 0,
            native: ClockDomainId(1),
            host: ClockDomainId(2),
        };
        assert_eq!(
            clock.at_ticks(i64::MAX as u64).unwrap().native.timestamp,
            Timestamp::MAX
        );
        assert_eq!(clock.at_ticks(i64::MAX as u64 + 1), None);
        assert_eq!(clock.at_ticks(u64::MAX), None);
    }
}
