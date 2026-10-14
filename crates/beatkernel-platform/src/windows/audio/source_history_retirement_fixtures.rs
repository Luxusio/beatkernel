//! Windows-gated production publication seams; no COM endpoint is opened.
//! Portable worker execution and foreign typing are separate verification tiers.
use super::*;
use beatkernel::audio::*;
use beatkernel::time::ClockDomainId;

fn rig() -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(1024, 1024, 1).unwrap()).unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(39),
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
fn identity(mixer: &Mixer) -> (u32, u64, u64) {
    let h = mixer.source_playback_history().unwrap();
    (
        h.source_rate(),
        h.initial_physical_frame(),
        h.initial_playback_frame(),
    )
}
fn publish(telemetry: &Telemetry, mixer: &Mixer, report: RenderReport, version: &mut u64) {
    telemetry.publish_with_source_history(
        Some(AudioStreamSnapshot {
            telemetry_available: true,
            status: AudioStreamStatus::Running,
            counters: StreamCounters {
                submitted_frames: report.start_frame + report.frames as u64,
                buffer_fills: 1,
                ..StreamCounters::default()
            },
            clock: None,
            render: Some(report),
        }),
        mixer.source_playback_history(),
        version,
    );
}

#[test]
fn cold_factory_uses_reopened_original_basis_and_retained_source_identity() {
    let (_, mut mixer) = rig();
    mixer.render(&mut [0.0; 3]).unwrap();
    let basis = mixer.output_frame_basis();
    let id = identity(&mixer);
    let telemetry = source_telemetry(&mixer).unwrap();
    assert_eq!(id, (1000, 0, 0));
    assert_eq!(basis.start_physical_frame(), 3);
    assert_eq!(
        telemetry
            .request_source_history_retirement(basis, id, 2)
            .unwrap(),
        Some(2)
    );
    assert_eq!(
        mixer
            .source_playback_history()
            .unwrap()
            .retirement_revision(),
        0
    );
    assert!(telemetry.read().render.is_none());
}

#[test]
fn callback_retirement_frees_real_capacity_before_next_render_without_heap_calls() {
    let (mut producer, mut mixer) = rig();
    let basis = mixer.output_frame_basis();
    let id = identity(&mixer);
    let telemetry = source_telemetry(&mixer).unwrap();
    let mut version = 0;
    for paused in [false, true, false, true] {
        producer.request_pause(paused);
        let report = mixer.render(&mut [0.0; 2]).unwrap();
        publish(&telemetry, &mixer, report, &mut version);
    }
    let snapshot = telemetry.read();
    assert_eq!(
        telemetry
            .request_source_history_retirement(basis, id, 6)
            .unwrap(),
        Some(2)
    );
    assert_eq!(telemetry.read(), snapshot);
    let request = telemetry.source_retirement_request().unwrap();
    let mut completed = 0;
    let (retired, allocations) = crate::audio::count_heap_calls(|| {
        crate::audio::source_history_retirement_mailbox::apply_source_history_retirement(
            &mut mixer,
            request,
            &mut completed,
        )
    });
    assert!(retired.unwrap() > 0);
    assert_eq!((allocations, completed), (0, 2));
    producer.request_pause(false);
    let report = mixer.render(&mut [0.0; 2]).unwrap();
    publish(&telemetry, &mixer, report, &mut version);
    assert_eq!(identity(&mixer), id);
    assert!(telemetry
        .request_source_history_retirement(basis, id, 1)
        .is_err());
    assert_eq!(telemetry.read().render.unwrap().start_frame, 8);
}

#[test]
fn retired_history_is_publishable_after_a_later_render_refusal() {
    let (mut producer, mut mixer) = rig();
    let basis = mixer.output_frame_basis();
    let id = identity(&mixer);
    let telemetry = source_telemetry(&mixer).unwrap();
    let mut version = 0;
    for paused in [false, true, false] {
        producer.request_pause(paused);
        let report = mixer.render(&mut [0.0; 2]).unwrap();
        publish(&telemetry, &mixer, report, &mut version);
    }
    telemetry
        .request_source_history_retirement(basis, id, 4)
        .unwrap();
    let mut completed = 0;
    crate::audio::source_history_retirement_mailbox::apply_source_history_retirement(
        &mut mixer,
        telemetry.source_retirement_request().unwrap(),
        &mut completed,
    )
    .unwrap();
    assert!(mixer.render(&mut [0.0; 257]).is_err());
    let mut failed = telemetry.read();
    failed.status = AudioStreamStatus::Failed {
        hresult: E_INVALIDARG_CODE,
    };
    failed.counters.native_failures += 1;
    telemetry.publish_with_source_history(
        Some(failed),
        mixer.source_playback_history(),
        &mut version,
    );
    assert_eq!(telemetry.read(), failed);
    assert!(telemetry
        .request_source_history_retirement(basis, id, 1)
        .is_err());
    assert_eq!(completed, 2);
}

fn complete(mixer: Mixer) -> NativeOutputState {
    match NativeOutputState::new(
        mixer,
        DeviceFormat::new(1000, 1, SampleEncoding::Float32, None).unwrap(),
        None,
        8,
    ) {
        Ok(state) => state,
        Err(_) => panic!("valid complete Source owner must prepare"),
    }
}

#[test]
fn complete_cold_publication_binds_first_unsent_basis_and_grants_no_native_progress() {
    let (_, mixer) = rig();
    let mut state = complete(mixer);
    let report = state.render_pending(4).unwrap();
    state.admit(1).unwrap();
    let basis = state.output_frame_basis();
    let id = identity(state.mixer());
    let telemetry = source_conditional_retirement::complete_source_telemetry(&state).unwrap();
    assert_eq!(basis.start_physical_frame(), 1);
    assert_eq!(state.mixer().output_frame_basis().start_physical_frame(), 4);
    assert_eq!(id, (1000, 0, 0));
    assert_eq!(
        telemetry.request_source_history_retirement(basis, id, 3),
        Ok(Some(2))
    );
    assert!(telemetry
        .request_source_history_retirement(state.mixer().output_frame_basis(), id, 3)
        .is_err());
    let query =
        InterpolatedTargetTime::new(TargetTime::from_frames(3, 1000).unwrap(), 0, 0, 1, 0, 1)
            .unwrap();
    let source = telemetry
        .read_source_playback_at(&query, id.0, id.1, id.2)
        .unwrap();
    assert_eq!(source.pulled_source_end, 4);
    assert_eq!(source.revision, 0);
    assert_eq!(source.snapshot.status, AudioStreamStatus::Ready);
    assert_eq!(source.snapshot.counters.submitted_frames, 0);
    assert!(source.snapshot.clock.is_none());
    assert!(source.snapshot.render.is_none());
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(state.admitted_frames(), 1);
}

#[test]
fn complete_native_mailbox_clamps_to_whole_pending_report_until_positive_admission() {
    let (mut producer, mixer) = rig();
    let mut state = complete(mixer);
    for paused in [false, true, false] {
        producer.request_pause(paused);
        state.render_pending(2).unwrap();
        state.admit(2).unwrap();
    }
    producer.request_pause(true);
    let report = state.render_pending(2).unwrap();
    state.admit(1).unwrap();
    let basis = state.output_frame_basis();
    let id = identity(state.mixer());
    let telemetry = source_conditional_retirement::complete_source_telemetry(&state).unwrap();
    assert_eq!(basis.start_physical_frame(), 7);
    assert_eq!(report.start_frame, 6);
    assert_eq!(
        telemetry.request_source_history_retirement(basis, id, 8),
        Ok(Some(2))
    );
    let request = telemetry.source_retirement_request().unwrap();
    let mut completed = 0;
    let (result, heap) = crate::audio::count_heap_calls(|| {
        state.apply_source_history_retirement(request, &mut completed)
    });
    assert_eq!(result, Ok(3));
    assert_eq!(heap, 0);
    assert_eq!(completed, 0, "clamped intent remains pending");
    assert_eq!(telemetry.source_retirement_request(), Some(request));
    assert_eq!(state.pending_report(), Some(report));
    assert_eq!(state.admitted_frames(), 1);
    assert_eq!(state.pending_samples(), [0.]);
    assert_eq!(state.output_frame_basis(), basis);
    assert_eq!(
        state
            .mixer()
            .source_playback_history()
            .unwrap()
            .retained_start_frame(),
        6
    );
    // The original request must be valid before the pending-report clamp.
    assert_eq!(
        state.apply_source_history_retirement(
            crate::audio::source_history_retirement_mailbox::SourceHistoryRetirementRequest {
                revision: 4,
                source_pin: 9,
            },
            &mut completed,
        ),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(completed, 0);
    state.admit(1).unwrap();
    assert_eq!(
        state.apply_source_history_retirement(request, &mut completed),
        Ok(0)
    );
    assert_eq!(completed, 2);
    assert_eq!(identity(state.mixer()), id);
    assert_eq!(
        (
            state.mixer().frame_cursor(),
            state.mixer().playback_frame_cursor()
        ),
        (8, 4)
    );
}
