//! Retained AudioWorklet contracts tested through real PCM and copied evidence.
//! Expected sample positions and ABI words are independent literal oracles.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
    transport::{PositiveRatePhaseError, Rate},
};
use beatkernel_bms_runtime::{
    bgm::{retained_bgm_cues, retained_rate_geometry},
    worklet_audio::{
        WorkletAudio, WorkletAudioBuilder, WorkletAudioConfig, WorkletAudioError,
        WorkletPauseEvent, WorkletRetainedConfig, WorkletRetainedEnvelope, WorkletRetainedEvent,
    },
};

const PROGRAM: u64 = 0x14044;
const BASE: u64 = (1 << 53) + 107;
const WIDE_SOURCE_FRAME: u64 = (1 << 54) + 123;
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<[usize; 3]> = const { Cell::new([0; 3]) };
}
struct TrackingAllocator;
fn counted(kind: usize) {
    let _ = TRACK.try_with(|enabled| {
        if enabled.get() {
            let _ = COUNTS.try_with(|counts| {
                let mut value = counts.get();
                value[kind] += 1;
                counts.set(value);
            });
        }
    });
}
// SAFETY: All allocation arguments and results are forwarded unchanged to System.
unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        counted(0);
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        counted(0);
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        counted(1);
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        counted(2);
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;
fn tracked<T>(f: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|c| c.set([0; 3]));
    TRACK.with(|c| c.set(true));
    let result = f();
    TRACK.with(|c| c.set(false));
    (result, COUNTS.with(Cell::get))
}
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn rate(n: i64, d: u64) -> Rate {
    Rate::new(n, d).unwrap()
}
fn region(start: i64, end: i64, repeat: bool) -> PracticeRegion {
    PracticeRegion::new(ts(start), ts(end), repeat).unwrap()
}
fn cue(voice: u64, sample: u64, at: i64) -> PracticeCue {
    PracticeCue {
        voice: VoiceId(voice),
        sample: SampleId(sample),
        at: ts(at),
        gain: 1.0,
    }
}
fn play(voice: u64, sample: u64, at: i64) -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(voice),
        sample: SampleId(sample),
        at: ts(at),
        gain: 1.0,
    }
}
fn config(hz: u32, max_frames: usize) -> WorkletAudioConfig {
    WorkletAudioConfig {
        format: AudioFormat::new(hz, 1).unwrap(),
        pcm_limits: PcmLimits::new(4096, 8192, 8).unwrap(),
        audio_limits: AudioLimits::new(32, 16, 32, max_frames, 32).unwrap(),
    }
}
fn retained_config(selected: Rate, initial: PracticeRegion) -> WorkletRetainedConfig {
    WorkletRetainedConfig {
        program_id: PROGRAM,
        initial_region: initial,
        initial_rate: selected,
        output_preroll: Duration::ZERO,
        open_end: false,
        limits: PracticeLimits::new(32, 8, 256, 32, 32).unwrap(),
        source_history_capacity: 64,
    }
}
fn build(
    hz: u32,
    samples: Vec<(u64, u32, Vec<f32>)>,
    cues: Vec<PracticeCue>,
    setup: WorkletRetainedConfig,
) -> WorkletAudio {
    let mut builder = WorkletAudioBuilder::new(config(hz, 256)).unwrap();
    for (id, hz, pcm) in samples {
        builder
            .insert_sample(SampleId(id), AudioFormat::new(hz, 1).unwrap(), pcm)
            .unwrap();
    }
    builder.finish_retained(cues, setup).unwrap()
}
fn standard(selected: Rate, initial: PracticeRegion) -> WorkletAudio {
    build(
        1000,
        vec![(1, 1000, (0..8).map(|i| i as f32 / 8.0).collect())],
        vec![cue(1, 1, 0)],
        retained_config(selected, initial),
    )
}
fn prep(hz: u32, geometry: &[PracticeRateCueGeometry], selected: Rate) -> PracticeRatePreparation {
    prepare_practice_rate(PROGRAM, hz, geometry, selected).unwrap()
}
fn standard_prep(selected: Rate) -> PracticeRatePreparation {
    prep(
        1000,
        &[PracticeRateCueGeometry {
            at: ts(0),
            source_frames: 8,
            source_hz: 1000,
        }],
        selected,
    )
}
fn next(audio: &mut WorkletAudio) -> WorkletRetainedEnvelope {
    let event = audio
        .poll_retained_event()
        .unwrap()
        .expect("actual generated event");
    assert_eq!(event.program_id, PROGRAM);
    assert_eq!(event.context_start, BASE);
    audio.acknowledge_retained_event(event.serial).unwrap();
    event
}
fn started(audio: &mut WorkletAudio) {
    assert!(matches!(
        next(audio).event,
        WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
            event: PracticeEvent::Boundary(PracticeReceipt {
                kind: PracticeBoundaryKind::Started,
                ..
            }),
            ..
        })
    ));
}
fn changed(audio: &mut WorkletAudio) -> PracticeRateEvent {
    match next(audio).event {
        WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
            event: PracticeEvent::Rate(event),
            ..
        }) => event,
        other => panic!("expected rate evidence, got {other:?}"),
    }
}
fn request_rate(audio: &mut WorkletAudio, id: u64, preparation: PracticeRatePreparation) {
    let m = audio.retained_metadata().unwrap();
    audio
        .try_set_rate_next(id, m.generation, m.revision, preparation)
        .unwrap();
}
fn close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        assert!((a - e).abs() < 0.0000002, "sample {i}: {a} != {e}");
    }
}

#[test]
fn cold_retained_normal_and_half_rate_are_real_pcm_without_an_immutable_fence() {
    for (selected, expected) in [
        (Rate::NORMAL, [0.0, 0.125, 0.25, 0.375]),
        (rate(1, 2), [0.0, 0.0625, 0.125, 0.1875]),
    ] {
        let mut audio = standard(selected, region(0, 20_000_000, false));
        assert_eq!(audio.playback_end_frame(), None);
        assert!(audio.source_playback_history().is_some());
        assert!(audio.poll_retained_event().unwrap().is_none());
        audio.arm(BASE, BASE).unwrap();
        audio.render(BASE, 4).unwrap();
        assert_eq!(&audio.output()[..4], &expected);
        started(&mut audio);
        let m = audio.retained_metadata().unwrap();
        assert!(m.started && m.active);
        assert_eq!((m.rate, m.revision), (selected, 0));
        assert_eq!(audio.report().unwrap().playback_end_physical_frame, None);
        assert_eq!(audio.report().unwrap().counters.commands_consumed, 0);
    }
}

#[test]
fn active_cut_preserves_fractional_bgm_and_live_key_heads_without_double_feeding() {
    let mut audio = build(
        1000,
        vec![(1, 1000, (0..8).map(|i| i as f32 / 16.0).collect())],
        vec![cue(1, 1, 0)],
        retained_config(rate(1, 2), region(0, 20_000_000, false)),
    );
    audio.enqueue(play(99, 1, 0)).unwrap();
    audio.arm(BASE, BASE).unwrap();
    audio.render(BASE, 3).unwrap();
    assert_eq!(&audio.output()[..3], &[0.0, 0.0625, 0.125]);
    started(&mut audio);
    let before = audio.retained_metadata().unwrap();
    request_rate(&mut audio, 11, standard_prep(rate(2, 1)));
    assert_eq!(audio.retained_metadata().unwrap(), before);
    assert!(audio.poll_retained_event().unwrap().is_none());
    audio.render(BASE + 3, 4).unwrap();
    assert_eq!(&audio.output()[..4], &[0.1875, 0.4375, 0.6875, 0.875]);
    let e = changed(&mut audio);
    assert_eq!(
        (
            e.request_id,
            e.physical_frame,
            e.playback_frame,
            e.active_elapsed_nanos
        ),
        (11, 3, 3, 3_000_000)
    );
    assert_eq!(
        (e.previous_rate, e.requested_rate, e.applied_rate),
        (rate(1, 2), rate(2, 1), rate(2, 1))
    );
    assert_eq!(
        (e.generation, e.request_revision, e.applied_revision),
        (before.generation, 0, 1)
    );
    assert_eq!(e.outcome, PracticeRateOutcome::Applied);
    let report = audio.report().unwrap();
    assert_eq!(
        (
            report.counters.commands_consumed,
            report.counters.commands_applied
        ),
        (1, 1)
    );
    assert_eq!(report.counters.voice_full, 0);
}

#[test]
fn odd_preroll_cut_retimes_original_future_cue_and_end_on_nondivisor_grid() {
    let mut setup = retained_config(rate(2, 3), region(0, 140_000, false));
    setup.output_preroll = Duration::from_nanos(3);
    let cues = vec![cue(1, 1, 0), cue(2, 2, 80_000)];
    let mut audio = build(
        44_100,
        vec![
            (1, 22_050, (0..64).map(|i| i as f32 / 64.0).collect()),
            (2, 22_050, vec![0.25; 64]),
        ],
        cues,
        setup,
    );
    audio.arm(BASE, BASE).unwrap();
    audio.render(BASE, 3).unwrap();
    close(&audio.output()[..3], &[0.0, 0.0, 1.0 / 192.0]);
    started(&mut audio);
    let p = prep(
        44_100,
        &[
            PracticeRateCueGeometry {
                at: ts(0),
                source_frames: 64,
                source_hz: 22_050,
            },
            PracticeRateCueGeometry {
                at: ts(80_000),
                source_frames: 64,
                source_hz: 22_050,
            },
        ],
        rate(1, 2),
    );
    request_rate(&mut audio, 12, p);
    audio.render(BASE + 3, 11).unwrap();
    let mut expected = [0.0; 11];
    for (offset, value) in expected.iter_mut().enumerate().take(9) {
        *value = (2.0 / 3.0 + offset as f32 / 4.0) / 64.0;
        if offset + 3 >= 7 {
            *value += 0.25;
        }
    }
    close(&audio.output()[..11], &expected);
    assert_eq!(changed(&mut audio).active_elapsed_nanos, 68_027);
    match next(&mut audio).event {
        WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
            event: PracticeEvent::Boundary(e),
            ..
        }) => assert_eq!(
            (e.kind, e.physical_frame),
            (PracticeBoundaryKind::Ended, 12)
        ),
        other => panic!("expected actual retained end, got {other:?}"),
    }
    assert!(!audio.retained_metadata().unwrap().active);
    assert_eq!(audio.playback_end_frame(), None);
}

#[test]
fn rate_waits_through_pause_and_resume_evidence_precedes_the_same_cut_rate() {
    let mut audio = standard(rate(1, 2), region(0, 20_000_000, false));
    audio.arm(BASE, BASE).unwrap();
    audio.render(BASE, 2).unwrap();
    assert_eq!(&audio.output()[..2], &[0.0, 0.0625]);
    let start = next(&mut audio);
    assert_eq!(start.serial, 1);
    assert!(matches!(
        start.event,
        WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
            event: PracticeEvent::Boundary(PracticeReceipt {
                kind: PracticeBoundaryKind::Started,
                request_id: 0,
                physical_frame: 0,
                playback_frame: 0,
                ..
            }),
            ..
        })
    ));
    let m = audio.retained_metadata().unwrap();
    audio.request_pause(20, m.generation, true).unwrap();
    assert!(audio.poll_retained_event().unwrap().is_none());
    audio.render(BASE + 2, 0).unwrap();
    assert!(audio.poll_retained_event().unwrap().is_none());
    audio.render(BASE + 2, 3).unwrap();
    assert_eq!(&audio.output()[..3], &[0.0; 3]);
    let pause = next(&mut audio);
    assert_eq!(pause.serial, 2);
    match pause.event {
        WorkletRetainedEvent::Pause(e) => assert_eq!(
            (e.request_id, e.physical_frame, e.playback_frame, e.paused),
            (20, 2, 2, true)
        ),
        other => panic!("expected adopted pause, got {other:?}"),
    }
    request_rate(&mut audio, 21, standard_prep(rate(2, 1)));
    audio.render(BASE + 5, 2).unwrap();
    assert!(audio.poll_retained_event().unwrap().is_none());
    assert_eq!(audio.retained_metadata().unwrap().revision, 0);
    audio.request_pause(22, m.generation, false).unwrap();
    audio.render(BASE + 7, 2).unwrap();
    assert_eq!(&audio.output()[..2], &[0.125, 0.375]);
    let resume = audio.poll_retained_event().unwrap().unwrap();
    assert_eq!(
        (resume.program_id, resume.context_start, resume.serial),
        (PROGRAM, BASE, 3)
    );
    match resume.event {
        WorkletRetainedEvent::Pause(e) => assert_eq!(
            (e.request_id, e.physical_frame, e.playback_frame, e.paused),
            (22, 7, 2, false)
        ),
        other => panic!("resume must precede rate at the same actual cut, got {other:?}"),
    }
    assert_eq!(audio.poll_retained_event().unwrap(), Some(resume));
    let snapshot = |audio: &WorkletAudio| {
        let history = audio.source_playback_history().unwrap();
        (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame(),
            history.retained_start_frame(),
            history.pulled_end_frame(),
            history.retirement_revision(),
            history.capacity(),
            (0..history.len())
                .map(|i| {
                    let span = history.span(i).unwrap();
                    (
                        span.physical_start(),
                        span.physical_end(),
                        span.playback_start(),
                        span.state(),
                    )
                })
                .collect::<Vec<_>>(),
        )
    };
    let before = snapshot(&audio);
    assert_eq!(
        (before.0, before.1, before.2, before.3, before.4, before.5, before.6),
        (1000, 0, 0, 0, 9, 0, 64)
    );
    assert_eq!(
        before.7,
        vec![
            (0, Some(2), 0, SourcePlaybackState::Running),
            (2, Some(7), 2, SourcePlaybackState::Frozen),
            (7, None, 2, SourcePlaybackState::Running),
        ]
    );
    let metadata = audio.retained_metadata().unwrap();
    assert_eq!(
        (metadata.generation, metadata.revision, metadata.rate),
        (1, 1, rate(2, 1))
    );
    audio.acknowledge_retained_event(resume.serial).unwrap();
    assert_eq!(
        snapshot(&audio),
        before,
        "Resume ACK never retires source history"
    );
    assert_eq!(
        audio.retire_source_playback_before(8),
        Err(WorkletAudioError::InvalidRetentionPin),
        "the original unreported Rate still pins physical7 after Resume ACK"
    );
    assert_eq!(
        snapshot(&audio),
        before,
        "refusal preserves every original history descriptor and revision"
    );
    assert_eq!(audio.retained_metadata().unwrap(), metadata);
    assert!(!audio.failed());
    assert_eq!(&audio.output()[..2], &[0.125, 0.375]);
    let changed = audio.poll_retained_event().unwrap().unwrap();
    assert_eq!(
        (changed.program_id, changed.context_start, changed.serial),
        (PROGRAM, BASE, 4)
    );
    let e = match changed.event {
        WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
            event: PracticeEvent::Rate(event),
            ..
        }) => event,
        other => panic!("original same-cut Rate must follow Resume ACK, got {other:?}"),
    };
    assert_eq!(
        (
            e.request_id,
            e.generation,
            e.physical_frame,
            e.playback_frame,
            e.active_elapsed_nanos
        ),
        (21, 1, 7, 2, 2_000_000)
    );
    assert_eq!(
        (
            e.request_revision,
            e.applied_revision,
            e.previous_rate,
            e.requested_rate,
            e.applied_rate,
            e.outcome
        ),
        (
            0,
            1,
            rate(1, 2),
            rate(2, 1),
            rate(2, 1),
            PracticeRateOutcome::Applied
        )
    );
    assert_eq!(audio.poll_retained_event().unwrap(), Some(changed));
    assert_eq!(
        audio.retire_source_playback_before(9),
        Err(WorkletAudioError::InvalidRetentionPin),
        "delivery alone does not release the Rate latch pin"
    );
    assert_eq!(snapshot(&audio), before);
    audio.acknowledge_retained_event(changed.serial).unwrap();
    assert_eq!(
        snapshot(&audio),
        before,
        "Rate ACK preserves unread input history too"
    );
    assert!(audio.poll_retained_event().unwrap().is_none());
    let safe_pin = audio.source_playback_history().unwrap().pulled_end_frame();
    assert_eq!(
        safe_pin, 9,
        "use the genuine source frontier after the event-free poll"
    );
    assert_eq!(audio.retire_source_playback_before(safe_pin).unwrap(), 2);
    let after = snapshot(&audio);
    assert_eq!((after.3, after.4, after.5), (7, 9, 1));
    assert_eq!(after.7, vec![(7, None, 2, SourcePlaybackState::Running)]);
    assert_eq!(audio.retained_metadata().unwrap(), metadata);
    assert_eq!(&audio.output()[..2], &[0.125, 0.375]);
}

#[test]
fn startup_silence_and_empty_render_do_not_fabricate_pause_adoption() {
    let mut audio = standard(Rate::NORMAL, region(0, 20_000_000, false));
    let m = audio.retained_metadata().unwrap();
    audio.request_pause(23, m.generation, true).unwrap();
    audio.arm(BASE + 3, BASE).unwrap();
    audio.render(BASE, 2).unwrap();
    assert!(audio.poll_retained_event().unwrap().is_none());
    assert!(audio.report().is_none());
    audio.render(BASE + 2, 0).unwrap();
    assert!(audio.poll_retained_event().unwrap().is_none());
    audio.render(BASE + 2, 2).unwrap();
    let e = audio.poll_retained_event().unwrap().unwrap();
    assert!(matches!(
        e.event,
        WorkletRetainedEvent::Pause(WorkletPauseEvent {
            request_id: 23,
            physical_frame: 0,
            playback_frame: 0,
            paused: true,
            ..
        })
    ));
}

#[test]
fn latched_event_repeats_wrong_ack_and_new_controls_cannot_drop_it() {
    let mut audio = standard(Rate::NORMAL, region(0, 20_000_000, false));
    audio.arm(BASE, BASE).unwrap();
    audio.render(BASE, 2).unwrap();
    let latch = audio.poll_retained_event().unwrap().unwrap();
    assert_eq!(audio.poll_retained_event().unwrap(), Some(latch));
    assert!(audio.acknowledge_retained_event(latch.serial + 1).is_err());
    assert_eq!(audio.poll_retained_event().unwrap(), Some(latch));
    let m = audio.retained_metadata().unwrap();
    assert!(audio
        .try_set_rate_next(31, m.generation, m.revision, standard_prep(rate(2, 1)))
        .is_err());
    assert!(audio
        .try_region_next(
            32,
            m.generation,
            m.revision,
            region(1_000_000, 3_000_000, false)
        )
        .is_err());
    assert!(audio
        .try_restart_original_next(33, m.generation, m.revision, standard_prep(Rate::NORMAL))
        .is_err());
    assert_eq!(audio.poll_retained_event().unwrap(), Some(latch));
    audio.acknowledge_retained_event(latch.serial).unwrap();
    assert!(audio.acknowledge_retained_event(latch.serial).is_err());
    assert!(audio.poll_retained_event().unwrap().is_none());
    request_rate(&mut audio, 34, standard_prep(rate(2, 1)));
    audio.render(BASE + 2, 1).unwrap();
    let actual = audio.poll_retained_event().unwrap().unwrap();
    assert!(actual.serial > latch.serial);
    assert_eq!(audio.poll_retained_event().unwrap(), Some(actual));
}

#[test]
fn invalid_generation_revision_program_and_geometry_leave_actual_state_and_pcm_intact() {
    let mut audio = standard(Rate::NORMAL, region(0, 20_000_000, false));
    audio.arm(BASE, BASE).unwrap();
    audio.render(BASE, 1).unwrap();
    started(&mut audio);
    let m = audio.retained_metadata().unwrap();
    let p = standard_prep(rate(2, 1));
    assert!(audio
        .try_set_rate_next(40, m.generation + 1, m.revision, p)
        .is_err());
    assert!(audio
        .try_set_rate_next(40, m.generation, m.revision + 1, p)
        .is_err());
    let foreign = prepare_practice_rate(
        PROGRAM + 1,
        1000,
        &[PracticeRateCueGeometry {
            at: ts(0),
            source_frames: 8,
            source_hz: 1000,
        }],
        rate(2, 1),
    )
    .unwrap();
    assert!(audio
        .try_set_rate_next(40, m.generation, m.revision, foreign)
        .is_err());
    let wrong_geometry = prep(
        1000,
        &[PracticeRateCueGeometry {
            at: ts(1),
            source_frames: 8,
            source_hz: 1000,
        }],
        rate(2, 1),
    );
    assert!(audio
        .try_set_rate_next(40, m.generation, m.revision, wrong_geometry)
        .is_err());
    assert_eq!(audio.retained_metadata().unwrap(), m);
    assert!(audio.poll_retained_event().unwrap().is_none());
    audio.render(BASE + 1, 2).unwrap();
    assert_eq!(&audio.output()[..2], &[0.125, 0.25]);
    assert!(!audio.failed());
}

#[test]
fn pending_rate_region_and_restart_share_one_outstanding_control_slot() {
    let mut audio = standard(Rate::NORMAL, region(0, 20_000_000, false));
    audio.arm(BASE, BASE).unwrap();
    audio.render(BASE, 1).unwrap();
    started(&mut audio);
    let m = audio.retained_metadata().unwrap();
    request_rate(&mut audio, 41, standard_prep(rate(2, 1)));
    assert!(audio
        .try_set_rate_next(42, m.generation, m.revision, standard_prep(rate(1, 2)))
        .is_err());
    assert!(audio
        .try_region_next(43, m.generation, m.revision, region(0, 2_000_000, false))
        .is_err());
    assert!(audio
        .try_restart_original_next(44, m.generation, m.revision, standard_prep(Rate::NORMAL))
        .is_err());
    audio.render(BASE + 1, 1).unwrap();
    assert_eq!(changed(&mut audio).request_id, 41);
}

#[test]
fn capacity_refusal_is_typed_correlated_and_plays_the_old_schedule_without_drops() {
    let cues = vec![cue(1, 1, 0), cue(2, 1, 2_000_000), cue(3, 1, 4_000_000)];
    let mut setup = retained_config(Rate::NORMAL, region(0, 20_000_000, false));
    setup.limits = PracticeLimits::new(8, 1, 128, 8, 8).unwrap();
    let mut audio = build(1000, vec![(1, 1000, vec![0.125])], cues, setup);
    audio.arm(BASE, BASE).unwrap();
    audio.render(BASE, 1).unwrap();
    started(&mut audio);
    let geometry = [0, 2_000_000, 4_000_000].map(|at| PracticeRateCueGeometry {
        at: ts(at),
        source_frames: 1,
        source_hz: 1000,
    });
    request_rate(&mut audio, 45, prep(1000, &geometry, rate(4, 1)));
    audio.render(BASE + 1, 5).unwrap();
    assert_eq!(&audio.output()[..5], &[0.0, 0.125, 0.0, 0.125, 0.0]);
    let e = changed(&mut audio);
    assert_eq!(e.request_id, 45);
    assert_eq!(
        e.outcome,
        PracticeRateOutcome::Refused(PracticeRateRefusal::BgmCapacity)
    );
    assert_eq!((e.request_revision, e.applied_revision), (0, 0));
    assert_eq!(e.applied_rate, Rate::NORMAL);
    assert_eq!(audio.retained_metadata().unwrap().revision, 0);
    assert_eq!(audio.report().unwrap().counters.voice_full, 0);
}

#[test]
fn scrub_and_loop_use_current_rate_then_f5_restores_original_rate_region_without_preroll() {
    let original = region(0, 4_000_000, true);
    let mut setup = retained_config(rate(1, 2), original);
    setup.output_preroll = Duration::from_nanos(1_000_000);
    let mut audio = build(
        1000,
        vec![(1, 1000, (0..8).map(|i| i as f32 / 8.0).collect())],
        vec![cue(1, 1, 0)],
        setup,
    );
    audio.arm(BASE, BASE).unwrap();
    audio.render(BASE, 2).unwrap();
    assert_eq!(&audio.output()[..2], &[0.0, 0.0]);
    started(&mut audio);
    request_rate(&mut audio, 50, standard_prep(rate(2, 1)));
    audio.render(BASE + 2, 1).unwrap();
    changed(&mut audio);
    let m = audio.retained_metadata().unwrap();
    audio
        .try_region_next(
            51,
            m.generation,
            m.revision,
            region(2_000_000, 6_000_000, true),
        )
        .unwrap();
    audio.render(BASE + 3, 1).unwrap();
    assert_eq!(audio.output()[0], 0.25);
    assert!(matches!(
        next(&mut audio).event,
        WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
            event: PracticeEvent::Boundary(PracticeReceipt {
                kind: PracticeBoundaryKind::Requested,
                ..
            }),
            ..
        })
    ));
    audio.render(BASE + 4, 3).unwrap();
    assert_eq!(&audio.output()[..3], &[0.5, 0.25, 0.5]);
    assert!(matches!(
        next(&mut audio).event,
        WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
            event: PracticeEvent::Boundary(PracticeReceipt {
                kind: PracticeBoundaryKind::Looped,
                ..
            }),
            ..
        })
    ));
    let m = audio.retained_metadata().unwrap();
    assert_eq!(m.rate, rate(2, 1));
    audio
        .try_restart_original_next(52, m.generation, m.revision, standard_prep(rate(1, 2)))
        .unwrap();
    audio.render(BASE + 7, 3).unwrap();
    assert_eq!(&audio.output()[..3], &[0.0, 0.0625, 0.125]);
    match next(&mut audio).event {
        WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
            event: PracticeEvent::Restart(e),
            ..
        }) => {
            assert_eq!(
                (e.boundary.request_id, e.boundary.applied_song_time),
                (52, original.start)
            );
            assert_eq!(
                (e.previous_rate, e.applied_rate, e.outcome),
                (rate(2, 1), rate(1, 2), PracticeRateOutcome::Applied)
            );
            assert_eq!(e.boundary.generation, m.generation + 1);
        }
        other => panic!("expected one compound restart, got {other:?}"),
    }
}

#[test]
fn equal_rate_has_no_pcm_phase_change_but_advances_one_explicit_revision() {
    let mut audio = standard(rate(1, 2), region(0, 20_000_000, false));
    audio.arm(BASE, BASE).unwrap();
    audio.render(BASE, 3).unwrap();
    started(&mut audio);
    request_rate(&mut audio, 53, standard_prep(rate(2, 4)));
    audio.render(BASE + 3, 3).unwrap();
    assert_eq!(&audio.output()[..3], &[0.1875, 0.25, 0.3125]);
    let e = changed(&mut audio);
    assert_eq!(e.outcome, PracticeRateOutcome::Noop);
    assert_eq!((e.request_revision, e.applied_revision), (0, 1));
}

#[test]
fn retained_cue_conversion_preserves_original_order_identity_gain_and_rejects_nonplay() {
    let commands = [
        play(3, 1, 5),
        AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(2),
            at: ts(-7),
            gain: 0.25,
        },
        play(2, 1, 5),
    ];
    let cues = retained_bgm_cues(&commands).unwrap();
    assert_eq!(cues.len(), 3);
    assert_eq!(
        (cues[0].voice, cues[0].sample, cues[0].at, cues[0].gain),
        (VoiceId(1), SampleId(2), ts(-7), 0.25)
    );
    assert_eq!((cues[1].voice, cues[2].voice), (VoiceId(3), VoiceId(2)));
    for command in [
        AudioCommand::Stop {
            voice: VoiceId(1),
            at: ts(0),
        },
        AudioCommand::SetRate {
            at: ts(0),
            rate: Rate::NORMAL,
        },
    ] {
        assert!(retained_bgm_cues(&[command]).is_err());
    }
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(32, 64, 2).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(AudioFormat::new(2000, 1).unwrap(), vec![0.0; 7], limits).unwrap(),
    )
    .unwrap();
    bank.insert(
        SampleId(2),
        PcmSample::new(format, vec![0.0; 3], limits).unwrap(),
    )
    .unwrap();
    let geometry = retained_rate_geometry(&bank, &cues).unwrap();
    assert_eq!(
        geometry
            .iter()
            .map(|g| (g.at, g.source_frames, g.source_hz))
            .collect::<Vec<_>>(),
        vec![(ts(-7), 3, 1000), (ts(5), 7, 2000), (ts(5), 7, 2000)]
    );
    assert!(retained_rate_geometry(&bank, &[cue(1, 99, 0)]).is_err());
}

#[test]
fn actual_control_render_codec_poll_and_ack_are_calibrated_heap_free() {
    let (_, calibration) = tracked(|| {
        let mut v = Vec::<u8>::with_capacity(1);
        v.reserve_exact(2048);
        drop(v);
    });
    assert!(
        calibration.iter().all(|count| *count > 0),
        "allocator probe must observe alloc/realloc/dealloc: {calibration:?}"
    );
    let mut audio = standard(rate(1, 2), region(0, 20_000_000, false));
    audio.arm(BASE, BASE).unwrap();
    audio.render(BASE, 2).unwrap();
    started(&mut audio);
    let m = audio.retained_metadata().unwrap();
    let p = standard_prep(rate(2, 1));
    let (result, counts) = tracked(|| -> Result<(), WorkletAudioError> {
        audio.try_set_rate_next(60, m.generation, m.revision, p)?;
        audio.render(BASE + 2, 2)?;
        let e = audio.poll_retained_event()?.expect("actual rate event");
        let words = e.to_words();
        assert_eq!(WorkletRetainedEnvelope::from_words(&words)?, e);
        assert_eq!(audio.poll_retained_event()?, Some(e));
        audio.acknowledge_retained_event(e.serial)?;
        Ok(())
    });
    result.unwrap();
    assert_eq!(
        counts, [0; 3],
        "live bind/render/codec/poll/ack must stay heap-free"
    );
}

fn put(words: &mut [u32; 80], field: usize, value: u64) {
    words[field * 2] = value as u32;
    words[field * 2 + 1] = (value >> 32) as u32;
}
fn rate_words() -> [u32; 80] {
    let mut words = [0; 80];
    for (field, value) in [
        (0, 1),
        (1, 2),
        (2, PROGRAM),
        (3, 9),
        (4, u64::MAX - 1),
        (5, 17),
        (6, 3),
        (7, WIDE_SOURCE_FRAME),
        (8, 9_007_199_254_740_993),
        (9, 4),
        (10, 5),
        (11, 1),
        (12, 2),
        (13, 3),
        (14, 2),
        (15, 3),
        (16, 2),
        (20, 0x0123_4567_89ab_cdef),
        (21, 0x0123_4567_89ab_cdef),
        (26, WIDE_SOURCE_FRAME),
        (27, 7),
        (28, 123),
        (29, 1),
        (30, 3),
        (31, WIDE_SOURCE_FRAME + 1),
        (32, 44_100),
        (33, 71),
        (34, (-3i64) as u64),
        (35, BASE),
    ] {
        put(&mut words, field, value);
    }
    words
}

#[test]
fn event_codec_decodes_independent_literal_wide_rate_fields_without_number_rounding() {
    let words = rate_words();
    let envelope = WorkletRetainedEnvelope::from_words(&words).unwrap();
    assert_eq!(
        (envelope.program_id, envelope.context_start, envelope.serial),
        (PROGRAM, BASE, 9)
    );
    match envelope.event {
        WorkletRetainedEvent::Practice(p) => {
            assert_eq!(
                (p.target_frame, p.target_rate, p.origin),
                (
                    WIDE_SOURCE_FRAME + 1,
                    44_100,
                    ClockPoint {
                        domain: ClockDomainId(71),
                        timestamp: ts(-3)
                    }
                )
            );
            assert_eq!(p.boundary.target_time, TargetTime::new(123, 1, 3).unwrap());
            assert_eq!(
                (p.boundary.source_frame, p.boundary.target_frame_offset),
                (WIDE_SOURCE_FRAME, 7)
            );
            match p.event {
                PracticeEvent::Rate(e) => {
                    assert_eq!(
                        (e.request_id, e.generation, e.iteration),
                        (u64::MAX - 1, 17, 3)
                    );
                    assert_eq!(
                        (e.physical_frame, e.playback_frame),
                        (WIDE_SOURCE_FRAME, 9_007_199_254_740_993)
                    );
                    assert_eq!(
                        e.active_elapsed_nanos,
                        0x0123_4567_89ab_cdef_0123_4567_89ab_cdef_i128
                    );
                    assert_eq!(
                        (e.previous_rate, e.requested_rate, e.applied_rate),
                        (rate(1, 2), rate(3, 2), rate(3, 2))
                    );
                    assert_eq!(e.outcome, PracticeRateOutcome::Applied);
                }
                other => panic!("wrong event: {other:?}"),
            }
        }
        other => panic!("wrong event: {other:?}"),
    }
    assert_eq!(envelope.to_words(), words);
}

#[test]
fn event_codec_rejects_absolute_context_overflow_even_with_valid_wide_relative_fields() {
    // A relative frame and an absolute context origin may each fit u64 while
    // their actual context-grid sum does not. Keep that invalid former literal
    // as an explicit refusal oracle, separate from precision-preserving decode.
    let mut words = rate_words();
    put(&mut words, 7, u64::MAX - 10);
    put(&mut words, 26, u64::MAX - 10);
    put(&mut words, 31, u64::MAX - 9);
    assert!(WorkletRetainedEnvelope::from_words(&words).is_err());
    let mut words = rate_words();
    put(&mut words, 35, u64::MAX);
    assert!(WorkletRetainedEnvelope::from_words(&words).is_err());
}

#[test]
fn event_codec_rejects_extent_versions_tags_noncanonical_rates_fractions_and_unused_words() {
    let valid = rate_words();
    for len in 0..80 {
        assert!(
            WorkletRetainedEnvelope::from_words(&valid[..len]).is_err(),
            "length {len}"
        );
    }
    let mut extra = valid.to_vec();
    extra.push(0);
    assert!(WorkletRetainedEnvelope::from_words(&extra).is_err());
    for (field, value) in [
        (0, 0),
        (0, 2),
        (1, 0),
        (1, 5),
        (2, 0),
        (3, 0),
        (5, 0),
        (7, 0),
        (11, 0),
        (11, u64::MAX),
        (12, 0),
        (13, 6),
        (14, 4),
        (17, 3),
        (18, 1),
        (19, 1),
        (22, 1),
        (23, 1),
        (24, 1),
        (25, 1),
        (29, 3),
        (30, 1),
        (30, 0),
        (32, 0),
        (32, u64::from(u32::MAX) + 1),
        (36, 1),
        (37, 1),
        (38, 1),
        (39, 1),
    ] {
        let mut bad = valid;
        put(&mut bad, field, value);
        assert!(
            WorkletRetainedEnvelope::from_words(&bad).is_err(),
            "field {field}={value}"
        );
    }
    let mut unreduced = valid;
    put(&mut unreduced, 29, 2);
    put(&mut unreduced, 30, 6);
    assert!(WorkletRetainedEnvelope::from_words(&unreduced).is_err());
}

#[test]
fn event_codec_pause_literal_requires_boolean_and_zeroes_every_practice_only_field() {
    let mut words = [0; 80];
    for (field, value) in [
        (0, 1),
        (1, 4),
        (2, PROGRAM),
        (3, 7),
        (4, 66),
        (5, 8),
        (7, 9),
        (8, 4),
        (35, BASE),
        (36, 1),
    ] {
        put(&mut words, field, value);
    }
    let expected = WorkletRetainedEnvelope {
        program_id: PROGRAM,
        context_start: BASE,
        serial: 7,
        event: WorkletRetainedEvent::Pause(WorkletPauseEvent {
            request_id: 66,
            generation: 8,
            physical_frame: 9,
            playback_frame: 4,
            paused: true,
        }),
    };
    assert_eq!(
        WorkletRetainedEnvelope::from_words(&words).unwrap(),
        expected
    );
    assert_eq!(expected.to_words(), words);
    for field in [
        6, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
        31, 32, 33, 34, 37, 38, 39,
    ] {
        let mut bad = words;
        put(&mut bad, field, 1);
        assert!(
            WorkletRetainedEnvelope::from_words(&bad).is_err(),
            "unused pause field {field}"
        );
    }
    let mut bad = words;
    put(&mut bad, 36, 2);
    assert!(WorkletRetainedEnvelope::from_words(&bad).is_err());
    let mut bad = words;
    put(&mut bad, 8, 10);
    assert!(WorkletRetainedEnvelope::from_words(&bad).is_err());
}

#[test]
fn event_codec_refusal_phase_errors_round_trip_with_unchanged_semantic_revision() {
    for (code, error) in [
        (1, PositiveRatePhaseError::NegativePreroll),
        (2, PositiveRatePhaseError::NonPositiveRate),
        (3, PositiveRatePhaseError::InvalidEnd),
        (4, PositiveRatePhaseError::NegativeElapsed),
        (5, PositiveRatePhaseError::RegressingElapsed),
        (6, PositiveRatePhaseError::Overflow),
        (7, PositiveRatePhaseError::DenominatorCapacity),
    ] {
        let mut words = rate_words();
        put(&mut words, 10, 4);
        put(&mut words, 15, 1);
        put(&mut words, 16, 2);
        put(&mut words, 17, 2);
        put(&mut words, 18, 4);
        put(&mut words, 19, code);
        let event = WorkletRetainedEnvelope::from_words(&words).unwrap();
        assert!(
            matches!(event.event,WorkletRetainedEvent::Practice(ProjectedPracticeEvent{event:PracticeEvent::Rate(PracticeRateEvent{outcome:PracticeRateOutcome::Refused(PracticeRateRefusal::Phase(e)),..}),..}) if e==error)
        );
        assert_eq!(event.to_words(), words);
    }
    for (refusal, phase) in [(0, 0), (4, 0), (4, 8), (5, 1), (9, 0)] {
        let mut bad = rate_words();
        put(&mut bad, 10, 4);
        put(&mut bad, 15, 1);
        put(&mut bad, 16, 2);
        put(&mut bad, 17, 2);
        put(&mut bad, 18, refusal);
        put(&mut bad, 19, phase);
        assert!(WorkletRetainedEnvelope::from_words(&bad).is_err());
    }
}

#[test]
fn unread_event_pins_history_until_ack_and_history_still_requires_explicit_retirement() {
    let mut audio = standard(Rate::NORMAL, region(0, 20_000_000, false));
    audio.arm(BASE, BASE).unwrap();
    audio.render(BASE, 2).unwrap();
    started(&mut audio);
    let m = audio.retained_metadata().unwrap();
    audio.request_pause(70, m.generation, true).unwrap();
    audio.render(BASE + 2, 2).unwrap();
    let event = audio.poll_retained_event().unwrap().unwrap();
    assert!(matches!(
        event.event,
        WorkletRetainedEvent::Pause(WorkletPauseEvent {
            physical_frame: 2,
            ..
        })
    ));
    let before = audio
        .source_playback_history()
        .unwrap()
        .retained_start_frame();
    assert!(
        audio.retire_source_playback_before(3).is_err(),
        "outbound pause evidence pins frame2"
    );
    assert_eq!(
        audio
            .source_playback_history()
            .unwrap()
            .retained_start_frame(),
        before
    );
    assert_eq!(audio.poll_retained_event().unwrap(), Some(event));
    audio.acknowledge_retained_event(event.serial).unwrap();
    assert_eq!(
        audio
            .source_playback_history()
            .unwrap()
            .retained_start_frame(),
        before,
        "ack does not retire unread input history"
    );
    audio.retire_source_playback_before(2).unwrap();
    assert_eq!(
        audio
            .source_playback_history()
            .unwrap()
            .retained_start_frame(),
        2
    );
    assert!(
        audio.retire_source_playback_before(5).is_err(),
        "retirement cannot invent a source frontier beyond actual rendering"
    );
}

#[test]
fn callback_partitioning_preserves_pcm_cut_identity_and_exact_end() {
    fn run(parts: &[usize]) -> (Vec<f32>, Vec<WorkletRetainedEvent>, PracticeMetadata) {
        let mut audio = standard(rate(1, 2), region(0, 4_000_000, false));
        audio.arm(BASE, BASE).unwrap();
        audio.render(BASE, 3).unwrap();
        let mut pcm = audio.output()[..3].to_vec();
        started(&mut audio);
        request_rate(&mut audio, 71, standard_prep(rate(2, 1)));
        let mut current = BASE + 3;
        let mut events = Vec::new();
        for &frames in parts {
            audio.render(current, frames).unwrap();
            current += frames as u64;
            pcm.extend_from_slice(&audio.output()[..frames]);
            while let Some(e) = audio.poll_retained_event().unwrap() {
                events.push(e.event);
                audio.acknowledge_retained_event(e.serial).unwrap();
            }
        }
        (pcm, events, audio.retained_metadata().unwrap())
    }
    let full = run(&[6]);
    assert_eq!(
        full.0,
        [0.0, 0.0625, 0.125, 0.1875, 0.4375, 0.0, 0.0, 0.0, 0.0]
    );
    for parts in [vec![2, 4], vec![1; 6], vec![0, 1, 2, 0, 3]] {
        let split = run(&parts);
        assert_eq!(split.0, full.0);
        assert_eq!(split.2, full.2);
        let core_events = |events: &[WorkletRetainedEvent]| {
            events
                .iter()
                .map(|e| match e {
                    WorkletRetainedEvent::Practice(p) => p.event,
                    other => panic!("unexpected {other:?}"),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            core_events(&split.1),
            core_events(&full.1),
            "projection offsets may differ, actual source cuts cannot"
        );
    }
}

#[test]
fn event_codec_boundary_and_compound_restart_have_independent_signed_literals() {
    let mut boundary = [0; 80];
    for (field, value) in [
        (0, 1),
        (1, 1),
        (2, PROGRAM),
        (3, 12),
        (4, 88),
        (5, 3),
        (6, 4),
        (7, 100),
        (8, 70),
        (22, (-9i64) as u64),
        (23, (-7i64) as u64),
        (24, 2),
        (25, 1),
        (26, 100),
        (28, 1),
        (29, 1),
        (30, 2),
        (31, 100),
        (32, 1000),
        (33, 91),
        (34, (-5i64) as u64),
        (35, BASE),
    ] {
        put(&mut boundary, field, value);
    }
    let decoded = WorkletRetainedEnvelope::from_words(&boundary).unwrap();
    assert!(
        matches!(decoded.event,WorkletRetainedEvent::Practice(ProjectedPracticeEvent{event:PracticeEvent::Boundary(PracticeReceipt{request_id:88,generation:3,iteration:4,physical_frame:100,playback_frame:70,requested_song_time,applied_song_time,correction_nanos:2,kind:PracticeBoundaryKind::Requested}),..}) if requested_song_time==ts(-9)&&applied_song_time==ts(-7))
    );
    assert_eq!(decoded.to_words(), boundary);
    let mut restart = boundary;
    put(&mut restart, 1, 3);
    for (field, value) in [
        (9, 2),
        (10, 3),
        (11, 2),
        (12, 1),
        (13, 1),
        (14, 2),
        (15, 1),
        (16, 2),
    ] {
        put(&mut restart, field, value);
    }
    let decoded = WorkletRetainedEnvelope::from_words(&restart).unwrap();
    assert!(matches!(
        decoded.event,
        WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
            event: PracticeEvent::Restart(PracticeRestartEvent {
                request_revision: 2,
                applied_revision: 3,
                outcome: PracticeRateOutcome::Applied,
                ..
            }),
            ..
        })
    ));
    assert_eq!(decoded.to_words(), restart);
    for field in [
        9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 36, 37, 38, 39,
    ] {
        let mut bad = boundary;
        put(&mut bad, field, 1);
        assert!(
            WorkletRetainedEnvelope::from_words(&bad).is_err(),
            "unused boundary field {field}"
        );
    }
    let mut invalid = boundary;
    put(&mut invalid, 25, 6);
    assert!(WorkletRetainedEnvelope::from_words(&invalid).is_err());
}

#[test]
fn cold_retained_setup_rejects_invalid_identity_rate_preroll_history_and_unknown_assets() {
    for (identity, selected, preroll, history, sample) in [
        (0, Rate::NORMAL, 0, 16, 1),
        (PROGRAM, Rate::new(0, 1).unwrap(), 0, 16, 1),
        (PROGRAM, Rate::new(-1, 1).unwrap(), 0, 16, 1),
        (PROGRAM, Rate::NORMAL, -1, 16, 1),
        (PROGRAM, Rate::NORMAL, 0, 0, 1),
        (PROGRAM, Rate::NORMAL, 0, 16, 99),
    ] {
        let mut builder = WorkletAudioBuilder::new(config(1000, 16)).unwrap();
        builder
            .insert_sample(
                SampleId(1),
                AudioFormat::new(1000, 1).unwrap(),
                vec![0.125; 8],
            )
            .unwrap();
        let mut setup = retained_config(selected, region(0, 8_000_000, false));
        setup.program_id = identity;
        setup.output_preroll = Duration::from_nanos(preroll);
        setup.source_history_capacity = history;
        assert!(
            builder
                .finish_retained(vec![cue(1, sample, 0)], setup)
                .is_err(),
            "invalid tuple {identity}/{selected:?}/{preroll}/{history}/{sample}"
        );
    }
}

#[test]
fn disabling_loop_produces_actual_boundary_and_preserves_remaining_finite_pcm() {
    let mut audio = standard(Rate::NORMAL, region(0, 4_000_000, true));
    audio.arm(BASE, BASE).unwrap();
    audio.render(BASE, 1).unwrap();
    started(&mut audio);
    let m = audio.retained_metadata().unwrap();
    audio
        .try_disable_loop_next(81, m.generation, m.revision)
        .unwrap();
    assert!(audio.poll_retained_event().unwrap().is_none());
    audio.render(BASE + 1, 1).unwrap();
    assert_eq!(audio.output()[0], 0.125);
    assert!(matches!(
        next(&mut audio).event,
        WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
            event: PracticeEvent::Boundary(PracticeReceipt {
                request_id: 81,
                kind: PracticeBoundaryKind::LoopDisabled,
                physical_frame: 1,
                ..
            }),
            ..
        })
    ));
    audio.render(BASE + 2, 4).unwrap();
    assert_eq!(&audio.output()[..4], &[0.25, 0.375, 0.0, 0.0]);
    assert!(matches!(
        next(&mut audio).event,
        WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
            event: PracticeEvent::Boundary(PracticeReceipt {
                kind: PracticeBoundaryKind::Ended,
                physical_frame: 4,
                ..
            }),
            ..
        })
    ));
    assert!(!audio.retained_metadata().unwrap().active);
    assert!(audio.poll_retained_event().unwrap().is_none());
}

#[test]
fn change_queued_at_exact_end_is_refused_then_original_restart_revives_one_attempt() {
    let mut audio = standard(Rate::NORMAL, region(0, 2_000_000, false));
    audio.arm(BASE, BASE).unwrap();
    audio.render(BASE, 2).unwrap();
    started(&mut audio);
    request_rate(&mut audio, 82, standard_prep(rate(2, 1)));
    audio.render(BASE + 2, 2).unwrap();
    assert_eq!(&audio.output()[..2], &[0.0; 2]);
    assert_eq!(
        changed(&mut audio).outcome,
        PracticeRateOutcome::Refused(PracticeRateRefusal::Ended)
    );
    assert!(matches!(
        next(&mut audio).event,
        WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
            event: PracticeEvent::Boundary(PracticeReceipt {
                kind: PracticeBoundaryKind::Ended,
                physical_frame: 2,
                ..
            }),
            ..
        })
    ));
    let ended = audio.retained_metadata().unwrap();
    assert!(!ended.active);
    audio
        .try_restart_original_next(
            83,
            ended.generation,
            ended.revision,
            standard_prep(Rate::NORMAL),
        )
        .unwrap();
    audio.render(BASE + 4, 1).unwrap();
    assert_eq!(audio.output()[0], 0.0);
    assert!(matches!(
        next(&mut audio).event,
        WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
            event: PracticeEvent::Restart(PracticeRestartEvent {
                outcome: PracticeRateOutcome::Applied,
                ..
            }),
            ..
        })
    ));
    let restarted = audio.retained_metadata().unwrap();
    assert!(restarted.active);
    assert_eq!(restarted.generation, ended.generation + 1);
}
