//! Mathematical frame admission fixtures; supplied packets prove no native acquisition.
use super::*;
use beatkernel::audio::{AudioCounters, RenderReport, TargetFrameBasis, TargetTime};
use crate::audio::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamStatus, StreamCounters,
    asio::MultimediaHostInterval,
};

fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn pair(output: i64, host: i64) -> ClockPair {
    ClockPair {
        source: point(2, output),
        target: point(1, host),
    }
}
fn source(rate: u32, cut: u64) -> NativeFrameBasis {
    NativeFrameBasis::Source(OutputFrameBasis::new(point(2, 0), rate, cut).unwrap())
}
fn target(rate: u32, start: TargetTime) -> NativeFrameBasis {
    NativeFrameBasis::Target(TargetFrameBasis::new(point(2, 0), start, rate).unwrap())
}
fn validator(output: i64) -> NativePresentationValidator {
    NativePresentationValidator::new(0, point(2, output), ClockDomainId(1))
}
fn admit(v: &mut NativePresentationValidator, basis: NativeFrameBasis, frame: u64, p: ClockPair) {
    let before = v.state;
    let prepared = v.prepare_frame_pair(v.epoch(), p, basis, frame).unwrap();
    assert_eq!(v.state, before, "preparation is nonmutating");
    assert_eq!(prepared.admission(), NativeObservationAdmission::Progress);
    assert_eq!(prepared.correlation_pair(), Some(p));
    assert_eq!(
        prepared.evidence(),
        &OriginalNativePresentationEvidence::SuppliedPair(p)
    );
    assert_eq!(v.commit(prepared), Ok(NativeObservationAdmission::Progress));
    assert_eq!(v.latest_pair(), Some(p));
    assert_eq!(v.latest_record().unwrap().pair(), p);
}
fn wasapi() -> AudioStreamSnapshot {
    AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters::default(),
        render: None,
        clock: Some(AudioClockSnapshot {
            position: 1,
            frequency: 1_000_000_000,
            qpc_100ns: 1,
            reading_quality: AudioClockReadingQuality::Accurate,
            host_point: Some(point(1, 100)),
            mapping_quality: beatkernel::time::ClockMappingQuality::Unknown,
        }),
    }
}
fn asio() -> AsioPresentationObservation {
    AsioPresentationObservation::from_render(
        RenderReport {
            start_frame: 0,
            frames: 1,
            playback_start_frame: 0,
            playback_frames: 1,
            paused: false,
            playback_end_physical_frame: None,
            active_voices: 0,
            pending_commands: 0,
            song_position: Timestamp::ZERO,
            producer_disconnected: false,
            counters: AudioCounters::default(),
        },
        1000,
        MultimediaHostInterval {
            before: point(1, 90),
            after: point(1, 110),
        },
        0,
        0,
        point(2, 0),
    )
    .unwrap()
}

#[test]
fn source_and_target_frames_progress_at_flat_integer_nanoseconds() {
    for basis in [
        source(4_000_000_000, 0),
        target(4_000_000_000, TargetTime::new(0, 0, 1).unwrap()),
    ] {
        let mut v = validator(0);
        admit(&mut v, basis, 1, pair(0, 100));
        admit(&mut v, basis, 2, pair(0, 200));
        admit(&mut v, basis, 3, pair(0, 300));
        admit(&mut v, basis, 4, pair(1, 400));
    }
}

#[test]
fn unchanged_frame_does_not_refresh_host_or_accepted_metadata() {
    for basis in [
        source(4_000_000_000, 0),
        target(4_000_000_000, TargetTime::new(0, 0, 1).unwrap()),
    ] {
        let mut v = validator(0);
        admit(&mut v, basis, 2, pair(0, 100));
        let before = v.state;
        for host in [100, 200, 10_000] {
            let unchanged = v.prepare_frame_pair(0, pair(0, host), basis, 2).unwrap();
            assert_eq!(unchanged.admission(), NativeObservationAdmission::Unchanged);
            assert_eq!(unchanged.correlation_pair(), None);
            assert_eq!(
                v.commit(unchanged),
                Ok(NativeObservationAdmission::Unchanged)
            );
            assert_eq!(v.state, before);
        }
        // A duplicate's later HOST value is not a new chronology/freshness watermark.
        admit(&mut v, basis, 3, pair(0, 150));
    }
}

#[test]
fn nonzero_source_cut_combines_frames_before_one_floor() {
    let basis = source(3, 2);
    let mut v = validator(666_666_666);
    admit(&mut v, basis, 1, pair(1_000_000_000, 100));
    let before = v.state;
    assert_eq!(
        v.prepare_frame_pair(0, pair(999_999_999, 100), basis, 1),
        Err(DisciplineError::InvalidConfig)
    );
    assert_eq!(v.state, before);
}

#[test]
fn fractional_target_start_is_added_before_final_floor() {
    let basis = target(3, TargetTime::new(0, 2, 3).unwrap());
    let mut v = validator(666_666_666);
    admit(&mut v, basis, 1, pair(1_000_000_000, 100));
    admit(&mut v, basis, 2, pair(1_333_333_333, 200));
    let before = v.state;
    assert_eq!(
        v.prepare_frame_pair(0, pair(1_333_333_332, 300), basis, 2),
        Err(DisciplineError::InvalidConfig)
    );
    assert_eq!(v.state, before);
}

#[test]
fn rejected_epoch_domains_origin_and_point_leave_full_state_untouched() {
    let basis = source(1000, 0);
    let v = validator(0);
    let before = v.state;
    for (epoch, p, b, frame, error) in [
        (
            1,
            pair(1_000_000, 100),
            basis,
            1,
            DisciplineError::EpochMismatch,
        ),
        (
            0,
            ClockPair {
                source: point(3, 1_000_000),
                target: point(1, 100),
            },
            basis,
            1,
            DisciplineError::DomainMismatch,
        ),
        (
            0,
            ClockPair {
                source: point(2, 1_000_000),
                target: point(3, 100),
            },
            basis,
            1,
            DisciplineError::DomainMismatch,
        ),
        (
            0,
            pair(1_000_001, 100),
            basis,
            1,
            DisciplineError::InvalidConfig,
        ),
        (
            0,
            pair(1_000_000, 100),
            source(1000, 1),
            0,
            DisciplineError::DomainMismatch,
        ),
    ] {
        assert_eq!(v.prepare_frame_pair(epoch, p, b, frame), Err(error));
        assert_eq!(v.state, before);
    }
}

#[test]
fn frame_and_host_regression_or_equal_host_progress_are_transactional() {
    let basis = source(4_000_000_000, 0);
    let mut v = validator(0);
    admit(&mut v, basis, 2, pair(0, 100));
    let before = v.state;
    for (frame, host) in [(1, 200), (2, 99), (3, 99), (3, 100)] {
        assert_eq!(
            v.prepare_frame_pair(0, pair(0, host), basis, frame),
            Err(DisciplineError::NonIncreasing)
        );
        assert_eq!(v.state, before);
    }
}

#[test]
fn basis_and_coordinate_mode_remain_immutable_even_at_identical_rounded_points() {
    let mut v = validator(0);
    admit(&mut v, source(4_000_000_000, 0), 1, pair(0, 100));
    let before = v.state;
    for changed in [
        source(3_000_000_000, 0),
        source(4_000_000_000, 1),
        target(4_000_000_000, TargetTime::new(0, 0, 1).unwrap()),
    ] {
        assert_eq!(
            v.prepare_frame_pair(0, pair(0, 200), changed, 2),
            Err(DisciplineError::ObservationSourceChanged)
        );
        assert_eq!(v.state, before);
    }
    let mut v = validator(333_333_333);
    admit(
        &mut v,
        target(3, TargetTime::new(0, 1, 3).unwrap()),
        0,
        pair(333_333_333, 100),
    );
    let before = v.state;
    let changed = target(6, TargetTime::new(0, 1, 3).unwrap());
    assert_eq!(
        v.prepare_frame_pair(0, pair(500_000_000, 200), changed, 1),
        Err(DisciplineError::ObservationSourceChanged)
    );
    assert_eq!(v.state, before);
}

#[test]
fn scalar_wasapi_and_asio_sources_cannot_mix_with_frame_mode() {
    let basis = source(1000, 0);
    let mut framed = validator(0);
    admit(&mut framed, basis, 1, pair(1_000_000, 100));
    let before = framed.state;
    assert_eq!(
        framed.prepare_pair(0, pair(2_000_000, 200)),
        Err(DisciplineError::ObservationSourceChanged)
    );
    assert_eq!(
        framed.prepare_wasapi(0, wasapi(), None),
        Err(DisciplineError::ObservationSourceChanged)
    );
    assert_eq!(
        framed.prepare_asio(0, asio(), None),
        Err(DisciplineError::ObservationSourceChanged)
    );
    assert_eq!(framed.state, before);
    for initial in [0, 1, 2] {
        let mut old = validator(0);
        let p = match initial {
            0 => old.prepare_pair(0, pair(0, 100)).unwrap(),
            1 => old.prepare_wasapi(0, wasapi(), None).unwrap(),
            _ => old.prepare_asio(0, asio(), None).unwrap(),
        };
        old.commit(p).unwrap();
        let before = old.state;
        assert_eq!(
            old.prepare_frame_pair(0, pair(1_000_000, 200), basis, 1),
            Err(DisciplineError::ObservationSourceChanged)
        );
        assert_eq!(old.state, before);
    }
}

#[test]
fn stale_tokens_compare_original_frames_even_when_pairs_are_equal() {
    let basis = source(4_000_000_000, 0);
    let mut a = validator(0);
    let mut b = validator(0);
    admit(&mut a, basis, 1, pair(0, 100));
    admit(&mut b, basis, 2, pair(0, 100));
    assert_eq!(a.latest_record(), b.latest_record());
    let next = a.prepare_frame_pair(0, pair(0, 200), basis, 3).unwrap();
    let before = b.state;
    assert_eq!(
        b.commit(next),
        Err(NativePreparationError::StalePreparation)
    );
    assert_eq!(b.state, before);
    let rebind = a.prepare_rebind(1, point(2, 0)).unwrap();
    assert_eq!(
        b.commit_rebind(rebind),
        Err(NativePreparationError::StalePreparation)
    );
    assert_eq!(b.state, before);
    admit(&mut a, basis, 2, pair(0, 150));
    let before = a.state;
    assert_eq!(
        a.commit(next),
        Err(NativePreparationError::StalePreparation)
    );
    assert_eq!(a.state, before);
}

#[test]
fn commit_reconstructs_candidate_frame_instead_of_accepting_tampered_sidecar() {
    let basis = source(1000, 0);
    let mut v = validator(0);
    let mut prepared = v
        .prepare_frame_pair(0, pair(1_000_000, 100), basis, 1)
        .unwrap();
    prepared.frame.as_mut().unwrap().relative_frame = 2;
    let before = v.state;
    assert_eq!(
        v.commit(prepared),
        Err(NativePreparationError::StalePreparation)
    );
    assert_eq!(v.state, before);
}

#[test]
fn actual_rebind_clears_frame_mode_and_rejects_retired_preparations() {
    let mut v = validator(0);
    let basis = source(4_000_000_000, 0);
    admit(&mut v, basis, 1, pair(0, 100));
    let stale = v.prepare_frame_pair(0, pair(0, 200), basis, 2).unwrap();
    let rebind = v.prepare_rebind(1, point(2, 0)).unwrap();
    v.commit_rebind(rebind).unwrap();
    assert_eq!(v.epoch(), 1);
    assert_eq!(v.latest_record(), None);
    assert_eq!(v.state.frame, None);
    let before = v.state;
    assert_eq!(
        v.commit(stale),
        Err(NativePreparationError::StalePreparation)
    );
    assert_eq!(v.state, before);
    // A distinct new-epoch basis can be adopted after actual rebind.
    admit(
        &mut v,
        target(1000, TargetTime::new(0, 0, 1).unwrap()),
        1,
        pair(1_000_000, 300),
    );
    let rebind = v.prepare_rebind(2, point(2, 0)).unwrap();
    v.commit_rebind(rebind).unwrap();
    let scalar = v.prepare_pair(2, pair(100, 400)).unwrap();
    v.commit(scalar).unwrap();
    assert_eq!(v.state.frame, None);
}

#[test]
fn checked_frame_addition_and_timestamp_overflow_preserve_state() {
    let basis = source(u32::MAX, u64::MAX);
    let origin = match basis {
        NativeFrameBasis::Source(b) => b.point_at_stream_frame(0).unwrap(),
        _ => unreachable!(),
    };
    let v = NativePresentationValidator::new(0, origin, ClockDomainId(1));
    let before = v.state;
    assert_eq!(
        v.prepare_frame_pair(
            0,
            ClockPair {
                source: origin,
                target: point(1, 100)
            },
            basis,
            1
        ),
        Err(DisciplineError::Overflow)
    );
    assert_eq!(v.state, before);
    let v = validator(0);
    let before = v.state;
    assert_eq!(
        v.prepare_frame_pair(0, pair(0, 100), source(1, 0), u64::MAX),
        Err(DisciplineError::Overflow)
    );
    assert_eq!(v.state, before);
    let basis = target(1, TargetTime::new(9_223_372_036, 0, 1).unwrap());
    let v = validator(9_223_372_036_000_000_000);
    let before = v.state;
    assert_eq!(
        v.prepare_frame_pair(0, pair(9_223_372_036_000_000_000, 100), basis, 1),
        Err(DisciplineError::Overflow)
    );
    assert_eq!(v.state, before);
}

#[test]
fn legacy_scalar_duplicate_semantics_are_preserved() {
    let mut v = validator(0);
    let first = v.prepare_pair(0, pair(100, 100)).unwrap();
    v.commit(first).unwrap();
    let before = v.state;
    let unchanged = v.prepare_pair(0, pair(100, 500)).unwrap();
    assert_eq!(unchanged.admission(), NativeObservationAdmission::Unchanged);
    v.commit(unchanged).unwrap();
    assert_eq!(v.state, before);
    assert_eq!(
        v.prepare_pair(0, pair(200, 100)),
        Err(DisciplineError::NonIncreasing)
    );
    let progressing = v.prepare_pair(0, pair(200, 200)).unwrap();
    v.commit(progressing).unwrap();
    assert_eq!(v.latest_pair(), Some(pair(200, 200)));
    assert_eq!(v.state.frame, None);
}
