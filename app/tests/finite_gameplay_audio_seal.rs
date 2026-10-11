//! Component owners and real software Mixer; not native presentation or device proof.
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, CommandScope, Mixer, MixerConfig,
        PcmLimits, PcmSample, SampleBank, SampleId, VoiceId,
    },
    chart::*,
    input::*,
    interaction::PressInstantEvaluator,
    judge::*,
    replay::{codec::ReplayCodecLimits, ReplaySession},
    runtime::{
        hazard_sound::{HazardSoundBinding, HazardSoundTimeline},
        input_sound::{InputSoundMarker, InputSoundTimeline},
        RuntimeProcessingClock, SoundBinding,
    },
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms_runtime::{
    gauge::{BmsGauge, GaugeProfile},
    local_players::PlayerId,
    local_runtime::{InputResult, MemberConfig, RuntimeGroup, SoloRuntime},
    replay_capture::LiveReplayCapture,
};

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
fn transport() -> Transport {
    Transport::new(ts(0), ts(0), Rate::NORMAL)
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
fn bindings(device: DeviceSelector) -> BindingMap {
    BindingMap::from_bindings([Binding {
        device,
        physical: PhysicalControlId::keyboard(4),
        game_control: GameControlId(1),
    }])
    .unwrap()
}
fn sound(player: u32) -> SoundBinding {
    SoundBinding {
        object: ObjectId(1),
        stage: JudgeStage::Instant,
        sample: SampleId(1),
        voice: VoiceId(player as u64 * 10 + 1),
        gain: 0.25,
    }
}
fn configs(count: u32) -> Vec<MemberConfig> {
    (1..=count)
        .map(|player| MemberConfig {
            player: PlayerId(player),
            device: Some(DeviceId(player as u64)),
            bindings: bindings(DeviceSelector::Exact(DeviceId(player as u64))),
            judge: judge(),
            sounds: vec![sound(player)],
        })
        .collect()
}
fn input(player: u32, at: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(player as u64), point(1, at), sequence);
    meta.original_clock_point = Some(point(9, 100 + at));
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(4),
        state,
    })
}
fn group(count: u32) -> (RuntimeGroup, beatkernel::audio::CommandConsumer) {
    let (producer, consumer) = command_queue(512).unwrap();
    let mut group = RuntimeGroup::new(
        ClockDomainId(1),
        ClockDomainId(2),
        transport(),
        producer,
        configs(count),
        8,
        &[],
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    group.set_song_end(ts(21_000_000)).unwrap();
    group
        .configure_input_sounds(
            (1..=count)
                .map(|player| {
                    (
                        PlayerId(player),
                        InputSoundTimeline::new(
                            vec![InputSoundMarker {
                                control: GameControlId(1),
                                at: ts(0),
                                sample: SampleId(1),
                                voice: VoiceId(player as u64 * 10 + 2),
                                gain: 0.5,
                            }],
                            1,
                        )
                        .unwrap(),
                    )
                })
                .collect(),
        )
        .unwrap();
    group
        .configure_hazard_sounds(
            (1..=count)
                .map(|player| {
                    (
                        PlayerId(player),
                        HazardSoundTimeline::new(
                            vec![HazardSoundBinding {
                                hazard: HazardId(1),
                                sample: SampleId(1),
                                voice: VoiceId(player as u64 * 10 + 3),
                                gain: 0.25,
                            }],
                            1,
                        )
                        .unwrap(),
                    )
                })
                .collect(),
        )
        .unwrap();
    (group, consumer)
}
fn control() -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(1),
        voice: VoiceId(999),
        at: ts(0),
        gain: 0.125,
    }
}
fn capture(judge: &JudgeEngine) -> LiveReplayCapture {
    LiveReplayCapture::new_section(
        judge,
        ClockDomainId(1),
        ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 4096).unwrap()).unwrap(),
        ts(0),
        0,
        Some(ts(21_000_000)),
    )
    .unwrap()
}
fn gauge() -> BmsGauge {
    BmsGauge::new(
        GaugeProfile::new(20_000_000, 80_000_000, 1_000_000, -1_000_000, false, vec![]).unwrap(),
    )
}

#[test]
fn all_members_keep_original_input_mine_gauge_and_capture_after_actual_source_seal() {
    for count in [2, 4, 64] {
        let (mut sealed, consumer) = group(count);
        let (mut normal, mut normal_consumer) = group(count);
        let mut quiet_captures = (1..=count)
            .map(|player| capture(sealed.member_judge(PlayerId(player)).unwrap()))
            .collect::<Vec<_>>();
        let mut normal_captures = (1..=count)
            .map(|player| capture(normal.member_judge(PlayerId(player)).unwrap()))
            .collect::<Vec<_>>();
        let mut gauges = (1..=count).map(|_| gauge()).collect::<Vec<_>>();
        sealed.enqueue_audio(control()).unwrap();
        let format = AudioFormat::new(1000, 1).unwrap();
        let pcm = PcmLimits::new(256, 256, 1).unwrap();
        let mut bank = SampleBank::new(format, pcm).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![1.0; 64], pcm).unwrap(),
        )
        .unwrap();
        let mut output = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(2),
                ts(0),
                AudioLimits::new(512, 256, 512, 128, 512).unwrap(),
            )
            .with_playback_end_frame(21),
            bank,
            consumer,
        )
        .unwrap();
        let mut rendered_samples = [7.0; 25];
        let actual = output.render(&mut rendered_samples).unwrap();
        assert_eq!(actual.playback_frames, 21);
        assert_eq!(actual.playback_end_physical_frame, Some(21));
        assert_eq!(&rendered_samples[..21], &[0.125; 21]);
        assert_eq!(&rendered_samples[21..], &[0.0; 4]);
        assert_eq!(actual.counters.commands_applied, 1);
        sealed.seal_gameplay_audio();
        sealed.seal_gameplay_audio();
        assert!(sealed.gameplay_audio_sealed());
        assert!(!normal.gameplay_audio_sealed());
        for (at, sequence, state) in [
            (6_000_000, 1, ButtonState::Down),
            (13_000_000, 2, ButtonState::Up),
        ] {
            if sequence == 2 {
                let quiet = sealed
                    .advance_to(point(1, 12_000_000), &Identity, point(2, 70_000_000))
                    .unwrap();
                let ordinary = normal
                    .advance_to(point(1, 12_000_000), &Identity, point(2, 70_000_000))
                    .unwrap();
                assert_eq!(quiet.len(), count as usize);
                for (q, n) in quiet.iter().zip(&ordinary) {
                    let index = q.player.0 as usize - 1;
                    assert_eq!(q.player, n.player);
                    assert_eq!(q.report.hazard_events, n.report.hazard_events);
                    assert_eq!(q.report.hazard_events.len(), 1);
                    assert_eq!(q.report.hazard_events[0].outcome, HazardOutcome::Triggered);
                    assert_eq!(q.report.audio_at, point(2, 70_000_000));
                    assert!(
                        q.report.audio_commands.is_empty() && q.report.audio_failures.is_empty()
                    );
                    assert_eq!(n.report.audio_commands.len(), 1);
                    gauges[index]
                        .observe(&q.report.judge_events, &q.report.hazard_events)
                        .unwrap();
                    assert_eq!(gauges[index].snapshot().level_units, 20_500_000);
                    quiet_captures[index].record_report(&q.report).unwrap();
                    normal_captures[index].record_report(&n.report).unwrap();
                }
            }
            for player in 1..=count {
                let event = input(player, at, sequence, state);
                let raw = point(2, 60_000_000 + sequence as i64);
                let InputResult::Processed(quiet) =
                    sealed.process_input(event.clone(), &Identity, raw).unwrap()
                else {
                    panic!("assigned player ignored")
                };
                let InputResult::Processed(ordinary) =
                    normal.process_input(event.clone(), &Identity, raw).unwrap()
                else {
                    panic!("assigned player ignored")
                };
                assert_eq!(quiet.len(), 1);
                assert_eq!(ordinary.len(), 1);
                let q = &quiet[0].report;
                let n = &ordinary[0].report;
                assert_eq!(quiet[0].player, PlayerId(player));
                assert_eq!(q.song_time, ts(at));
                assert!(!q.song_end_reached);
                assert_eq!(q.audio_at, raw);
                assert_eq!(q.bound_inputs, n.bound_inputs);
                assert_eq!(q.bound_inputs[0].physical.meta(), event.meta());
                assert_eq!(q.judge_events, n.judge_events);
                assert_eq!(q.hazard_events, n.hazard_events);
                assert!(q.audio_commands.is_empty() && q.audio_failures.is_empty());
                let index = player as usize - 1;
                gauges[index]
                    .observe(&q.judge_events, &q.hazard_events)
                    .unwrap();
                quiet_captures[index].record_report(q).unwrap();
                normal_captures[index].record_report(n).unwrap();
                if sequence == 1 {
                    assert_eq!(q.judge_events.len(), 1);
                    assert_eq!(n.audio_commands.len(), 1);
                    assert_eq!(gauges[index].snapshot().level_units, 21_000_000);
                }
                assert_eq!(
                    sealed
                        .member_judge(PlayerId(player))
                        .unwrap()
                        .stable_hash()
                        .unwrap(),
                    normal
                        .member_judge(PlayerId(player))
                        .unwrap()
                        .stable_hash()
                        .unwrap()
                );
            }
        }
        for (index, (quiet, ordinary)) in
            quiet_captures.into_iter().zip(normal_captures).enumerate()
        {
            assert_eq!(quiet.encoded_bytes(), ordinary.encoded_bytes());
            let quiet = quiet.into_file();
            let ordinary = ordinary.into_file();
            assert_eq!(quiet.header, ordinary.header);
            assert_eq!(quiet.records, ordinary.records);
            let replay = ReplaySession::from_records(quiet.header, judge(), quiet.records).unwrap();
            assert_eq!(
                replay.engine().stable_hash().unwrap(),
                sealed
                    .member_judge(PlayerId(index as u32 + 1))
                    .unwrap()
                    .stable_hash()
                    .unwrap()
            );
            let counters = sealed
                .member_telemetry(PlayerId(index as u32 + 1))
                .unwrap()
                .counters();
            assert_eq!(counters.inputs, 2);
            assert_eq!(counters.audio_commands, 0);
            sealed.fence_player(PlayerId(index as u32 + 1)).unwrap();
            assert!(sealed
                .fence_player_sounds(PlayerId(index as u32 + 1), ts(80_000_000))
                .unwrap()
                .is_none());
        }
        let mut commands = 0;
        while normal_consumer.try_pop().is_ok() {
            commands += 1;
        }
        assert_eq!(commands, count * 2);
        let mut tail = [9.0; 5];
        let after = output.render(&mut tail).unwrap();
        assert_eq!(tail, [0.0; 5]);
        assert_eq!(after.playback_frames, 0);
        assert_eq!(after.counters.commands_applied, 1);
    }
}

#[test]
fn solo_seal_does_not_erase_queued_credit_and_only_valid_fresh_epoch_reopens_it() {
    let (producer, mut consumer) = command_queue(16).unwrap();
    let mut solo = SoloRuntime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        transport(),
        bindings(DeviceSelector::Any),
        judge(),
        producer,
        vec![sound(1)],
        8,
    )
    .unwrap();
    solo.set_processing_clock(RuntimeProcessingClock::Disabled);
    let first = solo
        .process_input(
            input(1, 6_000_000, 1, ButtonState::Down),
            &Identity,
            point(2, 6_000_000),
        )
        .unwrap();
    assert_eq!(first.audio_commands.len(), 1);
    solo.seal_gameplay_audio();
    solo.seal_gameplay_audio();
    assert!(solo.gameplay_audio_sealed());
    assert_eq!(consumer.try_pop().unwrap(), first.audio_commands[0]);
    let before = solo.judge().stable_hash().unwrap();
    assert!(solo
        .replace_practice_state(judge(), transport(), Some(ts(21_000_000)), CommandScope(0))
        .is_err());
    assert!(solo.gameplay_audio_sealed());
    assert_eq!(solo.judge().stable_hash().unwrap(), before);
    solo.enqueue_audio(control()).unwrap();
    assert_eq!(consumer.try_pop().unwrap(), control());
    solo.replace_practice_state(judge(), transport(), Some(ts(21_000_000)), CommandScope(1))
        .unwrap();
    assert!(!solo.gameplay_audio_sealed());
    let fresh = solo
        .process_input(
            input(1, 6_000_000, 1, ButtonState::Down),
            &Identity,
            point(2, 6_000_000),
        )
        .unwrap();
    assert_eq!(fresh.judge_events.len(), 1);
    assert_eq!(fresh.audio_commands.len(), 1);
    assert_eq!(consumer.try_pop().unwrap(), fresh.audio_commands[0]);
    assert!(consumer.try_pop().is_err());
}

#[test]
fn refused_cohort_replacement_keeps_every_seal_and_existing_judge_hash() {
    for count in [2, 4, 64] {
        let (mut group, mut consumer) = group(count);
        group.seal_gameplay_audio();
        let hashes = (1..=count)
            .map(|player| {
                group
                    .member_judge(PlayerId(player))
                    .unwrap()
                    .stable_hash()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let mismatched = (1..=count)
            .rev()
            .map(|player| (PlayerId(player), judge()))
            .collect();
        assert!(group
            .replace_practice_states(
                mismatched,
                transport(),
                Some(ts(21_000_000)),
                CommandScope(1)
            )
            .is_err());
        assert!(group.gameplay_audio_sealed());
        for (player, hash) in (1..=count).zip(hashes) {
            assert_eq!(
                group
                    .member_judge(PlayerId(player))
                    .unwrap()
                    .stable_hash()
                    .unwrap(),
                hash
            );
        }
        assert!(consumer.try_pop().is_err());
        group
            .replace_practice_states(
                (1..=count)
                    .map(|player| (PlayerId(player), judge()))
                    .collect(),
                transport(),
                Some(ts(21_000_000)),
                CommandScope(1),
            )
            .unwrap();
        assert!(!group.gameplay_audio_sealed());
        for player in 1..=count {
            let InputResult::Processed(report) = group
                .process_input(
                    input(player, 6_000_000, 1, ButtonState::Down),
                    &Identity,
                    point(2, 6_000_000),
                )
                .unwrap()
            else {
                panic!("player ignored")
            };
            assert_eq!(report[0].report.audio_commands.len(), 1);
        }
        let mut admitted = 0;
        while consumer.try_pop().is_ok() {
            admitted += 1;
        }
        assert_eq!(admitted, count);
    }
}
