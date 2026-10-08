use crate::{
    native_converted_gameplay_fixtures::{native, point, rig},
    playback_pause::{NativePause, PausePhase},
};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};

fn pair(basis: TargetFrameBasis, frame: u64) -> beatkernel::time::ClockPair {
    native(
        basis,
        frame,
        1_000_000_000 + (u128::from(frame) * 1_000_000_000 / 48_000) as i64,
    )
    .1
}

#[test]
fn genuine_source_pause_waits_for_native_crossing_of_mapped_target_boundary_despite_audible_cache()
{
    let (mut producer, mut output) = rig(24_000, 48_000, None, None, 0);
    let basis = output.target_frame_basis();
    let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 24_000)
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap();
    let active = output.render_pending(1).unwrap();
    output.admit(1).unwrap();
    pause
        .observe_target(7, basis, output.boundaries(), active.source, pair(basis, 0))
        .unwrap();
    pause.request_in_epoch(7, true, pair(basis, 1)).unwrap();
    producer.request_pause(true);
    let paused = output.render_pending(8).unwrap();
    assert!(paused.source.unwrap().paused);
    assert!(output.pending_samples()[0] > 0.0);
    assert_eq!(
        output.boundaries().pause.unwrap().target_time,
        TargetTime::from_frames(4, 48_000).unwrap()
    );
    assert_eq!(
        pause
            .observe_target(
                7,
                basis,
                output.boundaries(),
                output.last_real_source_report(),
                pair(basis, 3)
            )
            .unwrap(),
        None
    );
    assert_eq!(pause.phase(), PausePhase::Pausing);
    let ack = pause
        .observe_target(
            7,
            basis,
            output.boundaries(),
            output.last_real_source_report(),
            pair(basis, 4),
        )
        .unwrap()
        .unwrap();
    assert!(ack.paused);
    assert_eq!(ack.playback_frame, 2);
    assert_eq!(
        ack.host,
        point(1, 1_000_000_000 + 4 * 1_000_000_000 / 48_000)
    );
    assert_eq!(pause.phase(), PausePhase::Paused);
    assert_eq!(pause.target_basis(), Some(basis));
}

#[test]
fn held_time_and_source_resume_cannot_ack_until_actual_mapped_resume_target_is_presented() {
    let (mut producer, mut output) = rig(24_000, 48_000, None, None, 0);
    let basis = output.target_frame_basis();
    let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 24_000)
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap();
    let active = output.render_pending(1).unwrap();
    output.admit(1).unwrap();
    pause
        .observe_target(7, basis, output.boundaries(), active.source, pair(basis, 0))
        .unwrap();
    pause.request_in_epoch(7, true, pair(basis, 1)).unwrap();
    producer.request_pause(true);
    output.render_pending(8).unwrap();
    pause
        .observe_target(
            7,
            basis,
            output.boundaries(),
            output.last_real_source_report(),
            pair(basis, 4),
        )
        .unwrap()
        .unwrap();
    output.admit(8).unwrap();
    assert!(pause.request_in_epoch(7, false, pair(basis, 9)).unwrap());
    producer.request_pause(false);
    let position = output.converter_owner().source_position();
    let held = output.render_held_pending(4).unwrap();
    assert_eq!(held.source, None);
    assert!(output.mixer().is_paused());
    assert_eq!(output.converter_owner().source_position(), position);
    assert_eq!(
        pause
            .observe_target(
                7,
                basis,
                output.boundaries(),
                output.last_real_source_report(),
                pair(basis, 13)
            )
            .unwrap(),
        None
    );
    assert_eq!(pause.phase(), PausePhase::Resuming);
    output.admit(4).unwrap();
    let mut early_source_resume = false;
    let mut mapped = false;
    for _ in 0..8 {
        let report = output.render_pending(1).unwrap();
        output.admit(1).unwrap();
        if report.resume_source_frame.is_some() && output.boundaries().resume.is_none() {
            early_source_resume = true;
            assert_eq!(
                pause
                    .observe_target(
                        7,
                        basis,
                        output.boundaries(),
                        output.last_real_source_report(),
                        pair(basis, 13)
                    )
                    .unwrap(),
                None
            );
        }
        if output.boundaries().resume.is_some() {
            mapped = true;
            break;
        }
    }
    assert!(early_source_resume && mapped);
    assert_eq!(
        output.boundaries().resume.unwrap().target_time,
        TargetTime::from_frames(16, 48_000).unwrap()
    );
    assert_eq!(
        pause
            .observe_target(
                7,
                basis,
                output.boundaries(),
                output.last_real_source_report(),
                pair(basis, 15)
            )
            .unwrap(),
        None
    );
    let ack = pause
        .observe_target(
            7,
            basis,
            output.boundaries(),
            output.last_real_source_report(),
            pair(basis, 16),
        )
        .unwrap()
        .unwrap();
    assert!(!ack.paused);
    assert_eq!(ack.playback_frame, 2);
    assert_eq!(pause.phase(), PausePhase::Running);
    assert_eq!(
        pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
        Timestamp::from_nanos(-250_000)
    );
}

#[test]
fn target_pause_rejects_wrong_epoch_basis_domain_and_forged_origin_atomically() {
    let (mut producer, mut output) = rig(24_000, 48_000, None, None, 0);
    let basis = output.target_frame_basis();
    let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 24_000)
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap();
    output.render_pending(1).unwrap();
    output.admit(1).unwrap();
    pause.request_in_epoch(7, true, pair(basis, 1)).unwrap();
    producer.request_pause(true);
    output.render_pending(8).unwrap();
    let facts = output.boundaries();
    let source = output.last_real_source_report();
    let before = format!("{pause:?}");
    assert!(pause
        .observe_target(8, basis, facts, source, pair(basis, 4))
        .is_err());
    assert_eq!(format!("{pause:?}"), before);
    let altered = TargetFrameBasis::new(basis.origin(), basis.start_time(), 32_000).unwrap();
    assert!(pause
        .observe_target(7, altered, facts, source, pair(basis, 4))
        .is_err());
    assert_eq!(format!("{pause:?}"), before);
    let mut wrong = pair(basis, 4);
    wrong.target.domain = ClockDomainId(99);
    assert!(pause
        .observe_target(7, basis, facts, source, wrong)
        .is_err());
    assert_eq!(format!("{pause:?}"), before);
    let mut forged = facts;
    forged.origin = Some(point(2, 1));
    assert!(pause
        .observe_target(7, basis, forged, source, pair(basis, 4))
        .is_err());
    assert_eq!(format!("{pause:?}"), before);
    assert!(pause
        .observe_target(7, basis, facts, source, pair(basis, 4))
        .unwrap()
        .is_some());
}

#[test]
fn target_rebind_requires_real_native_pause_ack_and_matching_frozen_source_owner() {
    let (mut producer, mut output) = rig(24_000, 48_000, None, None, 0);
    let basis = output.target_frame_basis();
    let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 24_000)
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap();
    output.render_pending(1).unwrap();
    output.admit(1).unwrap();
    pause.request_in_epoch(7, true, pair(basis, 1)).unwrap();
    producer.request_pause(true);
    output.render_pending(8).unwrap();
    assert_eq!(
        pause
            .observe_target(
                7,
                basis,
                output.boundaries(),
                output.last_real_source_report(),
                pair(basis, 3)
            )
            .unwrap(),
        None
    );
    output.admit(8).unwrap();
    output.render_held_pending(3).unwrap();
    let before = format!("{pause:?}");
    assert!(pause.rebind_target_output(8, &output).is_err());
    assert_eq!(format!("{pause:?}"), before);
    let ack = pause
        .observe_target(
            7,
            basis,
            output.boundaries(),
            output.last_real_source_report(),
            pair(basis, 4),
        )
        .unwrap()
        .unwrap();
    assert_eq!(ack.playback_frame, 2);
    let (mut other_producer, mut other) = rig(24_000, 48_000, None, None, 0);
    other.render_pending(8).unwrap();
    other.admit(8).unwrap();
    other_producer.request_pause(true);
    other.render_pending(8).unwrap();
    other.admit(8).unwrap();
    other.render_held_pending(3).unwrap();
    assert!(other.mixer().is_paused());
    assert_ne!(other.mixer().playback_frame_cursor(), 2);
    let before = format!("{pause:?}");
    assert!(pause.rebind_target_output(8, &other).is_err());
    assert_eq!(format!("{pause:?}"), before);
    pause.rebind_target_output(8, &output).unwrap();
    assert_eq!(pause.phase(), PausePhase::Paused);
    assert_eq!(pause.target_basis(), Some(output.target_frame_basis()));
}
