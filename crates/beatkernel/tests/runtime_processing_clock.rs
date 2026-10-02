use beatkernel::{
    audio::{
        AudioFormat, AudioLimits, CommandConsumer, Mixer, MixerConfig, PcmLimits, PcmSample,
        QueuePushError, SampleBank, SampleId, VoiceId, command_queue,
    },
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    interaction::InstantEvaluator,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow, Rule},
    replay::{REPLAY_VERSION, ReplayHeader, ReplayRecorder},
    runtime::{Runtime, RuntimeError, RuntimeProcessingClock, RuntimeReport, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use std::{cell::RefCell, collections::VecDeque};

thread_local! {
    static READINGS: RefCell<VecDeque<Option<u64>>> = const { RefCell::new(VecDeque::new()) };
}
fn script(readings: &[Option<u64>]) {
    READINGS.with(|queue| *queue.borrow_mut() = readings.iter().copied().collect());
}
fn scripted_clock() -> Option<u64> {
    READINGS.with(|queue| {
        queue
            .borrow_mut()
            .pop_front()
            .expect("unexpected clock read")
    })
}
fn absent_clock() -> Option<u64> {
    None
}
fn remaining_readings() -> usize {
    READINGS.with(|queue| queue.borrow().len())
}
fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
struct ExplicitDomains;
impl ClockMapper for ExplicitDomains {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}
fn fixture(capacity: usize) -> (Runtime, CommandConsumer) {
    let mut source = SourceChart::new(1000, Bpm::new(60, 1).unwrap()).unwrap();
    for id in 1..=3 {
        source.objects.push(SourceObject {
            id: ObjectId(id),
            start: Beat::new(2).unwrap(),
            end: None,
            interaction: InteractionId(id as u32),
            visual: VisualId(id as u32),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
    }
    let judge = JudgeEngine::new(
        source.compile().unwrap(),
        (1..=3)
            .map(|id| Rule {
                interaction: InteractionId(id),
                control: GameControlId(id),
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
    let bindings = BindingMap::from_bindings((1..=3).map(|id| Binding {
        device: DeviceSelector::Exact(DeviceId(u64::from(id))),
        physical: PhysicalControlId::keyboard(4),
        game_control: GameControlId(id),
    }))
    .unwrap();
    let (producer, consumer) = command_queue(capacity).unwrap();
    let sounds = (1..=3)
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
            ClockDomainId(1),
            ClockDomainId(2),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
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
fn input(device: u64, nanos: i64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(device), point(1, nanos), 1),
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    })
}
fn advance(runtime: &mut Runtime, nanos: i64) -> Result<RuntimeReport, RuntimeError> {
    runtime.advance_to(point(1, nanos), &ExplicitDomains, point(2, 3_000_000))
}
fn same_report(left: &RuntimeReport, right: &RuntimeReport) {
    assert_eq!(left.input, right.input);
    assert_eq!(left.bound_inputs, right.bound_inputs);
    assert_eq!(left.song_time, right.song_time);
    assert_eq!(left.song_end_reached, right.song_end_reached);
    assert_eq!(left.audio_at, right.audio_at);
    assert_eq!(left.input_mapping_quality, right.input_mapping_quality);
    assert_eq!(left.audio_mapping_quality, right.audio_mapping_quality);
    assert_eq!(left.judge_events, right.judge_events);
    assert_eq!(left.judge_error, right.judge_error);
    assert_eq!(left.audio_commands, right.audio_commands);
    assert_eq!(left.audio_failures, right.audio_failures);
}
fn replay(reports: &[RuntimeReport]) -> Vec<beatkernel::replay::ReplayRecord> {
    let mut recorder = ReplayRecorder::new(ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity: b"processing-clock-fixture".to_vec(),
        rules_identity: b"instant".to_vec(),
        options: vec![],
        seed: 0,
        normalized_clock: ClockDomainId(1),
    })
    .unwrap();
    for report in reports {
        recorder.record_report(report).unwrap();
    }
    recorder.into_parts().1
}

#[test]
fn clock_selection_preserves_actual_reports_replay_judge_queue_failure_and_pcm() {
    let mut baseline = None;
    for selection in [
        None,
        Some(RuntimeProcessingClock::External(scripted_clock)),
        Some(RuntimeProcessingClock::Disabled),
    ] {
        script(&[
            Some(1),
            Some(11),
            Some(20),
            Some(30),
            Some(40),
            Some(50),
            Some(60),
            Some(70),
        ]);
        let (mut runtime, consumer) = fixture(1);
        if let Some(clock) = selection {
            runtime.set_processing_clock(clock);
        }
        let first = runtime
            .process_input(input(1, 2_000_000), &ExplicitDomains, point(2, 3_000_000))
            .unwrap();
        let second = runtime
            .process_input(input(2, 2_000_000), &ExplicitDomains, point(2, 3_000_000))
            .unwrap();
        let deadline = advance(&mut runtime, 2_000_001).unwrap();
        assert_eq!(
            advance(&mut runtime, 2_000_000).unwrap_err(),
            RuntimeError::NonMonotonicHost
        );
        assert_eq!(first.audio_commands.len(), 1);
        assert_eq!(second.judge_events.len(), 1);
        assert!(second.audio_commands.is_empty());
        assert_eq!(second.audio_failures.len(), 1);
        assert_eq!(second.audio_failures[0].reason, QueuePushError::Full);
        assert_eq!(deadline.judge_events.len(), 1);
        let reports = vec![first, second, deadline];
        let hash = runtime.judge().stable_hash().unwrap();
        let counters = runtime.telemetry().counters();
        assert_eq!(
            (
                counters.inputs,
                counters.judge_results,
                counters.audio_commands,
                counters.queue_full,
                counters.rejected
            ),
            (2, 3, 1, 1, 1)
        );
        match selection {
            Some(RuntimeProcessingClock::Disabled) => {
                assert_eq!(runtime.telemetry().processing(), None)
            }
            Some(RuntimeProcessingClock::External(_)) => {
                let timing = runtime.telemetry().processing().unwrap();
                assert_eq!((timing.samples, timing.p50_ns, timing.max_ns), (4, 10, 10));
                assert_eq!(remaining_readings(), 0);
            }
            _ => assert_eq!(runtime.telemetry().processing().unwrap().samples, 4),
        }
        let format = AudioFormat::new(1000, 1).unwrap();
        let limits = PcmLimits::new(32, 32, 1).unwrap();
        let mut bank = SampleBank::new(format, limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25, 0.5], limits).unwrap(),
        )
        .unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(2),
                Timestamp::ZERO,
                AudioLimits::new(1, 1, 1, 8, 1).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        let mut pcm = [9.0; 6];
        mixer.render(&mut pcm).unwrap();
        assert_eq!(pcm, [0.0, 0.0, 0.0, 0.25, 0.5, 0.0]);
        let records = replay(&reports);
        if let Some((old_reports, old_hash, old_counters, old_records)) = &baseline {
            for (left, right) in reports.iter().zip(old_reports) {
                same_report(left, right);
            }
            assert_eq!(&hash, old_hash);
            assert_eq!(&counters, old_counters);
            assert_eq!(&records, old_records);
        } else {
            baseline = Some((reports, hash, counters, records));
        }
    }
}

#[test]
fn external_unknown_and_regressed_readings_omit_samples_but_valid_zero_is_retained() {
    let (mut runtime, _consumer) = fixture(4);
    runtime.set_processing_clock(RuntimeProcessingClock::External(absent_clock));
    advance(&mut runtime, 0).unwrap();
    assert_eq!(runtime.telemetry().processing(), None);
    runtime.set_processing_clock(RuntimeProcessingClock::External(scripted_clock));
    for readings in [[Some(5), None], [Some(9), Some(8)]] {
        script(&readings);
        advance(&mut runtime, 0).unwrap();
        assert_eq!(runtime.telemetry().processing(), None);
    }
    script(&[Some(7), Some(7)]);
    advance(&mut runtime, 0).unwrap();
    let zero = runtime.telemetry().processing().unwrap();
    assert_eq!(
        (zero.samples, zero.p50_ns, zero.p95_ns, zero.max_ns),
        (1, 0, 0, 0)
    );
    script(&[Some(0), Some(u64::MAX)]);
    advance(&mut runtime, 0).unwrap();
    let full_span = runtime.telemetry().processing().unwrap();
    assert_eq!((full_span.samples, full_span.max_ns), (2, u64::MAX));
    assert_eq!(runtime.telemetry().counters().rejected, 0);
}

#[test]
fn disabled_and_unknown_clocks_keep_rejection_evidence_without_mutating_judge_or_queue() {
    let (mut runtime, mut consumer) = fixture(4);
    for clock in [
        RuntimeProcessingClock::Disabled,
        RuntimeProcessingClock::External(absent_clock),
    ] {
        runtime.set_processing_clock(clock);
        let before = runtime.judge().stable_hash().unwrap();
        assert!(matches!(
            runtime.process_input(input(1, 2_000_000), &ExplicitDomains, point(99, 3_000_000)),
            Err(RuntimeError::UnmappedClock { .. })
        ));
        assert_eq!(runtime.judge().stable_hash().unwrap(), before);
        assert!(consumer.try_pop().is_err());
        assert_eq!(runtime.telemetry().processing(), None);
    }
    assert_eq!(runtime.telemetry().counters().rejected, 2);
    assert_eq!(runtime.telemetry().counters().inputs, 0);
    runtime.set_processing_clock(RuntimeProcessingClock::External(scripted_clock));
    script(&[Some(10), None]);
    assert!(advance(&mut runtime, -1).is_err());
    assert_eq!(runtime.telemetry().counters().rejected, 3);
    assert_eq!(runtime.telemetry().processing(), None);
    script(&[Some(100), Some(109)]);
    let report = runtime
        .process_input(input(1, 2_000_000), &ExplicitDomains, point(2, 3_000_000))
        .unwrap();
    assert_eq!(report.audio_commands.len(), 1);
    assert_eq!(consumer.try_pop().unwrap(), report.audio_commands[0]);
    assert_eq!(runtime.telemetry().processing().unwrap().p50_ns, 9);
}

#[test]
fn switching_clocks_retains_measurements_and_does_not_reset_forward_chronology() {
    let (mut runtime, _consumer) = fixture(4);
    runtime.set_processing_clock(RuntimeProcessingClock::External(scripted_clock));
    script(&[Some(10), Some(21)]);
    advance(&mut runtime, 10).unwrap();
    let first = runtime.telemetry().processing().unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    script(&[Some(100), Some(123)]);
    advance(&mut runtime, 11).unwrap();
    assert_eq!(runtime.telemetry().processing(), Some(first));
    assert_eq!(remaining_readings(), 2);
    runtime.set_processing_clock(RuntimeProcessingClock::External(scripted_clock));
    advance(&mut runtime, 12).unwrap();
    let retained = runtime.telemetry().processing().unwrap();
    assert_eq!(
        (retained.samples, retained.p50_ns, retained.max_ns),
        (2, 11, 23)
    );
    runtime.set_processing_clock(RuntimeProcessingClock::Native);
    assert_eq!(
        advance(&mut runtime, 11).unwrap_err(),
        RuntimeError::NonMonotonicHost
    );
    assert_eq!(runtime.telemetry().processing().unwrap().samples, 3);
    assert_eq!(runtime.telemetry().counters().rejected, 1);
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let last = runtime.telemetry().processing();
    advance(&mut runtime, 2_000_001).unwrap();
    assert_eq!(runtime.telemetry().counters().judge_results, 3);
    assert_eq!(runtime.telemetry().processing(), last);
}
