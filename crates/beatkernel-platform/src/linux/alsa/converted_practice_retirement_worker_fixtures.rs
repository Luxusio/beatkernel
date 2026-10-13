//! Scripted ordering through the production hook, not native timing or app ACK.
use super::*;
use crate::audio::SampleEncoding;
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};

fn rig(capacity: usize) -> (ConvertedNativeOutputState, PracticeController) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(64, 64, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.1, 0.2, 0.3, 0.4], limits).unwrap(),
    )
    .unwrap();
    let program = PreparedPracticeProgram::new(
        &bank,
        vec![PracticeCue {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        }],
        PracticeLimits::new(8, 1, 128, 8, capacity).unwrap(),
    )
    .unwrap();
    let (controller, endpoint) = practice_queue(&program).unwrap();
    let (_, consumer) = command_queue(16).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::ZERO,
            AudioLimits::new(16, 4, 16, 128, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer
        .install_practice(
            program,
            endpoint,
            PracticeRegion::new(Timestamp::ZERO, Timestamp::from_nanos(1_000_000), true).unwrap(),
        )
        .unwrap();
    // Capture the genuine Source origin before the converter can pull anything.
    mixer.enable_source_playback_history(64).unwrap();
    let mut owner = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(1500, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        32,
    )
    .unwrap_or_else(|failure| panic!("owner: {:?}", failure.error()));
    owner.enable_phase_history(64).unwrap();
    (owner, controller)
}
fn time(frame: u64) -> TargetTime {
    TargetTime::from_frames(frame, 1500).unwrap()
}
fn exact(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}
fn telemetry(owner: &ConvertedNativeOutputState) -> (ConvertedTelemetry, TargetFrameBasis) {
    let cold_basis = owner.target_frame_basis();
    (
        ConvertedTelemetry::with_phase_history(owner, cold_basis).unwrap(),
        cold_basis,
    )
}
fn identity(owner: &ConvertedNativeOutputState) -> (u32, u64, u64) {
    let history = owner.converter_owner().source_playback_history().unwrap();
    (
        history.source_rate(),
        history.initial_physical_frame(),
        history.initial_playback_frame(),
    )
}
fn queue(
    owner: &ConvertedNativeOutputState,
    t: &ConvertedTelemetry,
    cold_basis: TargetFrameBasis,
    source: u64,
    target: TargetTime,
) -> u64 {
    t.request_history_retirement(cold_basis, identity(owner), source, target)
        .unwrap_or_else(|error| {
            let source_history = owner.converter_owner().source_playback_history().unwrap();
            let phase_history = owner.phase_history().unwrap();
            let id = identity(owner);
            panic!("retirement rejected {error:?}; pins=({source},{target:?}); source len={} pulled={} retained={} spans={:?}; phase len={} start={:?} end={:?} spans={:?}; telemetry phase={:?}; telemetry compound={:?}; initial phase={:?}; initial compound={:?}",
                source_history.len(), source_history.pulled_end_frame(), source_history.retained_start_frame(),
                (0..source_history.len()).map(|index| source_history.span(index).unwrap()).collect::<Vec<_>>(),
                phase_history.len(), phase_history.start_time(), phase_history.end_time(),
                (0..phase_history.len()).map(|index| phase_history.span(index).unwrap().to_words()).collect::<Vec<_>>(),
                t.phase_snapshot_at(target),
                t.playback_phase_snapshot_at_interpolated_time_with_identity(&exact(target), id.0, id.1, id.2),
                t.phase_snapshot_at(time(0)),
                t.playback_phase_snapshot_at_interpolated_time_with_identity(&exact(time(0)), id.0, id.1, id.2));
        })
        .unwrap()
}
fn hook(owner: &mut ConvertedNativeOutputState, t: &ConvertedTelemetry, completed: &mut u64) {
    let mut pump = ConvertedPump {
        state: owner,
        telemetry: t,
        retirement_completed: *completed,
    };
    pump.before_pending().unwrap();
    *completed = pump.retirement_completed;
}
fn generate(
    owner: &mut ConvertedNativeOutputState,
    t: &ConvertedTelemetry,
    held: bool,
    frames: usize,
) -> ConvertedRenderReport {
    t.set_held(held);
    let mut pump = ConvertedPump {
        state: owner,
        telemetry: t,
        retirement_completed: 0,
    };
    let report = pump.render_pending(frames).unwrap();
    pump.admit(frames).unwrap();
    report
}
fn receipt_survives(
    owner: &ConvertedNativeOutputState,
    t: &ConvertedTelemetry,
    receipt: &ProjectedPracticeReceipt,
) {
    let physical = receipt.receipt.physical_frame;
    let history = owner.converter_owner().source_playback_history().unwrap();
    assert!(history
        .lookup_at_source_time(exact(
            TargetTime::from_frames(physical, history.source_rate()).unwrap()
        ))
        .is_ok());
    assert!(
        owner
            .converter_owner()
            .source_playback_history()
            .unwrap()
            .retained_start_frame()
            <= physical
    );
    let target = receipt.boundary.target_time;
    assert!(t
        .phase_snapshot_at_interpolated_time(&exact(target))
        .is_some());
    let id = identity(owner);
    assert!(t
        .playback_phase_snapshot_at_interpolated_time_with_identity(
            &exact(target),
            id.0,
            id.1,
            id.2
        )
        .is_some());
}
fn operational(
    owner: &ConvertedNativeOutputState,
) -> (
    Vec<f32>,
    Option<ConvertedRenderReport>,
    Option<TargetTime>,
    SourcePosition,
    u64,
    u64,
    AudioCounters,
    ConvertedBoundaryFacts,
) {
    (
        owner.pending_samples().to_vec(),
        owner.pending_report(),
        owner.admitted_target_time(),
        owner.converter_owner().source_position(),
        owner.mixer().frame_cursor(),
        owner.mixer().playback_frame_cursor(),
        owner.mixer().counters(),
        owner.boundaries(),
    )
}

#[test]
fn previously_queued_aggregate_protects_real_receipt_after_pop_before_next_request() {
    let (mut owner, mut controller) = rig(16);
    let (t, cold_basis) = telemetry(&owner);
    generate(&mut owner, &t, false, 3);
    let pins = controller.history_retention_pins();
    assert_eq!(pins, (Some(0), Some(time(0))));
    let prior = queue(&owner, &t, cold_basis, pins.0.unwrap(), pins.1.unwrap());
    // The controller has released its slot, but the app still owns this value.
    let held = Some(controller.try_pop_projected_receipt().unwrap());
    assert_eq!(
        controller.history_retention_pins(),
        (Some(1), Some(time(2)))
    );
    let before = operational(&owner);
    let mut completed = 0;
    hook(&mut owner, &t, &mut completed);
    assert_eq!(completed, prior);
    assert_eq!(operational(&owner), before);
    receipt_survives(&owner, &t, held.as_ref().unwrap());
    let held = held.unwrap();
    assert_eq!(
        queue(
            &owner,
            &t,
            cold_basis,
            held.receipt.physical_frame,
            held.boundary.target_time
        ),
        prior
    );
    receipt_survives(&owner, &t, &held);
}

#[test]
fn equal_aggregate_reuses_revision_and_hook_preserves_partially_admitted_pcm() {
    let (mut owner, mut controller) = rig(16);
    let (t, cold_basis) = telemetry(&owner);
    let report = owner.render_pending(3).unwrap();
    t.publish_generated(Some(report), &owner);
    owner.admit(1).unwrap();
    t.publish_submitted(owner.admitted_target_time().unwrap());
    let prior = queue(&owner, &t, cold_basis, 0, time(0));
    let held = controller.try_pop_projected_receipt().unwrap();
    let before = operational(&owner);
    let mut completed = 0;
    for _ in 0..8 {
        assert_eq!(
            queue(
                &owner,
                &t,
                cold_basis,
                held.receipt.physical_frame,
                held.boundary.target_time
            ),
            prior
        );
        hook(&mut owner, &t, &mut completed);
        assert_eq!(operational(&owner), before);
        assert_eq!(owner.pending_frames(), 2);
        receipt_survives(&owner, &t, &held);
    }
    owner.admit(2).unwrap();
    t.publish_submitted(owner.admitted_target_time().unwrap());
    assert_eq!(owner.admitted_target_time(), Some(report.target_end_time));
}

#[test]
fn raw_lookahead_has_only_source_pin_during_held_output_then_real_projection() {
    let (mut owner, mut controller) = rig(16);
    let (t, cold_basis) = telemetry(&owner);
    generate(&mut owner, &t, false, 3);
    controller.try_pop_projected_receipt().unwrap();
    controller.try_pop_projected_receipt().unwrap();
    assert_eq!(controller.history_retention_pins(), (Some(2), None));
    assert_eq!(
        controller.try_pop_projected_receipt(),
        Err(PracticeError::Empty)
    );
    generate(&mut owner, &t, true, 3);
    let prior = queue(&owner, &t, cold_basis, 2, time(6));
    let before = operational(&owner);
    let mut completed = 0;
    hook(&mut owner, &t, &mut completed);
    assert_eq!(completed, prior);
    assert_eq!(operational(&owner), before);
    assert_eq!(controller.history_retention_pins(), (Some(2), None));
    generate(&mut owner, &t, false, 3);
    assert_eq!(
        controller.history_retention_pins(),
        (Some(2), Some(time(6)))
    );
    let held = controller.try_pop_projected_receipt().unwrap();
    assert_eq!(
        (held.receipt.physical_frame, held.boundary.target_time),
        (2, time(6))
    );
    hook(&mut owner, &t, &mut completed);
    receipt_survives(&owner, &t, &held);
    assert_eq!(
        queue(&owner, &t, cold_basis, 2, held.boundary.target_time),
        prior
    );
}

#[test]
fn projection_after_previous_scan_is_not_older_than_previous_conservative_request() {
    let (mut owner, mut controller) = rig(16);
    let (t, cold_basis) = telemetry(&owner);
    generate(&mut owner, &t, false, 3);
    controller.try_pop_projected_receipt().unwrap();
    controller.try_pop_projected_receipt().unwrap();
    let scanned = controller.history_retention_pins();
    assert_eq!(scanned, (Some(2), None));
    let previous_target = owner.admitted_target_time().unwrap();
    let prior = queue(&owner, &t, cold_basis, scanned.0.unwrap(), previous_target);
    // New generation/projector work occurs after the prior controller scan.
    generate(&mut owner, &t, false, 3);
    let held = controller.try_pop_projected_receipt().unwrap();
    assert!(held.receipt.physical_frame >= scanned.0.unwrap());
    assert!(!exact(held.boundary.target_time)
        .compare_target_time(previous_target)
        .is_lt());
    let mut completed = 0;
    hook(&mut owner, &t, &mut completed);
    assert_eq!(completed, prior);
    receipt_survives(&owner, &t, &held);
}

#[test]
fn wrap_reuse_matches_reference_pcm_reports_counters_and_actual_receipts() {
    let (mut owner, mut controller) = rig(4);
    let (mut reference, mut reference_controller) = rig(4);
    let (t, cold_basis) = telemetry(&owner);
    let (reference_t, _reference_basis) = telemetry(&reference);
    let mut completed = 0;
    for round in 0..16u64 {
        assert_eq!(
            generate(&mut owner, &t, false, 3),
            generate(&mut reference, &reference_t, false, 3)
        );
        assert_eq!(operational(&owner), operational(&reference));
        let pins = controller.history_retention_pins();
        assert_eq!(pins, (Some(2 * round), Some(time(6 * round))));
        queue(&owner, &t, cold_basis, pins.0.unwrap(), pins.1.unwrap());
        let held = controller.try_pop_projected_receipt().unwrap();
        assert_eq!(
            held,
            reference_controller.try_pop_projected_receipt().unwrap()
        );
        hook(&mut owner, &t, &mut completed);
        receipt_survives(&owner, &t, &held);
        assert_eq!(
            controller.try_pop_projected_receipt().unwrap(),
            reference_controller.try_pop_projected_receipt().unwrap()
        );
        assert_eq!(
            controller.history_retention_pins(),
            reference_controller.history_retention_pins()
        );
        assert_eq!(operational(&owner), operational(&reference));
        assert_eq!(
            generate(&mut owner, &t, true, 3),
            generate(&mut reference, &reference_t, true, 3)
        );
        assert_eq!(operational(&owner), operational(&reference));
    }
    assert!(owner.phase_history().unwrap().retirement_revision() > 0);
    // A full queue is not reclaimed by scanning or the production hook.
    let (mut full, mut full_controller) = rig(2);
    let (full_t, full_basis) = telemetry(&full);
    generate(&mut full, &full_t, false, 1);
    generate(&mut full, &full_t, false, 1);
    queue(&full, &full_t, full_basis, 0, time(0));
    let before = operational(&full);
    let mut full_completed = 0;
    for _ in 0..8 {
        hook(&mut full, &full_t, &mut full_completed);
    }
    assert!(full_completed > 0);
    assert_eq!(operational(&full), before);
    assert_eq!(full.render_pending(1), Err(AudioError::PracticeReceiptFull));
    assert_eq!(operational(&full), before);
    full_controller.try_pop_projected_receipt().unwrap();
    assert_eq!(generate(&mut full, &full_t, false, 1).target_frames, 1);
}

#[test]
fn releasing_old_held_receipt_allows_later_request_and_old_target_expiry() {
    let (mut owner, mut controller) = rig(16);
    let (t, cold_basis) = telemetry(&owner);
    generate(&mut owner, &t, false, 3);
    let prior = queue(&owner, &t, cold_basis, 0, time(0));
    let mut held = Some(controller.try_pop_projected_receipt().unwrap());
    controller.try_pop_projected_receipt().unwrap();
    generate(&mut owner, &t, true, 3);
    let mut completed = 0;
    hook(&mut owner, &t, &mut completed);
    assert_eq!(completed, prior);
    receipt_survives(&owner, &t, held.as_ref().unwrap());
    // This test models releasing the value, not PracticePlayback.apply_boundary.
    assert!(held.take().is_some());
    let later = queue(&owner, &t, cold_basis, 2, time(6));
    assert!(later > prior);
    hook(&mut owner, &t, &mut completed);
    assert_eq!(completed, later);
    assert!(t.phase_snapshot_at(time(0)).is_none());
    assert_eq!(
        owner.phase_history().unwrap().retained_start_time(),
        time(3)
    );
    assert_eq!(controller.history_retention_pins(), (Some(2), None));
    generate(&mut owner, &t, false, 3);
    receipt_survives(&owner, &t, &controller.try_pop_projected_receipt().unwrap());
}

#[test]
fn aggressive_request_is_clamped_by_still_queued_receipts_without_claiming_transfer() {
    let (mut owner, controller) = rig(16);
    let (t, cold_basis) = telemetry(&owner);
    generate(&mut owner, &t, false, 3);
    generate(&mut owner, &t, true, 3);
    let requested = queue(&owner, &t, cold_basis, 2, time(6));
    let before = operational(&owner);
    let mut completed = 0;
    hook(&mut owner, &t, &mut completed);
    assert_eq!(completed, 0);
    assert_eq!(operational(&owner), before);
    assert_eq!(
        controller.history_retention_pins(),
        (Some(0), Some(time(0)))
    );
    assert!(t.phase_snapshot_at(time(0)).is_some());
    assert_eq!(t.retirement_request().unwrap().revision, requested);
}
