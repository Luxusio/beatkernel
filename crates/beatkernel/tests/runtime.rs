use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, CommandConsumer, Mixer, MixerConfig,
        PcmLimits, PcmSample, QueuePushError, SampleBank, SampleId, VoiceId,
    },
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        BackendId, Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector,
        EventMeta, GameControlId, NativeEventMeta, PhysicalControlId, PhysicalInputEvent,
    },
    interaction::InstantEvaluator,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow, Rule},
    runtime::{Runtime, RuntimeError, SoundBinding},
    telemetry::RuntimeTelemetry,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};

fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
struct Clocks;
impl ClockMapper for Clocks {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        match (from.domain.0, to.0) {
            (1, 2) => from
                .timestamp
                .checked_add(Duration::from_nanos(1_000_000_000)),
            (2, 3) => from
                .timestamp
                .checked_sub(Duration::from_nanos(1_000_000_000)),
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
    let mut chart = SourceChart::new(1000, Bpm::new(60, 1).unwrap()).unwrap();
    for (object, control) in [(1, 1), (2, 2)] {
        chart.objects.push(SourceObject {
            id: ObjectId(object),
            start: Beat::new(2).unwrap(),
            end: None,
            interaction: InteractionId(control),
            visual: VisualId(control),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
    }
    let judge = JudgeEngine::new(
        chart.compile().unwrap(),
        (1..=2)
            .map(|control| Rule {
                interaction: InteractionId(control),
                control: GameControlId(control),
                evaluator: Box::new(InstantEvaluator),
            })
            .collect(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    let bindings = BindingMap::from_bindings((1..=2).map(|control| Binding {
        device: DeviceSelector::Exact(DeviceId(control as u64)),
        physical: PhysicalControlId::keyboard(4),
        game_control: GameControlId(control),
    }))
    .unwrap();
    let (producer, consumer) = command_queue(capacity).unwrap();
    let sounds = (1..=2)
        .map(|id| SoundBinding {
            object: ObjectId(id),
            stage: JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(id),
            gain: 1.0,
        })
        .collect();
    (
        Runtime::new(
            ClockDomainId(2),
            ClockDomainId(3),
            Transport::new(
                Timestamp::from_nanos(1_000_000_000),
                Timestamp::ZERO,
                Rate::NORMAL,
            ),
            bindings,
            judge,
            producer,
            sounds,
            8,
        )
        .unwrap(),
        consumer,
    )
}
fn input(device: u64, nanos: i64, sequence: u64) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(device), point(1, nanos), sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(4),
        code: Some(30),
        timestamp: Some(point(1, nanos)),
    });
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    })
}

#[test]
fn physical_provenance_distinct_device_binding_and_audio_clock_reach_pcm() {
    let (mut runtime, consumer) = fixture(4);
    let report = runtime
        .process_input(input(2, 2_000_000, 5), &Clocks, point(2, 1_003_000_000))
        .unwrap();
    assert_eq!(report.judge_events[0].object, ObjectId(2));
    assert_eq!(report.bound_inputs[0].game_control, GameControlId(2));
    let meta = report.judge_events[0].input.unwrap();
    assert_eq!(meta.source, DeviceId(2));
    assert_eq!(meta.original_clock_point, Some(point(1, 2_000_000)));
    assert_eq!(meta.native.unwrap().timestamp, Some(point(1, 2_000_000)));
    assert_eq!(meta.timestamp, Timestamp::from_nanos(1_002_000_000));
    assert_eq!(
        report.audio_commands[0].at(),
        Timestamp::from_nanos(3_000_000)
    );
    assert_ne!(report.audio_commands[0].at(), report.judge_events[0].at);
    assert_eq!(report.input_mapping_quality, Clocks.quality());
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(1024, 4096, 4).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5], limits).unwrap(),
    )
    .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(3),
            Timestamp::ZERO,
            AudioLimits::new(4, 2, 4, 16, 4).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let mut pcm = [0.0; 7];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0, 0.0, 0.0, 0.25, 0.5, 0.0, 0.0]);
}

#[test]
fn full_queue_preserves_committed_judgment_and_exact_failed_command() {
    let (mut runtime, mut consumer) = fixture(1);
    runtime
        .process_input(input(1, 2_000_000, 1), &Clocks, point(3, 3_000_000))
        .unwrap();
    let report = runtime
        .process_input(input(2, 2_000_000, 1), &Clocks, point(3, 4_000_000))
        .unwrap();
    assert_eq!(report.judge_events.len(), 1);
    assert_eq!(report.audio_failures[0].reason, QueuePushError::Full);
    assert_eq!(
        report.audio_failures[0].command,
        AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: Timestamp::from_nanos(4_000_000),
            gain: 1.0
        }
    );
    assert_eq!(runtime.telemetry().counters().queue_full, 1);
    assert_eq!(runtime.telemetry().counters().judge_results, 2);
    assert_eq!(
        consumer.try_pop().unwrap().at(),
        Timestamp::from_nanos(3_000_000)
    );
    assert!(runtime
        .process_input(input(2, 2_000_000, 1), &Clocks, point(3, 5_000_000))
        .unwrap()
        .judge_events
        .is_empty());
}

#[test]
fn mapping_failures_and_regressions_are_explicit_without_fallback() {
    let (mut runtime, _consumer) = fixture(4);
    assert!(matches!(
        runtime.process_input(input(1, 2_000_000, 2), &Clocks, point(99, 0)),
        Err(RuntimeError::UnmappedClock { .. })
    ));
    runtime
        .process_input(input(1, 2_000_000, 2), &Clocks, point(3, 0))
        .unwrap();
    assert_eq!(
        runtime
            .process_input(input(1, 1_000_000, 2), &Clocks, point(3, 0))
            .unwrap_err(),
        RuntimeError::NonMonotonicHost
    );
    assert!(matches!(
        runtime.process_input(input(1, 2_000_000, 1), &Clocks, point(3, 0)),
        Err(RuntimeError::SequenceRegression { .. })
    ));
    assert!(matches!(
        runtime.process_input(input(1, i64::MAX, 3), &Clocks, point(3, 0)),
        Err(RuntimeError::UnmappedClock { .. })
    ));
    runtime
        .transport_mut()
        .set_rate(Timestamp::from_nanos(1_002_000_000), Rate::REVERSE)
        .unwrap();
    assert_eq!(
        runtime
            .process_input(input(1, 2_000_000, 3), &Clocks, point(3, 0))
            .unwrap_err(),
        RuntimeError::RequiresReplayRestore
    );
}

#[test]
fn disconnected_queue_and_timeout_are_reported_separately() {
    let (mut runtime, consumer) = fixture(4);
    drop(consumer);
    let report = runtime
        .process_input(input(1, 2_000_000, 1), &Clocks, point(3, 3_000_000))
        .unwrap();
    assert_eq!(
        report.audio_failures[0].reason,
        QueuePushError::Disconnected
    );
    let misses = runtime
        .advance_to(point(2, 1_003_000_000), &Clocks, point(3, 4_000_000))
        .unwrap();
    assert_eq!(misses.judge_events.len(), 1);
    assert!(misses.audio_commands.is_empty());
    assert_eq!(runtime.telemetry().counters().queue_disconnected, 1);
}

#[test]
fn bounded_percentiles_and_external_loss_counts_do_not_invent_latency() {
    let mut telemetry = RuntimeTelemetry::new(4);
    assert_eq!(telemetry.processing(), None);
    for value in [100, 20, 30, 40, 10] {
        telemetry.record_processing_ns(value);
    }
    let summary = telemetry.processing().unwrap();
    assert_eq!(
        (
            summary.samples,
            summary.p50_ns,
            summary.p95_ns,
            summary.p99_ns,
            summary.max_ns
        ),
        (4, 20, 40, 40, 40)
    );
    telemetry.report_input_drops(7);
    telemetry.report_audio_underruns(2);
    assert_eq!(telemetry.counters().input_drops, 7);
    assert_eq!(telemetry.counters().audio_underruns, 2);
}

#[test]
fn session_replacement_resets_chronology_and_routes_new_commands_to_fresh_queue() {
    let (mut runtime, mut old_consumer) = fixture(4);
    let initial = runtime.judge().snapshot().unwrap();
    let transport = runtime.transport().clone();
    runtime
        .process_input(input(1, 2_000_000, 8), &Clocks, point(3, 3_000_000))
        .unwrap();
    let (producer, mut new_consumer) = command_queue(4).unwrap();
    let (_, _, old_producer) = runtime.replace_session(
        JudgeEngine::from_snapshot(&initial).unwrap(),
        transport,
        producer,
    );
    let report = runtime
        .process_input(input(1, 2_000_000, 1), &Clocks, point(3, 9_000_000))
        .unwrap();
    assert!(report.judge_error.is_none());
    assert_eq!(
        new_consumer.try_pop().unwrap().at(),
        Timestamp::from_nanos(9_000_000)
    );
    assert_eq!(
        old_consumer.try_pop().unwrap().at(),
        Timestamp::from_nanos(3_000_000)
    );
    assert!(!old_consumer.is_disconnected());
    drop(old_producer);
    assert!(old_consumer.is_disconnected());
}
