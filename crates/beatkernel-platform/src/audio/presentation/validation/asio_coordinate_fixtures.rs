//! Supplied original ASIO blocks exercise admission, without SDK or device claims.
use super::*;
use beatkernel::audio::{AudioCounters, RenderReport};
use crate::audio::asio::MultimediaHostInterval;

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn block(
    frame: u64,
    frames: usize,
    hz: u32,
    before: i64,
    after: i64,
) -> AsioPresentationObservation {
    AsioPresentationObservation::from_render(
        RenderReport {
            start_frame: frame,
            frames,
            playback_start_frame: frame,
            playback_frames: frames,
            paused: false,
            playback_end_physical_frame: None,
            active_voices: 0,
            pending_commands: 0,
            song_position: Timestamp::ZERO,
            producer_disconnected: false,
            counters: AudioCounters::default(),
        },
        hz,
        MultimediaHostInterval {
            before: point(1, before),
            after: point(1, after),
        },
        0,
        0,
        point(2, 0),
    )
    .unwrap()
}
fn validator() -> NativePresentationValidator {
    NativePresentationValidator::new(7, point(2, 0), ClockDomainId(1))
}
fn admit(
    v: &mut NativePresentationValidator,
    observation: AsioPresentationObservation,
    basis: Option<OutputFrameBasis>,
) {
    let before = v.state;
    let prepared = v.prepare_asio(7, observation, basis).unwrap();
    assert_eq!(v.state, before);
    assert_eq!(prepared.admission(), NativeObservationAdmission::Progress);
    assert_eq!(
        prepared.evidence(),
        &OriginalNativePresentationEvidence::Asio { observation, basis }
    );
    assert_eq!(v.commit(prepared), Ok(NativeObservationAdmission::Progress));
    assert_eq!(
        v.latest_record().unwrap().evidence(),
        &OriginalNativePresentationEvidence::Asio { observation, basis }
    );
}

#[test]
fn original_nonoverlapping_blocks_progress_at_equal_nanoseconds_with_real_host_progress() {
    for hz in [2_000_000_000, u32::MAX] {
        for use_basis in [false, true] {
            let basis = use_basis.then(|| OutputFrameBasis::new(point(2, 0), hz, 0).unwrap());
            let mut v = validator();
            let first = block(0, 1, hz, 90, 110);
            let second = block(1, 1, hz, 190, 230);
            assert_eq!(first.output, second.output);
            admit(&mut v, first, basis);
            admit(&mut v, second, basis);
            assert_eq!(v.latest_pair().unwrap().source, point(2, 0));
            assert_eq!(v.latest_pair().unwrap().target, point(1, 210));
            assert_eq!(second.host.before, point(1, 190));
            assert_eq!(second.host.after, point(1, 230));
        }
    }
}

#[test]
fn duplicates_and_equal_host_deferral_preserve_full_original_record() {
    let mut v = validator();
    admit(&mut v, block(0, 1, 2_000_000_000, 90, 110), None);
    let before = v.state;
    for observation in [
        block(0, 1, 2_000_000_000, 90, 110),
        block(0, 1, 2_000_000_000, 9_000, 11_000),
    ] {
        let prepared = v.prepare_asio(7, observation, None).unwrap();
        assert_eq!(prepared.admission(), NativeObservationAdmission::Unchanged);
        assert_eq!(prepared.correlation_pair(), None);
        assert_eq!(
            v.commit(prepared),
            Ok(NativeObservationAdmission::Unchanged)
        );
        assert_eq!(v.state, before);
    }
    let deferred = v
        .prepare_asio(7, block(1, 1, 2_000_000_000, 95, 105), None)
        .unwrap();
    assert_eq!(
        deferred.admission(),
        NativeObservationAdmission::AwaitingHostProgress
    );
    assert_eq!(deferred.correlation_pair(), None);
    assert_eq!(
        v.commit(deferred),
        Ok(NativeObservationAdmission::AwaitingHostProgress)
    );
    assert_eq!(v.state, before);
    admit(&mut v, block(1, 1, 2_000_000_000, 190, 210), None);
}

#[test]
fn actual_block_overlap_frame_and_host_regression_remain_rejected_at_flat_time() {
    let mut v = validator();
    admit(&mut v, block(1, 2, u32::MAX, 90, 110), None);
    let before = v.state;
    for observation in [
        block(0, 1, u32::MAX, 190, 210),
        block(2, 1, u32::MAX, 190, 210),
        block(3, 1, u32::MAX, 80, 100),
    ] {
        assert_eq!(
            v.prepare_asio(7, observation, None),
            Err(DisciplineError::NonIncreasing)
        );
        assert_eq!(v.state, before);
    }
    admit(&mut v, block(3, 1, u32::MAX, 190, 210), None);
}

#[test]
fn original_rate_basis_epoch_domains_and_malformed_output_checks_are_unchanged() {
    let hz = 2_000_000_000;
    let basis = OutputFrameBasis::new(point(2, 0), hz, 0).unwrap();
    let mut v = validator();
    admit(&mut v, block(0, 1, hz, 90, 110), Some(basis));
    let before = v.state;
    let next = block(1, 1, hz, 190, 210);
    let changed = OutputFrameBasis::new(point(2, 0), hz, 1).unwrap();
    assert_eq!(
        v.prepare_asio(7, next, Some(changed)),
        Err(DisciplineError::ObservationSourceChanged)
    );
    assert_eq!(
        v.prepare_asio(8, next, Some(basis)),
        Err(DisciplineError::EpochMismatch)
    );
    let mut wrong_host = next;
    wrong_host.host.before.domain = ClockDomainId(9);
    wrong_host.host.after.domain = ClockDomainId(9);
    assert_eq!(
        v.prepare_asio(7, wrong_host, Some(basis)),
        Err(DisciplineError::DomainMismatch)
    );
    let mut malformed = next;
    malformed.output.timestamp = Timestamp::from_nanos(1);
    assert_eq!(
        v.prepare_asio(7, malformed, Some(basis)),
        Err(DisciplineError::AsioPresentation(
            AsioPresentationError::Malformed
        ))
    );
    assert_eq!(v.state, before);
    let mut without_basis = validator();
    admit(&mut without_basis, block(0, 1, hz, 90, 110), None);
    let before = without_basis.state;
    assert_eq!(
        without_basis.prepare_asio(7, block(1, 1, u32::MAX, 190, 210), None),
        Err(DisciplineError::FrequencyChanged)
    );
    assert_eq!(without_basis.state, before);
}

#[test]
fn stale_preparations_bind_full_bracket_and_new_epoch_at_identical_floor() {
    let mut a = validator();
    let mut b = validator();
    admit(&mut a, block(0, 1, u32::MAX, 90, 110), None);
    admit(&mut b, block(0, 1, u32::MAX, 95, 105), None);
    assert_eq!(a.latest_pair(), b.latest_pair());
    let next = a
        .prepare_asio(7, block(1, 1, u32::MAX, 190, 210), None)
        .unwrap();
    let before = b.state;
    assert_eq!(
        b.commit(next),
        Err(NativePreparationError::StalePreparation)
    );
    assert_eq!(b.state, before);
    let rebind = a.prepare_rebind(8, point(2, 0)).unwrap();
    a.commit_rebind(rebind).unwrap();
    let before = a.state;
    assert_eq!(
        a.commit(next),
        Err(NativePreparationError::StalePreparation)
    );
    assert_eq!(a.state, before);
    assert_eq!(a.latest_record(), None);
}

#[test]
fn malformed_or_overflowing_extent_is_refused_even_for_duplicate_block() {
    let mut v = validator();
    admit(&mut v, block(u64::MAX - 1, 1, u32::MAX, 90, 110), None);
    let before = v.state;
    let mut overflow = block(u64::MAX - 1, 1, u32::MAX, 190, 210);
    overflow.render.frames = 2;
    assert_eq!(
        v.prepare_asio(7, overflow, None),
        Err(DisciplineError::AsioPresentation(
            AsioPresentationError::Overflow
        ))
    );
    let mut zero = block(u64::MAX - 1, 1, u32::MAX, 190, 210);
    zero.render.frames = 0;
    assert_eq!(
        v.prepare_asio(7, zero, None),
        Err(DisciplineError::AsioPresentation(
            AsioPresentationError::Malformed
        ))
    );
    assert_eq!(v.state, before);
}

#[test]
fn actual_scalar_discipline_keeps_native_and_estimator_owners_atomic_at_flat_nanoseconds() {
    use crate::audio::presentation::discipline::{
        DisciplineConfig, ObservationAdmission, PresentationDiscipline,
    };
    let basis = OutputFrameBasis::new(point(2, 0), 2_000_000_000, 0).unwrap();
    let mut discipline = PresentationDiscipline::new(
        DisciplineConfig::default(),
        point(2, 0),
        ClockDomainId(1),
        Timestamp::ZERO,
    )
    .unwrap();
    assert_eq!(
        discipline.observe_asio_with_basis_in_epoch(0, block(0, 1, 2_000_000_000, 90, 110), basis),
        Ok(ObservationAdmission::Retained)
    );
    let original_pair = discipline.latest_pair();
    let before = format!("{discipline:?}");
    // Debug is a public complete snapshot of both private owners, including the
    // original native block. A scalar no-op must preserve that block too.
    assert_eq!(
        discipline.observe_asio_with_basis_in_epoch(0, block(1, 1, 2_000_000_000, 190, 210), basis),
        Ok(ObservationAdmission::Unchanged)
    );
    assert_eq!(format!("{discipline:?}"), before);
    assert_eq!(discipline.latest_pair(), original_pair);
    assert_eq!(discipline.retained_len(), 1);
    assert_eq!(
        discipline.validate_host(point(1, 2_000_000_101)),
        Err(DisciplineError::Stale)
    );
    assert_eq!(
        discipline.observe_asio_with_basis_in_epoch(
            0,
            block(0, 1, 2_000_000_000, 9_000, 11_000),
            basis
        ),
        Ok(ObservationAdmission::Unchanged)
    );
    assert_eq!(format!("{discipline:?}"), before);
    assert_eq!(
        discipline.observe_asio_with_basis_in_epoch(0, block(1, 1, 2_000_000_000, 95, 105), basis),
        Ok(ObservationAdmission::AwaitingHostProgress)
    );
    assert_eq!(format!("{discipline:?}"), before);
    assert_eq!(
        discipline.observe_asio_with_basis_in_epoch(0, block(2, 1, 2_000_000_000, 290, 310), basis),
        Ok(ObservationAdmission::Progress)
    );
    assert_eq!(
        discipline.latest_pair(),
        Some(ClockPair {
            source: point(2, 1),
            target: point(1, 300)
        })
    );
    let progressed = format!("{discipline:?}");
    assert_eq!(
        discipline.observe_asio_with_basis_in_epoch(
            0,
            block(2, 1, 2_000_000_000, 9_000, 11_000),
            basis
        ),
        Ok(ObservationAdmission::Unchanged)
    );
    assert_eq!(format!("{discipline:?}"), progressed);
}
