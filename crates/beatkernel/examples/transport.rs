//! Deterministic pause, resume, seek, and reverse transport without an OS clock.

use beatkernel::time::Timestamp;
use beatkernel::transport::{Rate, Transport, TransportError};

fn main() -> Result<(), TransportError> {
    let ns = Timestamp::from_nanos;
    let mut transport = Transport::new(ns(0), ns(0), Rate::NORMAL);
    transport.pause(ns(1_000_000_000))?;
    transport.resume(ns(2_000_000_000))?;
    transport.set_rate(ns(3_000_000_000), Rate::REVERSE)?;
    transport.seek(ns(4_000_000_000), ns(-500_000_000))?;

    for host in [500_000_000, 1_500_000_000, 2_500_000_000, 4_500_000_000] {
        println!(
            "host={host} ns -> song={} ns",
            transport.position_at(ns(host))?.as_nanos()
        );
    }
    Ok(())
}
