use beatkernel::time::{
    ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp,
};
use beatkernel::transport::{Rate, RateError, Transport, TransportError};

fn ts(nanos: i64) -> Timestamp {
    Timestamp::from_nanos(nanos)
}

fn ns(nanos: i64) -> Duration {
    Duration::from_nanos(nanos)
}

fn rate(numerator: i64, denominator: u64) -> Rate {
    Rate::new(numerator, denominator).unwrap()
}

#[test]
fn signed_time_arithmetic_is_checked_at_both_boundaries() {
    for value in [i64::MIN, -1, 0, 1, i64::MAX] {
        assert_eq!(ts(value).as_nanos(), value);
        assert_eq!(ns(value).as_nanos(), value);
    }
    assert_eq!(Timestamp::ZERO, ts(0));
    assert_eq!(Timestamp::MIN, ts(i64::MIN));
    assert_eq!(Timestamp::MAX, ts(i64::MAX));
    assert_eq!(Duration::ZERO, ns(0));
    assert_eq!(Duration::MIN, ns(i64::MIN));
    assert_eq!(Duration::MAX, ns(i64::MAX));
    assert_eq!(Timestamp::MAX.checked_add(ns(1)), None);
    assert_eq!(Timestamp::MIN.checked_add(ns(-1)), None);
    assert_eq!(Timestamp::MIN.checked_sub(ns(1)), None);
    assert_eq!(Timestamp::MAX.checked_sub(ns(-1)), None);
    assert_eq!(ts(-3).checked_add(ns(5)), Some(ts(2)));
    assert_eq!(ts(-3).checked_sub(ns(-5)), Some(ts(2)));
    assert_eq!(ts(-3).checked_duration_since(ts(2)), Some(ns(-5)));
    assert_eq!(Timestamp::MAX.checked_duration_since(Timestamp::MIN), None);
    assert_eq!(Timestamp::MIN.checked_duration_since(Timestamp::MAX), None);
    assert_eq!(Duration::MAX.checked_add(ns(1)), None);
    assert_eq!(Duration::MIN.checked_add(ns(-1)), None);
    assert_eq!(Duration::MIN.checked_sub(ns(1)), None);
    assert_eq!(Duration::MAX.checked_sub(ns(-1)), None);
    assert_eq!(ns(-3).checked_add(ns(5)), Some(ns(2)));
    assert_eq!(ns(-3).checked_sub(ns(5)), Some(ns(-8)));
    assert_eq!(Duration::MIN.checked_neg(), None);
    assert_eq!(Duration::MAX.checked_neg(), Some(ns(-i64::MAX)));
    assert_eq!(ns(-2).checked_mul(-3), Some(ns(6)));
    assert_eq!(Duration::MIN.checked_mul(-1), None);
    assert_eq!(Duration::MAX.checked_mul(2), None);
    assert_eq!(Duration::MIN.checked_mul(0), Some(Duration::ZERO));
    assert_eq!(Duration::MIN.checked_mul(1), Some(Duration::MIN));
}

struct OffsetMapper {
    source: ClockDomainId,
    target: ClockDomainId,
    offset: Duration,
    quality: ClockMappingQuality,
}

impl ClockMapper for OffsetMapper {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        if from.domain == to {
            Some(from.timestamp)
        } else if from.domain == self.source && to == self.target {
            from.timestamp.checked_add(self.offset)
        } else {
            None
        }
    }

    fn quality(&self) -> ClockMappingQuality {
        self.quality
    }
}

#[test]
fn clock_mapping_preserves_domains_and_reports_unavailable_results() {
    let host = ClockDomainId(1);
    let device = ClockDomainId(2);
    let unrelated = ClockDomainId(3);
    let mapper = OffsetMapper {
        source: host,
        target: device,
        offset: ns(17),
        quality: ClockMappingQuality::Estimated { max_error: ns(3) },
    };
    let point = ClockPoint {
        domain: host,
        timestamp: ts(-7),
    };
    assert_eq!(mapper.map(point, host), Some(ts(-7)));
    assert_eq!(mapper.map(point, device), Some(ts(10)));
    assert_eq!(mapper.map(point, unrelated), None);
    assert_eq!(
        mapper.map(
            ClockPoint {
                domain: device,
                timestamp: ts(10)
            },
            host
        ),
        None
    );
    assert_eq!(
        mapper.map(
            ClockPoint {
                domain: host,
                timestamp: Timestamp::MAX
            },
            device
        ),
        None
    );
    assert_eq!(
        mapper.quality(),
        ClockMappingQuality::Estimated { max_error: ns(3) }
    );
    for quality in [ClockMappingQuality::Exact, ClockMappingQuality::Unknown] {
        let mapper = OffsetMapper {
            source: host,
            target: device,
            offset: Duration::ZERO,
            quality,
        };
        assert_eq!(mapper.quality(), quality);
    }
}

#[test]
fn rational_rates_normalize_minimum_numerator_and_truncate_toward_zero() {
    assert_eq!(Rate::new(1, 0), Err(RateError::ZeroDenominator));
    assert_eq!(rate(6, 8), rate(3, 4));
    assert_eq!(rate(-6, 8), rate(-3, 4));
    assert_eq!(rate(0, u64::MAX), Rate::ZERO);
    assert_eq!(Rate::ZERO.numerator(), 0);
    assert_eq!(Rate::ZERO.denominator(), 1);
    assert_eq!(Rate::NORMAL, rate(1, 1));
    assert_eq!(Rate::REVERSE, rate(-1, 1));
    assert_eq!(rate(i64::MIN, 1).numerator(), i64::MIN);
    assert_eq!(rate(i64::MIN, 1 << 63), Rate::REVERSE);
    assert_eq!(rate(i64::MIN, 2).numerator(), -(1_i64 << 62));
    assert_eq!(rate(i64::MIN, 2).denominator(), 1);
    assert_eq!(rate(i64::MIN, u64::MAX).denominator(), u64::MAX);
    assert_eq!(rate(1, 2).scale(ns(3)), Some(ns(1)));
    assert_eq!(rate(1, 2).scale(ns(-3)), Some(ns(-1)));
    assert_eq!(rate(-1, 2).scale(ns(3)), Some(ns(-1)));
    assert_eq!(rate(-1, 2).scale(ns(-3)), Some(ns(1)));
    assert_eq!(rate(2, 1).scale(Duration::MAX), None);
    assert_eq!(Rate::REVERSE.scale(Duration::MIN), None);
    assert_eq!(Rate::ZERO.scale(Duration::MIN), Some(Duration::ZERO));
    assert_eq!(rate(1, u64::MAX).scale(Duration::MAX), Some(Duration::ZERO));
    assert_eq!(
        rate(i64::MIN, u64::MAX).scale(Duration::MIN),
        Some(ns(1_i64 << 62))
    );
}

#[test]
fn piecewise_history_has_continuous_changes_and_latest_same_time_anchor() {
    let mut transport = Transport::new(ts(10), ts(-20), Rate::NORMAL);
    assert_eq!(
        transport.position_at(ts(9)),
        Err(TransportError::BeforeOrigin)
    );
    assert_eq!(transport.position_at(ts(20)), Ok(ts(-10)));
    transport.set_rate(ts(20), rate(2, 1)).unwrap();
    assert_eq!(transport.position_at(ts(25)), Ok(ts(0)));
    transport.pause(ts(25)).unwrap();
    assert!(transport.is_paused());
    assert_eq!(transport.position_at(ts(100)), Ok(ts(0)));
    transport.seek(ts(30), ts(-50)).unwrap();
    assert!(transport.is_paused());
    transport.resume(ts(40)).unwrap();
    assert_eq!(transport.position_at(ts(45)), Ok(ts(-40)));
    transport.set_rate(ts(45), Rate::REVERSE).unwrap();
    transport.seek(ts(45), ts(7)).unwrap();
    transport.set_rate(ts(45), rate(-2, 1)).unwrap();
    assert_eq!(transport.position_at(ts(45)), Ok(ts(7)));
    assert_eq!(transport.position_at(ts(49)), Ok(ts(-1)));
    for (host, song) in [
        (10, -20),
        (19, -11),
        (20, -10),
        (24, -2),
        (25, 0),
        (29, 0),
        (30, -50),
        (39, -50),
        (40, -50),
        (44, -42),
    ] {
        assert_eq!(transport.position_at(ts(host)), Ok(ts(song)), "host={host}");
    }
    assert_eq!(transport.anchor().host_time, ts(45));
    assert_eq!(transport.anchor().song_time, ts(7));
    assert_eq!(transport.anchor().rate, rate(-2, 1));
    assert_eq!(transport.anchors().len(), 8);
}

#[test]
fn pause_and_zero_rate_remember_reverse_and_initial_zero_resumes_normal() {
    let mut transport = Transport::new(ts(0), ts(20), rate(-3, 2));
    transport.pause(ts(4)).unwrap();
    transport.pause(ts(5)).unwrap();
    transport.seek(ts(6), ts(-10)).unwrap();
    transport.resume(ts(8)).unwrap();
    assert_eq!(transport.anchor().rate, rate(-3, 2));
    assert_eq!(transport.position_at(ts(10)), Ok(ts(-13)));
    transport.set_rate(ts(10), Rate::ZERO).unwrap();
    transport.resume(ts(20)).unwrap();
    assert_eq!(transport.position_at(ts(22)), Ok(ts(-16)));
    transport.pause(ts(22)).unwrap();
    transport.set_rate(ts(23), rate(2, 1)).unwrap();
    assert!(!transport.is_paused());
    assert_eq!(transport.position_at(ts(25)), Ok(ts(-12)));

    let mut initially_paused = Transport::new(ts(-2), ts(5), Rate::ZERO);
    initially_paused.resume(ts(0)).unwrap();
    assert_eq!(initially_paused.anchor().rate, Rate::NORMAL);
    assert_eq!(initially_paused.position_at(ts(3)), Ok(ts(8)));
}

#[test]
fn fractional_noops_preserve_progression_and_advance_command_chronology() {
    let half = rate(1, 2);
    let mut transport = Transport::new(ts(0), ts(0), half);
    let initial_anchor = transport.anchor();
    for host in 1..=11 {
        transport.set_rate(ts(host), rate(2, 4)).unwrap();
        transport.resume(ts(host)).unwrap();
        assert_eq!(transport.position_at(ts(host)), Ok(ts(host / 2)));
        assert_eq!(transport.anchor(), initial_anchor);
        assert_eq!(transport.anchors().len(), 1);
    }
    assert_eq!(
        transport.pause(ts(10)),
        Err(TransportError::NonMonotonicHost)
    );
    assert_eq!(
        transport.seek(ts(10), ts(99)),
        Err(TransportError::NonMonotonicHost)
    );
    assert_eq!(transport.position_at(ts(2)), Ok(ts(1)));
    transport.pause(ts(11)).unwrap();
    let paused_anchor = transport.anchor();
    transport.pause(ts(20)).unwrap();
    transport.set_rate(ts(21), Rate::ZERO).unwrap();
    assert_eq!(transport.anchor(), paused_anchor);
    assert_eq!(
        transport.resume(ts(20)),
        Err(TransportError::NonMonotonicHost)
    );
    transport.resume(ts(21)).unwrap();
    assert_eq!(transport.position_at(ts(23)), Ok(ts(6)));

    let mut negative = Transport::new(ts(0), ts(0), rate(-1, 2));
    for host in 1..=5 {
        negative.resume(ts(host)).unwrap();
        negative.set_rate(ts(host), rate(-2, 4)).unwrap();
    }
    assert_eq!(negative.position_at(ts(5)), Ok(ts(-2)));
}

#[test]
fn true_fractional_changes_quantize_each_segment() {
    let mut transport = Transport::new(ts(0), ts(0), rate(1, 2));
    transport.set_rate(ts(1), rate(1, 3)).unwrap();
    assert_eq!(transport.position_at(ts(3)), Ok(ts(0)));
    transport.set_rate(ts(3), rate(-1, 2)).unwrap();
    assert_eq!(transport.position_at(ts(6)), Ok(ts(-1)));
}

#[test]
fn full_host_span_uses_wide_math_and_narrows_only_final_position() {
    let paused = Transport::new(Timestamp::MIN, ts(37), Rate::ZERO);
    assert_eq!(paused.position_at(Timestamp::MAX), Ok(ts(37)));
    let tiny = Transport::new(Timestamp::MIN, ts(0), rate(1, u64::MAX));
    assert_eq!(tiny.position_at(Timestamp::MAX), Ok(ts(1)));
    assert_eq!(tiny.position_at(ts(i64::MAX - 1)), Ok(ts(0)));
    let cancelled = Transport::new(Timestamp::MIN, Timestamp::MIN, Rate::NORMAL);
    assert_eq!(cancelled.position_at(Timestamp::MAX), Ok(Timestamp::MAX));
    let reverse_cancelled = Transport::new(Timestamp::MIN, Timestamp::MAX, Rate::REVERSE);
    assert_eq!(
        reverse_cancelled.position_at(Timestamp::MAX),
        Ok(Timestamp::MIN)
    );
    let overflow = Transport::new(Timestamp::MIN, ts(0), Rate::NORMAL);
    assert_eq!(
        overflow.position_at(Timestamp::MAX),
        Err(TransportError::Overflow)
    );
    let large_product = Transport::new(Timestamp::MIN, ts(0), rate(i64::MIN, 1));
    assert_eq!(
        large_product.position_at(Timestamp::MAX),
        Err(TransportError::Overflow)
    );
}

#[test]
fn failed_commands_are_atomic_and_seek_recovers_an_overflowing_trajectory() {
    let mut transport = Transport::new(ts(0), ts(i64::MAX - 1), rate(2, 1));
    let before = transport.clone();
    for failure in [
        transport.set_rate(ts(10), Rate::REVERSE),
        transport.pause(ts(10)),
        transport.resume(ts(10)),
        transport.set_rate(ts(10), rate(2, 1)),
    ] {
        assert_eq!(failure, Err(TransportError::Overflow));
    }
    assert_eq!(transport, before);
    transport.seek(ts(1), ts(-3)).unwrap();
    assert_eq!(transport.position_at(ts(2)), Ok(ts(-1)));
    assert_eq!(transport.anchor().rate, rate(2, 1));
    let before = transport.clone();
    assert_eq!(
        transport.seek(ts(0), ts(0)),
        Err(TransportError::NonMonotonicHost)
    );
    assert_eq!(
        transport.set_rate(ts(0), Rate::NORMAL),
        Err(TransportError::NonMonotonicHost)
    );
    assert_eq!(
        transport.pause(ts(0)),
        Err(TransportError::NonMonotonicHost)
    );
    assert_eq!(
        transport.resume(ts(0)),
        Err(TransportError::NonMonotonicHost)
    );
    assert_eq!(transport, before);
    transport.seek(ts(1), ts(8)).unwrap();
    assert_eq!(transport.position_at(ts(1)), Ok(ts(8)));

    let mut origin = Transport::new(ts(10), ts(0), Rate::NORMAL);
    let before = origin.clone();
    assert_eq!(
        origin.seek(ts(9), ts(0)),
        Err(TransportError::NonMonotonicHost)
    );
    assert_eq!(origin, before);
}

// Independent oracle: advance one host nanosecond at a time, carrying the
// division remainder. Actual commands discard the remainder; no-ops retain it.
// It never calls Rate::scale or computes a delta from a production anchor.
struct TickOracle {
    host: i64,
    song: i64,
    numerator: i64,
    denominator: i64,
    remainder: i64,
    remembered: (i64, i64),
    history: Vec<(i64, i64)>,
    anchor_count: usize,
}

impl TickOracle {
    fn new() -> Self {
        Self {
            host: 0,
            song: -200,
            numerator: 1,
            denominator: 1,
            remainder: 0,
            remembered: (1, 1),
            history: vec![(0, -200)],
            anchor_count: 1,
        }
    }

    fn advance(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.host += 1;
            self.remainder += self.numerator;
            self.song += self.remainder / self.denominator;
            self.remainder %= self.denominator;
            self.history.push((self.host, self.song));
        }
    }

    fn same_rate(&self, numerator: i64, denominator: i64) -> bool {
        self.numerator * denominator == numerator * self.denominator
    }

    fn set_rate(&mut self, numerator: i64, denominator: i64) {
        if !self.same_rate(numerator, denominator) {
            self.numerator = numerator;
            self.denominator = denominator;
            self.remainder = 0;
            self.anchor_count += 1;
            if numerator != 0 {
                self.remembered = (numerator, denominator);
            }
        }
    }

    fn pause(&mut self) {
        self.set_rate(0, 1);
    }

    fn resume(&mut self) {
        if self.numerator == 0 {
            self.set_rate(self.remembered.0, self.remembered.1);
        }
    }

    fn seek(&mut self, song: i64) {
        self.song = song;
        self.remainder = 0;
        self.anchor_count += 1;
        self.history.last_mut().unwrap().1 = song;
    }
}

fn random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

#[test]
fn seeded_sequences_match_incremental_oracle_after_1200_rate_changes_each() {
    let choices = [
        (1, 1),
        (1, 2),
        (2, 1),
        (-1, 1),
        (-3, 2),
        (5, 7),
        (-5, 3),
        (0, 1),
        (2, 3),
    ];
    for seed in [1, 0xfeed_cafe, 0x9e37_79b9_7f4a_7c15, u64::MAX] {
        let mut rng = seed;
        let mut oracle = TickOracle::new();
        let mut transport = Transport::new(ts(0), ts(-200), Rate::NORMAL);
        let mut actual_changes = 0;
        for step in 0..1200 {
            oracle.advance(random(&mut rng) % 9);
            assert_eq!(
                transport.position_at(ts(oracle.host)),
                Ok(ts(oracle.song)),
                "seed={seed} step={step} before rate change"
            );
            let mut index = random(&mut rng) as usize % choices.len();
            while oracle.same_rate(choices[index].0, choices[index].1) {
                index = (index + 1) % choices.len();
            }
            let (numerator, denominator) = choices[index];
            transport
                .set_rate(ts(oracle.host), rate(numerator, denominator as u64))
                .unwrap();
            oracle.set_rate(numerator, denominator);
            actual_changes += 1;
            match random(&mut rng) % 6 {
                0 => {
                    transport.pause(ts(oracle.host)).unwrap();
                    oracle.pause();
                }
                1 => {
                    transport.resume(ts(oracle.host)).unwrap();
                    oracle.resume();
                }
                2 => {
                    let song = (random(&mut rng) % 2001) as i64 - 1000;
                    transport.seek(ts(oracle.host), ts(song)).unwrap();
                    oracle.seek(song);
                }
                3 => {
                    transport
                        .set_rate(ts(oracle.host), rate(numerator * 2, denominator as u64 * 2))
                        .unwrap();
                    oracle.set_rate(numerator * 2, denominator * 2);
                }
                4 => {
                    transport.pause(ts(oracle.host)).unwrap();
                    oracle.pause();
                    transport.pause(ts(oracle.host)).unwrap();
                    oracle.pause();
                    transport.resume(ts(oracle.host)).unwrap();
                    oracle.resume();
                }
                _ => {}
            }
            assert_eq!(
                transport.position_at(ts(oracle.host)),
                Ok(ts(oracle.song)),
                "seed={seed} step={step} after commands"
            );
            assert_eq!(transport.is_paused(), oracle.numerator == 0);
            assert_eq!(transport.anchors().len(), oracle.anchor_count);
            let &(host, song) = &oracle.history[random(&mut rng) as usize % oracle.history.len()];
            assert_eq!(
                transport.position_at(ts(host)),
                Ok(ts(song)),
                "seed={seed} step={step} historical host={host}"
            );
        }
        assert!(actual_changes >= 1000);
        oracle.advance(31);
        for (host, song) in oracle.history {
            assert_eq!(
                transport.position_at(ts(host)),
                Ok(ts(song)),
                "seed={seed} final history host={host}"
            );
        }
    }
}
