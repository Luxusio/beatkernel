//! Actual PCM, remote Step authority, and durable replay travel together.
//! ConvertedMixer checks below prove generated correspondence, not native ACKs
//! or physical device presentation. Expected heads and phases are independent.
#[path = "support/replay_speed.rs"]
mod fixture;

use std::cmp::Ordering;

use beatkernel::{
    audio::*,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::JudgeEvent,
    replay::{
        codec::{decode_replay, encode_replay, ReplayFile},
        ReplayOperation,
    },
    time::{ClockDomainId, ClockPair, ClockPoint, Duration, ExtrapolationPolicy, Timestamp},
    transport::Rate,
};
use beatkernel_bms::{BmsChart, BmsInputMode};
use beatkernel_bms_runtime::{
    audio_authority::{
        AudioAuthority, AudioAuthorityConfig, AudioAuthorityCoordinateBasis, AudioAuthorityEpoch,
    },
    local_input::InputMerger,
    local_players::PlayerId,
    native_output_coordinate::NativeOutputCoordinate,
    practice_control::{PracticeTransactionReply, PracticeTransactionRequest},
    replay_playback::{decode_section_setup, reconstruct_section},
    replay_speed::ReplaySpeed,
    replay_speed_policy::split_schedule,
    section_start::{prepare_retained_at, prepare_section_replay},
    step_gameplay::{
        StepGameplay, StepGameplayConfig, StepRemoteHistorySnapshot, StepRetainedAdmissionKind,
        StepRetainedOptions,
    },
    step_replay::{StepReplay, StepReplayConfig},
    worklet_audio::{
        WorkletAudio, WorkletAudioBuilder, WorkletAudioConfig, WorkletRetainedConfig,
        WorkletRetainedEnvelope, WorkletRetainedEvent,
    },
    PreparedBms,
};

const PROGRAM: u64 = (1 << 53) + 2020;
const CONTEXT: u64 = (1 << 53) + 127;
const SEED: u64 = 91;
const MILLI_CHART: &str = "#BPM 60000\n#WAV01 key.wav\n#WAV02 music.wav\n\
    #00011:01\n#00112:01\n#00001:02\n";
const MICRO_CHART: &str = "#BPM 1000000\n#WAV01 key.wav\n#WAV02 music.wav\n\
    #00011:01\n#00012:00010000\n#00001:02\n";

fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(domain: u32, n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(n),
    }
}
fn raw(n: i64) -> ClockPoint {
    point(1, n)
}
fn host(n: i64) -> ClockPoint {
    point(11, 10_000_000_000 + n)
}
fn logical(n: i64) -> ClockPoint {
    point(33, 5_000_000_000 + n)
}
fn rate(n: i64, d: u64) -> Rate {
    Rate::new(n, d).unwrap()
}
fn speed(n: i64, d: u64) -> ReplaySpeed {
    ReplaySpeed::new(rate(n, d)).unwrap()
}
fn bounds() -> PracticeLimits {
    PracticeLimits::new(32, 8, 256, 32, 32).unwrap()
}
fn assets(text: &str, hz: u32) -> PreparedBms {
    let key: Vec<_> = (0..32).map(|i| 0.25 + i as f32 / 64.0).collect();
    let music: Vec<_> = (0..32).map(|i| i as f32 / 32.0).collect();
    fixture::prepared(text, hz, &[(1, &key), (2, &music)])
}
fn close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert!(
            (actual - expected).abs() < 0.000001,
            "sample{i}: {actual} != {expected}"
        );
    }
}
fn input(at: i64, key: u16, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(1), host(at), sequence),
        control: PhysicalControlId::keyboard(key),
        state: ButtonState::Down,
    })
}
fn retained_history(audio: &WorkletAudio) -> StepRemoteHistorySnapshot {
    let h = audio.source_playback_history().unwrap();
    StepRemoteHistorySnapshot {
        program_id: PROGRAM,
        context_start: CONTEXT,
        source_hz: h.source_rate(),
        initial_physical: h.initial_physical_frame(),
        initial_playback: h.initial_playback_frame(),
        retained_start: h.retained_start_frame(),
        pulled_end: h.pulled_end_frame(),
        retirement_revision: h.retirement_revision(),
        spans: (0..h.len()).map(|i| h.span(i).unwrap()).collect(),
    }
}

struct Live {
    step: StepGameplay,
    audio: WorkletAudio,
    merger: InputMerger,
    source: BmsChart,
    hz: u32,
    frames: u64,
    events: Vec<JudgeEvent>,
}
impl Live {
    fn new(text: &str, hz: u32, preroll: i64) -> Self {
        let prepared = assets(text, hz);
        let source = prepared.source.clone();
        let config = StepGameplayConfig {
            host_origin: host(0),
            output_origin: raw(0),
            preroll: Duration::from_nanos(preroll),
            early_ns: 0,
            late_ns: 0,
            offset_ns: 0,
            command_capacity: 64,
            bgm_pending: 8,
            bgm_lookahead: Duration::from_nanos(1_000_000_000),
            telemetry_capacity: 32,
        };
        let authority = AudioAuthority::new_with_coordinates(
            AudioAuthorityConfig {
                history_capacity: 64,
                max_observation_age: Duration::from_nanos(1_000_000_000),
                input_extrapolation: ExtrapolationPolicy::Forbid,
                max_input_ahead: Duration::ZERO,
            },
            AudioAuthorityEpoch {
                id: 1,
                stream_origin: raw(0),
                logical_origin: logical(0),
                host_domain: host(0).domain,
            },
            AudioAuthorityCoordinateBasis::SourceCounter {
                basis: OutputFrameBasis::new(raw(0), hz, 0).unwrap(),
                frequency: 1_000_000_000,
            },
        )
        .unwrap();
        let bindings =
            BindingMap::from_bindings([0x11, 0x12].into_iter().enumerate().map(|(i, lane)| {
                Binding {
                    device: DeviceSelector::Any,
                    physical: PhysicalControlId::keyboard(7 + i as u16),
                    game_control: GameControlId(lane),
                }
            }))
            .unwrap();
        let options = StepRetainedOptions {
            worklet: WorkletRetainedConfig {
                program_id: PROGRAM,
                initial_region: PracticeRegion::new(ts(0), ts(20_000_000), false).unwrap(),
                initial_rate: rate(1, 2),
                output_preroll: config.preroll,
                open_end: false,
                limits: bounds(),
                source_history_capacity: 64,
            },
            chart_seed: SEED,
            identity_limits: fixture::limits(),
        };
        let (mut step, bank) = StepGameplay::new_retained_audio_section(
            prepare_retained_at(prepared, ts(0)).unwrap(),
            config,
            bindings,
            Some(ts(20_000_000)),
            BmsInputMode::ButtonOnly,
            authority,
            None,
            options,
        )
        .unwrap();
        step.configure_capture(fixture::limits(), SEED).unwrap();
        let setup = step.retained_setup().unwrap();
        let mut builder = WorkletAudioBuilder::new(WorkletAudioConfig {
            format: bank.format(),
            pcm_limits: fixture::pcm_limits(),
            audio_limits: AudioLimits::new(64, 128, 64, 64, 64).unwrap(),
        })
        .unwrap();
        for (id, sample) in bank.into_samples() {
            builder
                .insert_sample(id, sample.format(), sample.samples().to_vec())
                .unwrap();
        }
        let mut audio = builder
            .finish_retained(setup.cues.clone(), setup.config)
            .unwrap();
        audio.arm(CONTEXT, CONTEXT).unwrap();
        step.activate_audio().unwrap();
        step.observe_audio_coordinate(
            1,
            ClockPair {
                source: raw(0),
                target: host(0),
            },
            NativeOutputCoordinate::Counter {
                position: 0,
                frequency: 1_000_000_000,
            },
        )
        .unwrap();
        assert!(
            step.take_commands(64).unwrap().is_none(),
            "retained cues have one PCM owner"
        );
        Self {
            step,
            audio,
            merger: InputMerger::new(host(0).domain, host(0), vec![DeviceId(1)], 32).unwrap(),
            source,
            hz,
            frames: 0,
            events: Vec::new(),
        }
    }
    fn elapsed(&self) -> i64 {
        (u128::from(self.frames) * 1_000_000_000 / u128::from(self.hz)) as i64
    }
    fn render(&mut self, frames: usize) -> Vec<f32> {
        self.audio.render(CONTEXT + self.frames, frames).unwrap();
        self.frames += frames as u64;
        let elapsed = self.elapsed();
        self.step
            .observe_audio_coordinate(
                1,
                ClockPair {
                    source: raw(elapsed),
                    target: host(elapsed),
                },
                NativeOutputCoordinate::Counter {
                    position: elapsed as u64,
                    frequency: 1_000_000_000,
                },
            )
            .unwrap();
        self.step
            .observe_retained_history(retained_history(&self.audio))
            .unwrap();
        self.audio.output()[..frames].to_vec()
    }
    fn commit_event(&mut self) -> WorkletRetainedEnvelope {
        let event = self
            .audio
            .poll_retained_event()
            .unwrap()
            .expect("actual rendered event");
        self.step.retain_output_event(event).unwrap();
        let elapsed = self.elapsed();
        self.step.record_audio_prefix(host(elapsed)).unwrap();
        assert_eq!(
            self.step
                .service_retained_event(&mut self.merger, host(elapsed), raw(elapsed))
                .unwrap(),
            Some(event.serial)
        );
        // Actual Worklet acknowledgment precedes the local serial acknowledgment.
        self.audio.acknowledge_retained_event(event.serial).unwrap();
        self.step.acknowledge_retained_event(event.serial).unwrap();
        event
    }
    fn press(&mut self, at: i64, key: u16, sequence: u64, expected_song: i64) {
        let elapsed = self.elapsed();
        self.merger
            .admit(input(at, key, sequence), host(elapsed))
            .unwrap();
        self.step.record_audio_prefix(host(elapsed)).unwrap();
        let report = self
            .step
            .process_next_audio_input(&mut self.merger, host(elapsed), raw(elapsed), None)
            .unwrap()
            .unwrap();
        assert_eq!(report.song_time, ts(expected_song));
        assert_eq!(
            report.input.as_ref().unwrap().meta().original_clock_point,
            Some(host(at))
        );
        assert_eq!(report.judge_events.len(), 1);
        self.events.extend(report.judge_events);
    }
    fn deliver_keys(&mut self) -> Vec<AudioCommand> {
        let mut sent = Vec::new();
        while let Some(batch) = self.step.take_commands(64).unwrap() {
            for &command in &batch.commands {
                self.audio.enqueue(command).unwrap();
                sent.push(command);
            }
            self.step
                .acknowledge(batch.sequence, batch.commands.len(), true)
                .unwrap();
        }
        sent
    }
    fn request_double(&mut self) {
        let cap = self.step.retained_capability().unwrap();
        let prepared = self
            .step
            .prepare_retained_transaction(
                PracticeTransactionRequest::Rate {
                    id: 1,
                    generation: cap.practice.generation,
                    expected: cap.rate,
                    requested_rate: rate(2, 1),
                },
                false,
            )
            .unwrap();
        self.audio
            .try_set_rate_next(
                1,
                cap.practice.generation,
                cap.rate.revision,
                prepared.preparation.unwrap(),
            )
            .unwrap();
        self.step
            .acknowledge_retained_admission(StepRetainedAdmissionKind::Transaction, 1, Ok(()))
            .unwrap();
        assert!(self.step.take_retained_response().is_none());
    }
    fn finish(mut self, expected_cut: (i128, u64, u64)) -> ReplayFile {
        assert_eq!(self.step.score().hits, 2);
        let effective = self.step.effective_header(PlayerId(1)).unwrap().clone();
        self.step.fail();
        let bytes = self.step.take_replay().unwrap().unwrap();
        let file = decode_replay(&bytes, fixture::limits()).unwrap();
        assert_eq!(file.header, effective);
        assert_eq!(file.header.normalized_clock, logical(0).domain);
        let (_, schedule) = split_schedule(&file.header.options).unwrap();
        assert_eq!(schedule.initial_speed(), speed(1, 2));
        assert_eq!(schedule.current_speed(), speed(2, 1));
        assert!(schedule.is_variable());
        assert_eq!(schedule.cuts().len(), 1);
        let cut = schedule.cuts()[0];
        assert_eq!(
            (cut.whole(), cut.numerator(), cut.denominator()),
            expected_cut
        );
        assert_eq!(
            decode_section_setup(&file.header.options)
                .unwrap()
                .chart_seed,
            SEED
        );
        assert_eq!(encode_replay(&file, fixture::limits()).unwrap(), bytes);
        let reconstructed =
            reconstruct_section(&self.source, file.clone(), fixture::limits()).unwrap();
        assert_eq!(reconstructed.results(), self.events);
        file
    }
}

#[test]
fn real_bgm_and_judged_key_continue_through_one_cut_then_capture_reconstructs_at_another_output_rate(
) {
    let mut live = Live::new(MILLI_CHART, 1000, 0);
    close(&live.render(1), &[0.0]);
    live.commit_event();
    // This is genuinely late acquired input: original0, received after frame1.
    live.press(0, 7, 0, 0);
    let sent = live.deliver_keys();
    assert_eq!(sent.len(), 1);
    assert!(
        matches!(sent[0], AudioCommand::Play { sample: SampleId(1), at, .. } if at == ts(1_000_000))
    );
    close(&live.render(2), &[0.265625, 0.2890625]);
    live.request_double();
    close(&live.render(4), &[0.3125, 0.40625, 0.5, 0.59375]);
    let event = live.commit_event();
    match event.event {
        WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
            event: PracticeEvent::Rate(cut),
            ..
        }) => {
            assert_eq!(
                (
                    cut.physical_frame,
                    cut.playback_frame,
                    cut.active_elapsed_nanos
                ),
                (3, 3, 3_000_000)
            );
            assert_eq!(cut.outcome, PracticeRateOutcome::Applied);
        }
        other => panic!("expected real same-attempt rate: {other:?}"),
    }
    assert!(matches!(
        live.step.take_retained_response(),
        Some(PracticeTransactionReply::Rate { result: Ok(_), .. })
    ));
    live.press(4_250_000, 8, 1, 4_000_000);
    let file = live.finish((1_500_000, 0, 1));
    let saved_inputs: Vec<_> = file
        .records
        .iter()
        .filter_map(|r| match &r.operation {
            ReplayOperation::Input(saved) => {
                Some((r.song_time, saved.physical.meta().original_clock_point))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        saved_inputs,
        [
            (ts(0), Some(host(0))),
            (ts(4_000_000), Some(host(4_250_000)))
        ]
    );

    let bytes = encode_replay(&file, fixture::limits()).unwrap();
    let prepared = prepare_section_replay(
        assets(MILLI_CHART, 1000),
        &file,
        fixture::limits(),
        fixture::pcm_limits(),
    )
    .unwrap();
    let replay_config = StepReplayConfig {
        output_origin: raw(0),
        preroll: Duration::ZERO,
        lookahead: Duration::from_nanos(1_000_000_000),
        max_pending: 64,
    };
    let (mut replay, bank) = StepReplay::new_with_rate(
        prepared,
        file.clone(),
        fixture::limits(),
        replay_config,
        rate(3, 2),
    )
    .unwrap();
    assert_eq!(replay.playback_rate(), rate(3, 2));
    let mut builder = WorkletAudioBuilder::new(WorkletAudioConfig {
        format: bank.format(),
        pcm_limits: fixture::pcm_limits(),
        audio_limits: AudioLimits::new(64, 128, 64, 64, 64).unwrap(),
    })
    .unwrap();
    for (id, sample) in bank.into_samples() {
        builder
            .insert_sample(id, sample.format(), sample.samples().to_vec())
            .unwrap();
    }
    let mut output = builder.finish().unwrap();
    let mut sent = Vec::new();
    while let Some(batch) = replay.take_commands(64).unwrap() {
        for &command in &batch.commands {
            output.enqueue(command).unwrap();
            sent.push(command);
        }
        replay
            .acknowledge(batch.sequence, batch.commands.len(), true)
            .unwrap();
    }
    assert!(matches!(sent[0], AudioCommand::SetRate { rate: actual, .. } if actual == rate(3, 2)));
    output.arm(CONTEXT, CONTEXT).unwrap();
    output.render(CONTEXT, 6).unwrap();
    replay
        .observe_output(output.report(), Some(raw(6_000_000)))
        .unwrap();
    assert_eq!(replay.song_time(), ts(9_000_000));
    assert_eq!(replay.recorded_until(), Some(ts(4_000_000)));
    assert_eq!(replay.score().hits, 2);
    assert_eq!(
        encode_replay(&file, fixture::limits()).unwrap(),
        bytes,
        "selected replay output speed does not rewrite variable source conditions"
    );
    assert_eq!(
        split_schedule(&file.header.options)
            .unwrap()
            .1
            .current_speed(),
        speed(2, 1)
    );
}

#[test]
fn odd_44100_and_48000_cuts_keep_preroll_fraction_late_inputs_pcm_and_reconstructed_history() {
    for (hz, elapsed_cut, source_cut, input_at) in [
        (44_100, 68_027, (34_012, 0, 1), 81_021),
        (48_000, 62_500, (31_248, 1, 2), 76_876),
    ] {
        let mut live = Live::new(MICRO_CHART, hz, 3);
        close(&live.render(1), &[0.0]);
        live.commit_event();
        live.press(4, 7, 0, 0);
        assert_eq!(live.deliver_keys().len(), 1);
        close(&live.render(2), &[0.25, 0.2734375]);
        live.request_double();
        close(&live.render(3), &[0.296875, 0.390625, 0.484375]);
        let actual = live.commit_event();
        match actual.event {
            WorkletRetainedEvent::Practice(ProjectedPracticeEvent {
                event: PracticeEvent::Rate(cut),
                ..
            }) => {
                assert_eq!(
                    (
                        cut.physical_frame,
                        cut.playback_frame,
                        cut.active_elapsed_nanos
                    ),
                    (3, 3, elapsed_cut)
                );
            }
            other => panic!("expected actual odd-grid rate cut: {other:?}"),
        }
        assert!(
            live.elapsed() > input_at,
            "the post-cut event is acquired late, not predicted"
        );
        live.press(input_at, 8, 1, 60_000);
        let file = live.finish(source_cut);
        let originals: Vec<_> = file
            .records
            .iter()
            .filter_map(|r| match &r.operation {
                ReplayOperation::Input(saved) => Some(saved.physical.meta().original_clock_point),
                _ => None,
            })
            .collect();
        assert_eq!(originals, [Some(host(4)), Some(host(input_at))]);
    }
}

fn exact_query(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}

#[test]
fn actual_converter_both_directions_retain_exact_fraction_and_wait_for_first_target_crossing() {
    for (
        source_hz,
        target_hz,
        prefill_frames,
        source_cut,
        source_fraction,
        target_denominator,
        elapsed,
        residual,
        middle,
        crossing,
    ) in [
        (
            44_100,
            48_000,
            1,
            2,
            (2, 121, 160),
            16_000,
            45_351,
            17_148,
            [0.2296875, 0.2696289],
            0.34433594,
        ),
        (
            48_000,
            44_100,
            2,
            3,
            (3, 13, 49),
            14_700,
            62_500,
            5_527,
            [
                0.25 + 13.0 / 147.0 * 0.0234375,
                0.2734375 + 26.0 / 147.0 * 0.0234375,
            ],
            0.296875 + 13.0 / 49.0 * 0.09375,
        ),
    ] {
        let prepared = assets(MICRO_CHART, source_hz);
        let bank = prepared.bank;
        let program = PreparedPracticeProgram::new_with_runtime_rates(
            &bank,
            vec![PracticeCue {
                voice: VoiceId(20),
                sample: SampleId(2),
                at: ts(0),
                gain: 1.0,
            }],
            bounds(),
            rate(1, 2),
            Duration::from_nanos(3),
            false,
            PROGRAM,
        )
        .unwrap();
        let (mut controller, endpoint) = practice_queue(&program).unwrap();
        let (mut producer, consumer) = command_queue(64).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                bank.format(),
                raw(0).domain,
                ts(0),
                AudioLimits::new(64, 128, 64, 64, 64).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        mixer
            .install_practice(
                program,
                endpoint,
                PracticeRegion::new(ts(0), ts(20_000_000), false).unwrap(),
            )
            .unwrap();
        // Match WorkletAudio::enqueue's actual retained pass ownership. A
        // legacy scope0 key belongs to another pass and is correctly discarded.
        producer.set_scope(CommandScope(controller.metadata().unwrap().generation));
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(10),
                sample: SampleId(1),
                at: ts((1_000_000_000u64 / u64::from(source_hz)) as i64),
                gain: 1.0,
            })
            .unwrap();
        let mut converted = ConvertedMixer::new(
            mixer,
            AudioFormat::new(target_hz, 1).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            ResampleQuality::Linear,
            32,
        )
        .unwrap();
        converted.enable_phase_history(64).unwrap();
        converted.enable_source_playback_history(64).unwrap();
        let mut first = [99.0];
        converted.render(&mut first).unwrap();
        close(&first, &[0.0]);
        let started = controller.try_pop_projected_event().unwrap();
        assert!(
            matches!(started.event, PracticeEvent::Boundary(e) if e.kind == PracticeBoundaryKind::Started)
        );
        assert_eq!(
            converted.pulled_source_frame_cursor(),
            2,
            "initial target0 needs source0 and1 in either conversion direction"
        );
        if prefill_frames == 2 {
            // Reverse conversion's target1 samples source1+13/147. Pull its
            // second endpoint before admission to exercise the intended cut3.
            let mut second = [99.0];
            converted.render(&mut second).unwrap();
            close(&second, &middle[..1]);
        }
        assert_eq!(converted.pulled_source_frame_cursor(), source_cut);
        let prepared = controller.prepare_rate(rate(2, 1)).unwrap();
        controller
            .try_set_rate_next(1, controller.metadata().unwrap().generation, &prepared)
            .unwrap();
        let mut actual_middle = [99.0; 2];
        let remaining = 3 - prefill_frames;
        converted.render(&mut actual_middle[..remaining]).unwrap();
        close(&actual_middle[..remaining], &middle[prefill_frames - 1..]);
        assert_eq!(
            converted.source_position(),
            SourcePosition {
                frame: source_fraction.0,
                numerator: source_fraction.1,
                denominator: source_fraction.2,
            }
        );
        assert_eq!(
            controller.try_pop_projected_event(),
            Err(PracticeError::Empty),
            "source lookahead is not a generated target crossing"
        );
        let mut actual_crossing = [99.0];
        let report = converted.render(&mut actual_crossing).unwrap();
        close(&actual_crossing, &[crossing]);
        let actual = controller.try_pop_projected_event().unwrap();
        assert_eq!(
            (
                actual.target_frame,
                actual.target_rate,
                actual.boundary.source_frame
            ),
            (3, target_hz, source_cut)
        );
        assert_eq!(
            (
                actual.boundary.target_time.seconds(),
                actual.boundary.target_time.numerator(),
                actual.boundary.target_time.denominator()
            ),
            (0, 1, target_denominator)
        );
        match actual.event {
            PracticeEvent::Rate(cut) => {
                assert_eq!(
                    (cut.physical_frame, cut.active_elapsed_nanos),
                    (source_cut, elapsed)
                );
                assert_eq!(cut.outcome, PracticeRateOutcome::Applied);
            }
            other => panic!("expected actual converted rate evidence: {other:?}"),
        }
        let phase = converted.phase_history().unwrap().span(0).unwrap();
        let cut_query = exact_query(TargetTime::new(0, source_cut, u64::from(source_hz)).unwrap());
        let previous = exact_query(TargetTime::new(0, 2, u64::from(target_hz)).unwrap());
        let crossing_query = exact_query(actual.boundary.target_time);
        assert_eq!(
            phase
                .compare_source_frame_at_interpolated_time(
                    previous,
                    report.target_end_time,
                    source_cut
                )
                .unwrap(),
            Ordering::Less
        );
        assert_eq!(
            phase
                .compare_source_frame_at_interpolated_time(
                    cut_query,
                    report.target_end_time,
                    source_cut
                )
                .unwrap(),
            Ordering::Equal
        );
        assert_eq!(
            phase
                .compare_source_frame_at_interpolated_time(
                    crossing_query,
                    report.target_end_time,
                    source_cut
                )
                .unwrap(),
            Ordering::Greater
        );
        let history = converted.source_playback_history().unwrap();
        let playback = history
            .lookup_at_interpolated_time(phase, cut_query, report.target_end_time)
            .unwrap();
        assert_eq!(
            phase
                .playback_elapsed_nanos_at_interpolated_time(
                    cut_query,
                    report.target_end_time,
                    playback,
                    0
                )
                .unwrap(),
            elapsed
        );
        assert_eq!(
            phase
                .point(actual.boundary.target_time, report.target_end_time)
                .unwrap()
                .elapsed_nanos_since(source_cut)
                .unwrap(),
            residual
        );
    }
}
