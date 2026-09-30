//! Native QPC receipt sampling over the portable checked integer mapping.

#![allow(unsafe_code)]

use std::io;

use beatkernel::time::{ClockDomainId, ClockMapper, ClockPoint};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

use crate::raw_input::QpcClockMapping;

/// A receipt sample, not a device hardware-event timestamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QpcReceipt {
    /// Unmodified QPC counter value in ticks.
    pub counter: i64,
    /// Cached QPC ticks per second.
    pub frequency: i64,
    /// Absolute QPC time converted to integer nanoseconds.
    pub native: ClockPoint,
    /// Elapsed host time from this clock's explicit initialization origin.
    pub normalized: ClockPoint,
}

/// A multimedia timer reading bracketed by the application's shared QPC clock.
///
/// This retains acquisition bounds, not a hardware-event timestamp or a claim
/// about multimedia timer resolution. The raw value wraps every 2^32 ms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MultimediaClockReceipt {
    /// QPC sampled immediately before the timer query.
    pub before: QpcReceipt,
    /// Unmodified `timeGetTime` milliseconds since boot, modulo 2^32.
    pub milliseconds: u32,
    /// QPC sampled immediately after the timer query.
    pub after: QpcReceipt,
}

/// A native QPC sampler with one explicit origin and output domain.
///
/// Construct one clock in the application composition root and copy it into
/// consumers that must share host time. Constructing two clocks separately
/// selects two different origins, even if the same domain ID is supplied.
/// Sampling may fail; it never invents a counter or a hardware timestamp.
#[derive(Clone, Copy, Debug)]
pub struct QpcClock {
    mapping: QpcClockMapping,
    output: ClockDomainId,
}

impl QpcClock {
    /// Samples the multimedia timer between two QPC observations.
    ///
    /// Runs off the real-time callback. This does not request a new timer
    /// period, infer timer accuracy, or assume ASIO drivers use this clock.
    /// Consumers must supply finite validity and honest measurement/drift error
    /// bounds when constructing a multimedia clock relation.
    pub fn sample_multimedia(&self) -> io::Result<MultimediaClockReceipt> {
        let before = self.sample()?;
        // SAFETY: this synchronous WinMM query takes no pointers and retains
        // no application state. It returns the native wrapping DWORD directly.
        let milliseconds = unsafe { windows::Win32::Media::timeGetTime() };
        let after = self.sample()?;
        if after.counter < before.counter {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "QPC moved backwards during multimedia clock acquisition",
            ));
        }
        Ok(MultimediaClockReceipt {
            before,
            milliseconds,
            after,
        })
    }

    /// Queries frequency and initialization counter, validating the time mapping.
    pub fn new(output: ClockDomainId) -> io::Result<Self> {
        let mut frequency = 0i64;
        // SAFETY: the pointer references a live, aligned, writable i64 for the
        // synchronous API call; Windows stores one LARGE_INTEGER and retains no pointer.
        if unsafe { QueryPerformanceFrequency(&mut frequency) } == 0 {
            return Err(os_error("QueryPerformanceFrequency"));
        }
        let origin = counter()?;
        let mapping = QpcClockMapping::new(frequency, origin, output)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(Self { mapping, output })
    }

    /// Samples QPC once and returns raw, absolute and normalized receipt time.
    pub fn sample(&self) -> io::Result<QpcReceipt> {
        let counter = counter()?;
        let native = self
            .mapping
            .point(counter)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let timestamp = self.mapping.map(native, self.output).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "QPC host-time mapping overflow")
        })?;
        Ok(QpcReceipt {
            counter,
            frequency: self.mapping.frequency(),
            native,
            normalized: ClockPoint {
                domain: self.output,
                timestamp,
            },
        })
    }

    /// Samples QPC without constructing or formatting an error on failure.
    /// Suitable for render diagnostics; None is unavailable, never zero time.
    pub fn sample_realtime(&self) -> Option<QpcReceipt> {
        let mut value = 0i64;
        // SAFETY: writable aligned counter; synchronous API retains no pointer.
        if unsafe { QueryPerformanceCounter(&mut value) } == 0 {
            return None;
        }
        self.at_counter(value)
    }

    // Pure conversion boundary shared with fixtures; no OS/error allocation.
    fn at_counter(&self, value: i64) -> Option<QpcReceipt> {
        let native = self.mapping.point(value).ok()?;
        let timestamp = self.mapping.map(native, self.output)?;
        Some(QpcReceipt {
            counter: value,
            frequency: self.mapping.frequency(),
            native,
            normalized: ClockPoint {
                domain: self.output,
                timestamp,
            },
        })
    }

    /// Returns the immutable mapping/origin used by every sample.
    pub const fn mapping(&self) -> &QpcClockMapping {
        &self.mapping
    }

    /// Returns the explicitly assigned normalized host clock domain.
    pub const fn output_domain(&self) -> ClockDomainId {
        self.output
    }
}

fn counter() -> io::Result<i64> {
    let mut value = 0i64;
    // SAFETY: value is a live, aligned, writable LARGE_INTEGER-sized i64;
    // QueryPerformanceCounter is synchronous and retains no reference.
    if unsafe { QueryPerformanceCounter(&mut value) } == 0 {
        return Err(os_error("QueryPerformanceCounter"));
    }
    Ok(value)
}

fn os_error(operation: &'static str) -> io::Error {
    let error = io::Error::last_os_error();
    io::Error::new(error.kind(), format!("{operation}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quantized_absolute_endpoints_preserve_raw_ticks_and_signed_origin() {
        let clock = QpcClock {
            mapping: QpcClockMapping::new(3, 2, ClockDomainId(9)).unwrap(),
            output: ClockDomainId(9),
        };
        // Quantize endpoints separately: floor(4/3 s) - floor(2/3 s).
        let sample = clock.at_counter(4).unwrap();
        assert_eq!((sample.counter, sample.frequency), (4, 3));
        assert_eq!(sample.native.timestamp.as_nanos(), 1_333_333_333);
        assert_eq!(sample.normalized.timestamp.as_nanos(), 666_666_667);
        assert_eq!(sample.normalized.domain, ClockDomainId(9));
        assert_ne!(sample.native.domain, sample.normalized.domain);
        assert_eq!(
            clock.at_counter(1).unwrap().normalized.timestamp.as_nanos(),
            -333_333_333
        );
        assert_eq!(
            clock.at_counter(2).unwrap().normalized.timestamp.as_nanos(),
            0
        );
    }
    #[test]
    fn invalid_or_unrepresentable_counters_remain_absent() {
        let clock = QpcClock {
            mapping: QpcClockMapping::new(1, 0, ClockDomainId(9)).unwrap(),
            output: ClockDomainId(9),
        };
        assert_eq!(clock.at_counter(-1), None);
        assert_eq!(clock.at_counter(i64::MAX), None);
        assert_eq!(
            clock
                .at_counter(9_223_372_036)
                .unwrap()
                .native
                .timestamp
                .as_nanos(),
            9_223_372_036_000_000_000
        );
        assert_eq!(clock.at_counter(9_223_372_037), None);
    }
}
