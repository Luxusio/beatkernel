//! Portable fault schedules over the real runtime, judge and bounded audio ring.
use beatkernel::{
    audio::{command_queue, AudioCommand, CommandConsumer, QueuePushError, SampleId, VoiceId},
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    interaction::InstantEvaluator,
    judge::{JudgeEngine, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeStage, JudgeWindow, Rule},
    runtime::{Runtime, RuntimeError, RuntimeProcessingClock, SoundBinding},
    telemetry::RuntimeCounters,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};

const TARGETS: [i64; 6] = [10, 20, 30, 40, 50, 60];

fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}

#[derive(Clone, Copy)]
enum Mapping {
    Available,
    InputOutage,
    AudioOutage,
}

impl ClockMapper for Mapping {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        match (from.domain.0, to.0, self) {
            (10, 20, Self::InputOutage) | (20, 30, Self::AudioOutage) => None,
            (10, 20, _) => from.timestamp.checked_add(Duration::from_nanos(900)),
            (20, 30, _) => from.timestamp.checked_sub(Duration::from_nanos(1000)),
            _ => None,
        }
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Estimated {
            max_error: Duration::from_nanos(4),
        }
    }
}

fn fixture(capacity: usize) -> (Runtime, CommandConsumer) {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    for (index, at) in TARGETS.into_iter().enumerate() {
        let id = index as u32 + 1;
        source.objects.push(SourceObject {
            id: ObjectId(u64::from(id)),
            start: Beat::new(at).unwrap(),
            end: None,
            interaction: InteractionId(id),
            visual: VisualId(id),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
    }
    let judge = JudgeEngine::new(
        source.compile().unwrap(),
        (1..=6)
            .map(|id| Rule {
                interaction: InteractionId(id),
                control: GameControlId(id),
                evaluator: Box::new(InstantEvaluator),
            })
            .collect(),
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
    let bindings = BindingMap::from_bindings((1..=6).map(|id| Binding {
        device: DeviceSelector::Exact(DeviceId(99)),
        physical: PhysicalControlId::keyboard(id),
        game_control: GameControlId(u32::from(id)),
    }))
    .unwrap();
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(20),
        ClockDomainId(30),
        Transport::new(Timestamp::from_nanos(1000), Timestamp::ZERO, Rate::NORMAL),
        bindings,
        judge,
        producer,
        (1..=6)
            .map(|id| SoundBinding {
                object: ObjectId(id),
                stage: JudgeStage::Instant,
                sample: SampleId(7),
                voice: VoiceId(id),
                gain: 0.5,
            })
            .collect(),
        8,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    (runtime, consumer)
}

fn input(note: u32) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(
            DeviceId(99),
            point(10, 100 + TARGETS[note as usize - 1]),
            100 + u64::from(note),
        ),
        control: PhysicalControlId::keyboard(note as u16),
        state: ButtonState::Down,
    })
}

fn command(note: u64) -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(note),
        sample: SampleId(7),
        at: Timestamp::from_nanos(100 + TARGETS[note as usize - 1]),
        gain: 0.5,
    }
}

#[derive(Clone, Copy)]
enum Step {
    InputOutage(u32),
    AudioOutage(u32),
    Hit(u32),
    Drain,
}

// The first script delays consumption across five inputs, forcing saturation
// at both capacities. The second moves drains and the outage into the prefix.
const LONG_STALL: &[Step] = &[
    Step::InputOutage(1),
    Step::Hit(1),
    Step::Hit(2),
    Step::Hit(3),
    Step::Hit(4),
    Step::Hit(5),
    Step::Drain,
    Step::Hit(6),
    Step::Drain,
];
const SPLIT_STALL: &[Step] = &[
    Step::Hit(1),
    Step::Drain,
    Step::AudioOutage(2),
    Step::Hit(2),
    Step::Hit(3),
    Step::Drain,
    Step::Hit(4),
    Step::Hit(5),
    Step::Hit(6),
    Step::Drain,
];

#[derive(Debug, PartialEq)]
struct Facts {
    hit_objects: Vec<u64>,
    rejected_voices: Vec<u64>,
    consumed: Vec<AudioCommand>,
    counters: RuntimeCounters,
    judge_hash: u64,
}

fn run(script: &[Step], capacity: usize, rejected: &[u64], accepted: &[u64]) -> Facts {
    let (mut runtime, mut consumer) = fixture(capacity);
    let mut facts = Facts {
        hit_objects: Vec::new(),
        rejected_voices: Vec::new(),
        consumed: Vec::new(),
        counters: RuntimeCounters::default(),
        judge_hash: 0,
    };
    for &step in script {
        if matches!(step, Step::Drain) {
            while let Ok(value) = consumer.try_pop() {
                facts.consumed.push(value);
            }
            assert!(consumer.try_pop().is_err());
            continue;
        }
        let (note, mapper) = match step {
            Step::InputOutage(note) => (note, Mapping::InputOutage),
            Step::AudioOutage(note) => (note, Mapping::AudioOutage),
            Step::Hit(note) => (note, Mapping::Available),
            Step::Drain => unreachable!(),
        };
        let original = input(note);
        let audio_point = point(20, 1100 + TARGETS[note as usize - 1]);
        if !matches!(mapper, Mapping::Available) {
            let before_hash = runtime.judge().stable_hash().unwrap();
            let before_time = runtime.judge().effective_song_time();
            let before_transport = runtime.transport().clone();
            let before_counters = runtime.telemetry().counters();
            let error = runtime
                .process_input(original, &mapper, audio_point)
                .unwrap_err();
            let (from, to) = if matches!(mapper, Mapping::InputOutage) {
                (10, 20)
            } else {
                (20, 30)
            };
            assert_eq!(
                error,
                RuntimeError::UnmappedClock {
                    from: ClockDomainId(from),
                    to: ClockDomainId(to)
                }
            );
            assert_eq!(runtime.judge().stable_hash().unwrap(), before_hash);
            assert_eq!(runtime.judge().effective_song_time(), before_time);
            assert_eq!(runtime.transport().anchors(), before_transport.anchors());
            assert_eq!(
                runtime.telemetry().counters(),
                RuntimeCounters {
                    rejected: before_counters.rejected + 1,
                    ..before_counters
                }
            );
            continue;
        }
        // A fault step and its following hit recreate precisely the same source
        // event, including acquisition sequence; no replacement sequence is minted.
        let report = runtime
            .process_input(original, &mapper, audio_point)
            .unwrap();
        let target = Timestamp::from_nanos(TARGETS[note as usize - 1]);
        assert_eq!(report.song_time, target);
        assert_eq!(report.audio_at, point(30, 100 + TARGETS[note as usize - 1]));
        assert_eq!(report.bound_inputs.len(), 1);
        assert_eq!(report.bound_inputs[0].game_control, GameControlId(note));
        assert_eq!(report.judge_error, None);
        assert_eq!(report.judge_events.len(), 1);
        let event = report.judge_events[0];
        assert_eq!(event.object, ObjectId(u64::from(note)));
        assert_eq!(event.at, target);
        assert_eq!(event.stage, JudgeStage::Instant);
        assert_eq!(
            event.outcome,
            JudgeOutcome::Hit {
                grade: JudgeGrade(7),
                delta: Duration::ZERO
            }
        );
        let meta = event.input.unwrap();
        assert_eq!(meta.source, DeviceId(99));
        assert_eq!(meta.sequence, 100 + u64::from(note));
        assert_eq!(
            meta.original_clock_point,
            Some(point(10, 100 + TARGETS[note as usize - 1]))
        );
        assert_eq!(
            meta.timestamp,
            Timestamp::from_nanos(1000 + TARGETS[note as usize - 1])
        );
        assert_eq!(runtime.judge().effective_song_time(), Some(target));
        facts.hit_objects.push(event.object.0);
        if rejected.contains(&u64::from(note)) {
            assert!(report.audio_commands.is_empty());
            assert_eq!(report.audio_failures.len(), 1);
            assert_eq!(report.audio_failures[0].reason, QueuePushError::Full);
            assert_eq!(report.audio_failures[0].command, command(u64::from(note)));
            facts.rejected_voices.push(u64::from(note));
        } else {
            assert!(report.audio_failures.is_empty());
            assert_eq!(report.audio_commands, [command(u64::from(note))]);
        }
    }
    assert_eq!(facts.hit_objects, [1, 2, 3, 4, 5, 6]);
    assert_eq!(facts.rejected_voices, rejected);
    assert_eq!(
        facts.consumed,
        accepted.iter().copied().map(command).collect::<Vec<_>>()
    );
    assert!(consumer.try_pop().is_err());
    assert!(!consumer.is_disconnected());
    facts.counters = runtime.telemetry().counters();
    assert_eq!(
        facts.counters,
        RuntimeCounters {
            inputs: 6,
            rejected: 1,
            judge_results: 6,
            audio_commands: accepted.len() as u64,
            queue_full: rejected.len() as u64,
            ..RuntimeCounters::default()
        }
    );
    assert_eq!(runtime.telemetry().processing(), None);
    facts.judge_hash = runtime.judge().stable_hash().unwrap();
    facts
}

#[test]
fn mapping_outage_long_stall_and_recovery_repeat_with_literal_prefixes() {
    for (capacity, rejected, accepted) in [
        (1, &[2, 3, 4, 5][..], &[1, 6][..]),
        (3, &[4, 5][..], &[1, 2, 3, 6][..]),
    ] {
        let baseline = run(LONG_STALL, capacity, rejected, accepted);
        for _ in 0..3 {
            assert_eq!(run(LONG_STALL, capacity, rejected, accepted), baseline);
        }
    }
}

#[test]
fn audio_mapping_outage_and_split_stalls_repeat_with_distinct_expected_admission() {
    for (capacity, rejected, accepted) in [
        (1, &[3, 5, 6][..], &[1, 2, 4][..]),
        (3, &[][..], &[1, 2, 3, 4, 5, 6][..]),
    ] {
        let baseline = run(SPLIT_STALL, capacity, rejected, accepted);
        for _ in 0..3 {
            assert_eq!(run(SPLIT_STALL, capacity, rejected, accepted), baseline);
        }
    }
}

#[test]
fn mapping_refusal_preserves_sequence_and_time_watermarks_for_intermediate_input() {
    for outage in [Mapping::InputOutage, Mapping::AudioOutage] {
        let (mut runtime, mut consumer) = fixture(3);
        let first = runtime
            .process_input(input(1), &Mapping::Available, point(20, 1110))
            .unwrap();
        assert_eq!(first.judge_events[0].object, ObjectId(1));
        assert_eq!(consumer.try_pop().unwrap(), command(1));
        let before_hash = runtime.judge().stable_hash().unwrap();
        let original = input(2);
        let error = runtime
            .process_input(original.clone(), &outage, point(20, 1120))
            .unwrap_err();
        let (from, to) = if matches!(outage, Mapping::InputOutage) {
            (10, 20)
        } else {
            (20, 30)
        };
        assert_eq!(
            error,
            RuntimeError::UnmappedClock {
                from: ClockDomainId(from),
                to: ClockDomainId(to),
            }
        );
        assert_eq!(runtime.judge().stable_hash().unwrap(), before_hash);
        // This is earlier than the refused event, with a lower sequence. Equal
        // retries alone would not expose premature private watermark updates.
        let probe = PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(99), point(10, 115), 101),
            control: PhysicalControlId::keyboard(99),
            state: ButtonState::Down,
        });
        let intermediate = runtime
            .process_input(probe, &Mapping::Available, point(20, 1115))
            .unwrap();
        assert_eq!(intermediate.song_time, Timestamp::from_nanos(15));
        assert_eq!(intermediate.audio_at, point(30, 115));
        assert!(intermediate.bound_inputs.is_empty());
        assert!(intermediate.judge_events.is_empty());
        assert_eq!(intermediate.judge_error, None);
        assert!(intermediate.audio_commands.is_empty());
        assert!(intermediate.audio_failures.is_empty());
        let admitted = intermediate.input.unwrap();
        assert_eq!(admitted.meta().source, DeviceId(99));
        assert_eq!(admitted.meta().sequence, 101);
        assert_eq!(admitted.meta().timestamp, Timestamp::from_nanos(1015));
        assert_eq!(admitted.meta().original_clock_point, Some(point(10, 115)));
        assert!(consumer.try_pop().is_err());
        let retry = runtime
            .process_input(original, &Mapping::Available, point(20, 1120))
            .unwrap();
        assert_eq!(retry.song_time, Timestamp::from_nanos(20));
        assert_eq!(retry.judge_events.len(), 1);
        assert_eq!(retry.judge_events[0].object, ObjectId(2));
        assert_eq!(retry.judge_events[0].input.unwrap().sequence, 102);
        assert_eq!(
            retry.judge_events[0].outcome,
            JudgeOutcome::Hit {
                grade: JudgeGrade(7),
                delta: Duration::ZERO,
            }
        );
        assert_eq!(retry.judge_error, None);
        assert!(retry.audio_failures.is_empty());
        assert_eq!(retry.audio_commands, [command(2)]);
        assert_eq!(consumer.try_pop().unwrap(), command(2));
        assert!(consumer.try_pop().is_err());
        assert_eq!(
            runtime.telemetry().counters(),
            RuntimeCounters {
                inputs: 3,
                unbound: 1,
                rejected: 1,
                judge_results: 2,
                audio_commands: 2,
                ..RuntimeCounters::default()
            }
        );
    }
}
