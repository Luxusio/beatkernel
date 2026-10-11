//! Independent literal expectations for the opt-in retained player workload.
#[path = "../examples/cohort_transition_stress/workload.rs"]
mod workload;

use beatkernel::{
    audio::{command_queue, AudioCommand, CommandScope, QueuePushError},
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    interaction::InstantEvaluator,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow, Rule},
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms_runtime::{
    local_input::{InputMerger, MergeError},
    local_players::PlayerId,
    local_runtime::{MemberConfig, RuntimeGroup},
};

#[test]
fn every_option_is_independently_bounded_before_allocation() {
    let defaults = workload::Options::default();
    assert_eq!(
        (defaults.members, defaults.cycles, defaults.notes_per_member),
        (2, 8, 8)
    );
    for members in [1, 2, 3, 4, 64] {
        for cycles in [1, 8, 64] {
            for notes_per_member in [1, 8, 64] {
                assert!(workload::Options {
                    members,
                    cycles,
                    notes_per_member
                }
                .validate()
                .is_ok());
            }
        }
    }
    // 64^3 is admitted exactly; every independent dimension rejects first.
    assert_eq!(64usize * 64 * 64, 262_144);
    for value in [0, 65, usize::MAX] {
        assert!(workload::Options {
            members: value,
            ..defaults
        }
        .validate()
        .is_err());
        assert!(workload::Options {
            cycles: value,
            ..defaults
        }
        .validate()
        .is_err());
        assert!(workload::Options {
            notes_per_member: value,
            ..defaults
        }
        .validate()
        .is_err());
    }
}

fn check_retained_workload(members: usize, cycles: usize, notes: usize) {
    use beatkernel::{
        input::CodecLimits,
        judge::{JudgeOutcome, JudgeStage},
        replay::{
            codec::{decode_replay, encode_replay, ReplayCodecLimits},
            ReplayOperation,
        },
    };
    use beatkernel_bms_runtime::replay_playback::{decode_section_setup, reconstruct_section};
    let report = workload::run(workload::Options {
        members,
        cycles,
        notes_per_member: notes,
    })
    .unwrap();
    assert_eq!(report.transitions, cycles - 1);
    assert_eq!(report.authored_edges, 2 * members * cycles * notes);
    assert_eq!(report.captured_input_edges, 2 * members * cycles * notes);
    assert_eq!(report.repair_records, 0);
    assert_eq!(report.captures.len(), members * cycles);
    assert_ne!(report.mixer_identity_before, 0);
    assert_eq!(report.mixer_identity_before, report.mixer_identity_after);
    assert_eq!(report.pending_commands, 0);
    assert_eq!(report.pending_inputs, 0);
    assert_eq!(report.active_voices, 0);
    let limits = ReplayCodecLimits::new(
        1_048_576,
        65_536,
        4096,
        CodecLimits::new(4096, 1024).unwrap(),
    )
    .unwrap();
    let mut advances = 0;
    for attempt in 1..=cycles {
        for member in 0..members {
            let captures: Vec<_> = report
                .captures
                .iter()
                .filter(|capture| {
                    capture.attempt as usize == attempt - 1
                        && capture.player == PlayerId(member as u32 + 1)
                })
                .collect();
            assert_eq!(captures.len(), 1, "one original player capture per attempt");
            let capture = captures[0];
            assert_eq!(capture.device, DeviceId(1001 + member as u64));
            let original = report
                .captures
                .iter()
                .find(|item| item.attempt == 0 && item.player == capture.player)
                .unwrap();
            assert_eq!(
                capture.file.header.chart_identity,
                original.file.header.chart_identity
            );
            assert_eq!(
                capture.file.header.rules_identity,
                original.file.header.rules_identity
            );
            assert_eq!(capture.file.header.seed, original.file.header.seed);
            let expected_path = if attempt == 1 {
                format!("member{}.bkr", member + 1)
            } else {
                format!("member{}.retry{}.bkr", member + 1, attempt - 1)
            };
            assert_eq!(
                capture.path.file_name().unwrap().to_str().unwrap(),
                expected_path
            );
            assert_eq!(capture.bytes, encode_replay(&capture.file, limits).unwrap());
            let decoded = decode_replay(&capture.bytes, limits).unwrap();
            assert_eq!(decoded.records, capture.file.records);
            assert_eq!(decoded.header, capture.file.header);
            let setup = decode_section_setup(&capture.file.header.options).unwrap();
            assert_eq!(setup.start, Timestamp::ZERO);
            assert_eq!(
                setup.end,
                Some(Timestamp::from_nanos((40 * notes as i64 + 20) * 1_000_000))
            );
            let replay = reconstruct_section(&report.source, decoded, limits).unwrap();
            assert_eq!(replay.results(), capture.reconstructed_results.as_slice());
            assert_eq!(capture.live_results, capture.reconstructed_results);
            let inputs: Vec<_> = capture
                .file
                .records
                .iter()
                .filter_map(|record| match &record.operation {
                    ReplayOperation::Input(input) => Some((record, input)),
                    ReplayOperation::Advance => {
                        advances += 1;
                        None
                    }
                })
                .collect();
            assert_eq!(inputs.len(), notes * 2);
            for pair in inputs.windows(2) {
                assert_eq!(
                    pair[1].1.physical.meta().sequence,
                    pair[0].1.physical.meta().sequence + 1
                );
                assert!(
                    pair[1]
                        .1
                        .physical
                        .meta()
                        .original_clock_point
                        .unwrap()
                        .timestamp
                        > pair[0]
                            .1
                            .physical
                            .meta()
                            .original_clock_point
                            .unwrap()
                            .timestamp
                );
            }
            for note in 0..notes {
                let (down_ms, up_ms) = if note % 2 == 0 {
                    (40 * note as i64 + 20, 40 * note as i64 + 21)
                } else {
                    (40 * note as i64 + 10, 40 * note as i64 + 30)
                };
                for (edge_index, (record, input)) in
                    inputs[note * 2..note * 2 + 2].iter().enumerate()
                {
                    assert_eq!(
                        record.song_time,
                        Timestamp::from_nanos(
                            if edge_index == 0 { down_ms } else { up_ms } * 1_000_000
                        )
                    );
                    let PhysicalInputEvent::Button(button) = &input.physical else {
                        panic!("authored button metadata retained")
                    };
                    assert_eq!(button.meta.source, capture.device);
                    assert_eq!(
                        button.meta.sequence,
                        ((attempt - 1) * 2 * notes + 2 * note + edge_index) as u64
                    );
                    assert_eq!(button.control, PhysicalControlId::keyboard(7));
                    assert_eq!(
                        button.state,
                        if edge_index == 0 {
                            ButtonState::Down
                        } else {
                            ButtonState::Up
                        }
                    );
                    assert_eq!(button.meta.clock_domain, ClockDomainId(3));
                    let start = &report.attempt_starts[attempt - 1];
                    let song_ms = if edge_index == 0 { down_ms } else { up_ms };
                    let original_ns = (start.physical_frame as i64 + song_ms) * 1_000_000;
                    assert_eq!(button.meta.original_clock_point, Some(host(original_ns)));
                    assert_eq!(
                        button.meta.timestamp,
                        Timestamp::from_nanos(5_000_000_000 + original_ns)
                    );
                }
            }
            let expected_stages: Vec<_> = (0..notes)
                .flat_map(|note| {
                    if note % 2 == 0 {
                        vec![(JudgeStage::Instant, 40 * note as i64 + 20)]
                    } else {
                        vec![
                            (JudgeStage::HoldHead, 40 * note as i64 + 10),
                            (JudgeStage::HoldTail, 40 * note as i64 + 30),
                        ]
                    }
                })
                .collect();
            assert_eq!(replay.results().len(), expected_stages.len());
            for (result, (stage, ms)) in replay.results().iter().zip(expected_stages) {
                assert_eq!(result.stage, stage);
                assert_eq!(result.at, Timestamp::from_nanos(ms * 1_000_000));
                assert_eq!(
                    result.outcome,
                    JudgeOutcome::Hit {
                        grade: JudgeGrade(1),
                        delta: Duration::ZERO
                    }
                );
                assert_eq!(result.input.unwrap().source, capture.device);
            }
        }
    }
    assert_eq!(report.advance_records, advances);
    assert!(
        advances > 0,
        "actual pump deadline advances are separately counted"
    );
    assert_eq!(report.attempt_starts.len(), cycles);
    let mut expected_pcm = vec![0.0; report.pcm.len()];
    for (index, start) in report.attempt_starts.iter().enumerate() {
        assert_eq!(start.attempt as usize, index);
        assert_eq!(start.song_start, Timestamp::ZERO);
        if index > 0 {
            assert!(start.physical_frame > report.attempt_starts[index - 1].physical_frame);
        }
        expected_pcm[start.physical_frame as usize] = 0.125;
        for note in 0..notes {
            let offsets = if note % 2 == 0 {
                vec![40 * note + 20]
            } else {
                vec![40 * note + 10, 40 * note + 30]
            };
            // The production pump schedules at the acknowledged playback end:
            // the event acquired after [song_ms-1,song_ms) renders at song_ms.
            for offset in offsets {
                expected_pcm[start.physical_frame as usize + offset] =
                    (members as f32 * 0.25).min(1.0);
            }
        }
    }
    // Complete equality also rejects the independently distinct -0.75 stale-scope sentinel.
    assert_eq!(report.pcm, expected_pcm);
    assert!(!report.pcm.contains(&-0.75));
    assert!(report
        .render_reports
        .iter()
        .any(|render| render.paused && render.frames > 0 && render.playback_frames == 0));
    use beatkernel_bms_runtime::native_gameplay_host::PauseState;
    assert!(
        report
            .pause_states
            .iter()
            .filter(|state| **state == PauseState::Paused)
            .count()
            >= cycles
    );
    assert!(
        report
            .pause_states
            .iter()
            .filter(|state| **state == PauseState::Running)
            .count()
            >= cycles
    );
    for pair in report.render_reports.windows(2) {
        assert_eq!(
            pair[1].start_frame,
            pair[0].start_frame + pair[0].frames as u64
        );
        assert!(pair[1].playback_start_frame >= pair[0].playback_start_frame);
    }
    // Mixer song_position retains an anchor, not an advancing song cursor.
    // Original captures prove the backward song boundary independently of
    // monotonically advancing physical and playback grids above.
    for member in 0..members {
        for previous_attempt in 0..cycles - 1 {
            let previous = report
                .captures
                .iter()
                .find(|capture| {
                    capture.player == PlayerId(member as u32 + 1)
                        && capture.attempt == previous_attempt
                })
                .unwrap();
            let next = report
                .captures
                .iter()
                .find(|capture| {
                    capture.player == PlayerId(member as u32 + 1)
                        && capture.attempt == previous_attempt + 1
                })
                .unwrap();
            let previous_end = previous.file.records.last().unwrap().song_time;
            assert_eq!(
                previous_end,
                Timestamp::from_nanos((40 * notes as i64 + 20) * 1_000_000)
            );
            let next_input = next
                .file
                .records
                .iter()
                .find(|record| matches!(record.operation, ReplayOperation::Input(_)))
                .unwrap();
            assert_eq!(next_input.song_time, Timestamp::from_nanos(20_000_000));
            assert!(next_input.song_time < previous_end);
            assert!(
                report.attempt_starts[previous_attempt + 1].physical_frame
                    > report.attempt_starts[previous_attempt].physical_frame
            );
            assert!(
                report.attempt_starts[previous_attempt + 1].playback_frame
                    > report.attempt_starts[previous_attempt].playback_frame
            );
        }
    }
    assert_eq!(report.generations, (1..=cycles as u64).collect::<Vec<_>>());
    assert_eq!(
        report.applied_generations,
        (2..=cycles as u64).collect::<Vec<_>>()
    );
    assert_eq!(
        report.committed_generations,
        (2..=cycles as u64).collect::<Vec<_>>()
    );
    assert_eq!(report.sentinel_injected, cycles > 1);
    let replacement = &report.replacement;
    assert_ne!(replacement.mixer_identity_before, 0);
    assert_eq!(
        replacement.mixer_identity_before,
        replacement.mixer_identity_after
    );
    assert_eq!((replacement.epoch_before, replacement.epoch_after), (0, 1));
    assert_eq!(replacement.retire_calls, 1);
    assert_eq!(replacement.open_calls, 1);
    assert!(replacement.premature_recovery_refused);
    assert!(replacement.paused_render_reports.len() >= 2);
    for render in &replacement.paused_render_reports {
        assert!(render.paused);
        assert!(render.frames > 0);
        assert_eq!(render.playback_frames, 0);
        assert_eq!(
            render.playback_start_frame,
            replacement.paused_render_reports[0].playback_start_frame
        );
    }
    assert_eq!(replacement.snapshots.len(), 2);
    let pairs: Vec<_> = replacement.snapshots.iter().map(|snapshot| {
        use beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence;
        assert_eq!(snapshot.epoch, 1);
        assert_eq!(snapshot.basis, replacement.basis);
        let OriginalNativePresentationEvidence::SuppliedPair(pair) = snapshot.evidence else {
            panic!("virtual output reports its actual supplied-pair evidence")
        };
        pair
    }).collect();
    assert!(pairs[1].source.timestamp > pairs[0].source.timestamp);
    assert!(pairs[1].target.timestamp > pairs[0].target.timestamp);
    assert!(report.replacement_refusal.contains("practice"));
    assert_eq!(report.refusal_mapping_calls, 0);
    assert_eq!(report.refusal_retire_calls, 0);
}

#[test]
fn actual_solo_and_two_three_four_sixty_four_player_pumps_retain_all_attempts() {
    for members in [1, 2, 3, 4, 64] {
        check_retained_workload(members, 3, 3);
    }
}

#[test]
fn one_instant_note_and_one_attempt_have_no_invented_hold_or_fresh_transition() {
    check_retained_workload(1, 1, 1);
    check_retained_workload(2, 2, 1);
}

#[test]
fn real_archive_and_presentation_failures_preserve_their_committed_prefix() {
    let options = workload::Options {
        members: 4,
        cycles: 3,
        notes_per_member: 3,
    };
    let archive = workload::run_fault(options, workload::Fault::Archive).unwrap();
    assert!(archive.error.contains("archive"), "{}", archive.error);
    assert_eq!(
        archive.generation, 1,
        "archive refusal precedes logical practice commit"
    );
    assert!(archive.applied_generations.is_empty());
    assert!(archive.committed_generations.is_empty());
    let presentation = workload::run_fault(options, workload::Fault::Presentation).unwrap();
    assert!(
        presentation.error.contains("presentation"),
        "{}",
        presentation.error
    );
    assert_eq!(
        presentation.generation, 2,
        "presentation refusal follows original logical commit"
    );
    assert_eq!(presentation.captures.len(), 4);
    assert_eq!(presentation.applied_generations, vec![2]);
    assert!(presentation.committed_generations.is_empty());
    for capture in &presentation.captures {
        assert_eq!(capture.attempt, 0);
        assert_eq!(
            capture
                .file
                .records
                .iter()
                .filter(|record| matches!(
                    record.operation,
                    beatkernel::replay::ReplayOperation::Input(_)
                ))
                .count(),
            6
        );
        assert_eq!(capture.reconstructed_results.len(), 4);
    }
}

#[test]
fn late_acquisition_fails_in_the_actual_pump_before_a_fresh_attempt() {
    let failure = workload::run_fault(
        workload::Options {
            members: 2,
            cycles: 3,
            notes_per_member: 3,
        },
        workload::Fault::LateFrontier,
    )
    .unwrap();
    assert!(
        failure.error.contains("input") || failure.error.contains("frontier"),
        "{}",
        failure.error
    );
    assert_eq!(failure.generation, 1);
    assert!(failure.applied_generations.is_empty());
    assert!(failure.committed_generations.is_empty());
}

#[test]
fn output_owner_refuses_unacknowledged_pause_and_expires_nonprogressing_replacement() {
    let options = workload::Options {
        members: 2,
        cycles: 2,
        notes_per_member: 1,
    };
    let unacknowledged =
        workload::run_fault(options, workload::Fault::UnacknowledgedPause).unwrap();
    assert!(
        unacknowledged.error.contains("pause") || unacknowledged.error.contains("paused"),
        "{}",
        unacknowledged.error
    );
    assert!(unacknowledged.captures.is_empty());
    assert!(unacknowledged.applied_generations.is_empty());
    assert!(unacknowledged.committed_generations.is_empty());
    let timeout = workload::run_fault(options, workload::Fault::ReplacementTimeout).unwrap();
    assert!(
        timeout.error.contains("timeout") || timeout.error.contains("timed out"),
        "{}",
        timeout.error
    );
    assert!(timeout.captures.is_empty());
    assert!(timeout.applied_generations.is_empty());
    assert!(timeout.committed_generations.is_empty());
}

fn host(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(1),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn edge(device: u64, ns: i64, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(device), host(ns), sequence),
        control: PhysicalControlId::keyboard(7),
        state: ButtonState::Down,
    })
}

#[test]
fn incomplete_and_late_frontiers_preserve_the_original_input_prefix() {
    let mut merger = InputMerger::new(
        ClockDomainId(1),
        host(0),
        vec![DeviceId(101), DeviceId(102)],
        2,
    )
    .unwrap();
    let first = edge(101, 10, 1);
    merger.admit(first.clone(), host(20)).unwrap();
    assert_eq!(
        merger.commit(host(10)),
        Err(MergeError::PendingBeforeFrontier {
            timestamp: Timestamp::from_nanos(10),
            frontier: Timestamp::from_nanos(10),
        })
    );
    assert_eq!(merger.pending(), 1);
    assert_eq!(merger.pop_ready(host(10)).unwrap(), Some(first));
    merger.commit(host(10)).unwrap();
    assert_eq!(
        merger.admit(edge(102, 10, 1), host(20)),
        Err(MergeError::LateInput {
            timestamp: Timestamp::from_nanos(10),
            committed: Timestamp::from_nanos(10),
        })
    );
    assert_eq!(merger.pending(), 0);
    merger.admit(edge(102, 11, 2), host(20)).unwrap();
    assert_eq!(merger.pending(), 1);
}

#[test]
fn bounded_queue_and_pending_input_fail_without_discarding_the_prefix() {
    let mut merger = InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(101)], 1).unwrap();
    let original = edge(101, 10, 1);
    merger.admit(original.clone(), host(20)).unwrap();
    assert_eq!(
        merger.admit(edge(101, 11, 2), host(20)),
        Err(MergeError::Capacity)
    );
    assert_eq!(merger.pop_ready(host(20)).unwrap(), Some(original));
    let (mut producer, mut consumer) = command_queue(1).unwrap();
    producer
        .try_push(AudioCommand::Seek {
            song_time: Timestamp::ZERO,
            at: Timestamp::from_nanos(10),
        })
        .unwrap();
    let rejected = producer
        .try_push(AudioCommand::Seek {
            song_time: Timestamp::ZERO,
            at: Timestamp::from_nanos(11),
        })
        .unwrap_err();
    assert_eq!(rejected.reason, QueuePushError::Full);
    assert_eq!(
        consumer.try_pop().unwrap(),
        AudioCommand::Seek {
            song_time: Timestamp::ZERO,
            at: Timestamp::from_nanos(10)
        }
    );
}

fn literal_engine(start: i64) -> JudgeEngine {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects.push(SourceObject {
        id: ObjectId(1),
        start: Beat::new(start).unwrap(),
        end: None,
        interaction: InteractionId(1),
        visual: VisualId(1),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    JudgeEngine::new(
        source.compile().unwrap(),
        vec![Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: Box::new(InstantEvaluator),
        }],
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
    .unwrap()
}

#[test]
fn invalid_last_member_practice_replacement_is_atomic_for_all_original_owners() {
    let (producer, _consumer) = command_queue(8).unwrap();
    let configs = (0..4)
        .map(|index| MemberConfig {
            player: PlayerId(7 + index),
            device: Some(DeviceId(101 + u64::from(index))),
            bindings: BindingMap::from_bindings([Binding {
                device: DeviceSelector::Exact(DeviceId(101 + u64::from(index))),
                physical: PhysicalControlId::keyboard(7),
                game_control: GameControlId(1),
            }])
            .unwrap(),
            judge: literal_engine(100),
            sounds: vec![],
        })
        .collect();
    let mut group = RuntimeGroup::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        producer,
        configs,
        8,
        &[],
    )
    .unwrap();
    let before = group.transport().anchor();
    let judges = (0..4)
        .map(|index| {
            (
                PlayerId(7 + index),
                literal_engine(if index == 3 { 101 } else { 100 }),
            )
        })
        .collect();
    let error = group
        .replace_practice_states(
            judges,
            Transport::new(Timestamp::from_nanos(1000), Timestamp::ZERO, Rate::NORMAL),
            None,
            CommandScope(1),
        )
        .unwrap_err();
    assert!(error.contains("object identity differs"));
    assert_eq!(group.audio_scope(), CommandScope(0));
    assert_eq!(group.transport().anchor(), before);
    assert!(!group.poisoned());
    for player in 7..11 {
        let judge = group.member_judge(PlayerId(player)).unwrap();
        assert_eq!(judge.effective_song_time(), None);
        assert_eq!(
            judge.chart().objects()[0].time.start,
            Timestamp::from_nanos(100)
        );
    }
}

#[test]
fn genuine_render_receipt_exhaustion_preserves_the_last_rendered_frame() {
    use beatkernel::audio::{
        practice_queue, AudioError, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits,
        PracticeLimits, PracticeRegion, PreparedPracticeProgram, SampleBank,
    };
    let format = AudioFormat::new(1000, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(64, 64, 1).unwrap()).unwrap();
    let program =
        PreparedPracticeProgram::new(&bank, vec![], PracticeLimits::new(1, 0, 8, 1, 1).unwrap())
            .unwrap();
    let (mut controller, endpoint) = practice_queue(&program).unwrap();
    let (_producer, consumer) = command_queue(1).unwrap();
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
    mixer
        .install_practice(
            program,
            endpoint,
            PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(2_000_000), true).unwrap(),
        )
        .unwrap();
    mixer.render(&mut [0.0; 1]).unwrap();
    let before = mixer.output_frame_basis();
    assert_eq!(
        mixer.render(&mut [0.0; 2]),
        Err(AudioError::PracticeReceiptFull)
    );
    assert_eq!(mixer.output_frame_basis(), before);
    let receipt = controller.try_pop_receipt().unwrap();
    assert_eq!(receipt.generation, 1);
    assert_eq!(receipt.physical_frame, 0);
    assert_eq!(receipt.applied_song_time, Timestamp::ZERO);
    let report = mixer.render(&mut [0.0; 2]).unwrap();
    assert_eq!(report.start_frame, 1);
}

#[test]
fn requesting_pause_is_insufficient_to_rebind_the_original_output() {
    use beatkernel::audio::{AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank};
    use beatkernel::time::ClockPair;
    use beatkernel_bms_runtime::playback_pause::{NativePause, PausePhase};
    let format = AudioFormat::new(1000, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(64, 64, 1).unwrap()).unwrap();
    let (mut producer, consumer) = command_queue(1).unwrap();
    let output = ClockPoint {
        domain: ClockDomainId(2),
        timestamp: Timestamp::ZERO,
    };
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
    let mut pause = NativePause::new(output, ClockDomainId(1), 1000).unwrap();
    pause
        .request(
            true,
            ClockPair {
                source: output,
                target: host(0),
            },
        )
        .unwrap();
    producer.request_pause(true);
    assert_eq!(pause.phase(), PausePhase::Pausing);
    assert!(pause
        .rebind_output(1, &mixer)
        .unwrap_err()
        .to_string()
        .contains("acknowledged"));
    mixer.render(&mut []).unwrap();
    assert!(
        pause.rebind_output(1, &mixer).is_err(),
        "empty callback cannot acknowledge pause"
    );
    assert_eq!(pause.epoch(), 0);
}
