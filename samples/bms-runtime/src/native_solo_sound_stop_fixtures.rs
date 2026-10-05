// Deferred common native publication and portable Mixer only; no device or completion claim.
use super::*;
use crate::gauge::GaugeFailure;
use beatkernel::input::{ContactId, Position2, TouchEvent, TouchPhase};

fn publish_actual(fixture: &mut Fixture, report: RuntimeReport) -> NativeGameplayResult<()> {
    publish(
        &mut NativeGameplaySession {
            runtime: &mut fixture.runtime,
            gauge: &mut fixture.gauge,
            bgm: &mut fixture.bgm,
            discipline: &mut fixture.discipline,
            pause: &mut fixture.pause,
            end: &mut fixture.end,
            completion: &mut fixture.completion,
            capture: &mut fixture.capture,
            competition: &mut fixture.competition,
            delivery: &mut fixture.delivery,
            pre_origin_inputs: &mut fixture.pre,
        },
        report,
    )
}
fn stop(voice: u64, ns: i64) -> AudioCommand {
    AudioCommand::Stop {
        voice: VoiceId(voice),
        at: Timestamp::from_nanos(ns),
    }
}
fn rendered(fixture: &mut Fixture) -> [f32; 64] {
    let mut output = [0.0; 64];
    for block in output.chunks_mut(32) {
        fixture.device.mixer.render(block).unwrap();
    }
    output
}
fn success(viewer: Option<&player::PlayerViewer>) {
    let mut fixture = gauge_fence::fatal_fixture(8, 128);
    if viewer.is_some() {
        player::publish_chart(&fixture.source, &fixture.source.compile().unwrap().chart).unwrap();
    }
    let actual = fixture
        .runtime
        .process_input(
            input(20_000_000, 1, ButtonState::Down),
            &ExplicitDomains,
            point(2, 40_000_000),
        )
        .unwrap();
    assert_eq!(
        actual.audio_commands,
        [
            AudioCommand::Play {
                sample: SampleId(1),
                voice: VoiceId(17),
                at: Timestamp::from_nanos(40_000_000),
                gain: 1.0
            },
            AudioCommand::Play {
                sample: SampleId(1),
                voice: VoiceId(18),
                at: Timestamp::from_nanos(40_000_000),
                gain: 1.0
            },
        ]
    );
    assert_eq!(actual.bound_inputs.len(), 2);
    let bound = actual.bound_inputs.clone();
    publish_actual(&mut fixture, actual).unwrap();
    assert_eq!(
        fixture.runtime.gameplay_fence(),
        Some(Timestamp::from_nanos(20_000_000))
    );
    assert_eq!(
        fixture.gauge.snapshot().failure,
        Some(GaugeFailure::InstantDeath)
    );
    assert_eq!(fixture.runtime.telemetry().counters().audio_commands, 4);
    assert!(
        fixture
            .runtime
            .fence_gameplay_sounds(Timestamp::ZERO)
            .is_none()
    );
    let hash = fixture.runtime.judge().stable_hash().unwrap();
    let records = fixture.capture.as_ref().unwrap().records().to_vec();
    assert_eq!(records.len(), 2);
    for (record, input) in records.iter().zip(&bound) {
        assert!(matches!(&record.operation, ReplayOperation::Input(value) if value == input));
    }
    // Fenced acquisition preserves the genuine full-width contact metadata without
    // re-entering the judge/router, acquiring a new owner, or retrying the Stops.
    let contact = PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(DeviceId(u64::MAX), point(1, 30_000_000), u64::MAX),
        control: PhysicalControlId::keyboard(4u16),
        contact: ContactId(u64::MAX),
        phase: TouchPhase::Down,
        position: Position2 { x: 12.5, y: -7.0 },
        pressure: Some(0.75),
    });
    let after = fixture
        .runtime
        .process_input(contact.clone(), &ExplicitDomains, point(2, 50_000_000))
        .unwrap();
    assert_eq!(after.input, Some(contact));
    assert!(
        after.bound_inputs.is_empty()
            && after.judge_events.is_empty()
            && after.hazard_events.is_empty()
    );
    assert!(after.audio_commands.is_empty() && after.audio_failures.is_empty());
    publish_actual(&mut fixture, after).unwrap();
    let advance = fixture
        .runtime
        .advance_to(point(1, 60_000_000), &ExplicitDomains, point(2, 60_000_000))
        .unwrap();
    publish_actual(&mut fixture, advance).unwrap();
    assert_eq!(fixture.runtime.telemetry().counters().audio_commands, 4);
    assert_eq!(fixture.capture.as_ref().unwrap().records(), records);
    assert_eq!(fixture.runtime.judge().stable_hash().unwrap(), hash);
    assert_eq!(rendered(&mut fixture), [0.0; 64]);
    assert_eq!(fixture.device.mixer.counters().commands_applied, 4);
    assert_eq!(fixture.device.mixer.counters().unknown_stops, 0);
    if let Some(viewer) = viewer {
        player::publish_pause(PauseState::Paused);
        player::publish_pause(PauseState::Running);
        let snapshot = viewer.take_latest().unwrap();
        assert_eq!(snapshot.score.hits, 2);
        assert_eq!(snapshot.gauge, fixture.gauge);
        assert_eq!(snapshot.pressed_lanes, 0);
    }
    let file = fixture.capture.take().unwrap().into_file();
    let rebuilt = crate::replay_playback::reconstruct(&fixture.source, file, limits()).unwrap();
    assert_eq!(rebuilt.engine().stable_hash().unwrap(), hash);
}

#[test]
fn common_solo_publication_stops_future_heads_with_identical_headless_and_attached_policy() {
    success(None);
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        success(Some(&viewer));
        Ok(())
    })
    .unwrap();
}

#[test]
fn native_stop_queue_refusals_preserve_plays_capture_and_presentation_errors_without_retry() {
    for capacity in [1, 2, 3, 8] {
        let mut fixture = gauge_fence::fatal_fixture(capacity, 1);
        let initial = fixture
            .runtime
            .advance_to(point(1, 0), &ExplicitDomains, point(2, 0))
            .unwrap();
        publish_actual(&mut fixture, initial).unwrap();
        let (publisher, viewer) = player::channel();
        player::with_publisher(publisher, || {
            player::publish_local_chart(
                &fixture.source,
                &fixture.source.compile().unwrap().chart,
                &[
                    crate::local_players::PlayerId(7),
                    crate::local_players::PlayerId(9),
                ],
            )
            .unwrap();
            let actual = fixture
                .runtime
                .process_input(
                    input(20_000_000, 1, ButtonState::Down),
                    &ExplicitDomains,
                    point(2, 40_000_000),
                )
                .unwrap();
            let failure = publish_actual(&mut fixture, actual).unwrap_err();
            let failure = failure
                .downcast_ref::<NativeReportObservationError>()
                .unwrap();
            assert!(failure.gauge_error.is_none() && failure.competition_error.is_none());
            assert!(failure.capture_error.is_some() && failure.presentation_error.is_some());
            let report = &failure.report;
            assert_eq!(
                (
                    report.bound_inputs.len(),
                    report.judge_events.len(),
                    report.hazard_events.len()
                ),
                (2, 2, 1)
            );
            let all = [
                AudioCommand::Play {
                    sample: SampleId(1),
                    voice: VoiceId(17),
                    at: Timestamp::from_nanos(40_000_000),
                    gain: 1.0,
                },
                AudioCommand::Play {
                    sample: SampleId(1),
                    voice: VoiceId(18),
                    at: Timestamp::from_nanos(40_000_000),
                    gain: 1.0,
                },
                stop(17, 40_000_000),
                stop(18, 40_000_000),
            ];
            let admitted = capacity.min(4);
            assert_eq!(report.audio_commands, all[..admitted]);
            assert_eq!(
                report
                    .audio_failures
                    .iter()
                    .map(|error| error.command)
                    .collect::<Vec<_>>(),
                all[admitted..]
            );
            assert!(
                report
                    .audio_failures
                    .iter()
                    .all(|error| error.reason == QueuePushError::Full)
            );
            assert_eq!(
                fixture.runtime.telemetry().counters().audio_commands,
                admitted as u64
            );
            assert_eq!(
                fixture.runtime.telemetry().counters().queue_full,
                (4 - admitted) as u64
            );
            assert_eq!(
                fixture.runtime.gameplay_fence(),
                Some(Timestamp::from_nanos(20_000_000))
            );
            assert_eq!(
                fixture.gauge.snapshot().failure,
                Some(GaugeFailure::InstantDeath)
            );
            assert_eq!(fixture.capture.as_ref().unwrap().records().len(), 1);
            let hash = fixture.runtime.judge().stable_hash().unwrap();
            let pcm = rendered(&mut fixture);
            assert_eq!(&pcm[..40], &[0.0; 40]);
            assert_eq!(
                &pcm[40..43],
                match capacity {
                    1 | 3 => &[0.25, 0.5, 0.0],
                    2 => &[0.5, 1.0, 0.0],
                    _ => &[0.0; 3],
                }
            );
            assert_eq!(
                fixture.device.mixer.counters().commands_applied,
                admitted as u64
            );
            assert_eq!(fixture.device.mixer.counters().unknown_stops, 0);
            assert!(
                fixture
                    .runtime
                    .fence_gameplay_sounds(Timestamp::from_nanos(70_000_000))
                    .is_none()
            );
            assert_eq!(
                fixture.runtime.telemetry().counters().audio_commands,
                admitted as u64
            );
            assert_eq!(fixture.runtime.judge().stable_hash().unwrap(), hash);
            player::publish_pause(PauseState::Paused);
            player::publish_pause(PauseState::Running);
            assert!(
                viewer
                    .take_latest()
                    .unwrap()
                    .players
                    .iter()
                    .all(|member| member.score.hits == 0)
            );
            Ok(())
        })
        .unwrap();
    }
}
