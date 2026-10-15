//! Deterministic publication after the callback's frozen control snapshot.
use super::*;
use crate::{
    audio::{
        command_queue, practice_queue, AudioFormat, AudioLimits, CommandProducer, MixerConfig,
        PcmLimits, PcmSample, PracticeBoundaryKind, PracticeController, PracticeCue, PracticeError,
        PracticeEvent, PracticeLimits, PracticeRateOutcome, PracticeRateRefusal, PracticeRegion,
        PreparedPracticeProgram, SampleBank, SampleId, VoiceId,
    },
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
    transport::Rate,
};

fn snapshot_rig() -> (Mixer, CommandProducer, PracticeController) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let pcm_limits = PcmLimits::new(64, 64, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.125, 0.25, 0.375, 0.5], pcm_limits).unwrap(),
    )
    .unwrap();
    let program = PreparedPracticeProgram::new_with_runtime_rates(
        &bank,
        vec![PracticeCue {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        }],
        PracticeLimits::new(4, 1, 64, 8, 16).unwrap(),
        Rate::NORMAL,
        Duration::ZERO,
        false,
        0x14043,
    )
    .unwrap();
    let (controller, endpoint) = practice_queue(&program).unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(9),
            Timestamp::ZERO,
            AudioLimits::new(8, 4, 8, 64, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer
        .install_practice(
            program,
            endpoint,
            PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(2_000_000), true).unwrap(),
        )
        .unwrap();
    (mixer, producer, controller)
}

#[test]
fn rate_published_after_callback_snapshot_waits_and_keeps_original_stale_generation_evidence() {
    let (mut mixer, _producer, mut controller) = snapshot_rig();
    let (mut reference, _reference_producer, mut reference_controller) = snapshot_rig();
    for (output, control) in [
        (&mut mixer, &mut controller),
        (&mut reference, &mut reference_controller),
    ] {
        let mut first = [99.0];
        output.render(&mut first).unwrap();
        assert_eq!(first, [0.125]);
        assert!(
            matches!(control.try_pop_event().unwrap(), PracticeEvent::Boundary(start)
            if start.kind == PracticeBoundaryKind::Started && start.generation == 1)
        );
    }

    let physical_start = mixer.frame_cursor;
    let playback_start = mixer.playback_frame_cursor;
    assert_eq!((physical_start, playback_start), (1, 1));
    let frames = 2;
    let snapshot = mixer
        .practice
        .as_ref()
        .unwrap()
        .preflight(playback_start, frames)
        .unwrap();
    assert_eq!(snapshot, 0);
    let frozen_decision = mixer
        .prepare_runtime_rate_decision(
            playback_start,
            physical_start,
            ClockPoint {
                domain: ClockDomainId(9),
                timestamp: Timestamp::ZERO,
            },
        )
        .unwrap();
    assert!(frozen_decision.is_none());

    // This publication happens after both callback decisions are frozen.
    // The original generation1 request must stay queued while this callback
    // reaches its autonomous generation2 loop boundary.
    let requested_rate = Rate::new(2, 1).unwrap();
    let prepared = controller.prepare_rate(requested_rate).unwrap();
    controller.try_set_rate_next(41, 1, &prepared).unwrap();
    let mut consumed = 0;
    let mut actual = [99.0; 2];
    for sample in &mut actual {
        mixer.practice_frame(mixer.frame_cursor, snapshot, &mut consumed, None);
        mixer.mix_frame(std::slice::from_mut(sample));
        mixer.frame_cursor += 1;
        mixer.playback_frame_cursor += 1;
    }
    mixer.project_direct_practice_receipts(physical_start, frames);
    assert_eq!(
        consumed, 0,
        "the frozen snapshot cannot consume a late request"
    );
    let mut expected = [99.0; 2];
    reference.render(&mut expected).unwrap();
    assert_eq!(actual, [0.25, 0.125]);
    assert_eq!(actual.map(f32::to_bits), expected.map(f32::to_bits));
    assert_eq!(mixer.rate(), Rate::NORMAL);
    assert_eq!(
        controller.metadata().unwrap(),
        reference_controller.metadata().unwrap()
    );
    assert_eq!(controller.metadata().unwrap().generation, 2);
    assert_eq!(controller.metadata().unwrap().revision, 0);

    let projected_loop = controller.try_pop_projected_event().unwrap();
    let expected_loop = reference_controller.try_pop_projected_event().unwrap();
    assert_eq!(projected_loop, expected_loop);
    assert_eq!(
        (projected_loop.target_frame, projected_loop.target_rate),
        (2, 1000)
    );
    assert_eq!(projected_loop.boundary.source_frame, 2);
    let loop_iteration = match projected_loop.event {
        PracticeEvent::Boundary(boundary) => {
            assert_eq!(boundary.kind, PracticeBoundaryKind::Looped);
            assert_eq!(boundary.generation, 2);
            boundary.iteration
        }
        event => panic!("late rate must not replace the real loop boundary: {event:?}"),
    };
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));

    // A real next callback now sees the queued original-generation request.
    // It refuses that exact request rather than reinterpreting it for the
    // newer loop or inventing an earlier application boundary.
    let mut next = [99.0];
    let mut reference_next = [99.0];
    mixer.render(&mut next).unwrap();
    reference.render(&mut reference_next).unwrap();
    assert_eq!(next, [0.25]);
    assert_eq!(next.map(f32::to_bits), reference_next.map(f32::to_bits));
    let projected_rate = controller.try_pop_projected_event().unwrap();
    assert_eq!(
        (projected_rate.target_frame, projected_rate.target_rate),
        (3, 1000)
    );
    assert_eq!(projected_rate.boundary.source_frame, 3);
    assert_eq!(projected_rate.origin.domain, ClockDomainId(9));
    assert_eq!(projected_rate.origin.timestamp, Timestamp::ZERO);
    match projected_rate.event {
        PracticeEvent::Rate(event) => {
            assert_eq!(
                event.outcome,
                PracticeRateOutcome::Refused(PracticeRateRefusal::StaleGeneration)
            );
            assert_eq!(
                (event.request_id, event.generation, event.iteration),
                (41, 2, loop_iteration)
            );
            assert_eq!((event.request_revision, event.applied_revision), (0, 0));
            assert_eq!((event.physical_frame, event.playback_frame), (3, 3));
            assert_eq!(event.active_elapsed_nanos, 1_000_000);
            assert_eq!(
                (
                    event.previous_rate,
                    event.requested_rate,
                    event.applied_rate
                ),
                (Rate::NORMAL, requested_rate, Rate::NORMAL)
            );
        }
        event => panic!("expected correlated stale-generation refusal: {event:?}"),
    }
    assert_eq!(
        controller.metadata().unwrap(),
        reference_controller.metadata().unwrap()
    );
    assert_eq!(controller.try_pop_event(), Err(PracticeError::Empty));
}
