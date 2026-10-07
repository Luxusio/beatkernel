//! Original native records joined transactionally to the actual audio authority.
use crate::{
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
    local_input::InputMerger,
    native_audio_presentation::{NativeAudioPresentation, NativeAudioSnapshot},
};
use beatkernel::{
    audio::{AudioCounters, OutputFrameBasis, RenderReport, command_queue},
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    runtime::{Runtime, RuntimeProcessingClock},
    time::{
        ClockDomainId, ClockMapper, ClockMappingQuality, ClockPair, ClockPoint, Duration,
        ExtrapolationPolicy, Timestamp,
    },
    transport::{Rate, Transport},
};
use beatkernel_platform::audio::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters,
    asio::{AsioPresentationObservation, MultimediaHostInterval},
    presentation::validation::{
        NativeObservationAdmission, NativePresentationValidator, OriginalNativePresentationEvidence,
    },
};
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    point(1, ns)
}
fn raw(ns: i64) -> ClockPoint {
    point(2, 1_000_000_000 + ns)
}
fn logical(ns: i64) -> ClockPoint {
    point(33, 5_000_000_000 + ns)
}
fn basis() -> OutputFrameBasis {
    OutputFrameBasis::new(raw(0), 1000, 0).unwrap()
}
fn authority(capacity: usize) -> AudioAuthority {
    AudioAuthority::new(
        AudioAuthorityConfig {
            history_capacity: capacity,
            max_observation_age: Duration::from_nanos(1000),
            input_extrapolation: ExtrapolationPolicy::Forbid,
            max_input_ahead: Duration::ZERO,
        },
        AudioAuthorityEpoch {
            id: 1,
            stream_origin: raw(0),
            logical_origin: logical(0),
            host_domain: ClockDomainId(1),
        },
    )
    .unwrap()
}
fn native() -> NativePresentationValidator {
    NativePresentationValidator::new(1, raw(0), ClockDomainId(1))
}
fn owner(capacity: usize) -> NativeAudioPresentation {
    NativeAudioPresentation::new(authority(capacity), native()).unwrap()
}
fn snapshot(position: u64, frequency: u64, host_ns: i64, qpc: u64) -> AudioStreamSnapshot {
    AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters::default(),
        render: None,
        clock: Some(AudioClockSnapshot {
            position,
            frequency,
            qpc_100ns: qpc,
            reading_quality: AudioClockReadingQuality::Accurate,
            host_point: Some(host(host_ns)),
            mapping_quality: ClockMappingQuality::Unknown,
        }),
    }
}
fn wasapi(position: u64, host_ns: i64) -> NativeAudioSnapshot {
    NativeAudioSnapshot {
        epoch: 1,
        basis: basis(),
        evidence: OriginalNativePresentationEvidence::Wasapi {
            snapshot: snapshot(position, 1_000_000_000, host_ns, host_ns as u64 / 100),
            basis: Some(basis()),
        },
    }
}
fn state(owner: &NativeAudioPresentation) -> String {
    let a = owner.authority();
    format!(
        "{:?}",
        (
            a.config(),
            a.epoch(),
            a.latest_observation(),
            a.acquired_prefix(),
            a.closed_host_prefix(),
            a.committed_input_host(),
            a.committed_operation(),
            a.committed_presentation(),
            a.history_len(),
            owner.latest_record()
        )
    )
}
fn merger() -> InputMerger {
    InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap()
}
fn event(ns: i64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), host(ns), 0),
        control: PhysicalControlId::keyboard(7u16),
        state: ButtonState::Down,
    })
}

#[test]
fn original_wasapi_two_pairs_drive_analytic_runtime_mapping_and_preserve_metadata() {
    let mut owner = owner(4);
    let first = wasapi(100, 100);
    let second = wasapi(300, 200);
    assert_eq!(
        owner.admit(first).unwrap(),
        NativeObservationAdmission::Progress
    );
    assert!(
        owner
            .authority()
            .prepare_input(host(125), host(200))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        owner.admit(second).unwrap(),
        NativeObservationAdmission::Progress
    );
    assert_eq!(owner.latest_record().unwrap().evidence(), &second.evidence);
    assert_eq!(
        owner.authority().latest_observation(),
        Some(ClockPair {
            source: raw(300),
            target: host(200)
        })
    );
    let original = event(125);
    let mut merger = merger();
    merger.admit(original.clone(), host(200)).unwrap();
    owner
        .authority_mut()
        .record_acquired_prefix(host(200))
        .unwrap();
    let prepared = owner
        .authority()
        .prepare_input(host(125), host(200))
        .unwrap()
        .unwrap();
    assert_eq!(prepared.output(), logical(150));
    assert_eq!(prepared.mapper().quality(), ClockMappingQuality::Unknown);
    let source =
        beatkernel_bms::parse("#BPM 60\n#WAV01 key.wav\n#00011:01", Default::default()).unwrap();
    let judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(91),
                early: Duration::ZERO,
                late: Duration::from_nanos(200),
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(7u16),
        game_control: GameControlId(0x11),
    }])
    .unwrap();
    let (producer, _consumer) = command_queue(8).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(33),
        ClockDomainId(2),
        Transport::new(logical(0).timestamp, Timestamp::ZERO, Rate::NORMAL),
        bindings,
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let delivered = merger.pop_ready(host(200)).unwrap().unwrap();
    assert_eq!(delivered, original);
    let report = runtime
        .process_input(delivered, prepared.mapper(), raw(400))
        .unwrap();
    assert_eq!(report.song_time, Timestamp::from_nanos(150));
    assert_eq!(report.judge_events.len(), 1);
    assert_eq!(report.audio_at, raw(400));
    assert_eq!(
        report.input.unwrap().meta().original_clock_point,
        Some(host(125))
    );
    owner.authority_mut().commit_input(prepared).unwrap();
    assert_eq!(owner.authority().committed_operation(), Some(logical(150)));
}

#[test]
fn cold_identity_and_preadmitted_pair_mismatches_are_refused_without_reconstruction() {
    for validator in [
        NativePresentationValidator::new(2, raw(0), ClockDomainId(1)),
        NativePresentationValidator::new(1, raw(1), ClockDomainId(1)),
        NativePresentationValidator::new(1, raw(0), ClockDomainId(4)),
    ] {
        assert!(NativeAudioPresentation::new(authority(4), validator).is_err());
    }
    let mut a = authority(4);
    a.observe(
        1,
        ClockPair {
            source: raw(200),
            target: host(100),
        },
    )
    .unwrap();
    let mut n = native();
    let token = n
        .prepare_wasapi(1, snapshot(100, 1_000_000_000, 100, 1), Some(basis()))
        .unwrap();
    n.commit(token).unwrap();
    assert!(NativeAudioPresentation::new(a, n).is_err());
    let mut used = authority(4);
    used.observe(
        1,
        ClockPair {
            source: raw(100),
            target: host(100),
        },
    )
    .unwrap();
    used.observe(
        1,
        ClockPair {
            source: raw(300),
            target: host(200),
        },
    )
    .unwrap();
    used.record_acquired_prefix(host(200)).unwrap();
    let mut pending = merger();
    pending.admit(event(125), host(200)).unwrap();
    let prepared = used.prepare_input(host(125), host(200)).unwrap().unwrap();
    pending.pop_ready(host(200)).unwrap().unwrap();
    used.commit_input(prepared).unwrap();
    let restart = used.prepare_correlation_restart(&pending).unwrap();
    used.commit_correlation_restart(restart, &pending).unwrap();
    assert_eq!(used.history_len(), 0);
    assert_eq!(used.committed_operation(), Some(logical(150)));
    assert!(NativeAudioPresentation::new(used, native()).is_err());
    let mut captured = authority(4);
    captured.record_acquired_prefix(host(200)).unwrap();
    let cold = NativeAudioPresentation::new(captured, native()).unwrap();
    assert_eq!(cold.authority().acquired_prefix(), Some(host(200)));
    let mut only_authority = authority(4);
    only_authority
        .observe(
            1,
            ClockPair {
                source: raw(100),
                target: host(100),
            },
        )
        .unwrap();
    assert!(NativeAudioPresentation::new(only_authority, native()).is_err());
    let mut only_native = native();
    let token = only_native
        .prepare_wasapi(1, snapshot(100, 1_000_000_000, 100, 1), Some(basis()))
        .unwrap();
    only_native.commit(token).unwrap();
    assert!(NativeAudioPresentation::new(authority(4), only_native).is_err());
    let mut a = authority(4);
    a.observe(
        1,
        ClockPair {
            source: raw(100),
            target: host(100),
        },
    )
    .unwrap();
    let mut n = native();
    let token = n
        .prepare_wasapi(1, snapshot(100, 1_000_000_000, 100, 1), Some(basis()))
        .unwrap();
    n.commit(token).unwrap();
    // Equal latest points do not prove that two previously admitted owners
    // share the same complete history; the combined transaction starts cold.
    assert!(NativeAudioPresentation::new(a, n).is_err());
}

#[test]
fn failed_native_preparation_does_not_pin_basis_but_first_success_does() {
    let mut owner = owner(4);
    let before = state(&owner);
    let invalid = NativeAudioSnapshot {
        epoch: 1,
        basis: basis(),
        evidence: OriginalNativePresentationEvidence::Wasapi {
            snapshot: snapshot(100, 0, 100, 1),
            basis: Some(basis()),
        },
    };
    assert!(owner.admit(invalid).is_err());
    assert_eq!(state(&owner), before);
    let accepted_basis = OutputFrameBasis::new(raw(0), 2000, 0).unwrap();
    let first = NativeAudioSnapshot {
        epoch: 1,
        basis: accepted_basis,
        evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
            source: raw(100),
            target: host(100),
        }),
    };
    assert_eq!(
        owner.admit(first).unwrap(),
        NativeObservationAdmission::Progress
    );
    let before = state(&owner);
    let wrong = NativeAudioSnapshot {
        epoch: 1,
        basis: basis(),
        evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
            source: raw(300),
            target: host(200),
        }),
    };
    assert!(owner.admit(wrong).is_err());
    assert_eq!(state(&owner), before);
    owner
        .admit(NativeAudioSnapshot {
            basis: accepted_basis,
            ..wrong
        })
        .unwrap();
    assert_eq!(owner.authority().history_len(), 2);
}

#[test]
fn snapshot_epoch_outer_basis_embedded_basis_and_source_kind_errors_are_atomic() {
    let mut owner = owner(4);
    owner.admit(wasapi(100, 100)).unwrap();
    let before = state(&owner);
    let mut wrong = wasapi(300, 200);
    wrong.epoch = 0;
    assert!(owner.admit(wrong).is_err());
    assert_eq!(state(&owner), before);
    let mut wrong = wasapi(300, 200);
    wrong.basis = OutputFrameBasis::new(raw(1), 1000, 0).unwrap();
    assert!(owner.admit(wrong).is_err());
    assert_eq!(state(&owner), before);
    let missing = NativeAudioSnapshot {
        epoch: 1,
        basis: basis(),
        evidence: OriginalNativePresentationEvidence::Wasapi {
            snapshot: snapshot(300, 1_000_000_000, 200, 2),
            basis: None,
        },
    };
    assert!(owner.admit(missing).is_err());
    assert_eq!(state(&owner), before);
    let changed = OutputFrameBasis::new(raw(0), 2000, 0).unwrap();
    let wrong = NativeAudioSnapshot {
        epoch: 1,
        basis: basis(),
        evidence: OriginalNativePresentationEvidence::Wasapi {
            snapshot: snapshot(300, 1_000_000_000, 200, 2),
            basis: Some(changed),
        },
    };
    assert!(owner.admit(wrong).is_err());
    assert_eq!(state(&owner), before);
    let supplied = NativeAudioSnapshot {
        epoch: 1,
        basis: basis(),
        evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
            source: raw(300),
            target: host(200),
        }),
    };
    assert!(owner.admit(supplied).is_err());
    assert_eq!(state(&owner), before);
    owner.admit(wasapi(300, 200)).unwrap();
    assert_eq!(owner.authority().history_len(), 2);
}

#[test]
fn full_pinned_authority_rejection_preserves_native_record_and_all_authority_state() {
    let mut owner = owner(2);
    owner.admit(wasapi(100, 100)).unwrap();
    owner.admit(wasapi(300, 200)).unwrap();
    let mut merger = merger();
    let original = event(125);
    merger.admit(original.clone(), host(200)).unwrap();
    owner
        .authority_mut()
        .record_acquired_prefix(host(200))
        .unwrap();
    let before = state(&owner);
    let record = owner.latest_record();
    assert!(owner.admit(wasapi(500, 300)).is_err());
    assert_eq!(state(&owner), before);
    assert_eq!(owner.latest_record(), record);
    assert_eq!(merger.peek_ready(host(200)).unwrap(), Some(&original));
    let prepared = owner
        .authority()
        .prepare_input(host(125), host(200))
        .unwrap()
        .unwrap();
    assert_eq!(prepared.output(), logical(150));
    merger.pop_ready(host(200)).unwrap().unwrap();
    owner.authority_mut().commit_input(prepared).unwrap();
    let frontier = owner
        .authority()
        .prepare_frontier(host(200), &merger)
        .unwrap()
        .unwrap();
    owner
        .authority_mut()
        .commit_frontier(frontier, &mut merger)
        .unwrap();
    owner.admit(wasapi(500, 300)).unwrap();
    assert_eq!(owner.authority().history_len(), 2);
    assert_eq!(
        owner.latest_record().unwrap().pair(),
        ClockPair {
            source: raw(500),
            target: host(300)
        }
    );
}

#[test]
fn quantized_native_counter_progress_does_not_refresh_authority_history_or_age() {
    let mut owner = owner(4);
    let native_snapshot = |position, host_ns, qpc| NativeAudioSnapshot {
        epoch: 1,
        basis: basis(),
        evidence: OriginalNativePresentationEvidence::Wasapi {
            snapshot: snapshot(position, 4_000_000_000, host_ns, qpc),
            basis: Some(basis()),
        },
    };
    owner.admit(native_snapshot(1, 100, 1)).unwrap();
    let before = format!("{:?}", owner.authority());
    assert_eq!(
        owner.admit(native_snapshot(2, 200, 2)).unwrap(),
        NativeObservationAdmission::Progress
    );
    assert_eq!(
        owner.latest_record().unwrap().pair(),
        ClockPair {
            source: raw(0),
            target: host(200)
        }
    );
    assert_eq!(format!("{:?}", owner.authority()), before);
    assert_eq!(owner.authority().history_len(), 1);
    let record = owner.latest_record();
    assert_eq!(
        owner.admit(native_snapshot(2, 700, 3)).unwrap(),
        NativeObservationAdmission::Unchanged
    );
    assert_eq!(owner.latest_record(), record);
    assert_eq!(format!("{:?}", owner.authority()), before);
    owner.admit(native_snapshot(4, 300, 3)).unwrap();
    owner
        .authority_mut()
        .record_acquired_prefix(host(300))
        .unwrap();
    let mut merger = merger();
    let before = state(&owner);
    assert!(
        owner
            .authority()
            .prepare_frontier(host(1301), &merger)
            .unwrap()
            .is_none()
    );
    assert_eq!(state(&owner), before);
    let frontier = owner
        .authority()
        .prepare_frontier(host(300), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(frontier.observed_host(), host(300));
    owner
        .authority_mut()
        .commit_frontier(frontier, &mut merger)
        .unwrap();
    assert_eq!(owner.authority().committed_presentation(), Some(logical(1)));
}

fn asio(frame: u64, before: i64, after: i64) -> AsioPresentationObservation {
    AsioPresentationObservation::from_render(
        RenderReport {
            start_frame: frame,
            frames: 16,
            playback_start_frame: frame,
            playback_frames: 16,
            paused: false,
            playback_end_physical_frame: None,
            active_voices: 0,
            pending_commands: 0,
            song_position: Timestamp::ZERO,
            producer_disconnected: false,
            counters: AudioCounters::default(),
        },
        1000,
        MultimediaHostInterval {
            before: host(before),
            after: host(after),
        },
        0,
        0,
        raw(0),
    )
    .unwrap()
}
#[test]
fn asio_coarse_midpoints_keep_full_brackets_and_do_not_refresh_authority() {
    let mut owner = owner(4);
    let first = asio(0, 90, 110);
    let dto = |observation| NativeAudioSnapshot {
        epoch: 1,
        basis: basis(),
        evidence: OriginalNativePresentationEvidence::Asio {
            observation,
            basis: Some(basis()),
        },
    };
    owner.admit(dto(first)).unwrap();
    let before = state(&owner);
    assert_eq!(
        owner.admit(dto(asio(16, 95, 105))).unwrap(),
        NativeObservationAdmission::AwaitingHostProgress
    );
    assert_eq!(state(&owner), before);
    let second = asio(16, 190, 230);
    owner.admit(dto(second)).unwrap();
    assert_eq!(
        owner.latest_record().unwrap().evidence(),
        &OriginalNativePresentationEvidence::Asio {
            observation: second,
            basis: Some(basis())
        }
    );
    assert_eq!(
        owner.authority().latest_observation(),
        Some(ClockPair {
            source: raw(16_000_000),
            target: host(210)
        })
    );
    let before = state(&owner);
    let missing = NativeAudioSnapshot {
        epoch: 1,
        basis: basis(),
        evidence: OriginalNativePresentationEvidence::Asio {
            observation: asio(32, 290, 310),
            basis: None,
        },
    };
    assert!(owner.admit(missing).is_err());
    assert_eq!(state(&owner), before);
}

#[test]
fn supplied_pair_path_and_lifecycle_identity_recheck_preserve_original_sources() {
    let mut owner = owner(4);
    let dto = |output, host_ns| NativeAudioSnapshot {
        epoch: 1,
        basis: basis(),
        evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
            source: raw(output),
            target: host(host_ns),
        }),
    };
    owner.admit(dto(100, 100)).unwrap();
    owner.admit(dto(300, 200)).unwrap();
    let before = state(&owner);
    let changed = OutputFrameBasis::new(raw(0), 2000, 0).unwrap();
    let changed_dto = NativeAudioSnapshot {
        epoch: 1,
        basis: changed,
        evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
            source: raw(500),
            target: host(300),
        }),
    };
    assert!(owner.admit(changed_dto).is_err());
    assert_eq!(state(&owner), before);
    assert_eq!(
        owner.admit(dto(300, 700)).unwrap(),
        NativeObservationAdmission::Unchanged
    );
    assert_eq!(state(&owner), before);
    let merger = merger();
    let next = AudioAuthorityEpoch {
        id: 2,
        stream_origin: point(4, 0),
        logical_origin: logical(400),
        host_domain: ClockDomainId(1),
    };
    let prepared = owner.authority().prepare_epoch(next, &merger).unwrap();
    owner
        .authority_mut()
        .commit_epoch(prepared, &merger)
        .unwrap();
    let before = state(&owner);
    assert!(owner.admit(dto(500, 300)).is_err());
    assert_eq!(state(&owner), before);
}
