//! Retained original Source epochs with actual Mixer history and pending PCM.
//! Supplied native evidence proves software publication, not device acquisition.
//! Subnanosecond native Counter units retain an ordinary Source PCM sample rate.
use super::*;
use crate::{
    audio_authority::AudioAuthorityConfig, gameplay::output::ports::SourcePlaybackIdentity,
};
use beatkernel::{
    audio::*,
    input::{ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent},
    time::{ClockDomainId, ClockPair, Duration, Timestamp},
};
use beatkernel_platform::audio::{
    asio::{AsioPresentationObservation, MultimediaHostInterval},
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    DeviceFormat, NativeOutputState, SampleEncoding, StreamCounters,
};

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

fn snapshot(
    epoch: u64,
    descriptor: AudioAuthorityCoordinateBasis,
    units: u64,
    at: ClockPoint,
) -> CoordinateNativeAudioSnapshot {
    let (basis, evidence, coordinate) = match descriptor {
        AudioAuthorityCoordinateBasis::SourceFrame(basis) => (
            basis,
            OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: basis.point_at_stream_frame(units).unwrap(),
                target: at,
            }),
            NativeOutputCoordinate::StreamFrame { frame: units },
        ),
        AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency } => (
            basis,
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
                        host_point: Some(at),
                        mapping_quality: beatkernel::time::ClockMappingQuality::Unknown,
                    }),
                },
                basis: Some(basis),
            },
            NativeOutputCoordinate::Counter {
                position: units,
                frequency,
            },
        ),
        _ => panic!("Source fixture retains a Source descriptor"),
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

fn asio_snapshot(
    epoch: u64,
    basis: OutputFrameBasis,
    frame: u64,
    before: i64,
    after: i64,
) -> CoordinateNativeAudioSnapshot {
    let physical = basis.start_physical_frame() + frame;
    let observation = AsioPresentationObservation::from_render(
        RenderReport {
            start_frame: physical,
            frames: 1,
            playback_start_frame: 9,
            playback_frames: 0,
            paused: true,
            playback_end_physical_frame: None,
            active_voices: 0,
            pending_commands: 0,
            song_position: Timestamp::ZERO,
            producer_disconnected: false,
            counters: AudioCounters::default(),
        },
        basis.sample_rate(),
        MultimediaHostInterval {
            before: host(before),
            after: host(after),
        },
        0,
        0,
        basis.origin(),
    )
    .unwrap();
    CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch,
            basis,
            evidence: OriginalNativePresentationEvidence::Asio {
                observation,
                basis: Some(basis),
            },
        },
        NativeOutputCoordinate::StreamFrame { frame },
    )
    .unwrap()
}

struct Rig {
    producer: CommandProducer,
    practice: PracticeController,
    output: NativeOutputState,
    creation: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    owner: NativeAudioPresentation,
    inputs: InputMerger,
}

fn rig(rate: u32, counter_frequency: Option<u64>) -> Rig {
    let format = AudioFormat::new(rate, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(8, 256, 1).unwrap()).unwrap();
    let program =
        PreparedPracticeProgram::new(&bank, vec![], PracticeLimits::new(8, 1, 64, 8, 16).unwrap())
            .unwrap();
    let (mut practice, endpoint) = practice_queue(&program).unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::from_nanos(100),
            AudioLimits::new(8, 4, 8, 256, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer
        .install_practice(
            program,
            endpoint,
            PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(100_000_000), false)
                .unwrap(),
        )
        .unwrap();
    mixer.render(&mut [0.0; 5]).unwrap();
    while practice.try_pop_projected_receipt().is_ok() {}
    mixer.enable_source_playback_history(16).unwrap();
    let creation = mixer.output_frame_basis();
    assert_eq!(creation.start_physical_frame(), 5);
    let identity = SourcePlaybackIdentity::new(rate, 5, mixer.playback_frame_cursor()).unwrap();
    let mut output = NativeOutputState::new(
        mixer,
        DeviceFormat::new(rate, 1, SampleEncoding::Float32, None).unwrap(),
        None,
        16,
    )
    .unwrap_or_else(|_| panic!("valid complete Source owner"));
    let active = output.render_pending(4).unwrap();
    assert_eq!((active.start_frame, active.playback_frames), (5, 4));
    output.admit(4).unwrap();
    producer.request_pause(true);
    let paused = output.render_pending(8).unwrap();
    assert!(paused.paused);
    assert_eq!((paused.start_frame, paused.playback_start_frame), (9, 9));
    assert_eq!(paused.playback_frames, 0);
    output.admit(3).unwrap();
    assert_eq!(output.output_frame_basis().start_physical_frame(), 12);
    assert_eq!(output.mixer().frame_cursor(), 17);
    assert!(output.paused_tail_admissible());
    let history = output.mixer().source_playback_history().unwrap();
    assert_eq!(history.initial_physical_frame(), 5);
    assert_eq!(history.initial_playback_frame(), 5);
    assert_eq!(history.span(1).unwrap().physical_start(), 9);
    assert_eq!(
        history.span(1).unwrap().state(),
        SourcePlaybackState::Frozen
    );

    let old_descriptor = descriptor(creation, counter_frequency);
    let epoch = AudioAuthorityEpoch {
        id: 7,
        stream_origin: creation.point_at_stream_frame(0).unwrap(),
        logical_origin: point(3, 500),
        host_domain: ClockDomainId(1),
    };
    let mut owner = NativeAudioPresentation::new(
        AudioAuthority::new_with_coordinates(
            AudioAuthorityConfig {
                history_capacity: 8,
                max_observation_age: Duration::from_nanos(1000),
                ..AudioAuthorityConfig::default()
            },
            epoch,
            old_descriptor,
        )
        .unwrap(),
        NativePresentationValidator::new(7, epoch.stream_origin, epoch.host_domain),
    )
    .unwrap();
    let first_units = u64::from(counter_frequency.is_some());
    for (units, at) in [(first_units, 100), (first_units + 1, 200)] {
        assert_eq!(
            owner
                .admit_coordinate(snapshot(7, old_descriptor, units, host(at)))
                .unwrap(),
            NativeObservationAdmission::Progress
        );
    }
    owner.pin_practice_mixer_basis(creation).unwrap();
    let mut inputs = InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap();
    let original = event(125, 1);
    inputs.admit(original.clone(), host(250)).unwrap();
    owner
        .authority_mut()
        .record_acquired_prefix(host(250))
        .unwrap();
    let mapped = owner
        .authority()
        .prepare_coordinate_input(host(125), host(250))
        .unwrap()
        .unwrap();
    assert_eq!(inputs.pop_ready(host(250)).unwrap(), Some(original));
    owner
        .authority_mut()
        .commit_coordinate_input(mapped, host(250))
        .unwrap();
    let frontier = owner
        .authority()
        .prepare_coordinate_frontier(host(250), &inputs)
        .unwrap()
        .unwrap();
    owner
        .authority_mut()
        .commit_coordinate_frontier(frontier, host(250), &mut inputs)
        .unwrap();
    Rig {
        producer,
        practice,
        output,
        creation,
        identity,
        owner,
        inputs,
    }
}

fn event(nanos: i64, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), host(nanos), sequence),
        control: PhysicalControlId::keyboard(7u16),
        state: ButtonState::Down,
    })
}

fn next_epoch(owner: &NativeAudioPresentation, basis: OutputFrameBasis) -> AudioAuthorityEpoch {
    let previous = owner.authority().epoch();
    let stream_origin = basis.point_at_stream_frame(0).unwrap();
    AudioAuthorityEpoch {
        id: previous.id + 1,
        stream_origin,
        logical_origin: previous.logical_output(stream_origin).unwrap(),
        host_domain: previous.host_domain,
    }
}

fn originals(
    epoch: u64,
    descriptor: AudioAuthorityCoordinateBasis,
) -> [CoordinateNativeAudioSnapshot; 2] {
    let first_units = u64::from(matches!(
        descriptor,
        AudioAuthorityCoordinateBasis::SourceCounter { .. }
    ));
    [
        snapshot(epoch, descriptor, first_units, host(260)),
        snapshot(epoch, descriptor, first_units + 1, host(300)),
    ]
}

fn state(owner: &NativeAudioPresentation) -> String {
    format!("{owner:?}")
}

fn watermarks(owner: &NativeAudioPresentation) -> String {
    let authority = owner.authority();
    format!(
        "{:?}",
        (
            authority.acquired_prefix(),
            authority.closed_host_prefix(),
            authority.committed_input_host(),
            authority.committed_operation(),
            authority.committed_presentation(),
            authority.committed_coordinate_operation(),
            authority.committed_coordinate_presentation(),
        )
    )
}

fn output_facts(output: &NativeOutputState) -> String {
    let mixer = output.mixer();
    let history = mixer.source_playback_history().unwrap();
    let spans: Vec<_> = (0..history.len())
        .map(|index| history.span(index))
        .collect();
    format!(
        "{:?}",
        (
            mixer.config(),
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            output.format(),
            output.max_frames(),
            output.output_frame_basis(),
            output.pending_report(),
            output.admitted_frames(),
            output.pending_samples(),
            (
                history.source_rate(),
                history.initial_physical_frame(),
                history.initial_playback_frame(),
                history.retained_start_frame(),
                history.pulled_end_frame(),
                history.retirement_revision(),
                history.capacity(),
                spans,
            ),
        )
    )
}

fn prepare(rig: &Rig, frequency: Option<u64>) -> PreparedCoordinateSourceNativeAudioEpoch {
    let association = rig
        .owner
        .prepare_coordinate_source_owner_association(&rig.output, rig.identity)
        .unwrap();
    let basis = rig.output.output_frame_basis();
    let next = next_epoch(&rig.owner, basis);
    let descriptor = descriptor(basis, frequency);
    rig.owner
        .prepare_coordinate_source_output_epoch(
            next,
            descriptor,
            originals(next.id, descriptor),
            &association,
            host(320),
            &rig.inputs,
        )
        .unwrap()
}

fn later_practice_boundary(rig: &mut Rig, frequency: Option<u64>) {
    rig.practice
        .try_request(PracticeRequest {
            id: 12,
            expected_generation: 1,
            at_playback_frame: 9,
            region: PracticeRegion::new(
                Timestamp::from_nanos(1_000_000),
                Timestamp::from_nanos(100_000_000),
                false,
            )
            .unwrap(),
        })
        .unwrap();
    rig.output.admit(5).unwrap();
    rig.producer.request_pause(false);
    let resumed = rig.output.render_pending(4).unwrap();
    assert_eq!((resumed.start_frame, resumed.playback_start_frame), (17, 9));
    assert!(!resumed.paused);
    rig.output.admit(4).unwrap();
    let projected = rig.practice.try_pop_projected_receipt().unwrap();
    assert_eq!(projected.receipt.kind, PracticeBoundaryKind::Requested);
    assert_eq!(projected.receipt.physical_frame, 17);
    assert!(rig
        .owner
        .prepare_projected_practice_boundary(projected, host(325))
        .unwrap()
        .is_none());
    let units = frequency.map_or(6, |frequency| frequency * 6 / 1000);
    let descriptor = rig.owner.authority().coordinate_basis().unwrap();
    rig.owner
        .admit_coordinate(snapshot(8, descriptor, units, host(700)))
        .unwrap();
    let plain = rig
        .owner
        .prepare_practice_boundary(projected.receipt, host(700))
        .unwrap()
        .unwrap();
    let projected_cut = rig
        .owner
        .prepare_projected_practice_boundary(projected, host(700))
        .unwrap()
        .unwrap();
    assert_eq!(plain.host(), projected_cut.host());
    assert_eq!(plain.raw_output(), point(2, 17_000_100));
    assert!(plain
        .coordinate_query()
        .unwrap()
        .unwrap()
        .compare_target_time(TargetTime::from_frames(17, 1000).unwrap())
        .is_eq());
    assert_eq!(rig.owner.practice_mixer_basis(), Some(rig.creation));
    let history = rig.output.mixer().source_playback_history().unwrap();
    assert_eq!(history.initial_physical_frame(), 5);
    assert_eq!(history.initial_playback_frame(), 5);
    assert_eq!(history.span(1).unwrap().physical_start(), 9);
    assert_eq!(history.span(2).unwrap().physical_start(), 17);
}

#[test]
fn source_borrowed_commit_preserves_original_pin_history_pending_pcm_and_all_frontiers() {
    for (old_frequency, frequency) in [(None, None), (Some(3000), Some(7000))] {
        let mut rig = rig(1000, old_frequency);
        let association = rig
            .owner
            .prepare_coordinate_source_owner_association(&rig.output, rig.identity)
            .unwrap();
        association.validate_recovered_state(&rig.output).unwrap();
        assert_eq!(association.basis(), rig.output.output_frame_basis());
        assert_eq!(association.practice_mixer_basis(), rig.creation);
        assert_eq!(association.source_identity(), rig.identity);
        assert_eq!(association.pulled_source_end(), 17);
        assert_eq!(association.playback_frame(), 9);
        assert_ne!(association.basis(), association.practice_mixer_basis());
        assert_ne!(association.basis(), rig.output.mixer().output_frame_basis());
        let history = rig.output.mixer().source_playback_history().unwrap();
        assert!(
            history.span(1).unwrap().physical_start() < association.basis().start_physical_frame()
        );
        let token = prepare(&rig, frequency);
        let before = state(&rig.owner);
        let frontiers = watermarks(&rig.owner);
        let output = output_facts(&rig.output);
        let inputs = format!("{:?}", rig.inputs);
        assert_ne!(rig.inputs.source_count(), 0);
        let capacity = rig.owner.authority().config().history_capacity;
        assert_eq!(token.basis(), association.basis());
        assert_eq!(token.practice_mixer_basis(), rig.creation);
        assert_eq!(
            token.coordinate_basis(),
            descriptor(token.basis(), frequency)
        );
        assert_eq!(token.epoch(), 8);
        assert_eq!(token.snapshots(), originals(8, token.coordinate_basis()));
        rig.owner
            .validate_coordinate_source_output_epoch(&token, host(325), &rig.inputs)
            .unwrap();
        assert_eq!(state(&rig.owner), before);
        rig.owner
            .commit_coordinate_source_output_epoch_borrowed(&token, host(325), &rig.inputs)
            .unwrap();
        assert_eq!(
            rig.owner.authority().epoch(),
            token.authority_token().next_epoch()
        );
        assert_eq!(
            rig.owner.authority().coordinate_basis(),
            Some(token.coordinate_basis())
        );
        assert_eq!(rig.owner.basis(), Some(token.basis()));
        assert_eq!(rig.owner.practice_mixer_basis(), Some(rig.creation));
        assert_eq!(rig.owner.latest_record(), Some(token.latest_record()));
        assert_eq!(rig.owner.authority().history_len(), 2);
        assert_eq!(rig.owner.authority().config().history_capacity, capacity);
        assert_eq!(watermarks(&rig.owner), frontiers);
        assert_eq!(output_facts(&rig.output), output);
        assert_eq!(format!("{:?}", rig.inputs), inputs);
        association.validate_recovered_state(&rig.output).unwrap();
        if let Some(frequency) = frequency {
            let OriginalNativePresentationEvidence::Wasapi { snapshot, basis } =
                *rig.owner.latest_record().unwrap().evidence()
            else {
                panic!("Counter publication retains original WASAPI metadata")
            };
            assert_eq!(basis, Some(token.basis()));
            assert_eq!(snapshot.clock.unwrap().frequency, frequency);
            assert_eq!(snapshot.clock.unwrap().position, 2);
            assert_ne!(frequency, u64::from(rig.creation.sample_rate()));
        }
        later_practice_boundary(&mut rig, frequency);
    }
}

#[test]
fn source_frame_counter_and_asio_keep_native_units_and_counter_subnanosecond_progress() {
    for evidence in 0..3 {
        let old_frequency = (evidence == 1).then_some(3_000_000_000);
        let frequency = (evidence == 1).then_some(4_000_000_000);
        let mut rig = rig(1000, old_frequency);
        let association = rig
            .owner
            .prepare_coordinate_source_owner_association(&rig.output, rig.identity)
            .unwrap();
        let basis = association.basis();
        let descriptor = descriptor(basis, frequency);
        let snapshots = if evidence == 2 {
            [
                asio_snapshot(8, basis, 0, 259, 261),
                asio_snapshot(8, basis, 1, 299, 301),
            ]
        } else {
            originals(8, descriptor)
        };
        let pairs = snapshots.map(|snapshot| {
            let validator = NativePresentationValidator::new(
                8,
                basis.point_at_stream_frame(0).unwrap(),
                ClockDomainId(1),
            );
            match snapshot.coordinate() {
                NativeOutputCoordinate::StreamFrame { frame }
                    if matches!(
                        snapshot.snapshot().evidence,
                        OriginalNativePresentationEvidence::SuppliedPair(_)
                    ) =>
                {
                    let OriginalNativePresentationEvidence::SuppliedPair(pair) =
                        snapshot.snapshot().evidence
                    else {
                        unreachable!()
                    };
                    validator
                        .prepare_frame_pair(8, pair, NativeFrameBasis::Source(basis), frame)
                        .unwrap()
                }
                _ => prepare_native_snapshot(&validator, snapshot.snapshot()).unwrap(),
            }
            .correlation_pair()
            .unwrap()
        });
        assert_eq!(basis.sample_rate(), 1000);
        if evidence == 1 {
            assert_eq!(pairs[0].source, pairs[1].source);
            assert_eq!(
                descriptor,
                AudioAuthorityCoordinateBasis::SourceCounter {
                    basis,
                    frequency: 4_000_000_000,
                }
            );
        } else {
            assert!(pairs[0].source.timestamp < pairs[1].source.timestamp);
        }
        assert!(snapshots[0]
            .source_query()
            .unwrap()
            .compare_interpolated_time(snapshots[1].source_query().unwrap())
            .is_lt());
        assert_ne!(snapshots[0].coordinate(), snapshots[1].coordinate());
        assert_eq!(pairs[0].target, host(260));
        assert_eq!(pairs[1].target, host(300));
        let token = rig
            .owner
            .prepare_coordinate_source_output_epoch(
                next_epoch(&rig.owner, basis),
                descriptor,
                snapshots,
                &association,
                host(320),
                &rig.inputs,
            )
            .unwrap();
        let frontiers = watermarks(&rig.owner);
        rig.owner
            .commit_coordinate_source_output_epoch_borrowed(&token, host(325), &rig.inputs)
            .unwrap();
        assert_eq!(watermarks(&rig.owner), frontiers);
        assert_eq!(rig.owner.latest_record().unwrap().pair(), pairs[1]);
        assert_eq!(
            rig.owner.latest_record().unwrap().evidence(),
            &snapshots[1].snapshot().evidence
        );
        assert_eq!(
            rig.owner
                .authority()
                .latest_coordinate_observation()
                .unwrap()
                .coordinate(),
            Some(snapshots[1].coordinate())
        );
    }
}

#[test]
fn source_owner_association_rejects_wrong_identity_and_changed_pending_or_render_state() {
    for counter in [false, true] {
        let frequency = counter.then_some(3000);
        let rig = rig(1000, frequency);
        let before = state(&rig.owner);
        let output = output_facts(&rig.output);
        for identity in [
            SourcePlaybackIdentity::new(1001, 5, 5).unwrap(),
            SourcePlaybackIdentity::new(1000, 6, 5).unwrap(),
            SourcePlaybackIdentity::new(1000, 5, 4).unwrap(),
        ] {
            assert!(rig
                .owner
                .prepare_coordinate_source_owner_association(&rig.output, identity)
                .is_err());
            assert_eq!(state(&rig.owner), before);
            assert_eq!(output_facts(&rig.output), output);
        }
        for mutation in 0..3 {
            let mut rig = self::rig(1000, frequency);
            let association = rig
                .owner
                .prepare_coordinate_source_owner_association(&rig.output, rig.identity)
                .unwrap();
            match mutation {
                0 => rig.output.admit(1).unwrap(),
                1 => {
                    rig.output.admit(5).unwrap();
                    rig.output.render_pending(1).unwrap();
                }
                2 => rig
                    .output
                    .reconfigure(
                        DeviceFormat::new(1000, 1, SampleEncoding::Float32, None).unwrap(),
                        None,
                        8,
                    )
                    .unwrap(),
                _ => unreachable!(),
            }
            let before = state(&rig.owner);
            let output = output_facts(&rig.output);
            assert!(association.validate_recovered_state(&rig.output).is_err());
            assert_eq!(state(&rig.owner), before);
            assert_eq!(output_facts(&rig.output), output);
        }
    }
}

#[test]
fn stale_authority_native_basis_pin_epoch_and_merger_reject_source_publication_atomically() {
    for old_frequency in [None, Some(3000)] {
        for mutation in 0..9 {
            let mut rig = rig(1000, old_frequency);
            let token = prepare(&rig, old_frequency.map(|_| 7000));
            match mutation {
                0 => rig
                    .owner
                    .authority_mut()
                    .record_acquired_prefix(host(251))
                    .unwrap(),
                1 => {
                    let descriptor = rig.owner.authority().coordinate_basis().unwrap();
                    let original = snapshot(7, descriptor, 3, host(240));
                    let native = match original.coordinate() {
                        NativeOutputCoordinate::StreamFrame { frame } => {
                            let OriginalNativePresentationEvidence::SuppliedPair(pair) =
                                original.snapshot().evidence
                            else {
                                unreachable!()
                            };
                            rig.owner
                                .validator
                                .prepare_frame_pair(
                                    7,
                                    pair,
                                    NativeFrameBasis::Source(rig.creation),
                                    frame,
                                )
                                .unwrap()
                        }
                        _ => prepare_native_snapshot(&rig.owner.validator, original.snapshot())
                            .unwrap(),
                    };
                    rig.owner.validator.commit(native).unwrap();
                }
                2 => rig.owner.basis = None,
                3 => rig.owner.practice_mixer_basis = None,
                4 => {
                    let rebind = rig
                        .owner
                        .validator
                        .prepare_rebind(8, rig.owner.authority().epoch().stream_origin)
                        .unwrap();
                    rig.owner.validator.commit_rebind(rebind).unwrap();
                }
                5 => rig.inputs.admit(event(280, 2), host(320)).unwrap(),
                6 => {
                    let original = event(280, 2);
                    rig.inputs.admit(original.clone(), host(320)).unwrap();
                    assert_eq!(rig.inputs.pop_ready(host(320)).unwrap(), Some(original));
                    assert_eq!(rig.inputs.pending(), 0);
                }
                7 => rig.inputs.commit(host(280)).unwrap(),
                8 => {
                    rig.owner.source_owner_identity =
                        Some(SourcePlaybackIdentity::new(1000, 6, 5).unwrap())
                }
                _ => unreachable!(),
            }
            let before = state(&rig.owner);
            let output = output_facts(&rig.output);
            let inputs = format!("{:?}", rig.inputs);
            assert!(rig
                .owner
                .validate_coordinate_source_output_epoch(&token, host(325), &rig.inputs)
                .is_err());
            assert!(rig
                .owner
                .commit_coordinate_source_output_epoch_borrowed(&token, host(325), &rig.inputs)
                .is_err());
            assert_eq!(state(&rig.owner), before);
            assert_eq!(output_facts(&rig.output), output);
            assert_eq!(format!("{:?}", rig.inputs), inputs);
        }
    }
}

#[test]
fn source_publication_rechecks_prepared_clock_and_does_not_reuse_a_published_token() {
    for old_frequency in [None, Some(3000)] {
        let mut rig = rig(1000, old_frequency);
        let token = prepare(&rig, old_frequency.map(|_| 7000));
        let before = state(&rig.owner);
        for now in [host(319), host(1301), point(4, 325)] {
            assert!(rig
                .owner
                .validate_coordinate_source_output_epoch(&token, now, &rig.inputs)
                .is_err());
            assert!(rig
                .owner
                .commit_coordinate_source_output_epoch_borrowed(&token, now, &rig.inputs)
                .is_err());
            assert_eq!(state(&rig.owner), before);
        }
        rig.owner
            .commit_coordinate_source_output_epoch_borrowed(&token, host(325), &rig.inputs)
            .unwrap();
        let published = state(&rig.owner);
        assert!(rig
            .owner
            .commit_coordinate_source_output_epoch_borrowed(&token, host(325), &rig.inputs)
            .is_err());
        assert_eq!(state(&rig.owner), published);
    }
}

#[test]
fn source_originals_reject_duplicate_torn_future_frequency_domain_and_epoch_without_mutation() {
    for old_frequency in [None, Some(3000)] {
        let rig = rig(1000, old_frequency);
        let association = rig
            .owner
            .prepare_coordinate_source_owner_association(&rig.output, rig.identity)
            .unwrap();
        let basis = association.basis();
        let frequency = old_frequency.map(|_| 7000);
        let descriptor = descriptor(basis, frequency);
        let valid = originals(8, descriptor);
        let second_units = if frequency.is_some() { 2 } else { 1 };
        let other_basis = OutputFrameBasis::new(basis.origin(), 1001, 12).unwrap();
        let mut candidates = vec![
            [valid[0], valid[0]],
            [valid[1], valid[0]],
            [valid[0], snapshot(7, descriptor, second_units, host(300))],
            [valid[0], snapshot(8, descriptor, second_units, host(330))],
            [valid[0], snapshot(8, descriptor, second_units, host(260))],
            [
                valid[0],
                snapshot(8, descriptor, second_units, point(4, 300)),
            ],
            [
                valid[0],
                snapshot(
                    8,
                    self::descriptor(other_basis, frequency),
                    second_units,
                    host(300),
                ),
            ],
        ];
        if frequency.is_some() {
            candidates.push([
                valid[0],
                snapshot(8, self::descriptor(basis, Some(7001)), 2, host(300)),
            ]);
            let mut torn = valid[1].snapshot();
            let OriginalNativePresentationEvidence::Wasapi {
                snapshot: mut native,
                basis,
            } = torn.evidence
            else {
                unreachable!()
            };
            let mut clock = native.clock.unwrap();
            clock.qpc_100ns = 1;
            native.clock = Some(clock);
            torn.evidence = OriginalNativePresentationEvidence::Wasapi {
                snapshot: native,
                basis,
            };
            candidates.push([
                valid[0],
                CoordinateNativeAudioSnapshot::new(torn, valid[1].coordinate()).unwrap(),
            ]);
        } else {
            // Each record is valid alone; a single created stream cannot switch
            // between supplied frame evidence and ASIO render evidence.
            candidates.push([valid[0], asio_snapshot(8, basis, 1, 299, 301)]);
        }
        let before = state(&rig.owner);
        let output = output_facts(&rig.output);
        let inputs = format!("{:?}", rig.inputs);
        for snapshots in candidates {
            assert!(rig
                .owner
                .prepare_coordinate_source_output_epoch(
                    next_epoch(&rig.owner, basis),
                    descriptor,
                    snapshots,
                    &association,
                    host(320),
                    &rig.inputs,
                )
                .is_err());
            assert_eq!(state(&rig.owner), before);
            assert_eq!(output_facts(&rig.output), output);
            assert_eq!(format!("{:?}", rig.inputs), inputs);
        }
    }
}

#[test]
fn source_descriptor_and_scalar_fences_preserve_the_live_authority_and_original_pin() {
    for old_frequency in [None, Some(3000)] {
        let rig = rig(1000, old_frequency);
        let association = rig
            .owner
            .prepare_coordinate_source_owner_association(&rig.output, rig.identity)
            .unwrap();
        let basis = association.basis();
        let frequency = old_frequency.map(|_| 7000);
        let descriptor = descriptor(basis, frequency);
        let next = next_epoch(&rig.owner, basis);
        let before = state(&rig.owner);
        assert!(rig
            .owner
            .prepare_output_epoch(
                next,
                basis,
                originals(8, descriptor).map(|snapshot| snapshot.snapshot()),
                host(320),
                &rig.inputs
            )
            .is_err());
        let wrong_class = if frequency.is_some() {
            AudioAuthorityCoordinateBasis::SourceFrame(basis)
        } else {
            AudioAuthorityCoordinateBasis::SourceCounter {
                basis,
                frequency: 7000,
            }
        };
        let wrong_basis = OutputFrameBasis::new(basis.origin(), basis.sample_rate(), 13).unwrap();
        let wrong_origin = OutputFrameBasis::new(point(2, 101), basis.sample_rate(), 12).unwrap();
        let wrong_domain = OutputFrameBasis::new(point(4, 100), basis.sample_rate(), 12).unwrap();
        let target = AudioAuthorityCoordinateBasis::TargetFrame(
            TargetFrameBasis::new(
                basis.origin(),
                TargetTime::from_frames(12, 1000).unwrap(),
                1000,
            )
            .unwrap(),
        );
        for (next, candidate) in [
            (AudioAuthorityEpoch { id: 7, ..next }, descriptor),
            (
                AudioAuthorityEpoch {
                    host_domain: ClockDomainId(4),
                    ..next
                },
                descriptor,
            ),
            (
                AudioAuthorityEpoch {
                    logical_origin: point(3, next.logical_origin.timestamp.as_nanos() + 1),
                    ..next
                },
                descriptor,
            ),
            (next, wrong_class),
            (next, target),
            (next, self::descriptor(wrong_basis, frequency)),
            (next, self::descriptor(wrong_origin, frequency)),
            (next, self::descriptor(wrong_domain, frequency)),
        ] {
            assert!(rig
                .owner
                .prepare_coordinate_source_output_epoch(
                    next,
                    candidate,
                    originals(8, descriptor),
                    &association,
                    host(320),
                    &rig.inputs
                )
                .is_err());
            assert_eq!(state(&rig.owner), before);
        }
    }
}

#[test]
fn equal_native_record_cannot_hide_a_changed_original_frame_basis_and_position() {
    let mut rig = rig(1000, None);
    let token = prepare(&rig, None);
    let record = rig.owner.latest_record().unwrap();
    let mut validator = NativePresentationValidator::new(
        7,
        rig.creation.point_at_stream_frame(0).unwrap(),
        ClockDomainId(1),
    );
    // This independently valid 2-kHz grid has the same creation point and maps
    // relative frame 2 to the original 1-kHz grid's relative frame 1. The equal
    // rounded record cannot authorize replacing its original exact frame grid.
    let changed_basis = OutputFrameBasis::new(rig.creation.origin(), 2000, 10).unwrap();
    assert_eq!(
        changed_basis.point_at_stream_frame(0).unwrap(),
        rig.creation.point_at_stream_frame(0).unwrap()
    );
    assert_eq!(
        changed_basis.point_at_stream_frame(2).unwrap(),
        record.pair().source
    );
    let changed = validator
        .prepare_frame_pair(7, record.pair(), NativeFrameBasis::Source(changed_basis), 2)
        .unwrap();
    validator.commit(changed).unwrap();
    assert_eq!(validator.latest_record(), rig.owner.latest_record());
    assert_ne!(
        validator.latest_frame_position(),
        rig.owner.validator.latest_frame_position()
    );
    rig.owner.validator = validator;
    let before = state(&rig.owner);
    assert!(rig
        .owner
        .validate_coordinate_source_output_epoch(&token, host(325), &rig.inputs)
        .is_err());
    assert!(rig
        .owner
        .commit_coordinate_source_output_epoch_borrowed(&token, host(325), &rig.inputs)
        .is_err());
    assert_eq!(state(&rig.owner), before);
}

#[test]
fn retained_suffix_or_created_metadata_does_not_supply_two_fresh_native_observations() {
    for old_frequency in [None, Some(3000)] {
        let rig = rig(1000, old_frequency);
        let association = rig
            .owner
            .prepare_coordinate_source_owner_association(&rig.output, rig.identity)
            .unwrap();
        assert!(rig.output.pending_report().unwrap().paused);
        assert!(rig.output.pending_frames() > 0);
        assert!(rig.output.paused_tail_admissible());
        let descriptor = descriptor(association.basis(), old_frequency.map(|_| 7000));
        let created = snapshot(8, descriptor, u64::from(old_frequency.is_some()), host(260));
        let before = state(&rig.owner);
        let output = output_facts(&rig.output);
        assert!(rig
            .owner
            .prepare_coordinate_source_output_epoch(
                next_epoch(&rig.owner, association.basis()),
                descriptor,
                [created, created],
                &association,
                host(320),
                &rig.inputs,
            )
            .is_err());
        let old = originals(7, rig.owner.authority().coordinate_basis().unwrap());
        assert!(rig
            .owner
            .prepare_coordinate_source_output_epoch(
                next_epoch(&rig.owner, association.basis()),
                descriptor,
                old,
                &association,
                host(320),
                &rig.inputs,
            )
            .is_err());
        assert_eq!(state(&rig.owner), before);
        assert_eq!(output_facts(&rig.output), output);
    }
}

#[test]
fn repeated_source_replacements_keep_the_original_creation_pin_and_merged_frozen_span() {
    for old_frequency in [None, Some(3000)] {
        let mut rig = rig(1000, old_frequency);
        let first = prepare(&rig, old_frequency.map(|_| 7000));
        rig.owner
            .commit_coordinate_source_output_epoch_borrowed(&first, host(325), &rig.inputs)
            .unwrap();
        rig.output.admit(5).unwrap();
        rig.output.render_pending(8).unwrap();
        rig.output.admit(2).unwrap();
        let association = rig
            .owner
            .prepare_coordinate_source_owner_association(&rig.output, rig.identity)
            .unwrap();
        assert_eq!(association.previous_epoch(), 8);
        assert_eq!(association.practice_mixer_basis(), rig.creation);
        assert_eq!(association.basis().start_physical_frame(), 19);
        let history = rig.output.mixer().source_playback_history().unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history.span(1).unwrap().physical_start(), 9);
        assert_eq!(history.pulled_end_frame(), 25);
        let descriptor = descriptor(association.basis(), old_frequency.map(|_| 11000));
        let first_units = u64::from(old_frequency.is_some());
        let originals = [
            snapshot(9, descriptor, first_units, host(800)),
            snapshot(9, descriptor, first_units + 1, host(850)),
        ];
        let token = rig
            .owner
            .prepare_coordinate_source_output_epoch(
                next_epoch(&rig.owner, association.basis()),
                descriptor,
                originals,
                &association,
                host(900),
                &rig.inputs,
            )
            .unwrap();
        let accepted = watermarks(&rig.owner);
        let output = output_facts(&rig.output);
        rig.owner
            .commit_coordinate_source_output_epoch_borrowed(&token, host(900), &rig.inputs)
            .unwrap();
        assert_eq!(rig.owner.authority().epoch().id, 9);
        assert_eq!(rig.owner.basis(), Some(association.basis()));
        assert_eq!(rig.owner.practice_mixer_basis(), Some(rig.creation));
        assert_eq!(watermarks(&rig.owner), accepted);
        assert_eq!(output_facts(&rig.output), output);
        assert_eq!(token.snapshots(), originals);
    }
}

#[test]
fn advanced_basis_cannot_be_substituted_for_the_original_pin_without_a_source_association() {
    for frequency in [None, Some(3000)] {
        let mut rig = rig(1000, frequency);
        let original = rig.creation;
        rig.owner.practice_mixer_basis = Some(rig.output.output_frame_basis());
        let before = state(&rig.owner);
        assert!(rig
            .owner
            .prepare_coordinate_source_owner_association(&rig.output, rig.identity)
            .is_err());
        assert_eq!(state(&rig.owner), before);
        rig.owner.practice_mixer_basis = Some(original);
        assert!(rig
            .owner
            .prepare_coordinate_source_owner_association(&rig.output, rig.identity)
            .is_ok());
    }
}

#[test]
fn matching_grid_without_original_source_history_cannot_create_an_owner_association() {
    let rig = rig(1000, None);
    let config = rig.output.mixer().config();
    let bank = SampleBank::new(config.format(), PcmLimits::new(8, 256, 1).unwrap()).unwrap();
    let (_producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(config, bank, consumer).unwrap();
    mixer.render(&mut [0.0; 5]).unwrap();
    assert_eq!(mixer.output_frame_basis(), rig.creation);
    assert!(mixer.source_playback_history().is_none());
    let missing_history = NativeOutputState::from_mixer(mixer);
    let before = state(&rig.owner);
    let error = rig
        .owner
        .prepare_coordinate_source_owner_association(&missing_history, rig.identity)
        .unwrap_err();
    assert!(error.to_string().contains("original playback history"));
    assert_eq!(state(&rig.owner), before);
}

#[test]
fn source_native_publication_binds_merger_registry_origin_domain_and_capacity() {
    for old_frequency in [None, Some(3000)] {
        let mut rig = rig(1000, old_frequency);
        let token = prepare(&rig, old_frequency.map(|_| 7000));
        let before = state(&rig.owner);
        for (device, domain, origin, capacity) in [
            (DeviceId(10), ClockDomainId(1), 0, 8),
            (DeviceId(9), ClockDomainId(4), 0, 8),
            (DeviceId(9), ClockDomainId(1), 1, 8),
            (DeviceId(9), ClockDomainId(1), 0, 4),
        ] {
            let mut inputs = InputMerger::new(
                domain,
                ClockPoint {
                    domain,
                    timestamp: Timestamp::from_nanos(origin),
                },
                vec![device],
                capacity,
            )
            .unwrap();
            let original = PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(
                    device,
                    ClockPoint {
                        domain,
                        timestamp: Timestamp::from_nanos(125),
                    },
                    1,
                ),
                control: PhysicalControlId::keyboard(7u16),
                state: ButtonState::Down,
            });
            let at = ClockPoint {
                domain,
                timestamp: Timestamp::from_nanos(250),
            };
            inputs.admit(original.clone(), at).unwrap();
            assert_eq!(inputs.pop_ready(at).unwrap(), Some(original));
            inputs.commit(at).unwrap();
            assert_eq!(inputs.pending(), 0);
            assert_eq!(inputs.source_count(), rig.inputs.source_count());
            let merger = format!("{inputs:?}");
            assert!(rig
                .owner
                .validate_coordinate_source_output_epoch(&token, host(325), &inputs)
                .is_err());
            assert!(rig
                .owner
                .commit_coordinate_source_output_epoch_borrowed(&token, host(325), &inputs)
                .is_err());
            assert_eq!(state(&rig.owner), before);
            assert_eq!(format!("{inputs:?}"), merger);
        }
    }
}

#[test]
fn malformed_original_wasapi_metadata_cannot_be_wrapped_as_valid_source_coordinates() {
    let rig = rig(1000, Some(3000));
    let descriptor = descriptor(rig.output.output_frame_basis(), Some(7000));
    let original = snapshot(8, descriptor, 1, host(260));
    let before = state(&rig.owner);
    for mutation in 0..4 {
        let mut raw = original.snapshot();
        let OriginalNativePresentationEvidence::Wasapi {
            snapshot: mut native,
            basis,
        } = raw.evidence
        else {
            unreachable!()
        };
        match mutation {
            0 => native.clock.as_mut().unwrap().frequency = 7001,
            1 => native.clock.as_mut().unwrap().position = 2,
            2 => native.clock.as_mut().unwrap().host_point = None,
            3 => native.status = AudioStreamStatus::Stopped,
            _ => unreachable!(),
        }
        raw.evidence = OriginalNativePresentationEvidence::Wasapi {
            snapshot: native,
            basis,
        };
        assert!(CoordinateNativeAudioSnapshot::new(raw, original.coordinate()).is_err());
        assert_eq!(state(&rig.owner), before);
    }
}
