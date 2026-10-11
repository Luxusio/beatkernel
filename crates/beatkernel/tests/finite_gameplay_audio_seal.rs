//! Real Runtime/queue/Mixer evidence. Calling the seal is not output completion proof.
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, CommandConsumer, Mixer, MixerConfig,
        PcmLimits, PcmSample, SampleBank, SampleId, VoiceId,
    },
    chart::*,
    input::*,
    interaction::PressInstantEvaluator,
    judge::*,
    runtime::{
        hazard_sound::{HazardSoundBinding, HazardSoundTimeline},
        input_sound::{InputSoundMarker, InputSoundTimeline},
        Runtime, RuntimeProcessingClock, SoundBinding,
    },
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
        COUNTING.with(|enabled| {
            if enabled.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        COUNTING.with(|enabled| {
            if enabled.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
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
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn input(at: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(u64::MAX), point(1, at), sequence);
    meta.original_clock_point = Some(point(9, 100 + at));
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(4),
        state,
    })
}
fn judge() -> JudgeEngine {
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
    let mut judge = JudgeEngine::new(
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
    judge
        .configure_hazards(
            HazardTimeline::new(
                vec![HazardMarker {
                    id: HazardId(1),
                    at: ts(12_000_000),
                    control: GameControlId(1),
                    value: 1,
                }],
                1,
            )
            .unwrap(),
        )
        .unwrap();
    judge
}
fn transport(rate: Rate) -> Transport {
    Transport::new_with_preroll(ts(0), ts(0), Duration::ZERO, rate, Some(ts(21_000_000))).unwrap()
}
fn runtime(rate: Rate) -> (Runtime, CommandConsumer) {
    let (producer, consumer) = command_queue(32).unwrap();
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(4),
        game_control: GameControlId(1),
    }])
    .unwrap();
    let mut owner = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        transport(rate),
        bindings,
        judge(),
        producer,
        vec![SoundBinding {
            object: ObjectId(1),
            stage: JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(1),
            gain: 1.0,
        }],
        8,
    )
    .unwrap();
    owner.set_processing_clock(RuntimeProcessingClock::Disabled);
    owner.set_song_end(ts(21_000_000)).unwrap();
    owner
        .configure_input_sounds(
            InputSoundTimeline::new(
                vec![InputSoundMarker {
                    control: GameControlId(1),
                    at: ts(0),
                    sample: SampleId(1),
                    voice: VoiceId(2),
                    gain: 0.5,
                }],
                1,
            )
            .unwrap(),
        )
        .unwrap();
    owner
        .configure_hazard_sounds(
            HazardSoundTimeline::new(
                vec![HazardSoundBinding {
                    hazard: HazardId(1),
                    sample: SampleId(1),
                    voice: VoiceId(3),
                    gain: 0.25,
                }],
                1,
            )
            .unwrap(),
        )
        .unwrap();
    (owner, consumer)
}
fn play(voice: u64, at: i64) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(1),
        voice: VoiceId(voice),
        at: ts(at),
        gain: 0.125,
    }
}
fn mixer(consumer: CommandConsumer, end: u64) -> Mixer {
    let format = AudioFormat::new(1000, 1).unwrap();
    let pcm = PcmLimits::new(256, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![1.0; 64], pcm).unwrap(),
    )
    .unwrap();
    Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            ts(0),
            AudioLimits::new(32, 16, 32, 50, 32).unwrap(),
        )
        .with_playback_end_frame(end),
        bank,
        consumer,
    )
    .unwrap()
}

#[test]
fn genuine_source_end_then_delayed_original_input_preserves_judge_hazard_and_raw_time() {
    for (n, d, note_output, mine_output, release_song, end_frame) in [
        (1, 2, 12_000_000, 24_000_000, 12_000_001, 42),
        (1, 1, 6_000_000, 12_000_000, 12_000_002, 21),
        (2, 1, 3_000_000, 6_000_000, 12_000_004, 11),
        (3, 2, 4_000_000, 8_000_000, 12_000_003, 14),
    ] {
        let rate = Rate::new(n, d).unwrap();
        let (mut sealed, consumer) = runtime(rate);
        let (mut baseline, mut baseline_consumer) = runtime(rate);
        sealed.enqueue_audio(play(99, 0)).unwrap();
        let mut output = mixer(consumer, end_frame);
        let mut pcm = vec![9.0; 50];
        let actual = output.render(&mut pcm).unwrap();
        assert_eq!(actual.playback_frames as u128, u128::from(end_frame));
        assert!(actual.playback_end_physical_frame.is_some());
        assert_eq!(&pcm[..end_frame as usize], vec![0.125; end_frame as usize]);
        assert!(pcm[end_frame as usize..]
            .iter()
            .all(|sample| *sample == 0.0));
        assert_eq!(actual.counters.commands_applied, 1);
        sealed.seal_gameplay_audio();
        sealed.seal_gameplay_audio();
        assert!(sealed.gameplay_audio_sealed());
        assert!(!baseline.gameplay_audio_sealed());
        assert_eq!(sealed.gameplay_fence(), None);
        for (at, sequence, state, expected_song) in [
            (note_output, 1, ButtonState::Down, 6_000_000),
            (mine_output + 2, 2, ButtonState::Up, release_song),
        ] {
            if sequence == 2 {
                // The mine observes the genuinely held button before its later Up.
                let raw = point(2, 70_000_002);
                let quiet = sealed
                    .advance_to(point(1, mine_output), &Identity, raw)
                    .unwrap();
                let ordinary = baseline
                    .advance_to(point(1, mine_output), &Identity, raw)
                    .unwrap();
                assert_eq!(quiet.song_time, ts(12_000_000));
                assert_eq!(quiet.audio_at, raw);
                assert_eq!(quiet.hazard_events, ordinary.hazard_events);
                assert_eq!(quiet.hazard_events.len(), 1);
                assert_eq!(quiet.hazard_events[0].outcome, HazardOutcome::Triggered);
                assert_eq!(quiet.hazard_events[0].at, ts(12_000_000));
                assert_eq!(quiet.hazard_events[0].value, 1);
                assert!(quiet.audio_commands.is_empty() && quiet.audio_failures.is_empty());
                assert_eq!(
                    ordinary.audio_commands,
                    [AudioCommand::Play {
                        sample: SampleId(1),
                        voice: VoiceId(3),
                        at: raw.timestamp,
                        gain: 0.25,
                    }]
                );
                assert_eq!(
                    sealed.judge().stable_hash().unwrap(),
                    baseline.judge().stable_hash().unwrap()
                );
            }
            let event = input(at, sequence, state);
            let raw = point(
                2,
                if sequence == 1 {
                    70_000_001
                } else {
                    70_000_003
                },
            );
            let quiet = sealed.process_input(event.clone(), &Identity, raw).unwrap();
            let ordinary = baseline
                .process_input(event.clone(), &Identity, raw)
                .unwrap();
            assert_eq!(quiet.song_time, ts(expected_song));
            assert!(!quiet.song_end_reached);
            assert_eq!(quiet.audio_at, raw);
            assert_eq!(quiet.input, ordinary.input);
            assert_eq!(quiet.bound_inputs, ordinary.bound_inputs);
            assert_eq!(quiet.bound_inputs[0].physical.meta(), event.meta());
            assert_eq!(quiet.judge_events, ordinary.judge_events);
            assert_eq!(quiet.hazard_events, ordinary.hazard_events);
            assert!(quiet.audio_commands.is_empty() && quiet.audio_failures.is_empty());
            assert!(quiet.judge_error.is_none());
            if sequence == 1 {
                assert_eq!(quiet.judge_events.len(), 1);
                assert_eq!(ordinary.audio_commands.len(), 1);
            } else {
                assert!(quiet.hazard_events.is_empty());
                assert!(ordinary.audio_commands.is_empty());
            }
            assert_eq!(
                sealed.judge().stable_hash().unwrap(),
                baseline.judge().stable_hash().unwrap()
            );
        }
        assert_eq!(sealed.telemetry().counters().inputs, 2);
        assert_eq!(sealed.telemetry().counters().audio_commands, 1); // Only the original control Play.
        assert_eq!(baseline.telemetry().counters().audio_commands, 2);
        assert!(baseline_consumer.try_pop().is_ok());
        assert!(baseline_consumer.try_pop().is_ok());
        assert!(baseline_consumer.try_pop().is_err());
        let after = output.render(&mut [1.0; 5]).unwrap();
        assert_eq!(
            after.counters.commands_applied,
            actual.counters.commands_applied
        );
        assert_eq!(after.playback_frames, 0);
        sealed.fence_gameplay();
        assert!(sealed.fence_gameplay_sounds(ts(90_000_000)).is_none());
    }
}

#[test]
fn seal_keeps_previously_admitted_commands_and_direct_control_producer_semantics() {
    let (mut owner, mut consumer) = runtime(Rate::NORMAL);
    let first = owner
        .process_input(
            input(6_000_000, 1, ButtonState::Down),
            &Identity,
            point(2, 6_000_000),
        )
        .unwrap();
    let admitted = first.audio_commands[0];
    assert_eq!(owner.telemetry().counters().audio_commands, 1);
    owner.seal_gameplay_audio();
    assert_eq!(owner.telemetry().counters().audio_commands, 1);
    assert_eq!(consumer.try_pop().unwrap(), admitted);
    assert!(consumer.try_pop().is_err());
    let control = AudioCommand::Stop {
        voice: VoiceId(99),
        at: ts(8_000_000),
    };
    owner.enqueue_audio(control).unwrap();
    assert_eq!(consumer.try_pop().unwrap(), control);
    assert_eq!(owner.telemetry().counters().audio_commands, 2);
}

#[test]
fn seal_is_allocation_free_and_neither_fences_judgment_nor_fabricates_completion() {
    let (mut owner, mut consumer) = runtime(Rate::NORMAL);
    ALLOCATIONS.with(|count| count.set(0));
    COUNTING.with(|enabled| enabled.set(true));
    owner.seal_gameplay_audio();
    owner.seal_gameplay_audio();
    let sealed = owner.gameplay_audio_sealed();
    COUNTING.with(|enabled| enabled.set(false));
    assert!(sealed);
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    assert_eq!(owner.judge().effective_song_time(), None);
    assert_eq!(owner.gameplay_fence(), None);
    assert_eq!(owner.song_end(), Some(ts(21_000_000)));
    assert_eq!(owner.telemetry().counters().audio_commands, 0);
    assert!(consumer.try_pop().is_err());
}

#[test]
fn explicit_fresh_state_epoch_reopens_sounds_without_implicitly_clearing_old_queue() {
    let (mut owner, mut consumer) = runtime(Rate::NORMAL);
    owner.enqueue_audio(play(99, 0)).unwrap();
    owner.seal_gameplay_audio();
    owner.replace_state(judge(), transport(Rate::NORMAL));
    assert!(!owner.gameplay_audio_sealed());
    assert_eq!(consumer.try_pop().unwrap(), play(99, 0));
    let fresh = owner
        .process_input(
            input(6_000_000, 1, ButtonState::Down),
            &Identity,
            point(2, 6_000_000),
        )
        .unwrap();
    assert_eq!(fresh.judge_events.len(), 1);
    assert_eq!(fresh.audio_commands.len(), 1);
    assert_eq!(consumer.try_pop().unwrap(), fresh.audio_commands[0]);
}

#[test]
fn sealed_empty_press_keeps_held_release_and_avoids_even_disconnected_sound_admission() {
    let (mut sealed, consumer) = runtime(Rate::NORMAL);
    let (mut normal, mut normal_consumer) = runtime(Rate::NORMAL);
    drop(consumer);
    sealed.seal_gameplay_audio();
    for (at, sequence, state) in [(0, 1, ButtonState::Down), (1_000_000, 2, ButtonState::Up)] {
        let quiet = sealed
            .process_input(input(at, sequence, state), &Identity, point(2, 80_000_000))
            .unwrap();
        let ordinary = normal
            .process_input(input(at, sequence, state), &Identity, point(2, 80_000_000))
            .unwrap();
        assert_eq!(quiet.bound_inputs, ordinary.bound_inputs);
        assert_eq!(quiet.judge_events, ordinary.judge_events);
        assert!(quiet.judge_events.is_empty());
        assert!(quiet.audio_commands.is_empty() && quiet.audio_failures.is_empty());
        assert_eq!(
            sealed.judge().stable_hash().unwrap(),
            normal.judge().stable_hash().unwrap()
        );
        if sequence == 1 {
            assert_eq!(
                ordinary.audio_commands,
                [AudioCommand::Play {
                    sample: SampleId(1),
                    voice: VoiceId(2),
                    at: ts(80_000_000),
                    gain: 0.5,
                }]
            );
        }
    }
    assert_eq!(sealed.telemetry().counters().queue_disconnected, 0);
    assert_eq!(sealed.telemetry().counters().audio_commands, 0);
    assert!(normal_consumer.try_pop().is_ok());
    assert!(normal_consumer.try_pop().is_err());
}

#[test]
fn a_stop_already_accepted_before_the_seal_is_not_removed_or_reacknowledged() {
    let (mut owner, mut consumer) = runtime(Rate::NORMAL);
    let first = owner
        .process_input(
            input(6_000_000, 1, ButtonState::Down),
            &Identity,
            point(2, 6_000_000),
        )
        .unwrap();
    owner.fence_gameplay();
    let stopped = owner.fence_gameplay_sounds(ts(7_000_000)).unwrap();
    assert_eq!(stopped.commands.len(), 3);
    let accepted = owner.telemetry().counters().audio_commands;
    owner.seal_gameplay_audio();
    assert!(owner.fence_gameplay_sounds(ts(8_000_000)).is_none());
    assert_eq!(owner.telemetry().counters().audio_commands, accepted);
    assert_eq!(consumer.try_pop().unwrap(), first.audio_commands[0]);
    for command in stopped.commands {
        assert_eq!(consumer.try_pop().unwrap(), command);
    }
    assert!(consumer.try_pop().is_err());
}
