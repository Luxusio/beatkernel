//! Real owner compaction and receipt protection, without native authority.
use super::*;
use crate::audio::{count_heap_calls, DeviceFormat, NativeOutputState, SampleEncoding};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};

fn rig(capacity: usize, practice: bool) -> (CommandProducer, Mixer, Option<PracticeController>) {
    rig_with_end(capacity, practice, None)
}
fn rig_with_end(
    capacity: usize,
    practice: bool,
    end: Option<u64>,
) -> (CommandProducer, Mixer, Option<PracticeController>) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(1024, 1024, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 128], limits).unwrap(),
    )
    .unwrap();
    let program = practice.then(|| {
        PreparedPracticeProgram::new(
            &bank,
            vec![PracticeCue {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.0,
            }],
            PracticeLimits::new(8, 1, 128, 8, 32).unwrap(),
        )
        .unwrap()
    });
    let (producer, consumer) = command_queue(8).unwrap();
    let config = MixerConfig::new(
        format,
        ClockDomainId(1),
        Timestamp::ZERO,
        AudioLimits::new(8, 2, 8, 128, 8).unwrap(),
    );
    let config = end.map_or(config, |end| config.with_playback_end_frame(end));
    let mut mixer = Mixer::new(config, bank, consumer).unwrap();
    let controller = program.map(|program| {
        let (controller, endpoint) = practice_queue(&program).unwrap();
        mixer
            .install_practice(
                program,
                endpoint,
                PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(100_000_000), false)
                    .unwrap(),
            )
            .unwrap();
        controller
    });
    mixer.enable_source_playback_history(capacity).unwrap();
    (producer, mixer, controller)
}
fn query(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, 1000).unwrap(), 0, 0, 1, 0, 1)
        .unwrap()
}
fn request(revision: u64, source_pin: u64) -> SourceHistoryRetirementRequest {
    SourceHistoryRetirementRequest {
        revision,
        source_pin,
    }
}

#[test]
fn bounded_history_survives_many_actual_pause_transitions_without_heap_or_cursor_change() {
    let (mut producer, mut mixer, _) = rig(3, false);
    let (mut reference_producer, mut reference, _) = rig(256, false);
    for producer in [&mut producer, &mut reference_producer] {
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.0,
            })
            .unwrap();
    }
    let mut completed = 0;
    for round in 0..100u64 {
        producer.request_pause(round % 2 == 1);
        reference_producer.request_pause(round % 2 == 1);
        let mut pcm = [7.0; 2];
        let mut reference_pcm = [7.0; 2];
        assert_eq!(
            mixer.render(&mut pcm).unwrap(),
            reference.render(&mut reference_pcm).unwrap()
        );
        assert_eq!(pcm, reference_pcm);
        if round == 0 {
            assert_eq!(pcm, [0.25; 2]);
        }
        let cursor = (mixer.frame_cursor(), mixer.playback_frame_cursor());
        let pin = cursor.0;
        let (retired, heap) = count_heap_calls(|| {
            apply_source_history_retirement(
                &mut mixer,
                request((round + 1) * 2, pin),
                &mut completed,
            )
        });
        retired.unwrap();
        assert_eq!(heap, 0);
        assert_eq!(
            cursor,
            (mixer.frame_cursor(), mixer.playback_frame_cursor())
        );
        let history = mixer.source_playback_history().unwrap();
        assert_eq!(
            (
                history.source_rate(),
                history.initial_physical_frame(),
                history.initial_playback_frame()
            ),
            (1000, 0, 0)
        );
        assert!(history.len() <= 2);
        assert!(history.lookup_at_source_time(query(pin)).is_ok());
        if round > 0 {
            assert!(history.lookup_at_source_time(query(0)).is_err());
        }
    }
    assert_eq!(completed, 200);
}

#[test]
fn queued_practice_receipt_clamps_then_same_request_retries_after_real_consumption() {
    let (mut producer, mut mixer, controller) = rig(8, true);
    let mut controller = controller.unwrap();
    let mut pcm = [0.0; 2];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.25; 2]);
    producer.request_pause(true);
    mixer.render(&mut pcm).unwrap();
    producer.request_pause(false);
    mixer.render(&mut pcm).unwrap();
    assert_eq!(mixer.practice_history_retention_pins().0, Some(0));
    let mut completed = 0;
    assert_eq!(
        apply_source_history_retirement(&mut mixer, request(2, 4), &mut completed).unwrap(),
        0
    );
    assert_eq!(completed, 0);
    assert!(mixer
        .source_playback_history()
        .unwrap()
        .lookup_at_source_time(query(0))
        .is_ok());
    assert_eq!(
        controller
            .try_pop_projected_receipt()
            .unwrap()
            .receipt
            .physical_frame,
        0
    );
    assert_eq!(mixer.practice_history_retention_pins().0, None);
    assert_eq!(
        apply_source_history_retirement(&mut mixer, request(2, 4), &mut completed).unwrap(),
        2
    );
    assert_eq!(completed, 2);
    assert!(mixer
        .source_playback_history()
        .unwrap()
        .lookup_at_source_time(query(0))
        .is_err());
    assert!(mixer
        .source_playback_history()
        .unwrap()
        .lookup_at_source_time(query(4))
        .is_ok());
}

#[test]
fn malformed_revision_and_outside_range_refuse_without_partial_completion() {
    let (_, mut mixer, _) = rig(4, false);
    mixer.render(&mut [0.0; 2]).unwrap();
    let mut completed = 0;
    for bad in [request(0, 1), request(1, 1), request(3, 1), request(2, 3)] {
        assert_eq!(
            apply_source_history_retirement(&mut mixer, bad, &mut completed),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(completed, 0);
        assert_eq!(mixer.frame_cursor(), 2);
        assert_eq!(
            mixer
                .source_playback_history()
                .unwrap()
                .retirement_revision(),
            0
        );
    }
    assert_eq!(
        apply_source_history_retirement(&mut mixer, request(2, 2), &mut completed).unwrap(),
        0
    );
    assert_eq!(completed, 2);
    assert_eq!(
        apply_source_history_retirement(&mut mixer, request(2, 2), &mut completed).unwrap(),
        0
    );
}

fn retained(mixer: Mixer, frames: usize) -> NativeOutputState {
    let format = DeviceFormat::new(1000, 2, SampleEncoding::Float32, None).unwrap();
    let matrix = ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap();
    match NativeOutputState::new(mixer, format, Some(matrix), frames) {
        Ok(state) => state,
        Err(_) => panic!("valid complete Source preparation must succeed"),
    }
}

#[test]
fn actual_active_suffix_pins_history_until_admission_without_losing_remix_or_queue() {
    let (mut producer, mixer, _) = rig(8, false);
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.,
        })
        .unwrap();
    let mut state = retained(mixer, 8);
    state.render_pending(2).unwrap();
    state.admit(2).unwrap();
    producer.request_pause(true);
    state.render_pending(2).unwrap();
    state.admit(2).unwrap();
    producer.request_pause(false);
    let report = state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    assert_eq!(
        state.pending_samples(),
        [0.25, -0.125, 0.25, -0.125, 0.25, -0.125]
    );
    assert_eq!(state.output_frame_basis().start_physical_frame(), 5);
    assert_eq!(state.mixer().frame_cursor(), 8);
    let config = state.mixer().configuration();
    let counters = state.mixer().counters();
    let mut completed = 0;
    let (result, heap) =
        count_heap_calls(|| state.apply_source_history_retirement(request(2, 8), &mut completed));
    assert_eq!(result, Ok(2));
    assert_eq!(heap, 0);
    assert_eq!(completed, 0, "a clamped request must stay pending");
    assert_eq!(
        state
            .mixer()
            .source_playback_history()
            .unwrap()
            .retained_start_frame(),
        4
    );
    state
        .reconfigure(
            state.format().unwrap(),
            Some(ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap()),
            1,
        )
        .unwrap();
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(state.admitted_frames(), 1);
    assert_eq!(state.mixer().configuration(), config);
    assert_eq!(state.mixer().counters(), counters);
    assert_eq!(
        state.apply_source_history_retirement(request(2, 8), &mut completed),
        Ok(0)
    );
    assert_eq!(completed, 0);
    state.admit(3).unwrap();
    assert_eq!(
        state.apply_source_history_retirement(request(2, 8), &mut completed),
        Ok(0)
    );
    assert_eq!(completed, 2);
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: Timestamp::from_nanos(6_000_000),
            gain: 0.5,
        })
        .unwrap();
    assert_eq!(state.render_pending(1).unwrap().start_frame, 8);
    assert_eq!(state.pending_samples(), [0.375, -0.1875]);
    assert_eq!(state.mixer().counters().commands_consumed, 2);
}

#[test]
fn block_start_pin_preserves_history_even_when_partial_suffix_is_after_an_internal_transition() {
    // An immutable endpoint creates a genuine Running -> Frozen transition
    // inside one report, after its start and before its first unsent frame.
    let (mut producer, mixer, _) = rig_with_end(8, false, Some(100));
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.,
        })
        .unwrap();
    let mut state = retained(mixer, 128);
    state.render_pending(98).unwrap();
    state.admit(98).unwrap();
    let report = state.render_pending(4).unwrap();
    assert_eq!(report.start_frame, 98);
    assert_eq!(report.playback_frames, 2);
    assert!(report.paused);
    assert_eq!(
        state.pending_samples(),
        [0.25, -0.125, 0.25, -0.125, 0., 0., 0., 0.]
    );
    state.admit(3).unwrap();
    assert_eq!(state.output_frame_basis().start_physical_frame(), 101);
    assert_eq!(state.mixer().practice_history_retention_pins().0, None);
    let history = state.mixer().source_playback_history().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history.span(1).unwrap().physical_start(), 100);
    let mut completed = 0;
    assert_eq!(
        state.apply_source_history_retirement(request(2, 102), &mut completed),
        Ok(0)
    );
    assert_eq!(completed, 0);
    let history = state.mixer().source_playback_history().unwrap();
    assert!(history.lookup_at_source_time(query(98)).is_ok());
    assert_eq!(history.retained_start_frame(), 0);
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(state.pending_samples(), [0., 0.]);
    state.admit(1).unwrap();
    assert_eq!(
        state.apply_source_history_retirement(request(2, 102), &mut completed),
        Ok(1)
    );
    assert_eq!(completed, 2);
    let history = state.mixer().source_playback_history().unwrap();
    assert!(history.lookup_at_source_time(query(98)).is_err());
    assert_eq!(history.retained_start_frame(), 100);
    assert_eq!(state.mixer().playback_frame_cursor(), 100);
}

#[test]
fn actual_practice_and_pending_pins_must_both_release_before_request_completion() {
    let (mut producer, mixer, controller) = rig(8, true);
    let mut controller = controller.unwrap();
    let mut state = retained(mixer, 8);
    state.render_pending(2).unwrap();
    state.admit(2).unwrap();
    producer.request_pause(true);
    state.render_pending(2).unwrap();
    state.admit(2).unwrap();
    producer.request_pause(false);
    state.render_pending(4).unwrap();
    let mut completed = 0;
    assert_eq!(state.mixer().practice_history_retention_pins().0, Some(0));
    assert_eq!(
        state.apply_source_history_retirement(request(2, 8), &mut completed),
        Ok(0)
    );
    assert_eq!(completed, 0);
    assert_eq!(
        controller
            .try_pop_projected_receipt()
            .unwrap()
            .receipt
            .physical_frame,
        0
    );
    assert_eq!(state.mixer().practice_history_retention_pins().0, None);
    assert_eq!(
        state.apply_source_history_retirement(request(2, 8), &mut completed),
        Ok(2)
    );
    assert_eq!(completed, 0);
    assert_eq!(
        state
            .mixer()
            .source_playback_history()
            .unwrap()
            .retained_start_frame(),
        4
    );
    state.admit(4).unwrap();
    assert_eq!(
        state.apply_source_history_retirement(request(2, 8), &mut completed),
        Ok(0)
    );
    assert_eq!(completed, 2);
}

#[test]
fn invalid_original_request_is_rejected_before_pending_and_practice_clamps() {
    let (_, mixer, _controller) = rig(8, true);
    let mut state = retained(mixer, 8);
    let report = state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    assert_eq!(state.mixer().practice_history_retention_pins().0, Some(0));
    let mut completed = 0;
    let counters = state.mixer().counters();
    let pcm = state.pending_samples().to_vec();
    for bad in [
        request(0, 2),
        request(1, 2),
        request(3, 2),
        request(2, 5),
        request(2, u64::MAX),
    ] {
        assert_eq!(
            state.apply_source_history_retirement(bad, &mut completed),
            Err(AudioError::InvalidFormat)
        );
        assert_eq!(completed, 0);
        assert_eq!(state.pending_samples(), pcm);
        assert_eq!(state.pending_report(), Some(report));
        assert_eq!(state.admitted_frames(), 1);
        assert_eq!(state.output_frame_basis().start_physical_frame(), 1);
        assert_eq!(state.mixer().counters(), counters);
        let history = state.mixer().source_playback_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history.retained_start_frame(), 0);
        assert_eq!(history.pulled_end_frame(), 4);
        assert_eq!(history.retirement_revision(), 0);
    }
    assert_eq!(
        state.apply_source_history_retirement(request(2, 4), &mut completed),
        Ok(0)
    );
    assert_eq!(completed, 0);
}
