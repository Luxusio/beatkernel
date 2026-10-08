//! Actual source execution, conversion and original native associations for replay.
use crate::{
    bgm::{BgmConfig, BgmFeeder},
    completion::ReplayCompletion,
    finite_replay_completion::FiniteReplayCompletion,
    gameplay::output::ports::TargetOutputTelemetry,
    native_converted_gameplay_fixtures::{native, point},
    playback_pause::PausePhase,
    replay_pause::ReplayPause,
};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPair, Duration, Timestamp},
};
use beatkernel_platform::audio::{ConvertedNativeOutputState, DeviceFormat, SampleEncoding};

fn pair(basis: TargetFrameBasis, frame: u64) -> ClockPair {
    native(
        basis,
        frame,
        1_000_000_000 + (frame * 1_000_000_000 / 48_000) as i64,
    )
    .1
}
fn rig(
    finite: Option<u64>,
    sample_frames: usize,
) -> (CommandProducer, ConvertedNativeOutputState, BgmFeeder) {
    let format = AudioFormat::new(44_100, 1).unwrap();
    let pcm = PcmLimits::new(4096, 4096, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; sample_frames], pcm).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(16).unwrap();
    let mut feeder = BgmFeeder::from_output_commands(
        vec![AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        }],
        BgmConfig {
            output_origin: point(2, 0),
            sample_rate: 44_100,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(1_000_000_000),
            max_pending: 16,
        },
    )
    .unwrap();
    feeder
        .feed(0, 16, |command| producer.try_push(command))
        .unwrap();
    let mut config = MixerConfig::new(
        format,
        ClockDomainId(2),
        Timestamp::ZERO,
        AudioLimits::new(16, 4, 16, 256, 16).unwrap(),
    );
    if let Some(end) = finite {
        config = config.with_playback_end_frame(end);
    }
    let mixer = Mixer::new(config, bank, consumer).unwrap();
    let output = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(48_000, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        128,
    )
    .unwrap_or_else(|_| panic!("actual replay converter setup"));
    (producer, output, feeder)
}
fn tuple(
    output: &ConvertedNativeOutputState,
    converted: ConvertedRenderReport,
) -> TargetOutputTelemetry {
    TargetOutputTelemetry {
        source: output.last_real_source_report(),
        converted: Some(converted),
        facts: output.boundaries(),
    }
}
fn render(output: &mut ConvertedNativeOutputState, frames: usize) -> ConvertedRenderReport {
    let report = output.render_pending(frames).unwrap();
    output.admit(frames).unwrap();
    report
}
fn pause_state(
    pause: &ReplayPause,
) -> (PausePhase, Option<RenderReport>, Option<TargetFrameBasis>) {
    (
        pause.phase(),
        pause.last_render_report(),
        pause.target_basis(),
    )
}

#[test]
fn replay_target_pause_native_ack_and_held_resume_keep_recorded_source_song_and_command_time() {
    let (mut producer, mut output, _feeder) = rig(None, 512);
    let basis = output.target_frame_basis();
    let start = Timestamp::from_nanos(50_000_000);
    let preroll = Duration::from_nanos(3_000_000);
    let mut pause = ReplayPause::new(point(2, 0), ClockDomainId(1), 44_100, start, preroll)
        .unwrap()
        .with_target_basis(7, basis)
        .unwrap();
    render(&mut output, 1);
    assert!(pause
        .request_target(true, 7, basis, output.boundaries(), pair(basis, 1))
        .unwrap());
    producer.request_pause(true);
    render(&mut output, 8);
    let facts = output.boundaries();
    let transition = facts.pause.unwrap();
    assert_eq!(transition.source_frame, 2); // Actual source lookahead was executed.
    assert_eq!(
        transition.target_time,
        TargetTime::from_frames(3, 48_000).unwrap()
    );
    assert_eq!(
        pause
            .observe_target(
                7,
                basis,
                facts,
                output.last_real_source_report(),
                pair(basis, 2)
            )
            .unwrap(),
        None
    );
    assert_eq!(pause.phase(), PausePhase::Pausing);
    let frozen = pause
        .observe_target(
            7,
            basis,
            facts,
            output.last_real_source_report(),
            pair(basis, 3),
        )
        .unwrap()
        .unwrap();
    assert!(frozen.paused);
    assert_eq!(
        frozen.song,
        Timestamp::from_nanos(47_000_000 + 2 * 1_000_000_000 / 44_100)
    );
    assert_eq!(frozen.host, pair(basis, 3).target);
    let source_cursor = output.mixer().playback_frame_cursor();
    let applied = output.mixer().counters().commands_applied;
    let cue_at = Timestamp::from_nanos(4 * 1_000_000_000 / 44_100);
    let recorded_cue = AudioCommand::Play {
        voice: VoiceId(2),
        sample: SampleId(1),
        at: cue_at,
        gain: 0.5,
    };
    producer.try_push(recorded_cue).unwrap();
    let held = output.render_held_pending(128).unwrap();
    assert!(held.source.is_none());
    assert!(output.pending_samples().iter().all(|sample| *sample == 0.0));
    output.admit(128).unwrap();
    assert_eq!(output.mixer().playback_frame_cursor(), source_cursor);
    assert_eq!(output.mixer().counters().commands_applied, applied);
    assert_eq!(
        pause.presentation_song(pair(basis, 137).source).unwrap(),
        None
    );
    assert_eq!(
        pause
            .observe_target(
                7,
                basis,
                output.boundaries(),
                output.last_real_source_report(),
                pair(basis, 137)
            )
            .unwrap(),
        None
    );
    assert!(pause
        .request_target(false, 7, basis, output.boundaries(), pair(basis, 137))
        .unwrap());
    // Native owner exits held mode before the recorded source producer resumes.
    producer.request_pause(false);
    render(&mut output, 16);
    let resume = output.boundaries().resume.unwrap();
    let resume_point = resume.target_time.point(basis.origin()).unwrap();
    let resume_frame = (u128::from(resume.target_time.numerator()) * 48_000
        / u128::from(resume.target_time.denominator())) as u64
        + resume.target_time.seconds() * 48_000;
    assert_eq!(
        pause
            .observe_target(
                7,
                basis,
                output.boundaries(),
                output.last_real_source_report(),
                pair(basis, resume_frame - 1)
            )
            .unwrap(),
        None
    );
    let resumed = pause
        .observe_target(
            7,
            basis,
            output.boundaries(),
            output.last_real_source_report(),
            pair(basis, resume_frame),
        )
        .unwrap()
        .unwrap();
    assert!(!resumed.paused);
    assert_eq!(resumed.song, frozen.song);
    assert_eq!(resumed.host, pair(basis, resume_frame).target);
    assert_eq!(pause.phase(), PausePhase::Running);
    assert!(pause.presentation_song(resume_point).unwrap().is_some());
    assert_eq!(output.mixer().counters().commands_applied, applied + 1);
    assert_eq!(
        output.mixer().playback_frame_cursor(),
        source_cursor + output.last_real_source_report().unwrap().playback_frames as u64
    );
}

#[test]
fn replay_target_pause_identity_and_malformed_facts_refuse_without_ack_or_cached_source_changes() {
    let (mut producer, mut output, _feeder) = rig(None, 512);
    let basis = output.target_frame_basis();
    let mut pause = ReplayPause::new(
        point(2, 0),
        ClockDomainId(1),
        44_100,
        Timestamp::ZERO,
        Duration::ZERO,
    )
    .unwrap()
    .with_target_basis(7, basis)
    .unwrap();
    render(&mut output, 1);
    let before = pause_state(&pause);
    let mut wrong = output.boundaries();
    wrong.source_rate = 48_000;
    assert!(pause
        .request_target(true, 7, basis, wrong, pair(basis, 1))
        .is_err());
    assert_eq!(pause_state(&pause), before);
    assert!(pause.request(true, pair(basis, 1)).is_err());
    assert_eq!(pause_state(&pause), before);
    pause
        .request_target(true, 7, basis, output.boundaries(), pair(basis, 1))
        .unwrap();
    producer.request_pause(true);
    render(&mut output, 8);
    let facts = output.boundaries();
    let source = output.last_real_source_report();
    let before = pause_state(&pause);
    assert!(pause
        .observe_target(8, basis, facts, source, pair(basis, 3))
        .is_err());
    assert_eq!(pause_state(&pause), before);
    let altered = TargetFrameBasis::new(basis.origin(), basis.start_time(), 32_000).unwrap();
    assert!(pause
        .observe_target(7, altered, facts, source, pair(basis, 3))
        .is_err());
    assert_eq!(pause_state(&pause), before);
    let mut invalid = pair(basis, 3);
    invalid.target.domain = ClockDomainId(99);
    assert!(pause
        .observe_target(7, basis, facts, source, invalid)
        .is_err());
    assert_eq!(pause_state(&pause), before);
    wrong = facts;
    wrong.pause.as_mut().unwrap().source_frame =
        source.unwrap().start_frame + source.unwrap().frames as u64 + 1;
    assert!(pause
        .observe_target(7, basis, wrong, source, pair(basis, 3))
        .is_err());
    assert_eq!(pause_state(&pause), before);
    assert!(
        pause
            .observe_target(7, basis, facts, source, pair(basis, 3))
            .unwrap()
            .unwrap()
            .paused
    );
}

#[test]
fn finite_replay_target_requires_exhausted_real_feeder_finished_records_and_original_mapped_crossing(
) {
    let (_producer, mut output, mut feeder) = rig(Some(3), 512);
    let basis = output.target_frame_basis();
    let mut completion = FiniteReplayCompletion::new(point(2, 0), 44_100, 3)
        .unwrap()
        .with_target_basis(7, basis, ClockDomainId(1))
        .unwrap();
    assert!(!completion
        .observe_target(false, &feeder, 7, basis, None, Some(pair(basis, 0)))
        .unwrap());
    let rendered = render(&mut output, 8);
    let telemetry = tuple(&output, rendered);
    assert_eq!(telemetry.facts.end.unwrap().source_frame, 3);
    assert_eq!(
        telemetry.facts.end.unwrap().target_time,
        TargetTime::from_frames(4, 48_000).unwrap()
    );
    assert!(telemetry.source.unwrap().active_voices > 0); // Finite endpoint freezes the voice.
    assert_eq!(feeder.report().outstanding, 1);
    assert!(!completion
        .observe_target(
            false,
            &feeder,
            7,
            basis,
            Some(telemetry),
            Some(pair(basis, 3))
        )
        .unwrap());
    feeder.retire_completed(3).unwrap();
    assert_eq!(feeder.report().remaining, 0);
    assert_eq!(feeder.report().outstanding, 0);
    assert!(!completion
        .observe_target(false, &feeder, 7, basis, None, Some(pair(basis, 4)))
        .unwrap());
    assert!(completion
        .observe_target(true, &feeder, 7, basis, None, Some(pair(basis, 5)))
        .unwrap());
}

#[test]
fn finite_replay_target_invalid_tuple_pair_and_epoch_preserve_first_genuine_lower_and_completion_state(
) {
    let (_producer, mut output, mut feeder) = rig(Some(3), 512);
    let basis = output.target_frame_basis();
    let mut completion = FiniteReplayCompletion::new(point(2, 0), 44_100, 3)
        .unwrap()
        .with_target_basis(7, basis, ClockDomainId(1))
        .unwrap();
    completion
        .observe_target(true, &feeder, 7, basis, None, Some(pair(basis, 0)))
        .unwrap();
    let rendered = render(&mut output, 8);
    let telemetry = tuple(&output, rendered);
    feeder.retire_completed(3).unwrap();
    let before = completion.clone();
    assert!(completion
        .observe_target(
            true,
            &feeder,
            8,
            basis,
            Some(telemetry),
            Some(pair(basis, 5))
        )
        .is_err());
    assert_eq!(completion, before);
    let mut wrong = telemetry;
    wrong.facts.source_rate = 48_000;
    assert!(completion
        .observe_target(true, &feeder, 7, basis, Some(wrong), Some(pair(basis, 5)))
        .is_err());
    assert_eq!(completion, before);
    let mut wrong = telemetry;
    wrong.facts.end.as_mut().unwrap().source_frame = 4;
    assert!(completion
        .observe_target(true, &feeder, 7, basis, Some(wrong), Some(pair(basis, 5)))
        .is_err());
    assert_eq!(completion, before);
    let mut invalid = pair(basis, 5);
    invalid.source.domain = ClockDomainId(99);
    assert!(completion
        .observe_target(true, &feeder, 7, basis, Some(telemetry), Some(invalid))
        .is_err());
    assert_eq!(completion, before);
    assert!(completion
        .observe(true, &feeder, telemetry.source, Some(pair(basis, 4).source))
        .is_err());
    assert_eq!(completion, before);
    assert!(completion
        .observe_target(
            true,
            &feeder,
            7,
            basis,
            Some(telemetry),
            Some(pair(basis, 4))
        )
        .unwrap());
    let committed = completion.clone();
    assert!(completion
        .observe_target(true, &feeder, 7, basis, None, Some(pair(basis, 3)))
        .is_err());
    assert_eq!(completion, committed);
}

#[test]
fn natural_replay_target_idle_source_lookahead_and_held_silence_cannot_ack_a_consumed_native_barrier(
) {
    let (_producer, mut output, mut feeder) = rig(None, 4);
    let basis = output.target_frame_basis();
    let mut completion = ReplayCompletion::new(ClockDomainId(2), 44_100)
        .with_target_basis(7, basis, ClockDomainId(1))
        .unwrap();
    let first = render(&mut output, 8);
    let first_tuple = tuple(&output, first);
    let first_source = first_tuple.source.unwrap();
    assert_eq!(first_source.active_voices, 0);
    feeder
        .retire_completed(first_source.counters.rendered_frames)
        .unwrap();
    assert!(!completion
        .observe_target(
            true,
            &feeder,
            7,
            basis,
            Some(first_tuple),
            Some(pair(basis, 0))
        )
        .unwrap());
    let second = render(&mut output, 2);
    let idle_tuple = tuple(&output, second);
    let idle_source = idle_tuple.source.unwrap();
    assert!(idle_source.start_frame >= first_source.counters.rendered_frames);
    let idle_source_end = idle_source.counters.rendered_frames;
    assert!(second.pulled_source_frame_cursor > second.source_position.frame);
    assert!(second
        .project_source_boundary(idle_source_end)
        .unwrap()
        .is_none());
    assert!(!completion
        .observe_target(
            true,
            &feeder,
            7,
            basis,
            Some(idle_tuple),
            Some(pair(basis, 10))
        )
        .unwrap());
    let held = output.render_held_pending(128).unwrap();
    let source_phase = output.converter_owner().source_position();
    assert!(output.pending_samples().iter().all(|sample| *sample == 0.0));
    output.admit(128).unwrap();
    let held_tuple = tuple(&output, held);
    assert!(!completion
        .observe_target(
            true,
            &feeder,
            7,
            basis,
            Some(held_tuple),
            Some(pair(basis, 138))
        )
        .unwrap());
    assert_eq!(output.converter_owner().source_position(), source_phase);
    let active = render(&mut output, 3);
    let mapped = active
        .project_source_boundary(idle_source_end)
        .unwrap()
        .unwrap();
    assert_eq!(
        mapped.target_time,
        TargetTime::from_frames(139, 48_000).unwrap()
    );
    let active_tuple = tuple(&output, active);
    assert!(!completion
        .observe_target(
            true,
            &feeder,
            7,
            basis,
            Some(active_tuple),
            Some(pair(basis, 138))
        )
        .unwrap());
    // A fixed mapped barrier survives more held output without granting an ACK.
    let held = output.render_held_pending(128).unwrap();
    output.admit(128).unwrap();
    assert!(!completion
        .observe_target(
            true,
            &feeder,
            7,
            basis,
            Some(tuple(&output, held)),
            Some(pair(basis, 139))
        )
        .unwrap());
    let later = render(&mut output, 1);
    assert!(completion
        .observe_target(
            true,
            &feeder,
            7,
            basis,
            Some(tuple(&output, later)),
            Some(pair(basis, 139))
        )
        .unwrap());
    assert_eq!(output.mixer().counters().commands_applied, 1);
    assert_eq!(feeder.report().total_admitted, 1);
}

#[test]
fn natural_replay_target_skipped_telemetry_chooses_actual_later_source_barrier_and_refusals_are_atomic(
) {
    let (_producer, mut output, mut feeder) = rig(None, 4);
    let basis = output.target_frame_basis();
    let mut completion = ReplayCompletion::new(ClockDomainId(2), 44_100)
        .with_target_basis(7, basis, ClockDomainId(1))
        .unwrap();
    let first = render(&mut output, 8);
    feeder
        .retire_completed(
            output
                .last_real_source_report()
                .unwrap()
                .counters
                .rendered_frames,
        )
        .unwrap();
    completion
        .observe_target(
            true,
            &feeder,
            7,
            basis,
            Some(tuple(&output, first)),
            Some(pair(basis, 0)),
        )
        .unwrap();
    let next = render(&mut output, 2);
    completion
        .observe_target(
            true,
            &feeder,
            7,
            basis,
            Some(tuple(&output, next)),
            Some(pair(basis, 1)),
        )
        .unwrap();
    // Several real active blocks are generated while auxiliary telemetry is
    // unavailable. Advancing native time cannot extrapolate an unmapped idle end.
    for _ in 0..3 {
        render(&mut output, 8);
    }
    assert!(!completion
        .observe_target(true, &feeder, 7, basis, None, Some(pair(basis, 2)))
        .unwrap());
    let later = render(&mut output, 2);
    let later_tuple = tuple(&output, later);
    let later_source_end = later_tuple.source.unwrap().counters.rendered_frames;
    assert!(later.source_start_position.frame > next.pulled_source_frame_cursor);
    assert!(later
        .project_source_boundary(later_source_end)
        .unwrap()
        .is_none());
    assert!(!completion
        .observe_target(
            true,
            &feeder,
            7,
            basis,
            Some(later_tuple),
            Some(pair(basis, 3))
        )
        .unwrap());
    let expected = completion.clone();
    let mut wrong = later_tuple;
    wrong.facts.source_rate = 48_000;
    assert!(completion
        .observe_target(true, &feeder, 7, basis, Some(wrong), Some(pair(basis, 100)))
        .is_err());
    assert!(completion
        .observe_target(
            true,
            &feeder,
            8,
            basis,
            Some(later_tuple),
            Some(pair(basis, 100))
        )
        .is_err());
    let altered = TargetFrameBasis::new(basis.origin(), basis.start_time(), 32_000).unwrap();
    assert!(completion
        .observe_target(
            true,
            &feeder,
            7,
            altered,
            Some(later_tuple),
            Some(pair(basis, 100))
        )
        .is_err());
    let mut invalid = pair(basis, 100);
    invalid.target.domain = ClockDomainId(99);
    assert!(completion
        .observe_target(true, &feeder, 7, basis, Some(later_tuple), Some(invalid))
        .is_err());
    let mut malformed = later_tuple;
    malformed
        .converted
        .as_mut()
        .unwrap()
        .source_position
        .denominator = 0;
    assert!(completion
        .observe_target(
            true,
            &feeder,
            7,
            basis,
            Some(malformed),
            Some(pair(basis, 100))
        )
        .is_err());
    assert!(completion
        .observe(
            true,
            feeder.report(),
            later_tuple.source,
            Some(pair(basis, 100).source)
        )
        .is_err());
    let mut reference = expected;
    let consumed = render(&mut output, 3);
    let mapping = consumed
        .project_source_boundary(later_source_end)
        .unwrap()
        .unwrap();
    let frontier = mapping.target_time.point(basis.origin()).unwrap();
    let high = pair(basis, consumed.target_frame_cursor);
    assert!(high.source.timestamp >= frontier.timestamp);
    let telemetry = tuple(&output, consumed);
    assert_eq!(
        completion
            .observe_target(true, &feeder, 7, basis, Some(telemetry), Some(high))
            .unwrap(),
        reference
            .observe_target(true, &feeder, 7, basis, Some(telemetry), Some(high))
            .unwrap()
    );
    assert!(completion
        .observe_target(true, &feeder, 7, basis, None, Some(high))
        .unwrap());
    assert!(completion
        .observe_target(true, &feeder, 7, basis, None, Some(pair(basis, 0)))
        .is_err());
}
