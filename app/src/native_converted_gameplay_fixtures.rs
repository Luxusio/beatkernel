//! Actual converted owner and original native-counter scripts, never a source-grid clock.
use crate::{
    audio_authority::AudioAuthorityConfig, local_input::InputMerger,
    native_audio_presentation::TargetNativeAudioSnapshot,
    native_audio_startup::new_target_audio_presentation,
};
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPair, ClockPoint, Timestamp},
};
use beatkernel::{
    input::{ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent},
    time::{Duration, ExtrapolationPolicy},
};
use beatkernel_platform::audio::presentation::validation::{
    NativeObservationAdmission, OriginalNativePresentationEvidence,
};
use beatkernel_platform::{
    audio::{ConvertedNativeOutputState, DeviceFormat, SampleEncoding},
    linux::{alsa_presentation_pair_with_target_basis, AlsaNativeTimestamp, AlsaTimingSnapshot},
};

pub(crate) fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
pub(crate) fn rig(
    source_rate: u32,
    target_rate: u32,
    gate: Option<u64>,
    end: Option<u64>,
    origin: i64,
) -> (CommandProducer, ConvertedNativeOutputState) {
    rig_gate(source_rate, target_rate, gate.map(Some), end, origin)
}
fn rig_gate(
    source_rate: u32,
    target_rate: u32,
    gate: Option<Option<u64>>,
    end: Option<u64>,
    origin: i64,
) -> (CommandProducer, ConvertedNativeOutputState) {
    let format = AudioFormat::new(source_rate, 1).unwrap();
    let limits = PcmLimits::new(4096, 4096, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 512], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = if gate.is_some() {
        command_queue_with_start_gate(16).unwrap()
    } else {
        command_queue(16).unwrap()
    };
    if let Some(Some(gate)) = gate {
        producer.schedule_start_at(gate).unwrap();
    }
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::from_nanos(origin),
            gain: 1.0,
        })
        .unwrap();
    let mut config = MixerConfig::new(
        format,
        ClockDomainId(2),
        Timestamp::from_nanos(origin),
        AudioLimits::new(16, 4, 16, 256, 16).unwrap(),
    );
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    let mixer = Mixer::new(config, bank, consumer).unwrap();
    let device = DeviceFormat::new(target_rate, 1, SampleEncoding::Float32, None).unwrap();
    let owner = ConvertedNativeOutputState::new(
        mixer,
        device,
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        128,
    )
    .unwrap_or_else(|_| panic!("valid exact converted fixture owner"));
    (producer, owner)
}
pub(crate) fn native(
    basis: TargetFrameBasis,
    played: u64,
    host_ns: i64,
) -> (AlsaTimingSnapshot, ClockPair) {
    let snapshot = AlsaTimingSnapshot {
        native_state: 3,
        submitted_frames: played + 7,
        delay_frames: 7,
        available_frames: 99,
        native_htstamp: AlsaNativeTimestamp {
            seconds: host_ns / 1_000_000_000,
            nanoseconds: host_ns % 1_000_000_000,
        },
        native_timestamp: Some(point(1, host_ns)),
        query_started: point(1, host_ns + 1_000),
        query_finished: point(1, host_ns + 1_001),
        estimated_played_frames: Some(played),
        quality: beatkernel::time::ClockMappingQuality::Unknown,
        timestamp_mode: 1,
        timestamp_type: 1,
    };
    let pair = alsa_presentation_pair_with_target_basis(snapshot, basis)
        .unwrap()
        .unwrap();
    (snapshot, pair)
}

fn presentation(
    basis: TargetFrameBasis,
) -> crate::native_audio_presentation::NativeAudioPresentation {
    new_target_audio_presentation(
        7,
        basis,
        ClockDomainId(1),
        point(33, 5_000_000_000),
        AudioAuthorityConfig {
            history_capacity: 8,
            max_observation_age: Duration::from_nanos(10_000_000),
            input_extrapolation: ExtrapolationPolicy::Forbid,
            max_input_ahead: Duration::ZERO,
        },
    )
    .unwrap()
}
fn snapshot(basis: TargetFrameBasis, played: u64, host_ns: i64) -> TargetNativeAudioSnapshot {
    TargetNativeAudioSnapshot {
        epoch: 7,
        basis,
        evidence: OriginalNativePresentationEvidence::SuppliedPair(
            native(basis, played, host_ns).1,
        ),
    }
}
fn state(owner: &crate::native_audio_presentation::NativeAudioPresentation) -> String {
    let a = owner.authority();
    format!(
        "{:?}",
        (
            a.epoch(),
            a.latest_observation(),
            a.history_len(),
            a.acquired_prefix(),
            a.closed_host_prefix(),
            a.committed_input_host(),
            a.committed_operation(),
            a.committed_presentation(),
            owner.latest_record(),
            owner.target_basis()
        )
    )
}

#[test]
fn genuine_target_native_pairs_drive_input_authority_and_preserve_original_acquisition_metadata() {
    let (_producer, mut output) = rig(44_100, 48_000, None, None, -123);
    let basis = output.target_frame_basis();
    output.render_pending(128).unwrap();
    output.admit(128).unwrap();
    assert!(output.mixer().frame_cursor() != 128);
    let mut owner = presentation(basis);
    let first = snapshot(basis, 48, 1_001_000_000);
    let second = snapshot(basis, 96, 1_002_000_000);
    assert_eq!(
        owner.admit_target(first).unwrap(),
        NativeObservationAdmission::Progress
    );
    assert_eq!(
        owner.admit_target(second).unwrap(),
        NativeObservationAdmission::Progress
    );
    assert_eq!(owner.target_basis(), Some(basis));
    assert_eq!(owner.latest_record().unwrap().evidence(), &second.evidence);
    let mut meta = EventMeta::new(DeviceId(9), point(1, 1_001_500_000), 23);
    meta.native = Some(beatkernel::input::NativeEventMeta {
        backend: beatkernel::input::BackendId(7),
        code: Some(7),
        timestamp: Some(point(99, 1_001_499_999)),
    });
    let original = PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(7),
        state: ButtonState::Down,
    });
    let mut merger = InputMerger::new(
        ClockDomainId(1),
        point(1, 1_000_000_000),
        vec![DeviceId(9)],
        8,
    )
    .unwrap();
    merger
        .admit(original.clone(), point(1, 1_002_000_000))
        .unwrap();
    owner
        .authority_mut()
        .record_acquired_prefix(point(1, 1_002_000_000))
        .unwrap();
    let prepared = owner
        .authority()
        .prepare_input(point(1, 1_001_500_000), point(1, 1_002_000_000))
        .unwrap()
        .unwrap();
    assert_eq!(prepared.original(), point(1, 1_001_500_000));
    assert_eq!(prepared.output(), point(33, 5_001_500_000));
    assert_eq!(
        merger.pop_ready(point(1, 1_002_000_000)).unwrap().unwrap(),
        original
    );
    owner.authority_mut().commit_input(prepared).unwrap();
    assert_eq!(
        owner.authority().committed_operation(),
        Some(point(33, 5_001_500_000))
    );
}

#[test]
fn wrong_domain_epoch_and_changed_target_basis_refuse_before_freshness_or_basis_commit() {
    let (_producer, output) = rig(44_100, 48_000, None, None, -123);
    let basis = output.target_frame_basis();
    let mut owner = presentation(basis);
    let before = state(&owner);
    let mut invalid = snapshot(basis, 48, 1_001_000_000);
    if let OriginalNativePresentationEvidence::SuppliedPair(ref mut pair) = invalid.evidence {
        pair.target.domain = ClockDomainId(99);
    }
    assert!(owner.admit_target(invalid).is_err());
    assert_eq!(state(&owner), before);
    invalid = snapshot(basis, 48, 1_001_000_000);
    invalid.epoch = 8;
    assert!(owner.admit_target(invalid).is_err());
    assert_eq!(state(&owner), before);
    owner
        .admit_target(snapshot(basis, 48, 1_001_000_000))
        .unwrap();
    let pinned = state(&owner);
    let altered = TargetFrameBasis::new(basis.origin(), basis.start_time(), 32_000).unwrap();
    assert!(owner
        .admit_target(snapshot(altered, 96, 1_002_000_000))
        .is_err());
    assert_eq!(state(&owner), pinned);
    assert_eq!(owner.target_basis(), Some(basis));
    assert_eq!(
        owner.authority().latest_observation(),
        Some(native(basis, 48, 1_001_000_000).1)
    );
    owner
        .admit_target(snapshot(basis, 96, 1_002_000_000))
        .unwrap();
}

#[test]
fn target_stream_basis_retains_fractional_first_unsent_start_and_freezes_on_native_progress() {
    let (_producer, mut output) = rig(44_100, 48_000, None, None, -123);
    output.render_pending(8).unwrap();
    output.admit(2).unwrap();
    let basis = output.target_frame_basis();
    assert_eq!(
        basis.start_time(),
        TargetTime::from_frames(2, 48_000).unwrap()
    );
    assert_ne!(output.mixer().frame_cursor(), 2);
    let mut owner = presentation(basis);
    let actual = snapshot(basis, 3, 1_000_000_100);
    owner.admit_target(actual).unwrap();
    assert_eq!(
        owner.authority().epoch().stream_origin.timestamp.as_nanos(),
        -123 + 2 * 1_000_000_000 / 48_000
    );
    assert_eq!(
        owner
            .authority()
            .latest_observation()
            .unwrap()
            .source
            .timestamp
            .as_nanos(),
        -123 + 5 * 1_000_000_000 / 48_000
    );
    assert_eq!(owner.target_basis(), Some(basis));
    let fake = TargetFrameBasis::new(
        basis.origin(),
        TargetTime::from_frames(output.mixer().frame_cursor(), 44_100).unwrap(),
        48_000,
    )
    .unwrap();
    let before = state(&owner);
    assert!(owner
        .admit_target(snapshot(fake, 4, 1_000_000_200))
        .is_err());
    assert_eq!(state(&owner), before);
}

#[test]
fn exact_target_replacement_prepares_then_commits_without_rewriting_input_or_native_history_on_refusal(
) {
    let (_producer, mut output) = rig(44_100, 48_000, None, None, -123);
    let old_basis = output.target_frame_basis();
    output.render_pending(128).unwrap();
    output.admit(128).unwrap();
    let mut owner = presentation(old_basis);
    owner
        .admit_target(snapshot(old_basis, 48, 1_001_000_000))
        .unwrap();
    owner
        .admit_target(snapshot(old_basis, 96, 1_002_000_000))
        .unwrap();
    output
        .reconfigure(
            DeviceFormat::new(32_000, 1, SampleEncoding::Float32, None).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            128,
        )
        .unwrap();
    let next_basis = output.target_frame_basis();
    assert_eq!(
        next_basis.start_time(),
        TargetTime::from_frames(128, 48_000).unwrap()
    );
    let next = crate::audio_authority::AudioAuthorityEpoch {
        id: 8,
        stream_origin: next_basis.point_at_stream_frame(0).unwrap(),
        logical_origin: point(33, 5_000_000_000 + 128 * 1_000_000_000 / 48_000),
        host_domain: ClockDomainId(1),
    };
    let snaps = [
        TargetNativeAudioSnapshot {
            epoch: 8,
            basis: next_basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(
                native(next_basis, 16, 1_003_000_000).1,
            ),
        },
        TargetNativeAudioSnapshot {
            epoch: 8,
            basis: next_basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(
                native(next_basis, 32, 1_004_000_000).1,
            ),
        },
    ];
    let mut merger = InputMerger::new(
        ClockDomainId(1),
        point(1, 1_000_000_000),
        vec![DeviceId(9)],
        8,
    )
    .unwrap();
    let before = state(&owner);
    let prepared = owner
        .prepare_target_output_epoch(next, next_basis, snaps, point(1, 1_004_000_000), &merger)
        .unwrap();
    assert_eq!(state(&owner), before);
    assert_eq!(prepared.basis(), next_basis);
    assert_eq!(prepared.epoch(), 8);
    assert_eq!(prepared.snapshots(), snaps);
    assert!(owner
        .validate_target_output_epoch(&prepared, point(99, 1_004_000_000), &merger)
        .is_err());
    assert_eq!(state(&owner), before);
    assert!(owner
        .validate_target_output_epoch(&prepared, point(1, 1_020_000_001), &merger)
        .is_err());
    assert_eq!(state(&owner), before);
    let event = PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), point(1, 1_003_500_000), 1),
        control: PhysicalControlId::keyboard(7),
        state: ButtonState::Down,
    });
    merger
        .admit(event.clone(), point(1, 1_004_000_000))
        .unwrap();
    assert!(owner
        .validate_target_output_epoch(&prepared, point(1, 1_004_000_000), &merger)
        .is_err());
    assert_eq!(state(&owner), before);
    assert_eq!(
        merger.pop_ready(point(1, 1_004_000_000)).unwrap().unwrap(),
        event
    );
    owner
        .commit_target_output_epoch(prepared, point(1, 1_004_000_000), &merger)
        .unwrap();
    assert_eq!(owner.target_basis(), Some(next_basis));
    assert_eq!(owner.authority().epoch(), next);
    assert_eq!(
        owner.latest_record().unwrap().evidence(),
        &snaps[1].evidence
    );
    let committed = state(&owner);
    assert!(owner
        .admit_target(snapshot(old_basis, 120, 1_005_000_000))
        .is_err());
    assert_eq!(state(&owner), committed);
}

#[test]
fn output_progress_after_target_replacement_preparation_invalidates_token_atomically() {
    let (_producer, output) = rig(44_100, 48_000, None, None, 0);
    let basis = output.target_frame_basis();
    let mut owner = presentation(basis);
    owner
        .admit_target(snapshot(basis, 48, 1_001_000_000))
        .unwrap();
    owner
        .admit_target(snapshot(basis, 96, 1_002_000_000))
        .unwrap();
    let next_basis = TargetFrameBasis::new(
        basis.origin(),
        TargetTime::from_frames(128, 48_000).unwrap(),
        32_000,
    )
    .unwrap();
    let next = crate::audio_authority::AudioAuthorityEpoch {
        id: 8,
        stream_origin: next_basis.point_at_stream_frame(0).unwrap(),
        logical_origin: point(33, 5_010_000_000),
        host_domain: ClockDomainId(1),
    };
    let snaps = [
        TargetNativeAudioSnapshot {
            epoch: 8,
            basis: next_basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(
                native(next_basis, 16, 1_004_000_000).1,
            ),
        },
        TargetNativeAudioSnapshot {
            epoch: 8,
            basis: next_basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(
                native(next_basis, 32, 1_005_000_000).1,
            ),
        },
    ];
    let merger = InputMerger::new(
        ClockDomainId(1),
        point(1, 1_000_000_000),
        vec![DeviceId(9)],
        8,
    )
    .unwrap();
    let prepared = owner
        .prepare_target_output_epoch(next, next_basis, snaps, point(1, 1_005_000_000), &merger)
        .unwrap();
    owner
        .admit_target(snapshot(basis, 120, 1_003_000_000))
        .unwrap();
    let before = state(&owner);
    assert!(owner
        .commit_target_output_epoch(prepared, point(1, 1_005_000_000), &merger)
        .is_err());
    assert_eq!(state(&owner), before);
}

#[test]
fn target_start_plan_arms_real_source_gate_but_publishes_only_actual_mapped_target_origin() {
    use crate::native_start::{HostStartWindow, TargetOutputStartPlan};
    let (mut producer, mut output) = rig_gate(44_100, 48_000, Some(None), None, 0);
    let basis = output.target_frame_basis();
    output.render_pending(8).unwrap();
    output.admit(8).unwrap();
    let first = native(basis, 4, 1_000_000_000 + 4 * 1_000_000_000 / 48_000).1;
    let second = native(basis, 8, 1_000_000_000 + 8 * 1_000_000_000 / 48_000).1;
    let window = HostStartWindow::new(point(1, 1_001_000_000), point(1, 1_001_050_000)).unwrap();
    let plan = TargetOutputStartPlan::from_owner(window, first, second, &output, 16, 0).unwrap();
    // Like the legacy start plan, selection uses the latest window endpoint.
    // 1050us corresponds to source position 1050*44100/1e6=46.305,
    // so the integer source gate is 47.
    // That gate first belongs to target frame ceil(47*160/147)=52.
    assert_eq!(plan.selected_source_frame(), 47);
    assert_eq!(plan.target_basis(), output.target_frame_basis());
    assert_eq!(
        plan.selected_output().unwrap(),
        point(2, 52 * 1_000_000_000 / 48_000)
    );
    assert!(plan.validate_startup(output.boundaries()).is_err());
    producer
        .schedule_start_at(plan.selected_source_frame())
        .unwrap();
    output.render_pending(64).unwrap();
    assert_eq!(output.mixer().applied_start_frame(), Some(47));
    assert!(output.pending_samples()[..44]
        .iter()
        .all(|sample| *sample == 0.0));
    assert!(output.pending_samples()[44] > 0.0);
    let facts = output.boundaries();
    let actual = plan.validate_startup(facts).unwrap();
    assert_eq!(actual, point(2, 52 * 1_000_000_000 / 48_000));
    assert_ne!(actual, point(2, 47 * 1_000_000_000 / 44_100));
    // An otherwise genuine mapped start cannot be reinterpreted using the
    // target rate (or another source grid). Source identity belongs to the
    // converter snapshot used to select this immutable startup plan.
    assert_eq!(facts.source_rate, 44_100);
    let planned_basis = plan.target_basis();
    let continuation_basis = output.target_frame_basis();
    for source_rate in [0, 24_000, 48_000] {
        let mut wrong_source = facts;
        wrong_source.source_rate = source_rate;
        assert_eq!(wrong_source.origin, facts.origin);
        assert_eq!(wrong_source.startup, facts.startup);
        assert!(plan.validate_startup(wrong_source).is_err());
        assert_eq!(output.boundaries(), facts);
        assert_eq!(plan.validate_startup(facts).unwrap(), actual);
        assert_eq!(plan.selected_source_frame(), 47);
        assert_eq!(plan.target_basis(), planned_basis);
        assert_eq!(output.target_frame_basis(), continuation_basis);
    }
    let mut forged = facts;
    forged.startup.as_mut().unwrap().source_frame = 44;
    assert!(plan.validate_startup(forged).is_err());
    forged = facts;
    forged.origin = Some(point(2, 1));
    assert!(plan.validate_startup(forged).is_err());
}

#[test]
fn target_start_safety_uses_pulled_source_frontier_and_actual_target_buffer_duration_without_mutation(
) {
    use crate::native_start::{HostStartWindow, TargetOutputStartPlan, TargetStartSnapshot};
    let (_producer, mut output) = rig_gate(44_100, 48_000, Some(None), None, 0);
    let basis = output.target_frame_basis();
    output.render_pending(8).unwrap();
    output.admit(8).unwrap();
    let snapshot = TargetStartSnapshot::from_owner(&output);
    assert_eq!(snapshot.source_rate, 44_100);
    assert_eq!(
        snapshot.generated_time,
        TargetTime::from_frames(8, 48_000).unwrap()
    );
    assert_eq!(
        snapshot.source_position,
        output.converter_owner().source_position()
    );
    assert_eq!(snapshot.pulled_source_frame, output.mixer().frame_cursor());
    assert!(snapshot.pulled_source_frame > snapshot.source_position.frame);
    let first = native(basis, 4, 1_000_000_000 + 4 * 1_000_000_000 / 48_000).1;
    let second = native(basis, 8, 1_000_000_000 + 8 * 1_000_000_000 / 48_000).1;
    let before = (
        output.converter_owner().target_time(),
        output.converter_owner().source_position(),
        output.mixer().frame_cursor(),
        output.mixer().counters(),
    );
    let close = HostStartWindow::new(point(1, 1_000_200_000), point(1, 1_000_210_000)).unwrap();
    assert!(TargetOutputStartPlan::from_snapshot(close, first, second, snapshot, 16, 0).is_err());
    // Sixteen target frames last 16/48000 seconds: their source safety extent
    // is ceil(16*44100/48000)=15, rather than sixteen source-grid frames.
    let window = HostStartWindow::new(point(1, 1_000_530_000), point(1, 1_000_580_000)).unwrap();
    let plan =
        TargetOutputStartPlan::from_snapshot(window, first, second, snapshot, 16, 0).unwrap();
    // Earliest 530us permits source gate 24 and satisfies the 9+15 safety
    // frontier. Selection uses latest 580us: source ceil(25.578)=26,
    // then mapped target ceil(26*160/147)=29.
    assert_eq!(plan.selected_source_frame(), 26);
    assert_eq!(
        plan.selected_output().unwrap(),
        point(2, 29 * 1_000_000_000 / 48_000)
    );
    let mut invalid = snapshot;
    invalid.source_rate = 0;
    assert!(TargetOutputStartPlan::from_snapshot(window, first, second, invalid, 16, 0).is_err());
    assert_eq!(
        (
            output.converter_owner().target_time(),
            output.converter_owner().source_position(),
            output.mixer().frame_cursor(),
            output.mixer().counters()
        ),
        before
    );
}

#[test]
fn exact_duration_start_projection_avoids_an_extra_source_frame_from_intermediate_target_rounding()
{
    use crate::native_start::{HostStartWindow, TargetOutputStartPlan};
    let (mut producer, mut output) = rig_gate(44_100, 48_000, Some(None), None, 0);
    let basis = output.target_frame_basis();
    output.render_pending(8).unwrap();
    output.admit(8).unwrap();
    let first = native(basis, 4, 1_000_000_000 + 4 * 1_000_000_000 / 48_000).1;
    let second = native(basis, 8, 1_000_000_000 + 8 * 1_000_000_000 / 48_000).1;
    let before = (
        output.converter_owner().target_time(),
        output.converter_owner().source_position(),
        output.mixer().frame_cursor(),
        output.mixer().counters(),
    );
    let window = HostStartWindow::new(point(1, 1_000_580_000), point(1, 1_000_589_000)).unwrap();
    let plan = TargetOutputStartPlan::from_owner(window, first, second, &output, 1, 0).unwrap();
    // Exact latest duration gives source 589000*44100/1e9=25.9749;
    // one source ceil selects 26. Rounding first to target frame 29
    // would instead sample source 29*147/160=26.64375 and select 27.
    assert_eq!(plan.selected_source_frame(), 26);
    assert_eq!(
        plan.selected_output().unwrap(),
        point(2, 29 * 1_000_000_000 / 48_000)
    );
    assert_eq!(
        (
            output.converter_owner().target_time(),
            output.converter_owner().source_position(),
            output.mixer().frame_cursor(),
            output.mixer().counters()
        ),
        before
    );
    assert_eq!(output.mixer().applied_start_frame(), None);
    producer
        .schedule_start_at(plan.selected_source_frame())
        .unwrap();
    output.render_pending(64).unwrap();
    assert_eq!(output.mixer().applied_start_frame(), Some(26));
    // Eight target frames already elapsed. The first eligible target frame is
    // ceil(26*48000/44100)=29, at offset 21 in this actual retained block.
    assert!(output.pending_samples()[..21]
        .iter()
        .all(|sample| *sample == 0.0));
    assert!(output.pending_samples()[21] > 0.0);
    let startup = output.boundaries().startup.unwrap();
    assert_eq!(startup.source_frame, 26);
    assert_eq!(
        startup.target_time,
        TargetTime::from_frames(29, 48_000).unwrap()
    );
    assert_eq!(
        plan.validate_startup(output.boundaries()).unwrap(),
        point(2, 29 * 1_000_000_000 / 48_000)
    );
}

mod shared_held_pump {
    use super::*;
    use crate::{
        bgm::{BgmConfig, BgmFeeder},
        gameplay_competition::{NoopGroupCompetition, NoopSoloCompetition},
        gauge::BmsGauge,
        live_pause::LivePauseObservation,
        local_players::PlayerId,
        local_runtime::{MemberConfig, RuntimeGroup, SoloRuntime},
        native_cohort::{
            run_cohort_audio_with_results_and_ports, AudioCohortSession, GameplayPlayerState,
        },
        native_end::{EndBoundary, NativeEnd},
        native_gameplay::{
            run_gameplay_audio_with_result_and_ports, AudioGameplayConfig, AudioGameplaySession,
            GameplaySession, InputBatch, NativeGameplayConfig, NativeGameplayDevice,
            NativeGameplayResult,
        },
        native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost, PauseState},
        native_pump_control::NativePumpControl,
        playback_pause::{NativePause, PausePhase},
    };
    use beatkernel::{
        input::BindingMap,
        judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
        telemetry::InputDeliveryTelemetry,
        transport::{Rate, Transport},
    };
    use beatkernel_platform::audio::presentation::discipline::PresentationDiscipline;
    use std::{cell::Cell, collections::VecDeque, rc::Rc};

    struct Device {
        output: ConvertedNativeOutputState,
        basis: TargetFrameBasis,
        step: Rc<Cell<u64>>,
        held: bool,
        pending: bool,
        fail: Option<bool>,
        effects: Vec<(u64, bool, u64)>,
        frozen_source: Option<u64>,
    }
    impl Device {
        fn pair(&self) -> ClockPair {
            let frame = match self.step.get() {
                0 => 0,
                1 => 1,
                2 => 3, // Source adoption exists, but the mapped target pause is frame four.
                3 => 4,
                n => 9 + (n - 3) * 8,
            };
            native(
                self.basis,
                frame,
                1_000_000_000 + (frame * 1_000_000_000 / 48_000) as i64,
            )
            .1
        }
    }
    impl NativeGameplayDevice for Device {
        fn output_replacement_pending(&self) -> bool {
            self.pending && matches!(self.step.get(), 4 | 5)
        }
        fn set_audio_held(&mut self, held: bool) -> NativeGameplayResult<()> {
            assert!(
                self.output.mixer().pause_requested(),
                "effect must precede producer resume"
            );
            if held {
                let boundary = self.output.boundaries().pause.unwrap();
                assert!(
                    self.pair().source.timestamp
                        >= boundary.target_time.point(self.basis.origin())?.timestamp
                );
                assert_eq!(
                    self.step.get(),
                    3,
                    "no held selection before mapped native ACK"
                );
            }
            self.effects.push((
                self.step.get(),
                held,
                self.output.mixer().playback_frame_cursor(),
            ));
            if self.fail == Some(held) {
                return Err("scripted native held effect refusal".into());
            }
            self.held = held;
            if held {
                self.frozen_source = Some(self.output.mixer().frame_cursor());
            }
            Ok(())
        }
        fn observe_audio(
            &mut self,
            p: &mut crate::native_audio_presentation::NativeAudioPresentation,
        ) -> NativeGameplayResult<()> {
            self.step.set(self.step.get() + 1);
            let count = if self.step.get() == 1 { 1 } else { 8 };
            if self.step.get() != 3 {
                if self.held {
                    let before = (
                        self.output.converter_owner().source_position(),
                        self.output.mixer().frame_cursor(),
                        self.output.last_real_source_report(),
                    );
                    self.output.render_held_pending(count)?;
                    assert_eq!(
                        (
                            self.output.converter_owner().source_position(),
                            self.output.mixer().frame_cursor(),
                            self.output.last_real_source_report()
                        ),
                        before
                    );
                    assert_eq!(Some(self.output.mixer().frame_cursor()), self.frozen_source);
                } else {
                    self.output.render_pending(count)?;
                }
                self.output.admit(count)?;
            }
            if self.step.get() == 1 {
                let startup = self.output.boundaries().startup.unwrap();
                assert_eq!(startup.source_frame, 0);
                assert_eq!(startup.target_time, TargetTime::from_frames(0, 48_000)?);
            }
            if self.step.get() == 2 {
                assert!(self.output.last_real_source_report().unwrap().paused);
                assert_eq!(
                    self.output.boundaries().pause.unwrap().target_time,
                    TargetTime::from_frames(4, 48_000)?
                );
                assert!(self.effects.is_empty());
            }
            if self.pending && matches!(self.step.get(), 5 | 6) {
                assert!(
                    self.output.mixer().pause_requested(),
                    "replacement pending must retain producer pause"
                );
                assert!(self.held);
            }
            p.admit_target(TargetNativeAudioSnapshot {
                epoch: 0,
                basis: self.basis,
                evidence: OriginalNativePresentationEvidence::SuppliedPair(self.pair()),
            })?;
            Ok(())
        }
        fn audio_pause_observation(
            &mut self,
            p: &crate::native_audio_presentation::NativeAudioPresentation,
            _: ClockPoint,
        ) -> NativeGameplayResult<LivePauseObservation> {
            let pair = p
                .authority()
                .latest_observation()
                .ok_or("missing actual target observation")?;
            Ok(LivePauseObservation::Target {
                epoch: 0,
                basis: self.basis,
                facts: self.output.boundaries(),
                source: self.output.last_real_source_report(),
                pair,
            })
        }
        fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
            Ok(self.output.last_real_source_report())
        }
        fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
            Ok(self.pair().target)
        }
        fn acquire(
            &mut self,
            _: &mut VecDeque<PhysicalInputEvent>,
        ) -> NativeGameplayResult<InputBatch> {
            Ok(InputBatch {
                completed_through: Some(self.pair().target),
                backlog: false,
                closed: self.step.get() >= 9,
            })
        }
        fn observe_end(
            &mut self,
            _: &mut NativeEnd,
            _: &PresentationDiscipline,
            _: Option<RenderReport>,
        ) -> NativeGameplayResult<Option<EndBoundary>> {
            panic!("unbounded fixture")
        }
        fn observe(&mut self, _: &mut PresentationDiscipline) -> NativeGameplayResult<()> {
            panic!("legacy presentation cannot authorize converted gameplay")
        }
        fn seed_resume(
            &mut self,
            _: &mut PresentationDiscipline,
            _: ClockPair,
        ) -> NativeGameplayResult<()> {
            panic!("legacy estimator cannot seed target resume")
        }
        fn publish_paused_audio_output(
            &mut self,
            _: crate::gameplay_presentation::GameplayAudioOutputContext<'_>,
            _: ClockPoint,
        ) -> NativeGameplayResult<bool> {
            Ok(false)
        }
        fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
            panic!("converted source scheduling must use real source callbacks")
        }
    }
    struct Host {
        step: Rc<Cell<u64>>,
        pauses: Vec<PauseState>,
    }
    impl NativeGameplayHost for Host {
        fn cancelled(&self) -> bool {
            false
        }
        fn pause_requested(&self) -> bool {
            self.step.get() <= 3
        }
        fn retry_pause_publication(&mut self) {}
        fn publish_pause(&mut self, state: PauseState) {
            self.pauses.push(state);
        }
        fn publish_section_end(&mut self, _: Timestamp) {}
        fn publish_report(
            &mut self,
            _: &beatkernel::runtime::RuntimeReport,
        ) -> NativeGameplayResult<()> {
            Ok(())
        }
        fn publish_local_reports(
            &mut self,
            _: &[crate::local_runtime::PlayerReport],
        ) -> NativeGameplayResult<()> {
            Ok(())
        }
        fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
    }
    struct Control;
    impl NativePumpControl for Control {
        type Moment = u64;
        fn now(&mut self) -> NativeGameplayResult<u64> {
            Ok(0)
        }
        fn checked_add(value: u64, d: std::time::Duration) -> Option<u64> {
            value.checked_add(d.as_secs())
        }
        fn wait(&mut self, _: std::time::Duration) -> NativeGameplayResult<()> {
            Ok(())
        }
    }
    fn member(id: u32) -> MemberConfig {
        let chart = beatkernel_bms::parse(
            "#BPM 120\n#WAV01 original.wav\n#00111:01\n",
            Default::default(),
        )
        .unwrap();
        let compiled = chart.compile().unwrap();
        let object = compiled.chart.objects()[0].id;
        let judge = JudgeEngine::new(
            compiled.chart,
            chart.rules(),
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
        MemberConfig {
            player: PlayerId(id),
            device: Some(DeviceId(id as u64)),
            bindings: BindingMap::from_bindings([]).unwrap(),
            judge,
            sounds: vec![beatkernel::runtime::SoundBinding {
                object,
                stage: beatkernel::judge::JudgeStage::Instant,
                sample: SampleId(1),
                voice: VoiceId(u64::from(id) + 100),
                gain: 1.0,
            }],
        }
    }
    fn exercise(cohort: bool, pending: bool, fail: Option<bool>) {
        let (producer, output) = rig(24_000, 48_000, Some(0), None, 0);
        let basis = output.target_frame_basis();
        let step = Rc::new(Cell::new(0));
        let mut device = Device {
            output,
            basis,
            step: step.clone(),
            held: false,
            pending,
            fail,
            effects: vec![],
            frozen_source: None,
        };
        let mut host = Host {
            step,
            pauses: vec![],
        };
        let mut p = new_target_audio_presentation(
            0,
            basis,
            ClockDomainId(1),
            point(3, 0),
            AudioAuthorityConfig {
                history_capacity: 16,
                max_observation_age: Duration::from_nanos(10_000_000),
                input_extrapolation: ExtrapolationPolicy::Forbid,
                max_input_ahead: Duration::ZERO,
            },
        )
        .unwrap();
        p.admit_target(TargetNativeAudioSnapshot {
            epoch: 0,
            basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(device.pair()),
        })
        .unwrap();
        let mut pause = NativePause::new(point(2, 0), ClockDomainId(1), 24_000)
            .unwrap()
            .with_target_basis(0, basis)
            .unwrap();
        let mut bgm = BgmFeeder::new(
            vec![],
            BgmConfig {
                output_origin: point(2, 0),
                sample_rate: 24_000,
                preroll: Duration::ZERO,
                lookahead: Duration::from_nanos(1_000_000),
                max_pending: 2,
            },
        )
        .unwrap();
        let mut end = None;
        let mut delivery = InputDeliveryTelemetry::new(16, ClockDomainId(1)).unwrap();
        let mut pre = 0;
        let config = NativeGameplayConfig {
            origin: point(1, 1_000_000_000),
            stream_origin: point(2, 0),
            playback_origin: point(2, 0),
            song_origin: Timestamp::ZERO,
            sample_rate: 24_000,
            end_song: None,
            advance_lag: Duration::ZERO,
            seconds: None,
            pause_supported: true,
            logical_schedule: true,
        };
        let transport = Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL);
        let audio_config = AudioGameplayConfig {
            gameplay: config,
            section_start: Timestamp::ZERO,
        };
        let result = if cohort {
            let mut group = RuntimeGroup::new(
                ClockDomainId(3),
                ClockDomainId(2),
                transport,
                producer,
                vec![member(7), member(99)],
                8,
                &[],
            )
            .unwrap();
            let mut states: Vec<GameplayPlayerState<NoopSoloCompetition>> = [7, 99]
                .into_iter()
                .map(|id| GameplayPlayerState {
                    player: PlayerId(id),
                    capture: None,
                    competition: None,
                    completion: None,
                    score: Default::default(),
                    gauge: BmsGauge::default(),
                    last_song: Timestamp::ZERO,
                })
                .collect();
            let mut merger = InputMerger::new(
                ClockDomainId(1),
                config.origin,
                vec![DeviceId(7), DeviceId(99)],
                16,
            )
            .unwrap();
            run_cohort_audio_with_results_and_ports(
                &mut device,
                AudioCohortSession {
                    group: &mut group,
                    network: None::<&mut NoopGroupCompetition>,
                    states: &mut states,
                    merger: &mut merger,
                    bgm: &mut bgm,
                    discipline: &mut p,
                    pause: &mut pause,
                    end: &mut end,
                    delivery: &mut delivery,
                    pre_origin_inputs: &mut pre,
                },
                audio_config,
                &mut Control,
                &mut host,
            )
            .map(|_| ())
        } else {
            let m = member(1);
            let mut runtime = SoloRuntime::new(
                ClockDomainId(3),
                ClockDomainId(2),
                transport,
                m.bindings,
                m.judge,
                producer,
                m.sounds,
                8,
            )
            .unwrap();
            let mut gauge = BmsGauge::default();
            let mut completion = None;
            let mut capture = None;
            let mut competition: Option<NoopSoloCompetition> = None;
            let mut merger =
                InputMerger::new_dynamic(ClockDomainId(1), config.origin, 4096, 16).unwrap();
            run_gameplay_audio_with_result_and_ports(
                &mut device,
                AudioGameplaySession {
                    session: GameplaySession {
                        runtime: &mut runtime,
                        gauge: &mut gauge,
                        bgm: &mut bgm,
                        discipline: &mut p,
                        pause: &mut pause,
                        end: &mut end,
                        completion: &mut completion,
                        capture: &mut capture,
                        competition: &mut competition,
                        delivery: &mut delivery,
                        pre_origin_inputs: &mut pre,
                    },
                    merger: &mut merger,
                },
                audio_config,
                &mut Control,
                &mut host,
            )
            .map(|_| ())
        };
        if let Some(failed_held) = fail {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("scripted native held effect refusal"));
            assert!(device.output.mixer().pause_requested());
            assert_eq!(
                pause.phase(),
                if failed_held {
                    PausePhase::Pausing
                } else {
                    PausePhase::Paused
                }
            );
            assert!(!host.pauses.contains(&PauseState::Resuming));
            assert_eq!(
                host.pauses
                    .iter()
                    .filter(|&&state| state == PauseState::Running)
                    .count(),
                1
            );
            if failed_held {
                assert!(!host.pauses.contains(&PauseState::Paused));
            }
        } else {
            result.unwrap();
            assert_eq!(
                device
                    .effects
                    .iter()
                    .map(|&(s, h, _)| (s, h))
                    .collect::<Vec<_>>(),
                vec![(3, true), (if pending { 6 } else { 4 }, false)]
            );
            assert_eq!(
                device.effects[0].2, device.effects[1].2,
                "held native duration cannot move source command cursor"
            );
            assert!(!device.output.mixer().pause_requested());
            assert!(host.pauses.contains(&PauseState::Paused));
            assert!(host.pauses.contains(&PauseState::Resuming));
            assert_eq!(pause.phase(), PausePhase::Running);
        }
        assert_eq!(pre, 0);
        assert_eq!(
            p.authority().epoch().logical_origin.domain,
            ClockDomainId(3)
        );
        assert_eq!(p.target_basis(), Some(basis));
        assert_eq!(
            p.latest_record().unwrap().evidence(),
            &OriginalNativePresentationEvidence::SuppliedPair(device.pair())
        );
        assert_eq!(
            device.output.mixer().config().format().sample_rate(),
            24_000
        );
        assert_eq!(device.output.target_frame_basis().origin(), point(2, 0));
    }
    #[test]
    fn actual_solo_target_pump_held_ack_resume_order_and_source_cursor() {
        exercise(false, false, None);
    }
    #[test]
    fn actual_cohort_target_pump_held_ack_resume_order_and_source_cursor() {
        exercise(true, false, None);
    }
    #[test]
    fn actual_target_pumps_replacement_pending_defers_held_and_producer_release() {
        for cohort in [false, true] {
            exercise(cohort, true, None);
        }
    }
    #[test]
    fn actual_target_pumps_effect_refusals_preserve_pause_and_producer_without_resume_publication()
    {
        for cohort in [false, true] {
            for held in [true, false] {
                exercise(cohort, false, Some(held));
            }
        }
    }
}

// The committed startup policy uses real source gates and converter telemetry;
// only native observations, host arrival and protocol service are controlled IO.
use crate::gameplay::output::ports::TargetOutputTelemetry;
use crate::native_start::{
    start_target_committed, NativeStartAgreement, NativeStartConfig, NativeStartDevice,
    NativeStartObservation, NativeStartResult, NativeStartTiming, NativeTargetStartDevice,
    SessionHostBracket,
};

struct CommittedTargetDevice {
    owner: ConvertedNativeOutputState,
    basis: TargetFrameBasis,
    generated: u64,
    presented: u64,
    converted: Option<ConvertedRenderReport>,
    starts: usize,
    retains: Vec<bool>,
    cancel_retained: bool,
    projection_missing: usize,
    crossing_missing: usize,
    missing_forever: bool,
    wrong_source: bool,
    race_arm: bool,
    lag_arrival: bool,
    first_positive: Option<u64>,
    host_samples: std::cell::RefCell<Vec<ClockPoint>>,
}
impl CommittedTargetDevice {
    fn render_target(&mut self, frames: usize) -> NativeStartResult<()> {
        let mut remaining = frames;
        while remaining != 0 {
            let count = remaining.min(128);
            let converted = self.owner.render_pending(count)?;
            if self.first_positive.is_none() {
                if let Some(offset) = self
                    .owner
                    .pending_samples()
                    .iter()
                    .position(|sample| *sample > 0.0)
                {
                    self.first_positive = Some(self.generated + offset as u64);
                }
            }
            self.owner.admit(count)?;
            self.generated += count as u64;
            self.converted = Some(converted);
            remaining -= count;
        }
        Ok(())
    }
    fn actual_pair(&self) -> ClockPair {
        native(
            self.basis,
            self.presented,
            1_000_000_000 + (self.presented * 1_000_000_000 / 48_000) as i64,
        )
        .1
    }
}
impl NativeStartDevice for CommittedTargetDevice {
    type Evidence = ClockPair;
    fn start(&mut self) -> NativeStartResult<()> {
        self.starts += 1;
        Ok(())
    }
    fn service_input(&mut self, retain: bool) -> NativeStartResult<bool> {
        self.retains.push(retain);
        Ok(!(retain && self.cancel_retained))
    }
    fn observe(&mut self) -> NativeStartResult<Option<NativeStartObservation<ClockPair>>> {
        self.render_target(4_800)?;
        self.presented = self.generated;
        let pair = self.actual_pair();
        Ok(Some(NativeStartObservation {
            timing: NativeStartTiming::Point(pair),
            evidence: pair,
        }))
    }
    fn render_report(&mut self) -> NativeStartResult<Option<RenderReport>> {
        Ok(self.owner.last_real_source_report())
    }
    fn buffer_frames(&self) -> NativeStartResult<u32> {
        Ok(128)
    }
    fn host_now(&self) -> NativeStartResult<ClockPoint> {
        let mut host = self.actual_pair().target;
        if self.lag_arrival && self.retains.last() == Some(&true) {
            host.timestamp = Timestamp::from_nanos(host.timestamp.as_nanos() - 100_000_000);
        }
        self.host_samples.borrow_mut().push(host);
        Ok(host)
    }
}
impl NativeTargetStartDevice for CommittedTargetDevice {
    fn target_identity(&self) -> NativeStartResult<(u64, TargetFrameBasis)> {
        Ok((7, self.basis))
    }
    fn target_output_telemetry(&mut self) -> NativeStartResult<Option<TargetOutputTelemetry>> {
        if self.missing_forever {
            return Ok(None);
        }
        let missing = if self.retains.last() == Some(&true) {
            &mut self.crossing_missing
        } else {
            &mut self.projection_missing
        };
        if *missing != 0 {
            *missing -= 1;
            return Ok(None);
        }
        let mut facts = self.owner.boundaries();
        if self.wrong_source {
            facts.source_rate = 48_000;
        }
        let telemetry = TargetOutputTelemetry {
            source: self.owner.last_real_source_report(),
            converted: self.converted,
            facts,
        };
        if self.race_arm && self.retains.last() == Some(&false) {
            // Native rendering races a cold snapshot's queue admission. The
            // actual producer frontier must refuse the now missed source gate.
            self.race_arm = false;
            self.render_target(14_400)?;
        }
        Ok(Some(telemetry))
    }
}
struct CommittedTargetAgreement {
    cancel: bool,
}
impl NativeStartAgreement for CommittedTargetAgreement {
    fn await_commit(
        &mut self,
        service: &mut dyn FnMut() -> NativeStartResult<bool>,
    ) -> NativeStartResult<bool> {
        Ok(service()? && !self.cancel)
    }
    fn committed_schedule(&self) -> NativeStartResult<crate::multiplayer_start::StartSchedule> {
        Ok(crate::multiplayer_start::StartSchedule {
            target_ns: 500_010_000,
            song_target_ns: 500_010_000,
            uncertainty_ns: 0,
        })
    }
    fn host_bracket(
        &self,
        sample: &mut dyn FnMut() -> NativeStartResult<ClockPoint>,
    ) -> NativeStartResult<SessionHostBracket> {
        Ok(SessionHostBracket::new(
            300_000_000,
            sample()?,
            300_000_000,
        )?)
    }
    fn clock_now_ns(&self) -> NativeStartResult<i64> {
        Ok(300_000_000)
    }
}
fn committed_target_rig(
    finite: Option<u64>,
) -> (
    CommittedTargetDevice,
    CommandProducer,
    crate::playback_pause::NativePause,
    Option<crate::native_end::NativeEnd>,
    NativeStartConfig,
) {
    let (producer, owner) = rig_gate(44_100, 48_000, Some(None), finite, 0);
    let basis = owner.target_frame_basis();
    let mut pause =
        crate::playback_pause::NativePause::new(point(2, 0), ClockDomainId(1), 44_100).unwrap();
    if let Some(end) = finite {
        pause = pause.with_playback_end_frame(end).unwrap();
    }
    let end = finite.map(|end| {
        crate::native_end::NativeEnd::new(point(2, 0), ClockDomainId(1), 44_100, end).unwrap()
    });
    (
        CommittedTargetDevice {
            owner,
            basis,
            generated: 0,
            presented: 0,
            converted: None,
            starts: 0,
            retains: Vec::new(),
            cancel_retained: false,
            projection_missing: 0,
            crossing_missing: 0,
            missing_forever: false,
            wrong_source: false,
            race_arm: false,
            lag_arrival: false,
            first_positive: None,
            host_samples: Default::default(),
        },
        producer,
        pause,
        end,
        NativeStartConfig {
            output_origin: point(2, 0),
            sample_rate: 44_100,
            playback_end_frame: finite,
            setup_timeout: std::time::Duration::from_millis(100),
            max_clock_age_ns: 1_000_000_000,
            max_rate_error_ppm: 0,
        },
    )
}

#[test]
fn committed_target_startup_arms_actual_source_gate_and_waits_host_arrival_with_original_pair() {
    let (mut device, mut producer, mut pause, mut end, config) = committed_target_rig(Some(128));
    device.lag_arrival = true;
    let mut agreement = CommittedTargetAgreement { cancel: false };
    let mut reports = Vec::new();
    let started = start_target_committed(
        &mut device,
        &mut agreement,
        &mut producer,
        &mut pause,
        &mut end,
        config,
        |report, _| {
            if let Some(report) = report {
                reports.push(report);
            }
            Ok(())
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(device.starts, 1);
    assert_eq!(started.plan.selected_source_frame(), 22_051);
    let mapped = point(2, 24_002 * 1_000_000_000 / 48_000);
    assert_eq!(started.plan.selected_output().unwrap(), mapped);
    assert_ne!(mapped, point(2, 22_051 * 1_000_000_000 / 44_100));
    assert_eq!(started.plan.target_basis(), device.basis);
    assert_eq!(producer.applied_start_frame(), Some(22_051));
    assert_eq!(device.first_positive, Some(24_002));
    assert_eq!(
        started.observation.evidence,
        started.observation.timing.point().unwrap()
    );
    assert_eq!(started.observation.evidence.source, point(2, 600_000_000));
    assert_eq!(
        started.host_origin,
        point(1, 1_000_000_000 + mapped.timestamp.as_nanos())
    );
    assert_eq!(started.host_window.earliest(), started.host_origin);
    assert_eq!(started.host_window.latest(), started.host_origin);
    assert_eq!(device.presented, 33_600); // Arrival services another real callback.
    assert!(device
        .host_samples
        .borrow()
        .iter()
        .any(|host| host.timestamp < started.host_origin.timestamp));
    assert!(
        device.host_samples.borrow().last().unwrap().timestamp
            >= started.host_window.latest().timestamp
    );
    let first_retained = device.retains.iter().position(|retain| *retain).unwrap();
    assert!(device.retains[..first_retained]
        .iter()
        .all(|retain| !retain));
    assert!(device.retains[first_retained..]
        .iter()
        .all(|retain| *retain));
    assert!(reports
        .iter()
        .all(|report| report.playback_frames <= report.frames));
    assert_eq!(device.owner.boundaries().source_rate, config.sample_rate);
    assert_eq!(
        started
            .plan
            .validate_startup(device.owner.boundaries())
            .unwrap(),
        mapped
    );
    let boundary = end
        .as_mut()
        .unwrap()
        .observe_target(
            7,
            device.basis,
            device.owner.boundaries(),
            device.owner.last_real_source_report(),
            device.actual_pair(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(boundary.playback_frame, 128);
    assert_eq!(boundary.physical_frame, 22_179);
}

#[test]
fn committed_target_startup_recovers_delayed_coherent_projection_and_crossing_metadata() {
    for projection in [true, false] {
        let (mut device, mut producer, mut pause, mut end, config) = committed_target_rig(None);
        if projection {
            device.projection_missing = 1;
        } else {
            device.crossing_missing = 1;
        }
        let started = start_target_committed(
            &mut device,
            &mut CommittedTargetAgreement { cancel: false },
            &mut producer,
            &mut pause,
            &mut end,
            config,
            |_, _| Ok(()),
        )
        .unwrap()
        .unwrap();
        let mapped = started
            .plan
            .validate_startup(device.owner.boundaries())
            .unwrap();
        assert_eq!(
            producer.applied_start_frame(),
            Some(started.plan.selected_source_frame())
        );
        assert_eq!(device.first_positive, Some(24_002));
        assert_eq!(mapped, point(2, 24_002 * 1_000_000_000 / 48_000));
        assert_eq!(
            started.observation.evidence,
            started.observation.timing.point().unwrap()
        );
        assert!(device.retains.iter().any(|retain| *retain));
        assert!(end.is_none());
    }
}

#[test]
fn committed_target_startup_auxiliary_timeout_and_invalid_source_preserve_cold_observers_and_owner()
{
    for missing in [true, false] {
        let (mut device, mut producer, mut pause, mut end, mut config) =
            committed_target_rig(Some(128));
        device.missing_forever = missing;
        device.wrong_source = !missing;
        config.setup_timeout = std::time::Duration::from_millis(10);
        let original_pause = format!("{pause:?}");
        let original_end = end.clone();
        let original_basis = device.basis;
        let error = start_target_committed(
            &mut device,
            &mut CommittedTargetAgreement { cancel: false },
            &mut producer,
            &mut pause,
            &mut end,
            config,
            |_, _| Ok(()),
        )
        .unwrap_err();
        assert!(error.to_string().contains(if missing {
            "telemetry timed out"
        } else {
            "source identity differs"
        }));
        assert_eq!(format!("{pause:?}"), original_pause);
        assert_eq!(end, original_end);
        assert_eq!(producer.applied_start_frame(), None);
        assert_eq!(device.owner.boundaries().startup, None);
        assert_eq!(device.owner.boundaries().source_rate, 44_100);
        assert_eq!(device.basis, original_basis);
        assert!(device.retains.iter().all(|retain| !retain));
        assert_eq!(device.owner.pending_frames(), 0);
        assert!(device.owner.mixer().frame_cursor() > 0);
    }
}

#[test]
fn committed_target_startup_cancellation_preserves_original_input_retention_phase_and_cleanup_ownership(
) {
    for after_arm in [false, true] {
        let (mut device, mut producer, mut pause, mut end, config) =
            committed_target_rig(Some(128));
        device.cancel_retained = after_arm;
        let original_pause = format!("{pause:?}");
        let original_end = end.clone();
        let result = start_target_committed(
            &mut device,
            &mut CommittedTargetAgreement { cancel: !after_arm },
            &mut producer,
            &mut pause,
            &mut end,
            config,
            |_, _| Ok(()),
        )
        .unwrap();
        assert!(result.is_none());
        assert_eq!(device.starts, 1);
        assert_eq!(producer.applied_start_frame(), None);
        assert_eq!(device.owner.pending_frames(), 0);
        assert_eq!(device.owner.boundaries().source_rate, 44_100);
        if after_arm {
            assert_eq!(device.retains.last(), Some(&true));
            assert_ne!(format!("{pause:?}"), original_pause);
            assert_ne!(end, original_end);
        } else {
            assert!(device.retains.iter().all(|retain| !retain));
            assert_eq!(format!("{pause:?}"), original_pause);
            assert_eq!(end, original_end);
        }
    }
}

#[test]
fn committed_target_startup_actual_source_frontier_race_refuses_missed_gate_without_observer_commit(
) {
    let (mut device, mut producer, mut pause, mut end, config) = committed_target_rig(Some(128));
    device.race_arm = true;
    let before_pause = format!("{pause:?}");
    let before_end = end.clone();
    let error = start_target_committed(
        &mut device,
        &mut CommittedTargetAgreement { cancel: false },
        &mut producer,
        &mut pause,
        &mut end,
        config,
        |_, _| Ok(()),
    )
    .unwrap_err();
    assert!(error.to_string().to_ascii_lowercase().contains("missed"));
    assert_eq!(format!("{pause:?}"), before_pause);
    assert_eq!(end, before_end);
    assert_eq!(producer.applied_start_frame(), None);
    assert!(device.owner.mixer().frame_cursor() > 22_051);
    assert!(device.owner.boundaries().startup.is_none());
    assert!(device.first_positive.is_none());
    assert!(device.retains.iter().all(|retain| !retain));
}

#[test]
fn committed_target_startup_zero_end_requires_genuine_mapped_start_and_finite_marker() {
    let (mut device, mut producer, mut pause, mut end, config) = committed_target_rig(Some(0));
    let started = start_target_committed(
        &mut device,
        &mut CommittedTargetAgreement { cancel: false },
        &mut producer,
        &mut pause,
        &mut end,
        config,
        |_, _| Ok(()),
    )
    .unwrap()
    .unwrap();
    let facts = device.owner.boundaries();
    assert_eq!(producer.applied_start_frame(), None);
    assert_eq!(device.first_positive, None);
    assert_eq!(
        facts.startup.unwrap().source_frame,
        started.plan.selected_source_frame()
    );
    assert_eq!(
        facts.end.unwrap().source_frame,
        started.plan.selected_source_frame()
    );
    assert_eq!(
        facts.startup.unwrap().target_time,
        facts.end.unwrap().target_time
    );
    assert_eq!(
        started.plan.validate_startup(facts).unwrap(),
        started.plan.selected_output().unwrap()
    );
    let boundary = end
        .as_mut()
        .unwrap()
        .observe_target(
            7,
            device.basis,
            facts,
            device.owner.last_real_source_report(),
            device.actual_pair(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(boundary.playback_frame, 0);
    assert_eq!(
        boundary.physical_frame,
        started.plan.selected_source_frame()
    );
    assert_eq!(boundary.output, started.plan.selected_output().unwrap());
    assert_eq!(
        end.as_mut()
            .unwrap()
            .observe_target(
                7,
                device.basis,
                facts,
                device.owner.last_real_source_report(),
                device.actual_pair()
            )
            .unwrap(),
        None
    );
}
