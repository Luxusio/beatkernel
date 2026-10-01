use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, CommandConsumer, Mixer, MixerConfig, PcmLimits,
        PcmSample, QueuePushError, SampleBank, SampleId, VoiceId, command_queue,
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
fn song_end_excludes_exact_boundary_input_without_fabricating_note_completion() {
    let (mut runtime, mut consumer) = fixture(4);
    runtime
        .set_song_end(Timestamp::from_nanos(2_000_000))
        .unwrap();
    let original_transport = runtime.transport().clone();
    let report = runtime
        .process_input(input(99, 2_000_000, 7), &Clocks, point(2, 1_003_000_000))
        .unwrap();
    assert!(report.song_end_reached);
    assert_eq!(report.song_time, Timestamp::from_nanos(2_000_000));
    assert!(report.input.is_none());
    assert!(report.bound_inputs.is_empty());
    assert!(report.judge_events.is_empty());
    assert!(report.judge_error.is_none());
    assert!(report.audio_commands.is_empty());
    assert!(consumer.try_pop().is_err());
    assert_eq!(runtime.transport().anchors(), original_transport.anchors());
    assert_eq!(report.input_mapping_quality, Clocks.quality());
    assert_eq!(report.audio_mapping_quality, Clocks.quality());
    assert_eq!(report.audio_at, point(3, 3_000_000));
    assert_eq!(runtime.telemetry().counters().inputs, 1);
    assert_eq!(runtime.telemetry().counters().unbound, 0);
    let snapshot = runtime.judge().stable_hash().unwrap();
    let later = runtime
        .advance_to(point(2, 1_010_000_000), &Clocks, point(3, 10_000_000))
        .unwrap();
    assert!(later.song_end_reached);
    assert!(later.judge_events.is_empty());
    assert_eq!(later.song_time, report.song_time);
    assert_eq!(runtime.judge().stable_hash().unwrap(), snapshot);
}

#[test]
fn strictly_earlier_hit_reaches_pcm_but_end_and_late_inputs_only_advance_actual_judge() {
    let (mut runtime, consumer) = fixture(4);
    runtime
        .set_song_end(Timestamp::from_nanos(2_000_001))
        .unwrap();
    let early = runtime
        .process_input(input(1, 2_000_000, 1), &Clocks, point(3, 3_000_000))
        .unwrap();
    assert!(!early.song_end_reached);
    assert!(early.input.is_some());
    assert_eq!(early.judge_events.len(), 1);
    let ended = runtime
        .process_input(input(2, 2_000_001, 1), &Clocks, point(3, 4_000_000))
        .unwrap();
    assert!(ended.song_end_reached);
    assert!(ended.input.is_none());
    assert_eq!(ended.judge_events.len(), 1);
    assert!(ended.audio_commands.is_empty());
    assert!(ended.audio_failures.is_empty());
    let later = runtime
        .process_input(input(2, 8_000_000, 2), &Clocks, point(3, 9_000_000))
        .unwrap();
    assert_eq!(later.song_time, ended.song_time);
    assert!(later.judge_events.is_empty());
    assert!(later.input.is_none());
    assert_eq!(runtime.telemetry().counters().inputs, 3);
    assert_eq!(runtime.telemetry().counters().unbound, 0);
    assert_eq!(runtime.telemetry().counters().judge_results, 2);
    assert_eq!(runtime.telemetry().counters().audio_commands, 1);
    let format = AudioFormat::new(1000, 1).unwrap();
    let pcm_limits = PcmLimits::new(1024, 4096, 4).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5], pcm_limits).unwrap(),
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
    let mut pcm = [99.0; 7];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0, 0.0, 0.0, 0.25, 0.5, 0.0, 0.0]);
}

#[test]
fn fenced_acquisition_still_validates_mapping_host_sequence_and_raw_song_regression() {
    let (mut runtime, _consumer) = fixture(4);
    runtime
        .set_song_end(Timestamp::from_nanos(2_000_000))
        .unwrap();
    runtime
        .process_input(input(1, 4_000_000, 5), &Clocks, point(3, 0))
        .unwrap();
    assert!(matches!(
        runtime.process_input(input(1, 4_000_000, 4), &Clocks, point(3, 0)),
        Err(RuntimeError::SequenceRegression { .. })
    ));
    assert_eq!(
        runtime
            .process_input(input(2, 3_000_000, 1), &Clocks, point(3, 0))
            .unwrap_err(),
        RuntimeError::NonMonotonicHost
    );
    assert!(matches!(
        runtime.process_input(input(1, i64::MAX, 6), &Clocks, point(3, 0)),
        Err(RuntimeError::UnmappedClock { .. })
    ));
    assert!(matches!(
        runtime.process_input(input(1, 5_000_000, 6), &Clocks, point(99, 0)),
        Err(RuntimeError::UnmappedClock { .. })
    ));
    assert_eq!(runtime.telemetry().counters().inputs, 1);
    runtime
        .transport_mut()
        .seek(
            Timestamp::from_nanos(1_005_000_000),
            Timestamp::from_nanos(3_000_000),
        )
        .unwrap();
    assert_eq!(
        runtime
            .process_input(input(1, 5_000_000, 6), &Clocks, point(3, 0))
            .unwrap_err(),
        RuntimeError::RequiresReplayRestore
    );
    assert_eq!(runtime.telemetry().counters().inputs, 1);
    let (mut reverse, _consumer) = fixture(4);
    reverse.set_song_end(Timestamp::ZERO).unwrap();
    reverse
        .transport_mut()
        .set_rate(Timestamp::from_nanos(1_000_000_000), Rate::REVERSE)
        .unwrap();
    assert_eq!(
        reverse
            .advance_to(point(2, 1_000_000_000), &Clocks, point(3, 0))
            .unwrap_err(),
        RuntimeError::RequiresReplayRestore
    );
}

#[test]
fn endpoint_configuration_is_atomic_setup_only_and_restoration_clears_it() {
    let (mut runtime, _consumer) = fixture(4);
    assert_eq!(runtime.song_end(), None);
    assert_eq!(
        runtime.set_song_end(Timestamp::from_nanos(-1)),
        Err(RuntimeError::InvalidSongEnd)
    );
    assert_eq!(runtime.song_end(), None);
    runtime.set_song_end(Timestamp::ZERO).unwrap();
    assert_eq!(
        runtime.set_song_end(Timestamp::from_nanos(10)),
        Err(RuntimeError::SongEndConfigurationLocked)
    );
    assert_eq!(runtime.song_end(), Some(Timestamp::ZERO));
    let restored = runtime.judge().snapshot().unwrap();
    let transport = runtime.transport().clone();
    runtime
        .advance_to(point(2, 1_000_000_001), &Clocks, point(3, 0))
        .unwrap();
    let mut exchanged = transport.clone();
    runtime.exchange_transport(&mut exchanged);
    let (mut producer, _unused_consumer) = command_queue(4).unwrap();
    runtime.exchange_audio_producer(&mut producer);
    assert_eq!(runtime.song_end(), Some(Timestamp::ZERO));
    runtime.replace_state(
        JudgeEngine::from_snapshot(&restored).unwrap(),
        transport.clone(),
    );
    assert_eq!(runtime.song_end(), None);
    runtime.set_song_end(Timestamp::from_nanos(20)).unwrap();
    let (producer, _unused_consumer) = command_queue(4).unwrap();
    runtime.replace_session(
        JudgeEngine::from_snapshot(&restored).unwrap(),
        transport,
        producer,
    );
    assert_eq!(runtime.song_end(), None);
    runtime
        .advance_to(point(2, 1_000_000_000), &Clocks, point(3, 0))
        .unwrap();
    assert_eq!(
        runtime.set_song_end(Timestamp::from_nanos(30)),
        Err(RuntimeError::SongEndConfigurationLocked)
    );
    let (mut invalid, _consumer) = fixture(4);
    assert!(
        invalid
            .advance_to(point(99, 0), &Clocks, point(3, 0))
            .is_err()
    );
    invalid.set_song_end(Timestamp::ZERO).unwrap();
    let (mut placeholder, _consumer) = fixture(4);
    placeholder
        .transport_mut()
        .seek(
            Timestamp::from_nanos(1_000_000_000),
            Timestamp::from_nanos(100),
        )
        .unwrap();
    placeholder.set_song_end(Timestamp::ZERO).unwrap();
}

#[test]
fn end_reached_keeps_actual_judge_failure_authoritative_and_acquisition_committed() {
    let (mut runtime, mut consumer) = fixture(4);
    runtime
        .set_song_end(Timestamp::from_nanos(2_000_000))
        .unwrap();
    // Caller-mutated judge chronology still requires explicit restoration;
    // the logical fence cannot fabricate a successful earlier judge prefix.
    runtime
        .judge_mut()
        .advance_to(Timestamp::from_nanos(3_000_000))
        .unwrap();
    let before = runtime.judge().stable_hash().unwrap();
    let report = runtime
        .process_input(input(1, 2_000_000, 7), &Clocks, point(3, 0))
        .unwrap();
    assert!(report.song_end_reached);
    assert!(report.judge_error.is_some());
    assert!(report.input.is_none());
    assert!(report.judge_events.is_empty());
    assert_eq!(runtime.judge().stable_hash().unwrap(), before);
    assert_eq!(runtime.telemetry().counters().inputs, 1);
    assert_eq!(runtime.telemetry().counters().unbound, 0);
    assert_eq!(runtime.telemetry().counters().rejected, 1);
    assert!(consumer.try_pop().is_err());
    assert!(matches!(
        runtime.process_input(input(1, 2_000_000, 6), &Clocks, point(3, 0)),
        Err(RuntimeError::SequenceRegression { .. })
    ));
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
    assert!(
        runtime
            .process_input(input(2, 2_000_000, 1), &Clocks, point(3, 5_000_000))
            .unwrap()
            .judge_events
            .is_empty()
    );
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

#[test]
fn explicit_audio_interleaves_with_hits_without_mutating_gameplay_or_input_chronology() {
    let (mut runtime, mut consumer) = fixture(3);
    let background = AudioCommand::Play {
        voice: VoiceId(99),
        sample: SampleId(7),
        at: Timestamp::from_nanos(9_000_000),
        gain: 0.25,
    };
    let initial = runtime.judge().stable_hash().unwrap();
    runtime.enqueue_audio(background).unwrap();
    assert_eq!(runtime.judge().stable_hash().unwrap(), initial);
    let report = runtime
        .process_input(input(1, 2_000_000, 0), &Clocks, point(3, 3_000_000))
        .unwrap();
    assert!(report.judge_error.is_none());
    assert_eq!(consumer.try_pop().unwrap(), background);
    assert_eq!(consumer.try_pop().unwrap(), report.audio_commands[0]);
    let stop = AudioCommand::Stop {
        voice: VoiceId(99),
        at: Timestamp::from_nanos(-1),
    };
    let after_hit = runtime.judge().stable_hash().unwrap();
    runtime.enqueue_audio(stop).unwrap();
    assert_eq!(runtime.judge().stable_hash().unwrap(), after_hit);
    assert_eq!(consumer.try_pop().unwrap(), stop); // Output times are not host times.
    assert!(matches!(
        runtime.process_input(input(2, 1_000_000, 0), &Clocks, point(3, 4_000_000)),
        Err(RuntimeError::NonMonotonicHost)
    ));
    assert_eq!(runtime.telemetry().counters().audio_commands, 3);
}

#[test]
fn explicit_audio_full_and_disconnect_return_exact_commands_and_share_counters() {
    let (mut runtime, mut consumer) = fixture(1);
    let command = AudioCommand::Stop {
        voice: VoiceId(u64::MAX),
        at: Timestamp::MAX,
    };
    let initial = runtime.judge().stable_hash().unwrap();
    runtime.enqueue_audio(command).unwrap();
    let full = runtime.enqueue_audio(command).unwrap_err();
    assert_eq!(full.command, command);
    assert_eq!(full.reason, QueuePushError::Full);
    assert_eq!(consumer.try_pop().unwrap(), command);
    drop(consumer);
    let disconnected = runtime.enqueue_audio(command).unwrap_err();
    assert_eq!(disconnected.command, command);
    assert_eq!(disconnected.reason, QueuePushError::Disconnected);
    assert_eq!(runtime.judge().stable_hash().unwrap(), initial);
    let counters = runtime.telemetry().counters();
    assert_eq!(counters.audio_commands, 1);
    assert_eq!(counters.queue_full, 1);
    assert_eq!(counters.queue_disconnected, 1);
    assert_eq!(counters.inputs, 0);
    assert_eq!(counters.judge_results, 0);
}
