//! Supplied worker/publication seams; these tests do not open an audio endpoint.
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
