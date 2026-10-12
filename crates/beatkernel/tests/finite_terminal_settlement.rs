//! Terminal settlement uses the real consumer; expiry is never execution evidence.
use beatkernel::{
    audio::*,
    chart::*,
    input::*,
    interaction::PressInstantEvaluator,
    judge::*,
    runtime::{Runtime, RuntimeProcessingClock, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
struct CountingAllocator;
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNTING.with(|on| {
            if on.get() {
                ALLOCATIONS.with(|n| n.set(n.get() + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        COUNTING.with(|on| {
            if on.get() {
                ALLOCATIONS.with(|n| n.set(n.get() + 1));
            }
        });
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(domain: u32, n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(n),
    }
}
fn play(at: i64) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(1),
        voice: VoiceId(1),
        gain: 0.25,
        at: ts(at),
    }
}
fn bank(format: AudioFormat) -> SampleBank {
    let limits = PcmLimits::new(4096, 4096, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![1.0; 64], limits).unwrap(),
    )
    .unwrap();
    bank
}
fn mixer(
    consumer: CommandConsumer,
    format: AudioFormat,
    origin: i64,
    end: Option<u64>,
    budget: usize,
) -> Mixer {
    let config = MixerConfig::new(
        format,
        ClockDomainId(2),
        ts(origin),
        AudioLimits::new(32, 8, 32, 64, budget).unwrap(),
    );
    Mixer::new(
        end.map_or(config, |end| config.with_playback_end_frame(end)),
        bank(format),
        consumer,
    )
    .unwrap()
}
fn rig(end: Option<u64>, budget: usize) -> (CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(32).unwrap();
    (
        producer,
        mixer(consumer, AudioFormat::new(1000, 1).unwrap(), 0, end, budget),
    )
}
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn runtime() -> (Runtime, CommandConsumer) {
    let mut chart = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    chart.objects.push(SourceObject {
        id: ObjectId(1),
        start: Beat::new(6_000_000).unwrap(),
        end: None,
        interaction: InteractionId(1),
        visual: VisualId(0),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    let judge = JudgeEngine::new(
        chart.compile().unwrap(),
        vec![Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: Box::new(PressInstantEvaluator),
        }],
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(4),
        game_control: GameControlId(1),
    }])
    .unwrap();
    let (producer, consumer) = command_queue(32).unwrap();
    let transport = Transport::new_with_preroll(
        ts(0),
        ts(0),
        Duration::ZERO,
        Rate::new(2, 1).unwrap(),
        Some(ts(21_000_000)),
    )
    .unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        transport,
        bindings,
        judge,
        producer,
        vec![SoundBinding {
            object: ObjectId(1),
            stage: JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(1),
            gain: 0.25,
        }],
        8,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    runtime.set_song_end(ts(21_000_000)).unwrap();
    (runtime, consumer)
}

#[test]
fn actual_last_callback_then_original_runtime_publication_is_expired_not_applied() {
    let (mut runtime, consumer) = runtime();
    let mut output = mixer(consumer, AudioFormat::new(1000, 1).unwrap(), 0, Some(11), 8);
    let previous = output.render(&mut [9.0; 10]).unwrap();
    assert_eq!(previous.playback_frames, 10);
    assert_eq!(previous.playback_end_physical_frame, None);
    let final_report = output.render(&mut [9.0; 5]).unwrap();
    assert_eq!(final_report.playback_end_physical_frame, Some(11));
    let mut meta = EventMeta::new(DeviceId(7), point(1, 3_000_000), 1);
    meta.original_clock_point = Some(point(9, 123_456));
    let event = PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    });
    let judged = runtime
        .process_input(event.clone(), &Identity, point(2, 10_000_000))
        .unwrap();
    assert_eq!(judged.song_time, ts(6_000_000));
    assert_eq!(judged.audio_at, point(2, 10_000_000));
    assert_eq!(judged.judge_events.len(), 1);
    assert_eq!(judged.bound_inputs[0].physical.meta(), event.meta());
    assert_eq!(judged.audio_commands, [play(10_000_000)]);
    assert!(judged.audio_failures.is_empty());
    let hash = runtime.judge().stable_hash().unwrap();
    runtime.seal_gameplay_audio();
    assert_eq!(runtime.telemetry().counters().audio_commands, 1);
    assert_eq!(final_report.counters.commands_consumed, 0);
    let mut silent = [9.0; 5];
    let settled = output.render(&mut silent).unwrap();
    assert_eq!(silent, [0.0; 5]);
    assert_eq!(settled.playback_frames, 0);
    assert_eq!(settled.playback_end_physical_frame, Some(11));
    assert_eq!(settled.counters.commands_consumed, 1);
    assert_eq!(settled.counters.commands_expired, 1);
    assert_eq!(settled.counters.commands_applied, 0);
    assert_eq!(settled.counters.settled_commands(), Some(1));
    assert_eq!(settled.counters.late_commands, 0);
    assert_eq!(settled.pending_commands, 0);
    assert_eq!(runtime.judge().stable_hash().unwrap(), hash);
    assert_eq!(
        output
            .render(&mut silent)
            .unwrap()
            .counters
            .commands_expired,
        1
    );
}

#[test]
fn existing_pending_and_new_queue_share_one_budget() {
    let (mut producer, mut output) = rig(Some(1), 2);
    producer.try_push(play(1_000_000)).unwrap();
    producer.try_push(play(2_000_000)).unwrap();
    let end = output.render(&mut [9.0]).unwrap();
    assert_eq!(end.pending_commands, 2);
    assert_eq!(end.counters.commands_consumed, 2);
    for at in [3_000_000, 4_000_000, 5_000_000] {
        producer.try_push(play(at)).unwrap();
    }
    let first = output.render(&mut [9.0]).unwrap();
    assert_eq!(first.counters.commands_expired, 2);
    assert_eq!(first.counters.commands_consumed, 2);
    assert_eq!(first.pending_commands, 0);
    let second = output.render(&mut [9.0]).unwrap();
    assert_eq!(second.counters.commands_expired, 4);
    assert_eq!(second.counters.commands_consumed, 4);
    // This publication necessarily occurs after the preceding snapshot.
    producer.try_push(play(6_000_000)).unwrap();
    let third = output.render(&mut [9.0]).unwrap();
    assert_eq!(third.counters.commands_expired, 6);
    assert_eq!(third.counters.commands_consumed, 6);
    assert_eq!(third.counters.commands_applied, 0);
    assert_eq!(third.playback_end_physical_frame, Some(1));
}

#[test]
fn valid_terminal_controls_do_not_modify_frozen_state() {
    let (mut producer, mut output) = rig(Some(1), 8);
    producer.try_push(play(0)).unwrap();
    let end = output.render(&mut [9.0]).unwrap();
    for command in [
        AudioCommand::Stop {
            voice: VoiceId(99),
            at: ts(0),
        },
        AudioCommand::Seek {
            song_time: ts(999),
            at: ts(1_000_000),
        },
        AudioCommand::SetRate {
            rate: Rate::REVERSE,
            at: ts(2_000_000),
        },
        play(3_000_000),
    ] {
        producer.try_push(command).unwrap();
    }
    let mut block = [9.0; 4];
    let settled = output.render(&mut block).unwrap();
    assert_eq!(block, [0.0; 4]);
    assert_eq!(settled.counters.commands_expired, 4);
    assert_eq!(settled.counters.commands_consumed, 5);
    assert_eq!(settled.counters.commands_applied, 1);
    assert_eq!(settled.counters.unknown_stops, 0);
    assert_eq!(settled.counters.late_commands, end.counters.late_commands);
    assert_eq!(settled.active_voices, end.active_voices);
    assert_eq!(settled.song_position, end.song_position);
    assert_eq!(output.rate(), Rate::NORMAL);
    assert_eq!(output.playback_frame_cursor(), 1);
}

#[test]
fn empty_zero_end_requires_real_nonempty_callback() {
    let (mut producer, mut output) = rig(Some(0), 8);
    producer.try_push(play(0)).unwrap();
    let empty = output.render(&mut []).unwrap();
    assert_eq!(empty.playback_end_physical_frame, None);
    assert_eq!(empty.counters.commands_consumed, 0);
    let actual = output.render(&mut [9.0]).unwrap();
    assert_eq!(actual.playback_end_physical_frame, Some(0));
    assert_eq!(actual.counters.commands_consumed, 1);
    assert_eq!(actual.counters.commands_expired, 1);
    assert_eq!(actual.counters.commands_applied, 0);
    producer.try_push(play(0)).unwrap();
    assert_eq!(output.render(&mut []).unwrap().counters, actual.counters);
    assert_eq!(
        output.render(&mut [9.0]).unwrap().counters.commands_expired,
        2
    );
}

#[test]
fn manual_pause_and_unlimited_playback_never_expire() {
    for end in [None, Some(11)] {
        let (mut producer, mut output) = rig(end, 8);
        producer.try_push(play(0)).unwrap();
        producer.request_pause(true);
        let paused = output.render(&mut [9.0; 12]).unwrap();
        assert_eq!(paused.counters.commands_consumed, 0);
        assert_eq!(paused.counters.commands_expired, 0);
        assert_eq!(paused.playback_end_physical_frame, None);
        producer.request_pause(false);
        let running = output.render(&mut [9.0]).unwrap();
        assert_eq!(running.counters.commands_applied, 1);
        assert_eq!(running.counters.commands_expired, 0);
    }
}

#[test]
fn unarmed_and_before_target_start_gate_never_settle() {
    let (mut producer, consumer) = command_queue_with_start_gate(32).unwrap();
    let mut output = mixer(consumer, AudioFormat::new(1000, 1).unwrap(), 0, Some(0), 8);
    producer.try_push(play(0)).unwrap();
    let unarmed = output.render(&mut [9.0; 2]).unwrap();
    assert_eq!(unarmed.counters.commands_consumed, 0);
    assert_eq!(unarmed.counters.commands_expired, 0);
    assert_eq!(unarmed.playback_end_physical_frame, None);
    producer.schedule_start_at(5).unwrap();
    let before = output.render(&mut [9.0; 2]).unwrap();
    assert_eq!(before.counters.commands_consumed, 0);
    assert_eq!(before.playback_end_physical_frame, None);
    let actual = output.render(&mut [9.0; 2]).unwrap();
    assert_eq!(actual.playback_end_physical_frame, Some(5));
    assert_eq!(actual.counters.commands_expired, 1);
}

#[test]
fn invalid_gain_sample_and_unrepresentable_time_remain_rejections() {
    let (mut producer, mut output) = rig(Some(0), 8);
    producer
        .try_push(AudioCommand::Play {
            sample: SampleId(1),
            voice: VoiceId(1),
            gain: f32::NAN,
            at: ts(0),
        })
        .unwrap();
    producer
        .try_push(AudioCommand::Play {
            sample: SampleId(999),
            voice: VoiceId(1),
            gain: 0.25,
            at: ts(0),
        })
        .unwrap();
    let rejected = output.render(&mut [9.0]).unwrap();
    assert_eq!(rejected.counters.commands_consumed, 2);
    assert_eq!(rejected.counters.commands_expired, 0);
    assert_eq!(rejected.counters.invalid_gains, 1);
    assert_eq!(rejected.counters.unknown_samples, 1);
    let (mut producer, consumer) = command_queue(32).unwrap();
    let mut output = mixer(
        consumer,
        AudioFormat::new(u32::MAX, 1).unwrap(),
        i64::MIN,
        Some(0),
        8,
    );
    producer.try_push(play(i64::MAX)).unwrap();
    let rejected = output.render(&mut [9.0]).unwrap();
    assert_eq!(rejected.counters.invalid_times, 1);
    assert_eq!(rejected.counters.commands_consumed, 1);
    assert_eq!(rejected.counters.commands_expired, 0);
}

#[test]
fn invalid_terminal_rate_is_checked_against_real_frozen_voice() {
    let (mut producer, mut output) = rig(Some(1), 8);
    let retained = Rate::new(1, u64::MAX).unwrap();
    producer
        .try_push(AudioCommand::SetRate {
            rate: retained,
            at: ts(0),
        })
        .unwrap();
    producer.try_push(play(0)).unwrap();
    let end = output.render(&mut [9.0]).unwrap();
    assert_eq!(end.active_voices, 1);
    producer
        .try_push(AudioCommand::SetRate {
            rate: Rate::new(1, u64::MAX - 2).unwrap(),
            at: ts(1_000_000),
        })
        .unwrap();
    let rejected = output.render(&mut [9.0]).unwrap();
    assert_eq!(rejected.counters.invalid_rates, 1);
    assert_eq!(rejected.counters.commands_expired, 0);
    assert_eq!(rejected.counters.commands_applied, 2);
    assert_eq!(output.rate(), retained);
}

#[test]
fn settled_counter_refuses_saturation_and_overflow() {
    assert_eq!(AudioCounters::default().settled_commands(), Some(0));
    assert_eq!(
        AudioCounters {
            commands_consumed: 5,
            commands_applied: 2,
            commands_expired: 3,
            ..Default::default()
        }
        .settled_commands(),
        Some(5)
    );
    for counters in [
        AudioCounters {
            commands_consumed: u64::MAX,
            ..Default::default()
        },
        AudioCounters {
            commands_applied: u64::MAX,
            ..Default::default()
        },
        AudioCounters {
            commands_expired: u64::MAX,
            ..Default::default()
        },
        AudioCounters {
            commands_applied: u64::MAX - 1,
            commands_expired: 1,
            ..Default::default()
        },
        AudioCounters {
            commands_applied: u64::MAX - 1,
            commands_expired: 2,
            ..Default::default()
        },
    ] {
        assert_eq!(counters.settled_commands(), None);
    }
}

#[test]
fn terminal_settlement_allocates_nothing_on_the_render_thread() {
    let (mut producer, mut output) = rig(Some(0), 2);
    for _ in 0..8 {
        producer.try_push(play(0)).unwrap();
    }
    ALLOCATIONS.with(|n| n.set(0));
    COUNTING.with(|on| on.set(true));
    let reports = std::array::from_fn::<_, 4, _>(|_| output.render(&mut [9.0; 4]).unwrap());
    COUNTING.with(|on| on.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    assert_eq!(
        reports.map(|report| report.counters.commands_expired),
        [2, 4, 6, 8]
    );
    assert!(reports
        .iter()
        .all(|report| report.counters.commands_applied == 0));
}
