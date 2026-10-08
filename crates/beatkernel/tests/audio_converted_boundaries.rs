use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};

fn rig(
    source_rate: u32,
    target_rate: u32,
    gate: Option<u64>,
    end: Option<u64>,
) -> (CommandProducer, ConvertedMixer) {
    rig_at(source_rate, target_rate, gate, end, -17_000_003)
}

fn rig_at(
    source_rate: u32,
    target_rate: u32,
    gate: Option<u64>,
    end: Option<u64>,
    origin_nanos: i64,
) -> (CommandProducer, ConvertedMixer) {
    let format = AudioFormat::new(source_rate, 1).unwrap();
    let pcm_limits = PcmLimits::new(1024, 1024, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![1.0; 128], pcm_limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = if gate.is_some() {
        command_queue_with_start_gate(8).unwrap()
    } else {
        command_queue(8).unwrap()
    };
    if let Some(frame) = gate {
        producer.schedule_start_at(frame).unwrap();
    }
    let origin = Timestamp::from_nanos(origin_nanos);
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: origin,
            gain: 1.0,
        })
        .unwrap();
    let mut config = MixerConfig::new(
        format,
        ClockDomainId(7),
        origin,
        AudioLimits::new(8, 2, 8, 128, 8).unwrap(),
    );
    if let Some(frame) = end {
        config = config.with_playback_end_frame(frame);
    }
    let mixer = Mixer::new(config, bank, consumer).unwrap();
    let converted = match ConvertedMixer::new(
        mixer,
        AudioFormat::new(target_rate, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    ) {
        Ok(converted) => converted,
        Err(failure) => panic!("unexpected conversion refusal: {:?}", failure.error()),
    };
    (producer, converted)
}

#[test]
fn startup_suppresses_lookahead_interpolation_before_exact_mapped_gate() {
    for (source_rate, target_rate) in [(44_100, 48_000), (32_000, 44_100), (48_000, 32_000)] {
        let (_producer, mut converted) = rig(source_rate, target_rate, Some(3), None);
        let mut output = [99.0; 8];
        let report = converted.render(&mut output).unwrap();
        let offset = (3_u64 * u64::from(target_rate)).div_ceil(u64::from(source_rate)) as usize;
        assert_eq!(&output[..offset], &vec![0.0; offset]);
        assert!(
            output[offset] > 0.0,
            "first eligible target sample must survive suppression"
        );
        let boundary = report.startup_boundary().unwrap().unwrap();
        assert_eq!(
            (boundary.source_frame, boundary.target_frame_offset),
            (3, offset)
        );
        assert_eq!(
            boundary.target_time,
            TargetTime::from_frames(offset as u64, target_rate).unwrap()
        );
        assert_eq!(report.startup_source_frame, Some(3));
        assert!(report.project_source_boundary(100).unwrap().is_none());
    }
}

#[test]
fn exclusive_finite_end_clips_target_suffix_without_using_source_report_extent() {
    for (source_rate, target_rate) in [(44_100, 48_000), (32_000, 44_100), (48_000, 32_000)] {
        let (mut producer, mut converted) = rig(source_rate, target_rate, None, Some(3));
        let mut output = [99.0; 8];
        let report = converted.render(&mut output).unwrap();
        let offset = (3_u64 * u64::from(target_rate)).div_ceil(u64::from(source_rate)) as usize;
        assert!(output[..offset].iter().all(|sample| *sample > 0.0));
        assert!(output[offset..].iter().all(|sample| *sample == 0.0));
        let source = report.source.unwrap();
        assert_eq!(source.playback_end_physical_frame, Some(3));
        assert!(source.paused);
        let boundary = report.end_boundary().unwrap().unwrap();
        assert_eq!(
            (boundary.source_frame, boundary.target_frame_offset),
            (3, offset)
        );
        assert_eq!(
            boundary.target_time,
            TargetTime::from_frames(offset as u64, target_rate).unwrap()
        );
        producer.request_pause(false);
        converted.render(&mut output).unwrap();
        assert_eq!(output, [0.0; 8], "a reached finite endpoint cannot reopen");
    }
}

#[test]
fn cached_audible_prefix_precedes_actual_source_pause_target_boundary() {
    let (mut producer, mut converted) = rig(24_000, 48_000, None, None);
    converted.render(&mut [0.0; 1]).unwrap();
    producer.request_pause(true);
    let mut output = [99.0; 8];
    let report = converted.render(&mut output).unwrap();
    let source = report.source.unwrap();
    assert!(source.paused);
    assert!(
        source.frames > 0,
        "this pause must actually be adopted by the source"
    );
    let boundary = report.pause_boundary().unwrap().unwrap();
    assert_eq!(
        (boundary.source_frame, boundary.target_frame_offset),
        (2, 3)
    );
    assert!(output[..3].iter().all(|sample| *sample > 0.0));
    assert!(output[3..].iter().all(|sample| *sample == 0.0));
    assert_eq!(
        boundary.target_time,
        TargetTime::from_frames(4, 48_000).unwrap()
    );
    let empty = converted.render(&mut []).unwrap();
    assert!(empty.source.unwrap().paused);
    assert_eq!(empty.source.unwrap().frames, 0);
    assert!(
        empty.pause_boundary().unwrap().is_none(),
        "empty cached source facts cannot acknowledge a new pause"
    );
}

#[test]
fn held_spans_advance_target_duration_but_never_adopt_pause_or_project_source_boundaries() {
    let (mut producer, mut converted) = rig(44_100, 48_000, Some(3), Some(9));
    producer.request_pause(true);
    let position = converted.source_position();
    let report = converted.render_held(&mut [99.0; 7]).unwrap();
    assert_eq!(report.state, ConvertedOutputState::Held);
    assert_eq!(report.source, None);
    assert_eq!(report.source_position, position);
    assert_eq!(
        report.target_start_time,
        TargetTime::from_frames(0, 48_000).unwrap()
    );
    assert_eq!(
        report.target_end_time,
        TargetTime::from_frames(7, 48_000).unwrap()
    );
    assert!(report.startup_boundary().unwrap().is_none());
    assert!(report.pause_boundary().unwrap().is_none());
    assert!(report.end_boundary().unwrap().is_none());
    assert!(report.project_source_boundary(0).unwrap().is_none());
    assert!(!converted.mixer().is_paused());
    producer.request_pause(false);
    let mut output = [99.0; 8];
    let active = converted.render(&mut output).unwrap();
    let boundary = active.startup_boundary().unwrap().unwrap();
    assert_eq!(boundary.target_frame_offset, 4);
    assert_eq!(
        boundary.target_time,
        TargetTime::from_frames(11, 48_000).unwrap()
    );
    assert_eq!(
        converted.target_time(),
        TargetTime::from_frames(15, 48_000).unwrap()
    );
}

#[test]
fn timestamp_overflow_refuses_before_pcm_queue_or_cursors_change() {
    let (_producer, mut converted) = rig_at(48_000, 48_000, None, None, i64::MAX);
    let before = (
        converted.target_time(),
        converted.target_frame_cursor(),
        converted.source_position(),
        converted.pulled_source_frame_cursor(),
        converted.mixer().counters(),
    );
    let mut output = [99.0; 1];
    assert_eq!(
        converted.render_held(&mut output),
        Err(AudioError::Overflow)
    );
    assert_eq!(output, [99.0; 1]);
    assert_eq!(converted.render(&mut output), Err(AudioError::Overflow));
    assert_eq!(output, [99.0; 1]);
    assert_eq!(
        (
            converted.target_time(),
            converted.target_frame_cursor(),
            converted.source_position(),
            converted.pulled_source_frame_cursor(),
            converted.mixer().counters()
        ),
        before
    );
    assert_eq!(
        converted
            .render(&mut [])
            .unwrap()
            .source
            .unwrap()
            .counters
            .commands_consumed,
        0
    );
    assert_eq!(converted.target_time(), TargetTime::new(0, 0, 1).unwrap());
}

#[test]
fn actual_nonempty_source_resume_waits_for_consumed_target_boundary_and_survives_cached_empty_reports(
) {
    let (mut producer, mut converted) = rig(24_000, 48_000, None, None);
    converted.render(&mut [0.0; 1]).unwrap();
    producer.request_pause(true);
    let paused = converted.render(&mut [0.0; 8]).unwrap();
    assert!(paused.source.unwrap().paused);
    assert_eq!(paused.pause_source_frame, Some(2));
    producer.request_pause(false);
    let mut actual_resume = None;
    let mut saw_unconsumed = false;
    let mut saw_empty = false;
    let mut crossed = false;
    for _ in 0..12 {
        let report = converted.render(&mut [0.0; 1]).unwrap();
        let source = report.source.unwrap();
        if actual_resume.is_none()
            && source.frames > 0
            && !source.paused
            && source.playback_frames > 0
        {
            actual_resume = Some(source.start_frame);
        }
        let Some(frame) = actual_resume else {
            assert_eq!(report.resume_source_frame, None);
            continue;
        };
        assert_eq!(report.resume_source_frame, Some(frame));
        // This fixture has no held time: one source frame is exactly two target
        // frames. Derive the crossing directly from that ratio, never a helper.
        let mapped_target = frame * 2;
        let first = report.target_frame_cursor - report.target_frames as u64;
        if mapped_target > report.target_frame_cursor {
            assert_eq!(report.resume_boundary().unwrap(), None);
            saw_unconsumed = true;
        } else if mapped_target >= first {
            let boundary = report.resume_boundary().unwrap().unwrap();
            assert_eq!(
                (boundary.source_frame, boundary.target_frame_offset),
                (frame, (mapped_target - first) as usize)
            );
            assert_eq!(
                boundary.target_time,
                TargetTime::from_frames(mapped_target, 48_000).unwrap()
            );
            crossed = true;
        } else {
            assert_eq!(report.resume_boundary().unwrap(), None);
        }
        if source.frames == 0 {
            saw_empty = true;
            assert_eq!(report.resume_source_frame, Some(frame));
        }
    }
    assert!(
        actual_resume.is_some() && saw_unconsumed && saw_empty && crossed,
        "exercise actual early source resume, cache and target crossing"
    );
    let held = converted.render_held(&mut [0.0; 3]).unwrap();
    assert_eq!(held.resume_source_frame, actual_resume);
    assert_eq!(held.resume_boundary().unwrap(), None);
}

#[test]
fn repeated_paused_source_pulls_keep_first_adoption_marker_and_new_pause_clears_prior_resume() {
    let (mut producer, mut converted) = rig(24_000, 48_000, None, None);
    converted.render(&mut [0.0; 1]).unwrap();
    producer.request_pause(true);
    let first = converted.render(&mut [0.0; 8]).unwrap();
    let pause = first.pause_source_frame.unwrap();
    for _ in 0..3 {
        let report = converted.render(&mut [0.0; 4]).unwrap();
        assert!(report.source.unwrap().paused);
        assert_eq!(report.pause_source_frame, Some(pause));
        assert_eq!(report.resume_source_frame, None);
    }
    producer.request_pause(false);
    let mut resume = None;
    for _ in 0..8 {
        let report = converted.render(&mut [0.0; 2]).unwrap();
        if report.resume_source_frame.is_some() {
            resume = report.resume_source_frame;
            break;
        }
    }
    assert!(resume.is_some());
    producer.request_pause(true);
    let mut adopted = false;
    for _ in 0..8 {
        let report = converted.render(&mut [0.0; 2]).unwrap();
        if report.source.unwrap().frames > 0 && report.source.unwrap().paused {
            assert_eq!(report.resume_source_frame, None);
            assert!(report.pause_source_frame.unwrap() > pause);
            adopted = true;
            break;
        }
    }
    assert!(adopted);
}
