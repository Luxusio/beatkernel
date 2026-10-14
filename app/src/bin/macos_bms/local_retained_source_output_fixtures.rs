//! Real cold audio configuration, complete PCM owner and independent cohort members.
//! Native HAL opening/publication is covered separately; these fixtures invent no ACK.
use super::*;
use beatkernel::{
    audio::{
        AudioCounters, AudioError, CommandScope, MixerConfig, OutputFrameBasis, PcmLimits,
        PcmSample, PracticeController, PracticeLimits, PracticeRegion, RenderReport, SampleBank,
        SampleId, SourcePlaybackSpan, VoiceId,
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
    replay::codec::ReplayCodecLimits,
    runtime::{RuntimeProcessingClock, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, Duration},
    transport::{Rate, Transport},
};
use beatkernel_bms_runtime::{
    gameplay::output::ports::SourcePlaybackIdentity,
    local_players::PlayerId,
    local_runtime::{InputResult, MemberConfig, RuntimeGroup},
    native_audio::{
        prepare_audio_with_speed, prepare_retained_audio_with_speed, NativePracticeAudioConfig,
    },
    native_audio_startup::{
        cold_source_coordinate_identity, prepare_source_coordinate_history,
        source_coordinate_history_capacity,
    },
    replay_capture::LiveReplayCapture,
};
use beatkernel_platform::audio::{DeviceFormat, NativeOutputState, SampleEncoding};

const FIRST: PlayerId = PlayerId(7);
const SECOND: PlayerId = PlayerId(u32::MAX);

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}

fn cohort_options() -> Options {
    parse(
        &[
            "--chart",
            "cold-cohort.bms",
            "--device",
            "42",
            "--local-player",
            "7:101",
            "--local-player",
            "4294967295:202",
            "--rate",
            "1000",
            "--channels",
            "1",
            "--buffer-frames",
            "64",
            "--preroll-ns",
            "0",
            "--voices",
            "4",
            "--bind",
            "11:04",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>(),
    )
    .unwrap()
}

fn native_format() -> DeviceFormat {
    DeviceFormat::new(1000, 1, SampleEncoding::Float32, None).unwrap()
}

fn bank() -> SampleBank {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(8192, 16384, 2).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    for (id, sample) in [(1, 0.25), (2, 0.5)] {
        bank.insert(
            SampleId(id),
            PcmSample::new(format, vec![sample; 512], limits).unwrap(),
        )
        .unwrap();
    }
    bank
}

fn member(player: PlayerId, sample: SampleId) -> MemberConfig {
    let mut source = SourceChart::new(1000, Bpm::new(60, 1).unwrap()).unwrap();
    for (id, tick) in [(1, 2), (2, 66)] {
        source.objects.push(SourceObject {
            id: ObjectId(id),
            start: Beat::new(tick).unwrap(),
            end: None,
            interaction: InteractionId(1),
            visual: VisualId(1),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
    }
    MemberConfig {
        player,
        device: Some(DeviceId(u64::from(player.0))),
        bindings: BindingMap::from_bindings([Binding {
            device: DeviceSelector::Exact(DeviceId(u64::from(player.0))),
            physical: PhysicalControlId::keyboard(4),
            game_control: GameControlId(1),
        }])
        .unwrap(),
        judge: JudgeEngine::new(
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
        .unwrap(),
        sounds: [ObjectId(1), ObjectId(2)]
            .into_iter()
            .map(|object| SoundBinding {
                object,
                stage: JudgeStage::Instant,
                sample,
                voice: VoiceId(u64::from(player.0)),
                gain: 1.0,
            })
            .collect(),
    }
}

struct SameDomain;
impl ClockMapper for SameDomain {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}

struct Cohort {
    output: NativeOutputState,
    group: RuntimeGroup,
    captures: BTreeMap<PlayerId, LiveReplayCapture>,
    identity: SourcePlaybackIdentity,
    creation: OutputFrameBasis,
    _practice: PracticeController,
}

fn retained_cohort() -> Cohort {
    let options = cohort_options();
    let config = initial_audio_config(&options, point(2, 0), true, None, false);
    let prepared = prepare_retained_audio_with_speed(
        bank(),
        Vec::new(),
        config,
        NativePracticeAudioConfig {
            region: PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(i64::MAX), false)
                .unwrap(),
            limits: PracticeLimits::new(1, 0, 16, 4, 8).unwrap(),
        },
        options.live_speed.speed(),
        None,
    )
    .unwrap();
    let mut mixer = prepared.audio.mixer;
    let creation = mixer.output_frame_basis();
    // Exercise public cold history composition at the actual initial maximum.
    // The native adapter owns later cold history growth and its HAL lifetime.
    let identity = prepare_source_coordinate_history(
        &mut mixer,
        source_coordinate_history_capacity(options.buffer).unwrap(),
    )
    .unwrap();
    let members = vec![member(FIRST, SampleId(1)), member(SECOND, SampleId(2))];
    let limits = ReplayCodecLimits::new(
        65536,
        128,
        4096,
        beatkernel::input::CodecLimits::new(4096, 4096).unwrap(),
    )
    .unwrap();
    let captures = members
        .iter()
        .map(|member| {
            (
                member.player,
                LiveReplayCapture::new(&member.judge, ClockDomainId(1), limits).unwrap(),
            )
        })
        .collect();
    let mut group = RuntimeGroup::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        prepared.audio.producer,
        members,
        16,
        &[],
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    group.set_audio_scope(CommandScope(1));
    Cohort {
        output: NativeOutputState::new(mixer, native_format(), None, 64).unwrap(),
        group,
        captures,
        identity,
        creation,
        _practice: prepared.practice,
    }
}

fn press(cohort: &mut Cohort, player: PlayerId, host_ns: i64, audio_ns: i64) {
    let mut meta = EventMeta::new(DeviceId(u64::from(player.0)), point(1, host_ns), 1);
    let original = point(9, host_ns * 3);
    meta.original_clock_point = Some(original);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(0x4d48_4944),
        code: Some(4),
        timestamp: Some(original),
    });
    let event = PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    });
    let InputResult::Processed(reports) = cohort
        .group
        .process_input(event.clone(), &SameDomain, point(2, audio_ns))
        .unwrap()
    else {
        panic!("exact assigned member was ignored");
    };
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].player, player);
    assert_eq!(reports[0].report.bound_inputs[0].physical, event);
    cohort
        .captures
        .get_mut(&player)
        .unwrap()
        .record_report(&reports[0].report)
        .unwrap();
}

#[derive(Debug, PartialEq)]
struct OutputSnapshot {
    config: MixerConfig,
    basis: OutputFrameBasis,
    report: Option<RenderReport>,
    admitted: usize,
    samples: Vec<f32>,
    counters: AudioCounters,
    cursors: (u64, u64),
    history: (u32, u64, u64, u64, u64, u64, usize, Vec<SourcePlaybackSpan>),
    generation: Option<u64>,
}

fn snapshot(output: &NativeOutputState) -> OutputSnapshot {
    let mixer = output.mixer();
    let history = mixer.source_playback_history().unwrap();
    OutputSnapshot {
        config: mixer.configuration(),
        basis: output.output_frame_basis(),
        report: output.pending_report(),
        admitted: output.admitted_frames(),
        samples: output.pending_samples().to_vec(),
        counters: mixer.counters(),
        cursors: (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        history: (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame(),
            history.pulled_end_frame(),
            history.retained_start_frame(),
            history.retirement_revision(),
            history.capacity(),
            (0..history.len())
                .map(|index| history.span(index).unwrap())
                .collect(),
        ),
        generation: mixer.practice_generation(),
    }
}

#[test]
fn retained_cohort_software_growth_preserves_original_tail_and_member_owners() {
    let mut cohort = retained_cohort();
    assert_eq!(
        cohort
            .output
            .mixer()
            .configuration()
            .limits()
            .max_render_frames(),
        AudioLimits::MAX_RENDER_FRAMES
    );
    assert_eq!(cohort.output.max_frames(), 64);
    assert_eq!(
        cohort
            .output
            .mixer()
            .source_playback_history()
            .unwrap()
            .capacity(),
        source_coordinate_history_capacity(64).unwrap()
    );
    press(&mut cohort, FIRST, 2_000_000, 0);
    let original = cohort.output.render_pending(64).unwrap();
    assert_eq!(cohort.output.pending_samples(), vec![0.25; 64]);
    cohort.output.admit(16).unwrap();
    let before = snapshot(&cohort.output);
    let hashes = [FIRST, SECOND].map(|id| {
        cohort
            .group
            .member_judge(id)
            .unwrap()
            .stable_hash()
            .unwrap()
    });
    let captures = cohort
        .captures
        .iter()
        .map(|(id, capture)| (*id, (capture.header().clone(), capture.records().to_vec())))
        .collect::<BTreeMap<_, _>>();
    let creation = cohort.creation;

    // Software storage can grow while retaining this 48-frame suffix. This is
    // not permission to reopen a fixed 128-frame or variable CoreAudio output:
    // native geometry must qualify the retained suffix separately. Drain it
    // below before asking this software owner for a fresh 128-frame block.
    cohort
        .output
        .reconfigure(native_format(), None, 128)
        .unwrap();

    assert_eq!(cohort.output.max_frames(), 128);
    assert_eq!(snapshot(&cohort.output), before);
    assert_eq!(cohort.output.pending_report(), Some(original));
    assert_eq!(cohort.output.pending_samples(), vec![0.25; 48]);
    assert_eq!(
        cohort.output.output_frame_basis().start_physical_frame(),
        16
    );
    assert_eq!(cohort.creation, creation);
    assert_eq!(
        cold_source_coordinate_identity(cohort.output.mixer()).unwrap(),
        cohort.identity
    );
    assert_eq!(cohort.group.audio_scope(), CommandScope(1));
    assert_eq!(
        [FIRST, SECOND].map(|id| cohort
            .group
            .member_judge(id)
            .unwrap()
            .stable_hash()
            .unwrap()),
        hashes
    );
    for (id, (header, records)) in &captures {
        assert_eq!(cohort.captures[id].header(), header);
        assert_eq!(cohort.captures[id].records(), records);
    }
    assert_eq!(cohort.captures[&FIRST].records().len(), 1);
    assert!(cohort.captures[&SECOND].records().is_empty());
    assert_eq!(
        cohort.output.render_pending(128),
        Err(AudioError::InvalidBuffer)
    );
    assert_eq!(snapshot(&cohort.output), before);

    cohort.output.admit(48).unwrap();
    press(&mut cohort, SECOND, 66_000_000, 64_000_000);
    let grown = cohort.output.render_pending(128).unwrap();
    assert_eq!((grown.start_frame, grown.frames), (64, 128));
    assert_eq!(cohort.output.pending_samples(), vec![0.75; 128]);
    assert_eq!(grown.counters.commands_consumed, 2);
    assert_eq!(grown.counters.voice_full, 0);
    assert_eq!(cohort.group.audio_scope(), CommandScope(1));
    assert_eq!(cohort.captures[&FIRST].records(), captures[&FIRST].1);
    assert_eq!(cohort.captures[&SECOND].records().len(), 1);
    assert_ne!(
        cohort.captures[&FIRST].records(),
        cohort.captures[&SECOND].records()
    );
    assert_eq!(
        cold_source_coordinate_identity(cohort.output.mixer()).unwrap(),
        cohort.identity
    );
    assert_eq!(
        cohort.output.mixer().practice_generation(),
        before.generation
    );

    cohort.output.admit(128).unwrap();
    cohort
        .group
        .enqueue_audio(AudioCommand::Stop {
            voice: VoiceId(u64::from(FIRST.0)),
            at: Timestamp::from_nanos(192_000_000),
        })
        .unwrap();
    let next = cohort.output.render_pending(128).unwrap();
    assert_eq!((next.start_frame, next.frames), (192, 128));
    assert_eq!(cohort.output.pending_samples(), vec![0.5; 128]);
    assert_eq!(next.counters.unknown_stops, 0);
}

#[test]
fn retained_cohort_growth_and_interpretation_refusals_preserve_complete_suffix() {
    let mut cohort = retained_cohort();
    press(&mut cohort, FIRST, 2_000_000, 0);
    cohort.output.render_pending(64).unwrap();
    cohort.output.admit(16).unwrap();
    let before = snapshot(&cohort.output);
    for frames in [0, AudioLimits::MAX_RENDER_FRAMES + 1] {
        assert!(cohort
            .output
            .reconfigure(native_format(), None, frames)
            .is_err());
        assert_eq!(snapshot(&cohort.output), before);
        assert_eq!(cohort.output.max_frames(), 64);
    }
    let changed = DeviceFormat::new(1000, 2, SampleEncoding::Float32, None).unwrap();
    assert_eq!(
        cohort.output.reconfigure(changed, None, 128),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(snapshot(&cohort.output), before);
    assert_eq!(cohort.group.audio_scope(), CommandScope(1));
    assert_eq!(cohort.captures[&FIRST].records().len(), 1);
    assert!(cohort.captures[&SECOND].records().is_empty());
}

#[test]
fn legacy_network_cohort_keeps_nominal_limit_finite_end_and_start_gate() {
    let options = cohort_options();
    let config = initial_audio_config(&options, point(2, 0), false, Some(192), true);
    let prepared =
        prepare_audio_with_speed(bank(), Vec::new(), config, options.live_speed.speed()).unwrap();
    let mut output = NativeOutputState::new(prepared.mixer, native_format(), None, 64).unwrap();
    assert_eq!(
        output.mixer().configuration().limits().max_render_frames(),
        64
    );
    assert_eq!(output.mixer().start_gate_frame(), Some(None));
    assert!(output.mixer().source_playback_history().is_none());
    assert_eq!(output.render_pending(128), Err(AudioError::RenderCapacity));
    assert_eq!(
        output.reconfigure(native_format(), None, 128),
        Err(AudioError::RenderCapacity)
    );
    assert_eq!(output.mixer().frame_cursor(), 0);
    assert_eq!(output.mixer().counters().commands_consumed, 0);
    assert_eq!(config.playback_end_frame, Some(192));
    assert_eq!(config.output_origin, point(2, 0));
    assert!(config.gated_start);
}

#[derive(Debug)]
struct InitialOpenFault(std::sync::Arc<u64>);
impl std::fmt::Display for InitialOpenFault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "retained initial output open refusal {}", self.0)
    }
}
impl std::error::Error for InitialOpenFault {}

/// Execute the shared finalizer contract used after native cleanup. This does
/// not execute the Mac-only complete-open branch or manufacture HAL failures.
#[test]
fn shared_cohort_finalizer_keeps_initial_open_error_and_attempts_every_failed_capture() {
    use beatkernel_bms_runtime::{
        competition::ScoreSummary, gauge::BmsGauge, native_cohort::PlayerState,
        native_cohort_setup::finish_cohort_with_results_and_network,
    };
    use std::{error::Error, path::Path, sync::Arc};

    for (first_save_fails, cleanup_fails) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        // Initial native opening follows member/capture preparation and precedes
        // gameplay. Preserve those real, still-pristine per-member captures.
        let Cohort { captures, .. } = retained_cohort();
        let expected = captures
            .iter()
            .map(|(id, capture)| (*id, (capture.header().clone(), capture.records().to_vec())))
            .collect::<BTreeMap<_, _>>();
        let states = captures
            .into_iter()
            .map(|(player, capture)| PlayerState {
                player,
                capture: Some(capture),
                competition: None,
                completion: None,
                score: ScoreSummary::default(),
                gauge: BmsGauge::default(),
                last_song: Timestamp::ZERO,
            })
            .collect();
        let paths = BTreeMap::from([
            (FIRST, PathBuf::from("initial-open-player7.bkr")),
            (SECOND, PathBuf::from("initial-open-player4294967295.bkr")),
        ]);
        // Deliberately reverse destinations: finalization must match by ID.
        let destinations = [SECOND, FIRST]
            .map(|player| (player, Some(paths[&player].clone())))
            .to_vec();
        let marker = Arc::new(71);
        let boxed = Box::new(InitialOpenFault(Arc::clone(&marker)));
        let original = boxed.as_ref() as *const InitialOpenFault;
        let mut saves = Vec::new();

        let result = finish_cohort_with_results_and_network(
            Err(boxed as Box<dyn Error>),
            states,
            None,
            destinations,
            if cleanup_fails {
                vec!["IOHID close after initial output open failure".to_owned()]
            } else {
                Vec::new()
            },
            Some(Path::new("initial-open-whole.bkr")),
            |capture, path, failed_session| {
                assert!(failed_session);
                let path = path.expect("recording member keeps its original destination");
                let player = *paths
                    .iter()
                    .find(|(_, expected)| expected.as_path() == path)
                    .expect("save used an original member destination")
                    .0;
                let file = capture
                    .expect("prepared member capture survives initial output failure")
                    .into_file();
                assert_eq!(file.header, expected[&player].0);
                assert_eq!(file.records, expected[&player].1);
                assert!(file.records.is_empty());
                saves.push(player);
                if first_save_fails && player == FIRST {
                    Err("first member replay save refused".into())
                } else {
                    Ok(())
                }
            },
            |_, _| panic!("initial open failure supplies no completed-play archive proof"),
        );

        assert_eq!(saves, [FIRST, SECOND]);
        let error = result.expect_err("initial output failure remains the primary outcome");
        let retained = error
            .downcast_ref::<InitialOpenFault>()
            .expect("finalization preserves the original concrete error type");
        assert_eq!(retained as *const InitialOpenFault, original);
        assert!(Arc::ptr_eq(&retained.0, &marker));
    }
}

/// Foreign typing checks the real device ports without claiming native execution.
#[cfg(target_os = "macos")]
#[test]
fn actual_cohort_device_has_mutable_source_mapping_and_coordinate_publication_ports() {
    use beatkernel_bms_runtime::{
        gameplay_presentation::{GameplayAudioOutputContext, GameplayDevice},
        native_audio_presentation::NativeAudioPresentation,
        native_gameplay::NativeGameplayResult,
        native_playback_phase::NativePlaybackMapping,
    };
    fn ports<'owner>() {
        let _: fn(
            &mut CohortDevice<'owner>,
            &NativeAudioPresentation,
            &beatkernel::audio::InterpolatedTargetTime,
            SourcePlaybackIdentity,
        ) -> NativeGameplayResult<Option<NativePlaybackMapping>> =
            <CohortDevice<'owner> as GameplayDevice>::conditional_playback_mapping_at;
        let _: fn(
            &mut CohortDevice<'owner>,
            GameplayAudioOutputContext<'_>,
            ClockPoint,
            SourcePlaybackIdentity,
            bool,
        ) -> NativeGameplayResult<bool> =
            <CohortDevice<'owner> as GameplayDevice>::publish_paused_coordinate_audio_output;
    }
    ports();
}
