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
