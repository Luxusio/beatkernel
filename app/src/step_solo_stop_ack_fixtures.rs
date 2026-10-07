//! Deferred live-owner ACK fixtures. Typed mine setup does not admit mine files,
//! and software output observations do not prove device presentation.
use crate::{
    gauge::GaugeFailure,
    mine_audio_consumers_fixtures::data,
    replay_audio::{completed_render_cursor, ReplayAudioError},
    step_gameplay::{
        validate_section_output_evidence, StepAudioBatch, StepGameplay, StepGameplayConfig,
        StepGameplayError,
    },
};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioLimits, CommandProducer, Mixer, MixerConfig,
        QueuePushError, RenderReport, SampleBank, SampleId, VoiceId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};

pub(super) const SECOND: i64 = 1_000_000_000;
const HOST: i64 = 10 * SECOND;
const OUTPUT: i64 = SECOND;
const TEXT: &str =
    "#BPM 60\n#WAV01 note\n#WAV02 press\n#00011:01000100\n#00031:02\n#000D1:00ZZ0000";
pub(super) fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
pub(super) fn host(song: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(1),
        timestamp: ts(HOST + song),
    }
}
pub(super) fn output(elapsed: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(2),
        timestamp: ts(OUTPUT + elapsed),
    }
}
pub(super) struct Identity;
impl ClockMapper for Identity {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
pub(super) fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: host(0),
        output_origin: output(0),
        preroll: Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 16,
        bgm_pending: 1,
        bgm_lookahead: Duration::from_nanos(SECOND),
        telemetry_capacity: 0,
    }
}
pub(super) fn bindings(source: DeviceSelector) -> BindingMap {
    BindingMap::from_bindings([Binding {
        device: source,
        physical: PhysicalControlId::keyboard(91u16),
        game_control: GameControlId(0x11),
    }])
    .unwrap()
}
pub(super) fn input(
    source: u64,
    song: i64,
    sequence: u64,
    state: ButtonState,
) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(source), host(song), sequence),
        control: PhysicalControlId::keyboard(91u16),
        state,
    })
}
pub(super) fn play(sample: u64, voice: u64, elapsed: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(sample),
        voice: VoiceId(voice),
        at: output(elapsed).timestamp,
        gain,
    }
}
pub(super) fn stop(voice: u64) -> AudioCommand {
    AudioCommand::Stop {
        voice: VoiceId(voice),
        at: output(SECOND).timestamp,
    }
}
pub(super) fn mixer(
    bank: SampleBank,
    capacity: usize,
    end: Option<u64>,
) -> (CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut configuration = MixerConfig::new(
        bank.format(),
        ClockDomainId(2),
        output(0).timestamp,
        AudioLimits::new(capacity, 8, 16, 64, capacity).unwrap(),
    );
    if let Some(end) = end {
        configuration = configuration.with_playback_end_frame(end);
    }
    (producer, Mixer::new(configuration, bank, consumer).unwrap())
}
pub(super) fn rejected_output(
    error: StepGameplayError,
    raw: RenderReport,
    presented: Option<ClockPoint>,
) {
    match error {
        StepGameplayError::Completion {
            rendered,
            presented: actual,
            ..
        } => {
            assert_eq!(rendered, Some(raw));
            assert_eq!(actual, presented);
        }
        error => panic!("original output evidence lost: {error:?}"),
    }
}
pub(super) fn owner(text: &str, end: Option<i64>) -> (StepGameplay, SampleBank) {
    let (mut game, bank) = StepGameplay::new_section(
        data(text, false),
        config(),
        bindings(DeviceSelector::Any),
        ts(0),
        end.map(ts),
    )
    .unwrap();
    game.activate(host(0)).unwrap();
    (game, bank)
}
fn fatal_owner() -> (StepGameplay, SampleBank, StepAudioBatch) {
    let (mut game, bank) = owner(TEXT, None);
    let hit = game
        .process_input(
            input(u64::MAX, 0, 1, ButtonState::Down),
            &Identity,
            output(0),
        )
        .unwrap();
    assert_eq!(hit.audio_commands, [play(1, 11, 0, 1.0)]);
    let fatal = game
        .advance_to(host(SECOND), &Identity, output(SECOND))
        .unwrap();
    assert_eq!(fatal.hazard_events.len(), 1);
    assert_eq!(fatal.hazard_events[0].value, 1295);
    assert_eq!(fatal.audio_commands, [stop(11), stop(12), stop(13)]);
    assert!(fatal.audio_failures.is_empty());
    assert_eq!(game.gameplay_fence(), Some(ts(SECOND)));
    assert_eq!(
        game.gauge().snapshot().failure,
        Some(GaugeFailure::InstantDeath)
    );
    assert_eq!(
        game.acknowledged_stop_commands(),
        0,
        "Runtime admission is not a remote ACK"
    );
    let batch = game.take_commands(16).unwrap().unwrap();
    assert_eq!(
        batch.commands,
        [play(1, 11, 0, 1.0), stop(11), stop(12), stop(13)]
    );
    assert_eq!(
        game.acknowledged_stop_commands(),
        0,
        "outbound extraction is not an ACK"
    );
    (game, bank, batch)
}

#[test]
fn solo_ack_counts_only_original_accepted_stop_prefix_and_preserves_typed_rejections() {
    let (mut game, bank, batch) = fatal_owner();
    let hash = game.judge().stable_hash().unwrap();
    let (mut producer, _mixer) = mixer(bank, 8, None);
    for &command in &batch.commands {
        producer.try_push(command).unwrap();
    }
    assert_eq!(game.acknowledged_stop_commands(), 0);
    game.acknowledge(batch.sequence, 4, true).unwrap();
    assert_eq!(game.acknowledged_stop_commands(), 3);
    let later = game
        .process_input(
            input(u64::MAX, 2 * SECOND, 2, ButtonState::Up),
            &Identity,
            output(2 * SECOND),
        )
        .unwrap();
    assert!(
        later.bound_inputs.is_empty()
            && later.judge_events.is_empty()
            && later.audio_commands.is_empty()
    );
    assert_eq!(game.judge().stable_hash().unwrap(), hash);
    assert_eq!(game.gameplay_fence(), Some(ts(SECOND)));
    assert!(game.take_commands(16).unwrap().is_none());
    assert!(
        !game.failed(),
        "numeric failure is distinct from protocol failure"
    );

    for (admitted, stops) in [(1usize, 0u64), (2, 1), (4, 3)] {
        let (mut game, bank, batch) = fatal_owner();
        let hash = game.judge().stable_hash().unwrap();
        let (mut producer, _mixer) = mixer(bank, admitted, None);
        for &command in &batch.commands[..admitted] {
            producer.try_push(command).unwrap();
        }
        if admitted < 4 {
            let refusal = producer.try_push(batch.commands[admitted]).unwrap_err();
            assert_eq!(refusal.command, batch.commands[admitted]);
            assert_eq!(refusal.reason, QueuePushError::Full);
        }
        match game
            .acknowledge(batch.sequence, admitted, false)
            .unwrap_err()
        {
            StepGameplayError::AudioRejected {
                batch: actual,
                admitted: count,
            } => {
                assert_eq!(actual, batch);
                assert_eq!(count, admitted);
            }
            error => panic!("partial ACK lost original batch: {error:?}"),
        }
        assert_eq!(game.acknowledged_stop_commands(), stops);
        assert!(game.failed());
        assert_eq!(game.gameplay_fence(), Some(ts(SECOND)));
        assert_eq!(
            game.gauge().snapshot().failure,
            Some(GaugeFailure::InstantDeath)
        );
        assert_eq!(game.judge().stable_hash().unwrap(), hash);
        assert!(matches!(
            game.acknowledge(batch.sequence, admitted, false),
            Err(StepGameplayError::Failed)
        ));
        assert_eq!(game.acknowledged_stop_commands(), stops);
    }

    for case in 0..5 {
        let (mut game, _bank, retained) = if case == 0 {
            let (game, bank) = owner(TEXT, None);
            (game, bank, None)
        } else {
            let (game, bank, batch) = fatal_owner();
            (game, bank, Some(batch))
        };
        let sequence = retained.as_ref().map_or(1, |batch| batch.sequence);
        let credited = if case == 4 {
            game.acknowledge(sequence, 4, true).unwrap();
            3
        } else {
            0
        };
        let (received, count) = match case {
            1 => (sequence + 1, 4),
            2 => (sequence, 5),
            3 => (sequence, 3),
            _ => (sequence, 4),
        };
        match game.acknowledge(received, count, true).unwrap_err() {
            StepGameplayError::InvalidAcknowledgement {
                sequence,
                admitted,
                success,
                batch,
            } => {
                assert_eq!((sequence, admitted, success), (received, count, true));
                assert_eq!(batch, if case == 4 { None } else { retained });
            }
            error => panic!("invalid ACK changed its error: {error:?}"),
        }
        assert_eq!(game.acknowledged_stop_commands(), credited);
        assert!(game.failed());
    }
}

#[test]
fn solo_output_requires_ack_keeps_raw_diagnostics_and_preserves_finite_ordinary_completion() {
    for acknowledge in [false, true] {
        let (mut game, bank, batch) = fatal_owner();
        let (mut producer, mut output_mixer) = mixer(bank, 8, None);
        for &command in &batch.commands {
            producer.try_push(command).unwrap();
        }
        if acknowledge {
            game.acknowledge(batch.sequence, 4, true).unwrap();
        }
        let mut pcm = [0.0; 22];
        let raw = output_mixer.render(&mut pcm).unwrap();
        let mut expected = [0.0; 22];
        expected[0] = 1.0;
        expected[1] = -1.0;
        assert_eq!(pcm, expected);
        assert_eq!(
            (raw.counters.commands_applied, raw.counters.unknown_stops),
            (4, 3)
        );
        assert!(
            matches!(completed_render_cursor(&raw), Err(ReplayAudioError::RejectedRender(actual)) if actual == raw)
        );
        assert!(
            validate_section_output_evidence(
                output(0),
                10,
                None,
                None,
                Some(raw),
                Some(output(2_200_000_000)),
                None
            )
            .is_err()
        );
        let result = game.observe_completion(Some(raw), Some(output(2_200_000_000)));
        if acknowledge {
            assert!(
                !result.unwrap(),
                "terminal gameplay still requires a subsequent idle render"
            );
            let idle = output_mixer.render(&mut [0.0]).unwrap();
            assert!(
                game.observe_completion(Some(idle), Some(output(2_300_000_000)))
                    .unwrap()
            );
            assert_eq!(game.acknowledged_stop_commands(), 3);
            assert!(!game.failed());
        } else {
            rejected_output(result.unwrap_err(), raw, Some(output(2_200_000_000)));
            assert_eq!(game.acknowledged_stop_commands(), 0);
        }
        assert_eq!(game.song_time(), ts(SECOND));
        assert_eq!((game.score().hits, game.score().misses), (1, 0));
    }

    for case in 0..8 {
        let (mut game, bank, batch) = fatal_owner();
        let (mut producer, mut output_mixer) = mixer(bank, 8, None);
        for &command in &batch.commands {
            producer.try_push(command).unwrap();
        }
        game.acknowledge(batch.sequence, 4, true).unwrap();
        let first = output_mixer.render(&mut [0.0; 5]).unwrap();
        assert!(
            !game
                .observe_completion(Some(first), Some(output(500_000_000)))
                .unwrap()
        );
        let next = output_mixer.render(&mut [0.0; 17]).unwrap();
        let mut bad = next;
        let mut presented = output(2_200_000_000);
        match case {
            0 => bad.counters.unknown_stops = 4,
            1 => bad.counters.commands_applied = 2,
            2 => bad.counters.unknown_samples = 1,
            3 => bad.counters.pending_full = 1,
            4 => {
                bad = first;
                bad.active_voices = 1;
            }
            5 => {
                assert!(
                    game.observe_completion(Some(next), Some(presented))
                        .unwrap()
                );
                bad = output_mixer.render(&mut [0.0]).unwrap();
                bad.counters.unknown_stops = 2;
                presented = output(2_300_000_000);
            }
            6 => presented.domain = ClockDomainId(99),
            7 => bad.playback_start_frame += 1,
            _ => unreachable!(),
        }
        let hash = game.judge().stable_hash().unwrap();
        let score = game.score().clone();
        let feed = game.bgm_report();
        let gauge = game.gauge().clone();
        let error = game
            .observe_completion(Some(bad), Some(presented))
            .unwrap_err();
        rejected_output(error, bad, Some(presented));
        assert_eq!(game.song_time(), ts(SECOND));
        assert_eq!(game.judge().stable_hash().unwrap(), hash);
        assert_eq!(game.score(), &score);
        assert_eq!(game.gauge(), &gauge);
        assert_eq!(game.bgm_report(), feed);
        assert_eq!(game.acknowledged_stop_commands(), 3);
        assert!(game.failed());
    }

    // Preserve the existing finite command total for an ordinary, no-mine owner.
    for forged_total in [false, true] {
        let text = TEXT.replace("\n#000D1:00ZZ0000", "");
        let (mut game, bank) = owner(&text, Some(3 * SECOND));
        for (song, sequence, state) in [
            (0, 1, ButtonState::Down),
            (500_000_000, 2, ButtonState::Up),
            (2 * SECOND, 3, ButtonState::Down),
            (2 * SECOND, 4, ButtonState::Up),
        ] {
            game.process_input(
                input(u64::MAX, song, sequence, state),
                &Identity,
                output(song),
            )
            .unwrap();
        }
        game.advance_to(host(3 * SECOND), &Identity, output(3 * SECOND))
            .unwrap();
        let batch = game.take_commands(16).unwrap().unwrap();
        assert_eq!(
            batch.commands,
            [play(1, 11, 0, 1.0), play(1, 12, 2 * SECOND, 1.0)]
        );
        let (mut producer, mut output_mixer) = mixer(bank, 8, Some(30));
        for &command in &batch.commands {
            producer.try_push(command).unwrap();
        }
        game.acknowledge(batch.sequence, 2, true).unwrap();
        let mut pcm = [0.0; 32];
        let mut raw = output_mixer.render(&mut pcm).unwrap();
        let mut expected = [0.0; 32];
        expected[0] = 1.0;
        expected[1] = -1.0;
        expected[20] = 1.0;
        expected[21] = -1.0;
        assert_eq!(pcm, expected);
        assert_eq!(raw.playback_end_physical_frame, Some(30));
        assert_eq!(game.acknowledged_stop_commands(), 0);
        assert_eq!(raw.counters.unknown_stops, 0);
        if forged_total {
            raw.counters.commands_consumed = 3;
            raw.counters.commands_applied = 3;
            rejected_output(
                game.observe_completion(Some(raw), Some(output(3 * SECOND)))
                    .unwrap_err(),
                raw,
                Some(output(3 * SECOND)),
            );
        } else {
            assert!(
                !game
                    .observe_completion(Some(raw), Some(output(3 * SECOND - 1)))
                    .unwrap()
            );
            assert!(
                game.observe_completion(Some(raw), Some(output(3 * SECOND)))
                    .unwrap()
            );
            assert_eq!((game.score().hits, game.score().misses), (2, 0));
            assert_eq!(game.gameplay_fence(), None);
        }
    }
}
