//! Real Source owner/history pause conservation across native epoch replacement.
//! Controlled native packets prove application policy, not physical device timing.
use super::*;
use crate::{
    audio_authority::{
        AudioAuthority, AudioAuthorityConfig, AudioAuthorityCoordinateBasis, AudioAuthorityEpoch,
        PreparedCoordinateControlCutoff,
    },
    gameplay::output::ports::{
        NativePlaybackObservation, NativeSourcePlaybackQuery, SourcePlaybackFacts,
        SourcePlaybackIdentity, SourcePlaybackPacketError,
    },
    local_input::InputMerger,
    native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
};
use beatkernel::{
    audio::*,
    input::EventMeta,
    time::{ClockMappingQuality, Duration, ExtrapolationPolicy},
};
use beatkernel_platform::audio::{
    presentation::validation::{NativePresentationValidator, OriginalNativePresentationEvidence},
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    DeviceFormat, NativeOutputState, SampleEncoding, StreamCounters,
};

const RATE: u32 = 1000;
const SAMPLE_FRAMES: usize = 128;

fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn host(nanos: i64) -> ClockPoint {
    point(1, nanos)
}
fn descriptor(basis: OutputFrameBasis, frequency: Option<u64>) -> AudioAuthorityCoordinateBasis {
    match frequency {
        Some(frequency) => AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency },
        None => AudioAuthorityCoordinateBasis::SourceFrame(basis),
    }
}
fn basis(descriptor: AudioAuthorityCoordinateBasis) -> OutputFrameBasis {
    match descriptor {
        AudioAuthorityCoordinateBasis::SourceFrame(basis)
        | AudioAuthorityCoordinateBasis::SourceCounter { basis, .. } => basis,
        _ => panic!("fixture requires original Source coordinates"),
    }
}
fn snapshot(
    epoch: u64,
    descriptor: AudioAuthorityCoordinateBasis,
    units: u64,
    nanos: i64,
) -> CoordinateNativeAudioSnapshot {
    let basis = basis(descriptor);
    let (evidence, coordinate) = match descriptor {
        AudioAuthorityCoordinateBasis::SourceFrame(_) => (
            OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(units).unwrap(),
                target: host(nanos),
            }),
            NativeOutputCoordinate::StreamFrame { frame: units },
        ),
        AudioAuthorityCoordinateBasis::SourceCounter { frequency, .. } => (
            OriginalNativePresentationEvidence::Wasapi {
                snapshot: AudioStreamSnapshot {
                    telemetry_available: true,
                    status: AudioStreamStatus::Running,
                    counters: StreamCounters::default(),
                    render: None,
                    clock: Some(AudioClockSnapshot {
                        position: units,
                        frequency,
                        qpc_100ns: units + 1,
                        reading_quality: AudioClockReadingQuality::Accurate,
                        host_point: Some(host(nanos)),
                        mapping_quality: ClockMappingQuality::Unknown,
                    }),
                },
                basis: Some(basis),
            },
            NativeOutputCoordinate::Counter {
                position: units,
                frequency,
            },
        ),
        _ => unreachable!(),
    };
    CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch,
            basis,
            evidence,
        },
        coordinate,
    )
    .unwrap()
}
fn units_for_frames(descriptor: AudioAuthorityCoordinateBasis, frames: u64) -> u64 {
    match descriptor {
        AudioAuthorityCoordinateBasis::SourceFrame(_) => frames,
        AudioAuthorityCoordinateBasis::SourceCounter { frequency, .. } => {
            frames * frequency / u64::from(RATE)
        }
        _ => unreachable!(),
    }
}
fn authority(epoch: u64, descriptor: AudioAuthorityCoordinateBasis) -> AudioAuthority {
    AudioAuthority::new_with_coordinates(
        AudioAuthorityConfig {
            history_capacity: 32,
            max_observation_age: Duration::from_nanos(1_000_000_000),
            input_extrapolation: ExtrapolationPolicy::Forbid,
            max_input_ahead: Duration::ZERO,
        },
        AudioAuthorityEpoch {
            id: epoch,
            stream_origin: basis(descriptor).point_at_stream_frame(0).unwrap(),
            logical_origin: point(3, 500),
            host_domain: ClockDomainId(1),
        },
        descriptor,
    )
    .unwrap()
}
fn cutoff(
    authority: &AudioAuthority,
    adoption: &PreparedCoordinatePauseAdoption,
    now: ClockPoint,
    inputs: &InputMerger,
) -> PreparedCoordinateControlCutoff {
    let at = authority
        .presented_coordinate_boundary_host(&adoption.exact_cut(), now)
        .unwrap()
        .unwrap();
    if adoption.paused() {
        authority.prepare_coordinate_control_cutoff(
            authority.epoch().id,
            adoption.exact_cut(),
            at,
            now,
            inputs,
        )
    } else {
        authority.prepare_coordinate_resume_control_cutoff(
            authority.epoch().id,
            adoption.exact_cut(),
            at,
            now,
            inputs,
        )
    }
    .unwrap()
    .unwrap()
}

struct Rig {
    producer: CommandProducer,
    output: NativeOutputState,
    creation: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    pause: NativePause,
    native: NativeAudioPresentation,
    inputs: InputMerger,
    old_adoption: PreparedCoordinatePauseAdoption,
    old_cutoff: PreparedCoordinateControlCutoff,
}
fn rig(frequency: Option<u64>) -> Rig {
    rig_with_history(frequency, true)
}
fn rig_with_history(frequency: Option<u64>, keep_history: bool) -> Rig {
    let format = AudioFormat::new(RATE, 1).unwrap();
    let asset_bytes = SAMPLE_FRAMES * std::mem::size_of::<f32>();
    let limits = PcmLimits::new(asset_bytes, asset_bytes, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; SAMPLE_FRAMES], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue_with_start_gate(8).unwrap();
    producer.schedule_start_at(2).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::from_nanos(100),
            AudioLimits::new(8, 4, 8, 64, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let startup = mixer.render(&mut [0.0; 5]).unwrap();
    assert_eq!(startup.playback_frames, 3);
    assert_eq!(mixer.applied_start_frame(), Some(2));
    if keep_history {
        mixer.enable_source_playback_history(16).unwrap();
    }
    let creation = mixer.output_frame_basis();
    let identity = SourcePlaybackIdentity::new(RATE, 5, 3).unwrap();
    let mut output = NativeOutputState::new(
        mixer,
        DeviceFormat::new(RATE, 1, SampleEncoding::Float32, None).unwrap(),
        None,
        16,
    )
    .unwrap_or_else(|_| panic!("valid complete Source owner"));
    let active = output.render_pending(4).unwrap();
    assert_eq!((active.start_frame, active.playback_start_frame), (5, 3));
    assert!(output.pending_samples().iter().all(|sample| *sample > 0.0));
    output.admit(4).unwrap();
    let old_descriptor = descriptor(creation, frequency);
    let a = authority(7, old_descriptor);
    let mut native = NativeAudioPresentation::new(
        a,
        NativePresentationValidator::new(
            7,
            creation.point_at_stream_frame(0).unwrap(),
            ClockDomainId(1),
        ),
    )
    .unwrap();
    let first = u64::from(frequency.is_some());
    for (units, nanos) in [(first, 100), (units_for_frames(old_descriptor, 3), 200)] {
        native
            .admit_coordinate(snapshot(7, old_descriptor, units, nanos))
            .unwrap();
    }
    native.pin_practice_mixer_basis(creation).unwrap();
    let mut pause = NativePause::new(creation.origin(), ClockDomainId(1), RATE)
        .unwrap()
        .with_start_frame(2)
        .unwrap()
        .with_coordinate_binding(7, old_descriptor, identity)
        .unwrap();
    assert!(pause.request_coordinate(true, native.authority()).unwrap());
    producer.request_pause(true);
    let paused = output.render_pending(8).unwrap();
    assert_eq!((paused.start_frame, paused.playback_start_frame), (9, 7));
    assert!(paused.paused);
    assert_eq!(paused.playback_frames, 0);
    let adoption = pause
        .prepare_coordinate_adoption(Some(paused), None)
        .unwrap()
        .unwrap();
    native
        .admit_coordinate(snapshot(
            7,
            old_descriptor,
            units_for_frames(old_descriptor, 5),
            1000,
        ))
        .unwrap();
    let mut inputs = InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap();
    let event = PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), host(200), 1),
        control: PhysicalControlId::keyboard(7u16),
        state: ButtonState::Down,
    });
    inputs.admit(event.clone(), host(1000)).unwrap();
    let a = native.authority_mut();
    a.record_acquired_prefix(host(1000)).unwrap();
    let mapped = a
        .prepare_coordinate_input(host(200), host(1000))
        .unwrap()
        .unwrap();
    assert_eq!(inputs.pop_ready(host(1000)).unwrap(), Some(event));
    a.commit_coordinate_input(mapped, host(1000)).unwrap();
    inputs.commit(host(200)).unwrap();
    let cut = cutoff(a, &adoption, host(1000), &inputs);
    let acknowledged = pause
        .commit_coordinate_adoption(adoption.clone(), a, &cut, host(1000), &inputs)
        .unwrap();
    assert_eq!(
        (acknowledged.physical_frame, acknowledged.playback_frame),
        (9, 7)
    );
    a.commit_coordinate_control_cutoff(cut, host(1000), &inputs)
        .unwrap();
    output.admit(3).unwrap();
    assert_eq!(output.output_frame_basis().start_physical_frame(), 12);
    assert_eq!(output.mixer().frame_cursor(), 17);
    assert_eq!(output.pending_frames(), 5);
    assert_eq!(output.pending_samples(), [0.0; 5]);
    Rig {
        producer,
        output,
        creation,
        identity,
        pause,
        native,
        inputs,
        old_adoption: adoption,
        old_cutoff: cut,
    }
}

fn packet(
    epoch: u64,
    descriptor: AudioAuthorityCoordinateBasis,
    identity: SourcePlaybackIdentity,
    output: &NativeOutputState,
    report: RenderReport,
    units: u64,
    nanos: i64,
) -> NativePlaybackObservation<()> {
    let original = snapshot(epoch, descriptor, units, nanos);
    let query = original.source_query().unwrap();
    let history = output.mixer().source_playback_history().unwrap();
    let facts = SourcePlaybackFacts {
        playback: history.lookup_at_source_time(query).unwrap(),
        pulled_source_end: history.pulled_end_frame(),
        submitted_source_end: output.output_frame_basis().start_physical_frame(),
        render: Some(report),
        revision: 2,
    };
    NativePlaybackObservation::new(
        original,
        NativeSourcePlaybackQuery::new(epoch, basis(descriptor), identity, query, facts, Some(()))
            .unwrap(),
    )
    .unwrap()
}
fn output_facts(output: &NativeOutputState) -> String {
    let mixer = output.mixer();
    let history = mixer.source_playback_history().map(|history| {
        (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame(),
            history.retained_start_frame(),
            history.pulled_end_frame(),
            history.retirement_revision(),
            (0..history.len())
                .map(|index| history.span(index))
                .collect::<Vec<_>>(),
        )
    });
    format!(
        "{:?}",
        (
            mixer.config(),
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.counters(),
            mixer.applied_start_frame(),
            output.output_frame_basis(),
            output.format(),
            output.max_frames(),
            output.pending_report(),
            output.admitted_frames(),
            output.pending_samples(),
            history,
        )
    )
}
fn preserved(pause: &NativePause) -> (u64, u64, u64, ClockPoint, u32, Option<u64>) {
    (
        pause.frozen,
        pause.gap,
        pause.start_frame,
        pause.origin,
        pause.rate,
        pause.playback_end,
    )
}

fn with_facts(
    observation: NativePlaybackObservation<()>,
    facts: SourcePlaybackFacts,
) -> Result<NativePlaybackObservation<()>, crate::gameplay::output::ports::SourcePlaybackPacketError>
{
    let source = observation.source();
    NativePlaybackObservation::new(
        observation.coordinate(),
        NativeSourcePlaybackQuery::new(
            source.epoch(),
            source.basis(),
            source.identity(),
            source.query(),
            facts,
            Some(()),
        )?,
    )
}

fn publish(
    r: &mut Rig,
    epoch: u64,
    frequency: Option<u64>,
    nanos: i64,
) -> AudioAuthorityCoordinateBasis {
    let association = r
        .native
        .prepare_coordinate_source_owner_association(&r.output, r.identity)
        .unwrap();
    let proof = r
        .pause
        .prepare_coordinate_source_output(&r.output, &association)
        .unwrap();
    let descriptor = descriptor(r.output.output_frame_basis(), frequency);
    let floor = r.output.mixer().frame_cursor();
    let first_frame = floor - basis(descriptor).start_physical_frame();
    let mut candidate = r.pause.clone();
    candidate
        .rebind_coordinate_source_output(&proof, epoch, descriptor)
        .unwrap();
    if r.output.pending_frames() != 0 {
        r.output.admit(r.output.pending_frames()).unwrap();
    }
    let fresh = r.output.render_pending(4).unwrap();
    r.output.admit(2).unwrap();
    let first_units = units_for_frames(descriptor, first_frame).max(u64::from(frequency.is_some()));
    let last_units = units_for_frames(descriptor, first_frame + 1);
    let originals = [
        snapshot(epoch, descriptor, first_units, nanos),
        snapshot(epoch, descriptor, last_units, nanos + 100),
    ];
    let accepted = packet(
        epoch,
        descriptor,
        r.identity,
        &r.output,
        fresh,
        last_units,
        nanos + 100,
    );
    assert!(candidate
        .observe_coordinate_source_replacement(&accepted)
        .unwrap());
    r.pause
        .validate_coordinate_source_replacement(&candidate, &accepted)
        .unwrap();
    let previous = r.native.authority().epoch();
    let stream_origin = basis(descriptor).point_at_stream_frame(0).unwrap();
    let next = AudioAuthorityEpoch {
        id: epoch,
        stream_origin,
        logical_origin: previous.logical_output(stream_origin).unwrap(),
        host_domain: previous.host_domain,
    };
    let native = r
        .native
        .prepare_coordinate_source_output_epoch(
            next,
            descriptor,
            originals,
            &association,
            host(nanos + 100),
            &r.inputs,
        )
        .unwrap();
    r.native
        .commit_coordinate_source_output_epoch(native, host(nanos + 100), &r.inputs)
        .unwrap();
    r.pause = candidate;
    descriptor
}

#[test]
fn cold_source_pause_proof_survives_owner_move_and_actual_changed_counter_descriptor() {
    for (old_frequency, new_frequency) in [(None, None), (Some(3000), Some(7000))] {
        let r = rig(old_frequency);
        let prior = r.pause.clone();
        let owner_before = output_facts(&r.output);
        let association = r
            .native
            .prepare_coordinate_source_owner_association(&r.output, r.identity)
            .unwrap();
        let prepared = r
            .pause
            .prepare_coordinate_source_output(&r.output, &association)
            .unwrap();
        assert_eq!(r.pause, prior);
        assert_eq!(output_facts(&r.output), owner_before);
        assert_eq!(association.practice_mixer_basis(), r.creation);
        assert_eq!(association.basis(), r.output.output_frame_basis());
        let Rig {
            output, mut pause, ..
        } = r;
        // The complete owner moves into the new native-open boundary while the
        // cold proof retains no borrowed Mixer/history reference.
        let moved_output = output;
        let actual = descriptor(moved_output.output_frame_basis(), new_frequency);
        pause
            .rebind_coordinate_source_output(&prepared, 8, actual)
            .unwrap();
        assert_eq!(pause.epoch(), 8);
        assert_eq!(pause.phase(), PausePhase::Paused);
        assert_eq!(preserved(&pause), preserved(&prior));
        assert_eq!(
            pause.coordinate_source_identity(),
            Some(SourcePlaybackIdentity::new(RATE, 5, 3).unwrap())
        );
        assert_eq!(pause.coordinate_binding.unwrap().basis, actual);
        assert_eq!(pause.coordinate_source_pull_floor, Some(17));
        assert!(pause.min_host.unwrap().timestamp >= prior.last_pair.unwrap().target.timestamp);
        assert!(pause.reference.is_none());
        assert!(pause.coordinate_reference.is_none());
        assert!(pause.boundary.is_none());
        assert!(pause.coordinate_replacement_observation.is_none());
        assert_eq!(
            pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
            Timestamp::ZERO
        );
        assert_eq!(output_facts(&moved_output), owner_before);
        assert!(moved_output.paused_tail_admissible());
    }
}

#[test]
fn old_paused_suffix_is_not_fresh_and_merged_frozen_history_may_precede_new_basis() {
    for (old_frequency, new_frequency) in [(None, None), (Some(3000), Some(7000))] {
        let mut r = rig(old_frequency);
        let association = r
            .native
            .prepare_coordinate_source_owner_association(&r.output, r.identity)
            .unwrap();
        let prepared = r
            .pause
            .prepare_coordinate_source_output(&r.output, &association)
            .unwrap();
        let actual = descriptor(r.output.output_frame_basis(), new_frequency);
        let mut candidate = r.pause.clone();
        candidate
            .rebind_coordinate_source_output(&prepared, 8, actual)
            .unwrap();
        let rebound = candidate.clone();
        let retained = r.output.pending_report().unwrap();
        r.output.admit(5).unwrap();
        let old = packet(
            8,
            actual,
            r.identity,
            &r.output,
            retained,
            units_for_frames(actual, 1),
            1100,
        );
        assert!(!candidate
            .observe_coordinate_source_replacement(&old)
            .unwrap());
        assert_eq!(candidate, rebound);
        let fresh = r.output.render_pending(4).unwrap();
        assert_eq!(fresh.start_frame, 17);
        assert_eq!(fresh.playback_start_frame, 7);
        assert_eq!(fresh.playback_frames, 0);
        assert!(fresh.paused);
        assert_eq!(r.output.pending_samples(), [0.0; 4]);
        // Fresh rendering without positive submission/native crossing is not
        // enough; packets still refer to the old accepted native frontier.
        let early = packet(
            8,
            actual,
            r.identity,
            &r.output,
            fresh,
            units_for_frames(actual, 1),
            1200,
        );
        assert!(!candidate
            .observe_coordinate_source_replacement(&early)
            .unwrap());
        assert_eq!(candidate, rebound);
        r.output.admit(2).unwrap();
        let ready = packet(
            8,
            actual,
            r.identity,
            &r.output,
            fresh,
            units_for_frames(actual, 6),
            1300,
        );
        assert_eq!(ready.source().facts().playback.physical_start(), 9);
        assert!(
            ready.source().facts().playback.physical_start() < basis(actual).start_physical_frame()
        );
        assert!(candidate
            .coordinate_source_replacement_observation_ready(&ready)
            .unwrap());
        assert!(candidate
            .observe_coordinate_source_replacement(&ready)
            .unwrap());
        r.pause
            .validate_coordinate_source_replacement(&candidate, &ready)
            .unwrap();
        assert_eq!(candidate.phase(), PausePhase::Paused);
        assert_eq!(preserved(&candidate), preserved(&r.pause));
        assert_eq!(r.output.mixer().playback_frame_cursor(), 7);
        assert_eq!(candidate.coordinate_source_pull_floor, Some(21));
        let accepted = candidate.clone();
        assert!(candidate
            .observe_coordinate_source_replacement(&early)
            .is_err());
        assert_eq!(candidate, accepted);
    }
}

#[test]
fn stale_cold_pause_owner_history_startup_and_created_descriptor_reject_without_mutation() {
    for frequency in [None, Some(3000)] {
        let mut r = rig(frequency);
        let association = r
            .native
            .prepare_coordinate_source_owner_association(&r.output, r.identity)
            .unwrap();
        let prepared = r
            .pause
            .prepare_coordinate_source_output(&r.output, &association)
            .unwrap();
        let cold_output = output_facts(&r.output);
        for case in 0..11 {
            let mut changed = r.pause.clone();
            match case {
                0 => changed.phase = PausePhase::Resuming,
                1 => changed.frozen += 1,
                2 => changed.start_frame += 1,
                3 => changed.origin = point(2, 101),
                4 => changed.rate += 1,
                5 => {
                    changed.coordinate_binding.as_mut().unwrap().source_identity =
                        SourcePlaybackIdentity::new(RATE, 5, 4).unwrap()
                }
                6 => changed.min_physical_frame = 18,
                7 => changed.coordinate_source_pull_floor = Some(18),
                8 => changed.playback_end = Some(6),
                9 => changed.end_marker = Some(17),
                _ => changed.last_report.as_mut().unwrap().frames += 1,
            }
            let before = changed.clone();
            assert!(
                changed
                    .prepare_coordinate_source_output(&r.output, &association)
                    .is_err(),
                "case {case}"
            );
            assert_eq!(changed, before, "case {case}");
            assert_eq!(output_facts(&r.output), cold_output, "case {case}");
        }
        let actual = descriptor(r.output.output_frame_basis(), frequency.map(|_| 7000));
        let wrong_basis = OutputFrameBasis::new(r.creation.origin(), RATE, 13).unwrap();
        let wrong_class = match actual {
            AudioAuthorityCoordinateBasis::SourceFrame(basis) => descriptor(basis, Some(7000)),
            AudioAuthorityCoordinateBasis::SourceCounter { basis, .. } => descriptor(basis, None),
            _ => unreachable!(),
        };
        let target = AudioAuthorityCoordinateBasis::TargetFrame(
            TargetFrameBasis::new(
                r.creation.origin(),
                TargetTime::from_frames(12, RATE).unwrap(),
                RATE,
            )
            .unwrap(),
        );
        for (epoch, malformed) in [
            (7, actual),
            (6, actual),
            (8, descriptor(wrong_basis, frequency.map(|_| 7000))),
            (8, wrong_class),
            (8, target),
            (
                8,
                AudioAuthorityCoordinateBasis::SourceCounter {
                    basis: basis(actual),
                    frequency: 0,
                },
            ),
        ] {
            let mut candidate = r.pause.clone();
            let before = candidate.clone();
            assert!(candidate
                .rebind_coordinate_source_output(&prepared, epoch, malformed)
                .is_err());
            assert_eq!(candidate, before);
            assert_eq!(output_facts(&r.output), cold_output);
        }
        let mut changed = r.pause.clone();
        changed.gap += 1;
        let before = changed.clone();
        assert!(changed
            .rebind_coordinate_source_output(&prepared, 8, actual)
            .is_err());
        assert_eq!(changed, before);
        // Admission and new real rendering after capture make the old cold
        // association stale even though rate/origin/frozen playback still agree.
        r.output.admit(5).unwrap();
        r.output.render_pending(2).unwrap();
        let progressed = output_facts(&r.output);
        assert!(r
            .pause
            .prepare_coordinate_source_output(&r.output, &association)
            .is_err());
        assert_eq!(output_facts(&r.output), progressed);
        assert_eq!(r.pause.phase(), PausePhase::Paused);
    }
    let no_history = rig_with_history(None, false);
    let before = output_facts(&no_history.output);
    assert!(no_history
        .native
        .prepare_coordinate_source_owner_association(&no_history.output, no_history.identity)
        .is_err());
    assert_eq!(output_facts(&no_history.output), before);
}

#[test]
fn malformed_torn_unsubmitted_and_mismatched_frozen_publications_preserve_candidate_atomically() {
    for (old_frequency, new_frequency) in [(None, None), (Some(3000), Some(7000))] {
        let mut r = rig(old_frequency);
        let association = r
            .native
            .prepare_coordinate_source_owner_association(&r.output, r.identity)
            .unwrap();
        let proof = r
            .pause
            .prepare_coordinate_source_output(&r.output, &association)
            .unwrap();
        let actual = descriptor(r.output.output_frame_basis(), new_frequency);
        let mut candidate = r.pause.clone();
        candidate
            .rebind_coordinate_source_output(&proof, 8, actual)
            .unwrap();
        r.output.admit(5).unwrap();
        let fresh = r.output.render_pending(4).unwrap();
        r.output.admit(2).unwrap();
        let good = packet(
            8,
            actual,
            r.identity,
            &r.output,
            fresh,
            units_for_frames(actual, 6),
            1300,
        );
        let before = candidate.clone();
        let owner_before = output_facts(&r.output);
        for case in 0..8 {
            let mut facts = good.source().facts();
            match case {
                0 => facts.render.as_mut().unwrap().paused = false,
                1 => facts.render.as_mut().unwrap().playback_frames = 1,
                2 => facts.render.as_mut().unwrap().playback_start_frame = 8,
                3 => facts.render.as_mut().unwrap().playback_end_physical_frame = Some(20),
                4 => {
                    facts.playback =
                        SourcePlaybackSpan::new(9, None, 8, SourcePlaybackState::Frozen).unwrap()
                }
                5 => {
                    facts.playback =
                        SourcePlaybackSpan::new(9, None, 7, SourcePlaybackState::Running).unwrap()
                }
                6 => facts.revision = 0,
                _ => facts.pulled_source_end = 16,
            }
            let constructor_error = match case {
                // Playback 8 at physical 9 would advance five playback frames
                // since identity (5, 3) in only four physical frames.
                4 => Some(SourcePlaybackPacketError::InvalidSpan),
                // The claimed admitted prefix 19 exceeds the claimed pull 16.
                7 => Some(SourcePlaybackPacketError::InvalidPrefix),
                _ => None,
            };
            if let Some(expected) = constructor_error {
                assert_eq!(
                    with_facts(good, facts).unwrap_err(),
                    expected,
                    "case {case}"
                );
            } else {
                let invalid = with_facts(good, facts).unwrap();
                assert!(
                    candidate
                        .observe_coordinate_source_replacement(&invalid)
                        .is_err(),
                    "case {case}"
                );
            }
            assert_eq!(candidate, before, "case {case}");
            assert_eq!(output_facts(&r.output), owner_before, "case {case}");
        }
        for case in 0..3 {
            let mut facts = good.source().facts();
            match case {
                0 => facts.revision = 3,
                1 => facts.submitted_source_end = basis(actual).start_physical_frame(),
                _ => facts.submitted_source_end = facts.pulled_source_end + 1,
            }
            let expected = if case == 0 {
                SourcePlaybackPacketError::InvalidRevision
            } else {
                SourcePlaybackPacketError::InvalidPrefix
            };
            assert_eq!(with_facts(good, facts).unwrap_err(), expected);
            assert_eq!(candidate, before);
            assert_eq!(output_facts(&r.output), owner_before);
        }
        for missing in [None, Some(RenderReport { frames: 0, ..fresh })] {
            let mut facts = good.source().facts();
            facts.render = missing;
            let incomplete = with_facts(good, facts).unwrap();
            assert!(!candidate
                .observe_coordinate_source_replacement(&incomplete)
                .unwrap());
            assert_eq!(candidate, before);
        }
        let stale = packet(
            7,
            actual,
            r.identity,
            &r.output,
            fresh,
            units_for_frames(actual, 6),
            1300,
        );
        assert!(candidate
            .observe_coordinate_source_replacement(&stale)
            .is_err());
        assert_eq!(candidate, before);
        if new_frequency.is_some() {
            let wrong_clock = descriptor(basis(actual), Some(11_000));
            let mismatched = packet(
                8,
                wrong_clock,
                r.identity,
                &r.output,
                fresh,
                units_for_frames(wrong_clock, 6),
                1300,
            );
            assert!(candidate
                .observe_coordinate_source_replacement(&mismatched)
                .is_err());
            assert_eq!(candidate, before);
        }
        assert!(candidate
            .observe_coordinate_source_replacement(&good)
            .unwrap());
        let accepted = candidate.clone();
        let mut changed_facts = good.source().facts();
        changed_facts.revision += 2;
        let changed_packet = with_facts(good, changed_facts).unwrap();
        assert!(r
            .pause
            .validate_coordinate_source_replacement(&candidate, &changed_packet)
            .is_err());
        assert_eq!(candidate, accepted);
        for case in 0..6 {
            let mut changed = candidate.clone();
            match case {
                0 => changed.gap += 1,
                1 => changed.frozen += 1,
                2 => changed.start_frame += 1,
                3 => changed.min_host = Some(host(0)),
                4 => {
                    changed.coordinate_binding.as_mut().unwrap().source_identity =
                        SourcePlaybackIdentity::new(RATE, 5, 4).unwrap()
                }
                _ => changed.phase = PausePhase::Running,
            }
            let prior = changed.clone();
            assert!(
                r.pause
                    .validate_coordinate_source_replacement(&changed, &good)
                    .is_err(),
                "case {case}"
            );
            assert_eq!(changed, prior);
            assert_eq!(output_facts(&r.output), owner_before);
        }
    }
}

#[test]
fn actual_resume_repause_and_second_replacement_keep_source_startup_history_and_gap() {
    for (old_frequency, new_frequency, repeated_frequency) in
        [(None, None, None), (Some(3000), Some(7000), Some(11_000))]
    {
        let mut r = rig(old_frequency);
        let config = r.output.mixer().config();
        let descriptor = publish(&mut r, 8, new_frequency, 2000);
        assert_eq!(r.native.authority().epoch().id, 8);
        let paused = r.pause.clone();
        // Previously prepared request/cutoff cannot acknowledge a new epoch.
        assert!(r
            .pause
            .commit_coordinate_adoption(
                r.old_adoption.clone(),
                r.native.authority(),
                &r.old_cutoff,
                host(2100),
                &r.inputs,
            )
            .is_err());
        assert_eq!(r.pause, paused);
        r.output.admit(2).unwrap();
        assert!(r
            .pause
            .request_coordinate(false, r.native.authority())
            .unwrap());
        r.producer.request_pause(false);
        let resumed_report = r.output.render_pending(4).unwrap();
        assert_eq!(
            (
                resumed_report.start_frame,
                resumed_report.playback_start_frame
            ),
            (21, 7)
        );
        assert_eq!(resumed_report.playback_frames, 4);
        assert!(r
            .output
            .pending_samples()
            .iter()
            .all(|sample| *sample > 0.0));
        let adoption = r
            .pause
            .prepare_coordinate_adoption(Some(resumed_report), None)
            .unwrap()
            .unwrap();
        assert_eq!(adoption.physical_frame(), 21);
        assert_eq!(r.pause.phase(), PausePhase::Resuming);
        // Rendering/adoption alone cannot substitute for actual native crossing.
        assert!(r
            .native
            .authority()
            .presented_coordinate_boundary_host(&adoption.exact_cut(), host(2100))
            .unwrap()
            .is_none());
        r.output.admit(4).unwrap();
        r.native
            .admit_coordinate(snapshot(
                8,
                descriptor,
                units_for_frames(descriptor, 10),
                3000,
            ))
            .unwrap();
        r.native
            .authority_mut()
            .record_acquired_prefix(host(3000))
            .unwrap();
        let cut = cutoff(r.native.authority(), &adoption, host(3000), &r.inputs);
        let resumed = r
            .pause
            .commit_coordinate_adoption(adoption, r.native.authority(), &cut, host(3000), &r.inputs)
            .unwrap();
        r.native
            .authority_mut()
            .commit_coordinate_control_cutoff(cut, host(3000), &r.inputs)
            .unwrap();
        assert!(!resumed.paused);
        assert_eq!(resumed.source_identity, r.identity);
        assert_eq!(resumed.playback_frame, 7);
        assert_eq!(r.pause.phase(), PausePhase::Running);
        assert_eq!(r.pause.gap, 14);
        assert_eq!(r.pause.start_frame, 2);
        assert_eq!(
            r.pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
            Timestamp::from_nanos(-12_000_000)
        );
        assert!(r
            .pause
            .request_coordinate(true, r.native.authority())
            .unwrap());
        r.producer.request_pause(true);
        let report = r.output.render_pending(4).unwrap();
        assert_eq!((report.start_frame, report.playback_start_frame), (25, 11));
        assert!(report.paused);
        assert_eq!(report.playback_frames, 0);
        let adoption = r
            .pause
            .prepare_coordinate_adoption(Some(report), None)
            .unwrap()
            .unwrap();
        r.output.admit(1).unwrap();
        r.native
            .admit_coordinate(snapshot(
                8,
                descriptor,
                units_for_frames(descriptor, 14),
                4000,
            ))
            .unwrap();
        r.native
            .authority_mut()
            .record_acquired_prefix(host(4000))
            .unwrap();
        let cut = cutoff(r.native.authority(), &adoption, host(4000), &r.inputs);
        let repaused = r
            .pause
            .commit_coordinate_adoption(adoption, r.native.authority(), &cut, host(4000), &r.inputs)
            .unwrap();
        r.native
            .authority_mut()
            .commit_coordinate_control_cutoff(cut, host(4000), &r.inputs)
            .unwrap();
        assert!(repaused.paused);
        assert_eq!(r.pause.frozen, 11);
        assert_eq!(r.pause.gap, 14);
        assert_eq!(r.output.output_frame_basis().start_physical_frame(), 26);
        assert_eq!(r.output.mixer().frame_cursor(), 29);
        let repeated = publish(&mut r, 9, repeated_frequency, 5000);
        assert_eq!(r.pause.coordinate_binding.unwrap().basis, repeated);
        assert_eq!(r.pause.epoch(), 9);
        assert_eq!(r.pause.phase(), PausePhase::Paused);
        assert_eq!(r.pause.frozen, 11);
        assert_eq!(r.pause.gap, 14);
        assert_eq!(r.pause.start_frame, 2);
        assert_eq!(r.output.mixer().applied_start_frame(), Some(2));
        assert_eq!(r.output.mixer().config(), config);
        let history = r.output.mixer().source_playback_history().unwrap();
        assert_eq!(history.initial_physical_frame(), 5);
        assert_eq!(history.initial_playback_frame(), 3);
        assert_eq!(history.pulled_end_frame(), 33);
        assert_eq!(history.len(), 4);
        assert_eq!(history.span(1).unwrap().physical_start(), 9);
        assert_eq!(history.span(1).unwrap().physical_end(), Some(21));
        assert_eq!(
            history.span(2).unwrap().state(),
            SourcePlaybackState::Running
        );
        assert_eq!(history.span(3).unwrap().physical_start(), 25);
        assert_eq!(history.span(3).unwrap().playback_start(), 11);
        assert_eq!(r.output.pending_samples(), [0.0; 2]);
    }
}
