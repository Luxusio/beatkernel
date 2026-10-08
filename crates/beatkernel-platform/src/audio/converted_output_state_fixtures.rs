use super::*;
use crate::audio::{count_heap_calls, fail_allocation, SampleEncoding};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};

fn sample(frame: usize) -> f32 {
    0.125 + frame as f32 / 512.0
}
fn rig(rate: u32) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(rate, 1).unwrap();
    let limits = PcmLimits::new(1024, 1024, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, (0..128).map(sample).collect(), limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(16).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::from_nanos(-123),
            gain: 1.0,
        })
        .unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::from_nanos(-123),
            AudioLimits::new(16, 4, 16, 256, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}
fn device(rate: u32) -> DeviceFormat {
    DeviceFormat::new(rate, 1, SampleEncoding::Float32, None).unwrap()
}
fn matrix() -> ChannelMatrix {
    ChannelMatrix::default_mix(1, 1).unwrap()
}
fn owner(mixer: Mixer, rate: u32, max: usize) -> ConvertedNativeOutputState {
    ConvertedNativeOutputState::new(mixer, device(rate), matrix(), ResampleQuality::Linear, max)
        .unwrap_or_else(|_| panic!("valid converted owner"))
}
fn close(actual: f32, expected: f64) {
    assert!(
        (f64::from(actual) - expected).abs() < 2e-6,
        "{actual} != {expected}"
    );
}

#[test]
fn target_suffix_first_unsent_basis_survives_short_admission_and_smaller_period_without_heap() {
    let (_producer, mixer) = rig(44_100);
    let mut state = owner(mixer, 48_000, 8);
    let ((report, admitted), calls) =
        count_heap_calls(|| (state.render_pending(8), state.admit(2)));
    let report = report.unwrap();
    admitted.unwrap();
    assert_eq!(calls, 0);
    assert_eq!(report.target_frames, 8);
    assert_eq!(state.pending_frames(), 6);
    for (index, actual) in state.pending_samples().iter().enumerate() {
        close(
            *actual,
            0.125 + (index + 2) as f64 * 147.0 / (160.0 * 512.0),
        );
    }
    let basis = state.target_frame_basis();
    assert_eq!(
        basis.start_time(),
        TargetTime::from_frames(2, 48_000).unwrap()
    );
    assert_eq!(basis.sample_rate(), 48_000);
    assert_eq!(basis.origin().timestamp, Timestamp::from_nanos(-123));
    let counters = state.mixer().counters();
    let pulled = state.mixer().frame_cursor();
    state.reconfigure(device(48_000), matrix(), 2).unwrap();
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(state.pending_frames(), 6);
    assert_eq!(state.target_frame_basis(), basis);
    assert!(state.render_pending(2).is_err());
    for _ in 0..3 {
        state.admit(2).unwrap();
    }
    assert_eq!(state.mixer().counters(), counters);
    assert_eq!(state.mixer().frame_cursor(), pulled);
    assert_eq!(
        state.target_frame_basis().start_time(),
        TargetTime::from_frames(8, 48_000).unwrap()
    );
    let (fresh, calls) = count_heap_calls(|| state.render_pending(2));
    fresh.unwrap();
    assert_eq!(calls, 0);
    close(
        state.pending_samples()[0],
        0.125 + 8.0 * 147.0 / (160.0 * 512.0),
    );
}

#[test]
fn incompatible_pending_rate_encoding_matrix_and_invalid_counts_refuse_atomically() {
    let (_producer, mixer) = rig(44_100);
    let mut state = owner(mixer, 48_000, 8);
    state.render_pending(8).unwrap();
    state.admit(2).unwrap();
    let pcm = state.pending_samples().to_vec();
    let basis = state.target_frame_basis();
    let report = state.pending_report();
    let counters = state.mixer().counters();
    for (format, mix, capacity) in [
        (device(32_000), matrix(), 8),
        (
            DeviceFormat::new(
                48_000,
                1,
                SampleEncoding::Pcm {
                    container_bits: 16,
                    valid_bits: 16,
                },
                None,
            )
            .unwrap(),
            matrix(),
            8,
        ),
        (device(48_000), ChannelMatrix::new(1, 1, &[0.5]).unwrap(), 8),
        (device(48_000), matrix(), 0),
    ] {
        assert!(state.reconfigure(format, mix, capacity).is_err());
        assert_eq!(state.pending_samples(), pcm);
        assert_eq!(state.pending_report(), report);
        assert_eq!(state.target_frame_basis(), basis);
        assert_eq!(state.mixer().counters(), counters);
        assert_eq!(state.admitted_frames(), 2);
        assert_eq!(state.max_frames(), 8);
    }
    for count in [0, 7, usize::MAX] {
        assert!(state.admit(count).is_err());
        assert_eq!(state.pending_samples(), pcm);
    }
    state.admit(6).unwrap();
    state.reconfigure(device(32_000), matrix(), 8).unwrap();
    assert_eq!(state.target_frame_basis().sample_rate(), 32_000);
    assert_eq!(
        state.target_frame_basis().start_time(),
        TargetTime::from_frames(8, 48_000).unwrap()
    );
}

#[test]
fn held_rate_epochs_advance_exact_physical_duration_without_source_phase_or_queue() {
    let (mut producer, mixer) = rig(44_100);
    let mut state = owner(mixer, 48_000, 8);
    state.render_pending(8).unwrap();
    state.admit(8).unwrap();
    let source = state.converter_owner().source_position();
    let counters = state.mixer().counters();
    producer.request_pause(true);
    state.reconfigure(device(32_000), matrix(), 8).unwrap();
    let (held, calls) = count_heap_calls(|| state.render_held_pending(3));
    let held = held.unwrap();
    assert_eq!(calls, 0);
    assert_eq!(held.state, ConvertedOutputState::Held);
    assert_eq!(held.source, None);
    assert_eq!(state.pending_samples(), [0.; 3]);
    assert_eq!(state.converter_owner().source_position(), source);
    assert_eq!(state.mixer().counters(), counters);
    assert!(!state.mixer().is_paused());
    state.admit(1).unwrap();
    const DEN: u128 = 14_112_000;
    let ticks = 8 * (DEN / 48_000) + DEN / 32_000;
    assert_eq!(
        state
            .target_frame_basis()
            .point_at_stream_frame(0)
            .unwrap()
            .timestamp
            .as_nanos(),
        -123 + (ticks * 1_000_000_000 / DEN) as i64
    );
    state.admit(2).unwrap();
    producer.request_pause(false);
    state.reconfigure(device(44_100), matrix(), 8).unwrap();
    let (fresh, calls) = count_heap_calls(|| state.render_pending(3));
    fresh.unwrap();
    assert_eq!(calls, 0);
    close(
        state.pending_samples()[0],
        0.125 + 8.0 * 147.0 / (160.0 * 512.0),
    );
    let start = 8 * (DEN / 48_000) + 3 * (DEN / 32_000);
    assert_eq!(
        state
            .target_frame_basis()
            .point_at_stream_frame(0)
            .unwrap()
            .timestamp
            .as_nanos(),
        -123 + (start * 1_000_000_000 / DEN) as i64
    );
}

#[test]
fn cached_audible_target_tail_cannot_admit_pause_from_source_report_alone() {
    let (mut producer, mixer) = rig(24_000);
    let mut state = owner(mixer, 48_000, 8);
    state.render_pending(1).unwrap();
    state.admit(1).unwrap();
    producer.request_pause(true);
    let report = state.render_pending(8).unwrap();
    assert!(report.source.unwrap().paused);
    assert!(state.pending_samples()[0] > 0.0);
    assert_eq!(
        state.boundaries().pause.unwrap().target_time,
        TargetTime::from_frames(4, 48_000).unwrap()
    );
    assert_ne!(
        state.target_frame_basis().start_time(),
        state.boundaries().pause.unwrap().target_time
    );
    state.admit(3).unwrap();
    assert!(state.pending_samples().iter().all(|sample| *sample == 0.0));
    assert_eq!(
        state.target_frame_basis().start_time(),
        state.boundaries().pause.unwrap().target_time
    );
}

#[test]
fn cold_allocation_refusals_return_original_mixer_and_growth_preserves_exact_retry() {
    let mut refusals = 0;
    for index in 0..8 {
        let (mut producer, mixer) = rig(44_100);
        let mix = matrix();
        let attempt = fail_allocation(index, || {
            ConvertedNativeOutputState::new(mixer, device(48_000), mix, ResampleQuality::Linear, 8)
        });
        let failure = match attempt {
            Err(failure) => failure,
            Ok(_) => break,
        };
        refusals += 1;
        assert_eq!(failure.error(), &AudioError::AllocationFailed);
        let (_, mixer) = failure.into_parts();
        let mut mixer = mixer.unwrap();
        assert_eq!(mixer.frame_cursor(), 0);
        assert_eq!(mixer.counters().commands_consumed, 0);
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(2),
                sample: SampleId(1),
                at: Timestamp::from_nanos(-123),
                gain: 0.5,
            })
            .unwrap();
        let mut output = [0.; 2];
        mixer.render(&mut output).unwrap();
        assert_eq!(output, [sample(0) * 1.5, sample(1) * 1.5]);
        assert_eq!(mixer.counters().commands_consumed, 2);
    }
    assert!(
        refusals > 0 && refusals < 8,
        "exercise real fallible allocations through to successful construction"
    );
    let (_producer, mixer) = rig(44_100);
    let mut state = owner(mixer, 48_000, 4);
    state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let before = state.pending_samples().to_vec();
    let basis = state.target_frame_basis();
    let report = state.pending_report();
    let mix = matrix();
    assert_eq!(
        fail_allocation(0, || state.reconfigure(device(48_000), mix, 16)),
        Err(AudioError::AllocationFailed)
    );
    assert_eq!(state.pending_samples(), before);
    assert_eq!(state.target_frame_basis(), basis);
    assert_eq!(state.pending_report(), report);
    assert_eq!(state.max_frames(), 4);
    state.reconfigure(device(48_000), matrix(), 16).unwrap();
    state.admit(3).unwrap();
    state.render_pending(2).unwrap();
    close(
        state.pending_samples()[0],
        0.125 + 4.0 * 147.0 / (160.0 * 512.0),
    );
}

#[test]
fn resume_facts_require_actual_adoption_and_consumed_target_crossing_then_survive_coalescing() {
    let (mut producer, mixer) = rig(24_000);
    let mut state = owner(mixer, 48_000, 8);
    state.render_pending(1).unwrap();
    state.admit(1).unwrap();
    producer.request_pause(true);
    state.render_pending(8).unwrap();
    state.admit(8).unwrap();
    producer.request_pause(false);
    let mut actual = None;
    let mut mapped = None;
    let mut unconsumed = false;
    let mut cached = false;
    for _ in 0..12 {
        let report = state.render_pending(1).unwrap();
        let source = report.source.unwrap();
        if actual.is_none() && source.frames > 0 && !source.paused && source.playback_frames > 0 {
            actual = Some(source.start_frame);
        }
        if let Some(frame) = actual {
            assert_eq!(report.resume_source_frame, Some(frame));
            if frame * 2 > report.target_frame_cursor {
                assert_eq!(state.boundaries().resume, None);
                unconsumed = true;
            }
            if let Some(boundary) = report.resume_boundary().unwrap() {
                assert_eq!(
                    boundary.target_time,
                    TargetTime::from_frames(frame * 2, 48_000).unwrap()
                );
                mapped = Some(boundary);
            }
            assert_eq!(state.boundaries().resume, mapped);
            if source.frames == 0 {
                cached = true;
            }
        } else {
            assert_eq!(state.boundaries().resume, None);
        }
        state.admit(1).unwrap();
    }
    assert!(actual.is_some() && mapped.is_some() && unconsumed && cached);
    let source_position = state.converter_owner().source_position();
    let source_report = state.last_real_source_report();
    let held = state.render_held_pending(3).unwrap();
    assert_eq!(held.source, None);
    assert_eq!(held.resume_boundary().unwrap(), None);
    assert_eq!(state.boundaries().resume, mapped);
    assert_eq!(state.last_real_source_report(), source_report);
    assert_eq!(state.converter_owner().source_position(), source_position);
    state.admit(1).unwrap();
    assert_eq!(state.boundaries().resume, mapped);
}

#[test]
fn held_resume_request_does_not_adopt_source_and_exact_resume_time_includes_held_span() {
    let (mut producer, mixer) = rig(24_000);
    let mut state = owner(mixer, 48_000, 8);
    state.render_pending(1).unwrap();
    state.admit(1).unwrap();
    producer.request_pause(true);
    state.render_pending(8).unwrap();
    state.admit(8).unwrap();
    producer.request_pause(false);
    let before = state.converter_owner().source_position();
    let counters = state.mixer().counters();
    state.render_held_pending(4).unwrap();
    assert_eq!(state.boundaries().resume, None);
    assert!(state.mixer().is_paused());
    assert_eq!(state.converter_owner().source_position(), before);
    assert_eq!(state.mixer().counters(), counters);
    state.admit(4).unwrap();
    let mut mapped = None;
    for _ in 0..12 {
        let report = state.render_pending(1).unwrap();
        if let Some(boundary) = report.resume_boundary().unwrap() {
            assert_eq!(
                boundary.target_time,
                TargetTime::from_frames(4 + boundary.source_frame * 2, 48_000).unwrap()
            );
            mapped = Some(boundary);
        }
        state.admit(1).unwrap();
    }
    assert!(mapped.is_some());
    assert_eq!(state.boundaries().resume, mapped);
}

#[test]
fn active_zero_pcm_is_never_held_and_pending_render_cannot_relabel_its_provenance() {
    let (mut producer, mixer) = rig(44_100);
    producer
        .try_push(AudioCommand::Stop {
            voice: VoiceId(1),
            at: Timestamp::from_nanos(-123),
        })
        .unwrap();
    let mut state = owner(mixer, 48_000, 8);
    assert!(!state.pending_is_held());
    let report = state.render_pending(8).unwrap();
    assert_eq!(report.state, ConvertedOutputState::Active);
    assert!(report.source.unwrap().frames > 0);
    assert!(state.mixer().playback_frame_cursor() > 0);
    assert!(state.pending_samples().iter().all(|sample| *sample == 0.0));
    assert!(!state.pending_is_held());
    state.admit(2).unwrap();
    assert!(!state.pending_is_held());
    let pcm = state.pending_samples().to_vec();
    let phase = state.converter_owner().source_position();
    let basis = state.target_frame_basis();
    let counters = state.mixer().counters();
    let source_report = state.last_real_source_report();
    let boundaries = state.boundaries();
    assert_eq!(state.render_held_pending(2), Err(AudioError::InvalidBuffer));
    assert!(!state.pending_is_held());
    assert_eq!(state.pending_samples(), pcm);
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(state.admitted_frames(), 2);
    assert_eq!(state.converter_owner().source_position(), phase);
    assert_eq!(state.target_frame_basis(), basis);
    assert_eq!(state.mixer().counters(), counters);
    assert_eq!(state.last_real_source_report(), source_report);
    assert_eq!(state.boundaries(), boundaries);
    state.admit(6).unwrap();
    assert!(!state.pending_is_held());
    let (held, calls) = count_heap_calls(|| state.render_held_pending(3));
    assert_eq!(calls, 0);
    let held = held.unwrap();
    assert_eq!(held.state, ConvertedOutputState::Held);
    assert_eq!(held.source, None);
    assert!(state.pending_is_held());
    assert_eq!(state.converter_owner().source_position(), phase);
    assert_eq!(state.mixer().counters(), counters);
    state.admit(1).unwrap();
    assert!(state.pending_is_held());
    assert_eq!(state.pending_report(), Some(held));
    state.admit(2).unwrap();
    assert!(!state.pending_is_held());
    let active = state.render_pending(2).unwrap();
    assert_eq!(active.state, ConvertedOutputState::Active);
    assert!(state.pending_samples().iter().all(|sample| *sample == 0.0));
    assert!(!state.pending_is_held());
}

#[test]
fn held_partial_prefix_and_cold_failures_preserve_actual_generation_bytes_phase_and_reports() {
    let (mut producer, mixer) = rig(44_100);
    let mut state = owner(mixer, 48_000, 8);
    state.render_pending(8).unwrap();
    state.admit(8).unwrap();
    producer.request_pause(true);
    let held = state.render_held_pending(8).unwrap();
    assert!(state.pending_is_held());
    assert_eq!(held.state, ConvertedOutputState::Held);
    let (prefix, calls) = count_heap_calls(|| {
        state.admit(2)?;
        Ok::<_, AudioError>(state.pending_is_held())
    });
    assert_eq!(prefix, Ok(true));
    assert_eq!(calls, 0);
    let pcm = state.pending_samples().to_vec();
    let phase = state.converter_owner().source_position();
    let basis = state.target_frame_basis();
    let source_report = state.last_real_source_report();
    let boundaries = state.boundaries();
    let counters = state.mixer().counters();
    for operation in 0..9 {
        let failed = match operation {
            0 => state.render_pending(2).map(|_| ()),
            1 => state.render_held_pending(2).map(|_| ()),
            2 => state.admit(0),
            3 => state.admit(7),
            4 => state.reconfigure(device(32_000), matrix(), 8),
            5 => state.reconfigure(device(48_000), ChannelMatrix::new(1, 1, &[0.5]).unwrap(), 8),
            6 => state.reconfigure(
                DeviceFormat::new(
                    48_000,
                    1,
                    SampleEncoding::Pcm {
                        container_bits: 16,
                        valid_bits: 16,
                    },
                    None,
                )
                .unwrap(),
                matrix(),
                8,
            ),
            7 => state.reconfigure(device(48_000), matrix(), 0),
            _ => {
                let mix = matrix();
                fail_allocation(0, || state.reconfigure(device(48_000), mix, 16))
            }
        };
        assert!(failed.is_err());
        assert!(state.pending_is_held());
        assert_eq!(state.pending_samples(), pcm);
        assert_eq!(state.pending_report(), Some(held));
        assert_eq!(state.admitted_frames(), 2);
        assert_eq!(state.converter_owner().source_position(), phase);
        assert_eq!(state.target_frame_basis(), basis);
        assert_eq!(state.last_real_source_report(), source_report);
        assert_eq!(state.boundaries(), boundaries);
        assert_eq!(state.mixer().counters(), counters);
        assert_eq!(state.max_frames(), 8);
    }
    state.reconfigure(device(48_000), matrix(), 2).unwrap();
    assert!(state.pending_is_held());
    assert_eq!(state.pending_samples(), pcm);
    assert_eq!(state.pending_report(), Some(held));
    state.admit(6).unwrap();
    assert!(!state.pending_is_held());
    assert_eq!(state.pending_report(), None);
    assert!(state.pending_samples().is_empty());
    // The fully admitted report remains an internal past block; failing a fresh
    // render cannot turn that old held generation into new pending evidence.
    let drained_basis = state.target_frame_basis();
    for held_render in [false, true] {
        for frames in [0, 3] {
            let refused = if held_render {
                state.render_held_pending(frames)
            } else {
                state.render_pending(frames)
            };
            assert!(refused.is_err());
            assert!(!state.pending_is_held());
            assert_eq!(state.pending_report(), None);
            assert_eq!(state.converter_owner().source_position(), phase);
            assert_eq!(state.target_frame_basis(), drained_basis);
            assert_eq!(state.last_real_source_report(), source_report);
            assert_eq!(state.boundaries(), boundaries);
            assert_eq!(state.mixer().counters(), counters);
        }
    }
    state.render_held_pending(2).unwrap();
    assert!(state.pending_is_held());
    assert_eq!(state.pending_samples(), [0.0; 2]);
    assert_eq!(state.converter_owner().source_position(), phase);
    assert_eq!(state.mixer().counters(), counters);
}
