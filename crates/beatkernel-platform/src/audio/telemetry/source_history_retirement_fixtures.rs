//! Real generated history bounds; retirement intent grants no native authority.
use super::*;
use crate::audio::count_heap_calls;
use beatkernel::audio::*;

fn rig() -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(1024, 1024, 1).unwrap();
    let bank = SampleBank::new(format, limits).unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(19),
            Timestamp::ZERO,
            AudioLimits::new(8, 4, 8, 256, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.enable_source_playback_history(4).unwrap();
    (producer, mixer)
}
fn history(mixer: &Mixer) -> &SourcePlaybackHistory {
    mixer.source_playback_history().unwrap()
}
fn identity(mixer: &Mixer) -> (u32, u64, u64) {
    let h = history(mixer);
    (
        h.source_rate(),
        h.initial_physical_frame(),
        h.initial_playback_frame(),
    )
}
fn query(frame: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(TargetTime::from_frames(frame, 1000).unwrap(), 0, 0, 1, 0, 1)
        .unwrap()
}
fn scalar(report: RenderReport) -> AudioStreamSnapshot {
    AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters {
            submitted_frames: report.start_frame + report.frames as u64,
            buffer_fills: 1,
            ..StreamCounters::default()
        },
        clock: None,
        render: Some(report),
    }
}
fn publish(t: &Telemetry, mixer: &Mixer, report: RenderReport, version: &mut u64) {
    t.publish_with_source_history(Some(scalar(report)), Some(history(mixer)), version);
}

#[test]
fn request_preserves_original_scalar_source_revision_and_real_owner_history() {
    let (_, mut mixer) = rig();
    let basis = mixer.output_frame_basis();
    let id = identity(&mixer);
    let telemetry = Telemetry::with_source_history_and_retirement(history(&mixer), basis).unwrap();
    let report = mixer.render(&mut [0.0; 6]).unwrap();
    let mut version = 0;
    publish(&telemetry, &mixer, report, &mut version);
    let before = telemetry
        .read_source_playback_at(&query(2), id.0, id.1, id.2)
        .unwrap();
    let (request, allocations) =
        count_heap_calls(|| telemetry.request_source_history_retirement(basis, id, 4));
    assert_eq!((request.unwrap(), allocations), (Some(2), 0));
    let pending = telemetry.source_retirement_request().unwrap();
    assert_eq!((pending.revision, pending.source_pin), (2, 4));
    assert_eq!(telemetry.read(), scalar(report));
    assert_eq!(telemetry.version.load(Ordering::SeqCst), 2);
    assert_eq!(version, 2);
    assert_eq!(history(&mixer).retirement_revision(), 0);
    let after = telemetry
        .read_source_playback_at(&query(2), id.0, id.1, id.2)
        .unwrap();
    assert_eq!(before.snapshot, after.snapshot);
    assert_eq!(before.playback, after.playback);
    assert_eq!(
        (before.pulled_source_end, before.revision),
        (after.pulled_source_end, after.revision)
    );
}

#[test]
fn original_basis_and_identity_are_required_before_any_mailbox_change() {
    let (_, mut mixer) = rig();
    let basis = mixer.output_frame_basis();
    let id = identity(&mixer);
    let telemetry = Telemetry::with_source_history_and_retirement(history(&mixer), basis).unwrap();
    let report = mixer.render(&mut [0.0; 4]).unwrap();
    publish(&telemetry, &mixer, report, &mut 0);
    assert!(matches!(
        telemetry.request_source_history_retirement(mixer.output_frame_basis(), id, 2),
        Err(AudioError::InvalidFormat)
    ));
    for wrong in [
        (999, id.1, id.2),
        (id.0, id.1 + 1, id.2),
        (id.0, id.1, id.2 + 1),
    ] {
        assert!(matches!(
            telemetry.request_source_history_retirement(basis, wrong, 2),
            Err(AudioError::InvalidFormat)
        ));
    }
    assert!(telemetry.source_retirement_request().is_none());
    assert_eq!(
        telemetry
            .request_source_history_retirement(basis, id, 2)
            .unwrap(),
        Some(2)
    );
}

#[test]
fn current_published_bounds_include_endpoint_and_reject_unpublished_pull() {
    let (_, mut mixer) = rig();
    let basis = mixer.output_frame_basis();
    let id = identity(&mixer);
    let telemetry = Telemetry::with_source_history_and_retirement(history(&mixer), basis).unwrap();
    let report = mixer.render(&mut [0.0; 4]).unwrap();
    let mut version = 0;
    publish(&telemetry, &mixer, report, &mut version);
    mixer.render(&mut [0.0; 3]).unwrap();
    assert!(matches!(
        telemetry.request_source_history_retirement(basis, id, 5),
        Err(AudioError::InvalidFormat)
    ));
    assert!(telemetry.source_retirement_request().is_none());
    assert_eq!(
        telemetry
            .request_source_history_retirement(basis, id, 4)
            .unwrap(),
        Some(2)
    );
}

#[test]
fn repeated_pin_is_idempotent_and_regression_leaves_previous_request() {
    let (_, mut mixer) = rig();
    let basis = mixer.output_frame_basis();
    let id = identity(&mixer);
    let telemetry = Telemetry::with_source_history_and_retirement(history(&mixer), basis).unwrap();
    let report = mixer.render(&mut [0.0; 6]).unwrap();
    publish(&telemetry, &mixer, report, &mut 0);
    assert_eq!(
        telemetry
            .request_source_history_retirement(basis, id, 3)
            .unwrap(),
        Some(2)
    );
    assert_eq!(
        telemetry
            .request_source_history_retirement(basis, id, 3)
            .unwrap(),
        Some(2)
    );
    assert!(matches!(
        telemetry.request_source_history_retirement(basis, id, 2),
        Err(AudioError::InvalidFormat)
    ));
    assert_eq!(telemetry.source_retirement_request().unwrap().source_pin, 3);
    assert_eq!(
        telemetry
            .request_source_history_retirement(basis, id, 6)
            .unwrap(),
        Some(4)
    );
}

#[test]
fn odd_and_exhausted_generations_expose_no_qualified_request() {
    let (_, mut mixer) = rig();
    let basis = mixer.output_frame_basis();
    let id = identity(&mixer);
    let telemetry = Telemetry::with_source_history_and_retirement(history(&mixer), basis).unwrap();
    let report = mixer.render(&mut [0.0; 4]).unwrap();
    publish(&telemetry, &mixer, report, &mut 0);
    for version in [3, u64::MAX] {
        telemetry.version.store(version, Ordering::SeqCst);
        assert_eq!(
            telemetry
                .request_source_history_retirement(basis, id, 2)
                .unwrap(),
            None
        );
        assert!(telemetry.source_retirement_request().is_none());
    }
    telemetry.version.store(2, Ordering::SeqCst);
    assert_eq!(
        telemetry
            .request_source_history_retirement(basis, id, 2)
            .unwrap(),
        Some(2)
    );
}

#[test]
fn absent_history_publication_and_nonopted_factories_cannot_queue_retirement() {
    let (_, mixer) = rig();
    let basis = mixer.output_frame_basis();
    let id = identity(&mixer);
    for telemetry in [
        Telemetry::new(),
        Telemetry::with_source_history(history(&mixer)).unwrap(),
    ] {
        assert!(matches!(
            telemetry.request_source_history_retirement(basis, id, 0),
            Err(AudioError::InvalidCapacity)
        ));
        assert!(telemetry.source_retirement_request().is_none());
    }
    let telemetry = Telemetry::with_source_history_and_retirement(history(&mixer), basis).unwrap();
    telemetry.publish_with_source_history(None, None, &mut 0);
    assert!(matches!(
        telemetry.request_source_history_retirement(basis, id, 0),
        Err(AudioError::InvalidFormat)
    ));
    assert!(telemetry.source_retirement_request().is_none());
}

#[test]
fn genuinely_retired_publication_uses_current_range_and_keeps_creation_identity() {
    let (mut producer, mut mixer) = rig();
    let basis = mixer.output_frame_basis();
    let id = identity(&mixer);
    let telemetry = Telemetry::with_source_history_and_retirement(history(&mixer), basis).unwrap();
    let mut version = 0;
    for paused in [false, true, false] {
        producer.request_pause(paused);
        let report = mixer.render(&mut [0.0; 2]).unwrap();
        publish(&telemetry, &mixer, report, &mut version);
    }
    assert_eq!(mixer.retire_source_playback_before(2).unwrap(), 1);
    telemetry.publish_with_source_history(None, Some(history(&mixer)), &mut version);
    assert!(matches!(
        telemetry.request_source_history_retirement(basis, id, 1),
        Err(AudioError::InvalidFormat)
    ));
    assert!(telemetry.source_retirement_request().is_none());
    assert_eq!(
        telemetry
            .request_source_history_retirement(basis, id, 2)
            .unwrap(),
        Some(2)
    );
    assert_eq!(identity(&mixer), id);
}

#[test]
fn empty_cold_history_accepts_only_real_initial_source_point() {
    let (_, mixer) = rig();
    let basis = mixer.output_frame_basis();
    let id = identity(&mixer);
    let telemetry = Telemetry::with_source_history_and_retirement(history(&mixer), basis).unwrap();
    assert!(matches!(
        telemetry.request_source_history_retirement(basis, id, 1),
        Err(AudioError::InvalidFormat)
    ));
    assert_eq!(
        telemetry
            .request_source_history_retirement(basis, id, 0)
            .unwrap(),
        Some(2)
    );
    assert!(telemetry
        .read_source_playback_at(&query(0), id.0, id.1, id.2)
        .is_none());
    assert!(telemetry.read().render.is_none());
}
