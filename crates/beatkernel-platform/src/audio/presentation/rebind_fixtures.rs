//! Deferred native metadata admission without acquiring a backend or clock.
use super::*;
use crate::audio::{AudioClockSnapshot, AudioClockReadingQuality, AudioStreamStatus, StreamCounters};
use crate::audio::asio::{AsioPresentationObservation, MultimediaHostInterval};
use beatkernel::audio::{AudioCounters, RenderReport};
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn snapshot(position: u64, frequency: u64, host: i64) -> AudioStreamSnapshot {
    AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters::default(),
        render: None,
        clock: Some(AudioClockSnapshot {
            position,
            frequency,
            qpc_100ns: host.unsigned_abs() / 100,
            reading_quality: AudioClockReadingQuality::Accurate,
            host_point: Some(point(1, host)),
            mapping_quality: ClockMappingQuality::Unknown,
        }),
    }
}
fn observer() -> PresentationDiscipline {
    PresentationDiscipline::new(
        DisciplineConfig::default(),
        point(2, 0),
        ClockDomainId(1),
        Timestamp::ZERO,
    )
    .unwrap()
}
fn asio(rate: u32, start: u64, host: i64) -> AsioPresentationObservation {
    let report = RenderReport {
        start_frame: start,
        frames: 64,
        playback_start_frame: start,
        playback_frames: 64,
        paused: false,
        playback_end_physical_frame: None,
        active_voices: 0,
        pending_commands: 0,
        song_position: Timestamp::ZERO,
        producer_disconnected: false,
        counters: AudioCounters::default(),
    };
    AsioPresentationObservation::from_render(
        report,
        rate,
        MultimediaHostInterval {
            before: point(1, host),
            after: point(1, host + 100),
        },
        0,
        0,
        point(2, 0),
    )
    .unwrap()
}
#[test]
fn new_epoch_resets_wasapi_frequency_and_rejects_delayed_old_snapshot_before_source_selection() {
    let mut discipline = observer();
    discipline
        .observe_in_epoch(0, snapshot(1_000_000_000, 1_000_000_000, 1_000_000_000))
        .unwrap();
    discipline
        .rebind_output(
            1,
            point(2, 0),
            point(2, 0),
            Timestamp::from_nanos(604_800_000_000_000),
        )
        .unwrap();
    assert!(discipline.validator.latest_record().is_none());
    assert!(discipline.latest_pair().is_none());
    assert_eq!(
        discipline.observe_in_epoch(0, snapshot(3, 0, 9_000_000_000)),
        Err(DisciplineError::EpochMismatch)
    );
    assert!(discipline.validator.latest_record().is_none());
    discipline
        .observe_in_epoch(1, snapshot(48_000, 48_000, 5_000_000_000))
        .unwrap();
    let source = discipline.validator.latest_record();
    let pair = discipline.latest_pair();
    assert_eq!(
        discipline.observe_in_epoch(1, snapshot(48_001, 44_100, 5_100_000_000)),
        Err(DisciplineError::FrequencyChanged)
    );
    assert_eq!(discipline.validator.latest_record(), source);
    assert_eq!(discipline.latest_pair(), pair);
}
#[test]
fn refused_rebind_preserves_original_identity_and_mixing_remains_refused_within_successful_epoch() {
    let mut discipline = observer();
    discipline
        .observe_in_epoch(0, snapshot(1_000_000_000, 1_000_000_000, 1_000_000_000))
        .unwrap();
    let source = discipline.validator.latest_record();
    let pair = discipline.latest_pair();
    assert_eq!(
        discipline.rebind_output(1, point(3, 1), point(4, 1), Timestamp::ZERO),
        Err(DisciplineError::DomainMismatch)
    );
    assert_eq!(discipline.epoch(), 0);
    assert_eq!(discipline.validator.latest_record(), source);
    assert_eq!(discipline.latest_pair(), pair);
    discipline
        .rebind_output(1, point(2, 0), point(2, 0), Timestamp::ZERO)
        .unwrap();
    let supplied = ClockPair {
        source: point(2, 1),
        target: point(1, 1),
    };
    discipline.observe_clock_pair_in_epoch(1, supplied).unwrap();
    assert_eq!(
        discipline.observe_in_epoch(1, snapshot(2, 1_000_000_000, 200)),
        Err(DisciplineError::ObservationSourceChanged)
    );
    assert_eq!(discipline.latest_pair(), Some(supplied));
    discipline
        .rebind_output(2, point(2, 0), point(2, 0), Timestamp::ZERO)
        .unwrap();
    discipline
        .observe_asio_in_epoch(2, asio(48_000, 0, 1_000))
        .unwrap();
    assert_eq!(
        discipline.observe_clock_pair_in_epoch(2, supplied),
        Err(DisciplineError::ObservationSourceChanged)
    );
}
#[test]
fn asio_rate_transition_needs_rebind_and_old_epoch_refusal_retains_new_block_evidence() {
    let mut discipline = observer();
    discipline
        .observe_asio_in_epoch(0, asio(48_000, 0, 1_000))
        .unwrap();
    let source = discipline.validator.latest_record();
    let pair = discipline.latest_pair();
    assert!(
        discipline
            .observe_asio_in_epoch(0, asio(44_100, 64, 2_000))
            .is_err()
    );
    assert_eq!(discipline.validator.latest_record(), source);
    assert_eq!(discipline.latest_pair(), pair);
    discipline
        .rebind_output(1, point(2, 0), point(2, 0), Timestamp::ZERO)
        .unwrap();
    discipline
        .observe_asio_in_epoch(1, asio(44_100, 0, 2_000))
        .unwrap();
    let source = discipline.validator.latest_record();
    let pair = discipline.latest_pair();
    assert_eq!(
        discipline.observe_asio_in_epoch(0, asio(48_000, 64, 9_000)),
        Err(DisciplineError::EpochMismatch)
    );
    assert_eq!(discipline.validator.latest_record(), source);
    assert_eq!(discipline.latest_pair(), pair);
    discipline
        .observe_asio_in_epoch(1, asio(44_100, 64, 3_000))
        .unwrap();
    assert_eq!(discipline.epoch(), 1);
}
#[test]
fn maximum_epoch_and_failed_origin_order_leave_source_pair_and_retention_unchanged() {
    let mut discipline = observer();
    assert_eq!(
        discipline.rebind_output(1, point(2, 2), point(2, 1), Timestamp::ZERO),
        Err(DisciplineError::InvalidConfig)
    );
    assert_eq!(discipline.epoch(), 0);
    assert!(discipline.validator.latest_record().is_none());
    discipline
        .rebind_output(
            u64::MAX,
            point(2, 0),
            point(2, 1),
            Timestamp::from_nanos(i64::MAX),
        )
        .unwrap();
    discipline
        .observe_clock_pair_in_epoch(
            u64::MAX,
            ClockPair {
                source: point(2, 1),
                target: point(1, 1),
            },
        )
        .unwrap();
    let pair = discipline.latest_pair();
    let source = discipline.validator.latest_record();
    let retained = discipline.retained_len();
    assert_eq!(
        discipline.rebind_output(u64::MAX, point(2, 0), point(2, 0), Timestamp::ZERO),
        Err(DisciplineError::InvalidEpoch)
    );
    assert_eq!(
        discipline.rebind_output(0, point(2, 0), point(2, 0), Timestamp::ZERO),
        Err(DisciplineError::InvalidEpoch)
    );
    assert_eq!(discipline.latest_pair(), pair);
    assert_eq!(discipline.validator.latest_record(), source);
    assert_eq!(discipline.retained_len(), retained);
}
