//! Shared solo finalization after the caller has attempted all native cleanup.
use crate::{
    competition_live::LiveCompetition, native_gameplay::NativeGameplayResult,
    replay_capture::LiveReplayCapture,
};
use std::path::Path;

/// Finish competition and attempt one capture publication before returning the original first error.
pub fn finish_solo(
    outcome: NativeGameplayResult<()>,
    output_stop: NativeGameplayResult<()>,
    input_close: NativeGameplayResult<()>,
    competition: Option<&mut LiveCompetition>,
    capture: Option<LiveReplayCapture>,
    path: Option<&Path>,
    save: impl FnOnce(Option<LiveReplayCapture>, Option<&Path>, bool) -> NativeGameplayResult<()>,
) -> NativeGameplayResult<()> {
    let failed_session = outcome.is_err() || output_stop.is_err() || input_close.is_err();
    if let Some(competition) = competition {
        competition.finish();
    }
    let saved = save(capture, path, failed_session);
    if let Err(error) = &saved {
        eprintln!(
            "replay save error after cleanup (valid captured prefix retained until save): {error}"
        );
    }
    outcome?;
    output_stop?;
    input_close?;
    saved?;
    Ok(())
}
/// Original exclusive-create capture behavior shared by solo and cohort callers.
pub fn save_capture(
    capture: Option<LiveReplayCapture>,
    path: Option<&Path>,
    failed_session: bool,
) -> NativeGameplayResult<()> {
    let Some(capture) = capture else {
        return Ok(());
    };
    let path = path.ok_or("enabled replay capture missing save path")?;
    let records = capture.records().len();
    let bytes = capture.encoded_bytes();
    println!(
        "replay capture: records={records}, encoded_bytes={bytes}, status={}, path={path:?}; accepted judge operations, physical output unverified",
        if failed_session {
            "valid prefix of failed session"
        } else {
            "complete recorded session"
        }
    );
    let written = capture.save_new(path)?;
    println!("replay create_new saved {written} bytes to {path:?}");
    Ok(())
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use std::{cell::Cell, error::Error, fmt, sync::Arc};
    #[derive(Debug)]
    struct Marker(Arc<usize>);
    impl fmt::Display for Marker {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "stage{}", self.0)
        }
    }
    impl Error for Marker {}
    fn failure(token: &Arc<usize>) -> NativeGameplayResult<()> {
        Err(Box::new(Marker(Arc::clone(token))))
    }
    #[test]
    fn every_failure_combination_saves_once_and_retains_first_error_identity() {
        let tokens: [Arc<usize>; 4] = std::array::from_fn(Arc::new);
        for mask in 0..16 {
            let calls = Cell::new(0);
            let stage = |index| {
                if mask & (1 << index) != 0 {
                    failure(&tokens[index])
                } else {
                    Ok(())
                }
            };
            let result = finish_solo(
                stage(0),
                stage(1),
                stage(2),
                None,
                None,
                None,
                |capture, path, failed| {
                    calls.set(calls.get() + 1);
                    assert!(capture.is_none());
                    assert!(path.is_none());
                    assert_eq!(failed, mask & 7 != 0);
                    stage(3)
                },
            );
            assert_eq!(calls.get(), 1);
            if mask == 0 {
                assert!(result.is_ok());
            } else {
                let first = (0..4).find(|index| mask & (1 << index) != 0).unwrap();
                let error = result.unwrap_err();
                assert!(Arc::ptr_eq(
                    &error.downcast_ref::<Marker>().unwrap().0,
                    &tokens[first]
                ));
            }
        }
    }
    #[test]
    fn disabled_capture_still_passes_supplied_path_and_cancellation_success() {
        let path = Path::new("untouched/path.bkr");
        finish_solo(
            Ok(()),
            Ok(()),
            Ok(()),
            None,
            None,
            Some(path),
            |capture, actual, failed| {
                assert!(capture.is_none());
                assert_eq!(actual, Some(path));
                assert!(!failed);
                Ok(())
            },
        )
        .unwrap();
    }
    struct Identity;
    impl beatkernel::time::ClockMapper for Identity {
        fn map(
            &self,
            from: beatkernel::time::ClockPoint,
            to: beatkernel::time::ClockDomainId,
        ) -> Option<beatkernel::time::Timestamp> {
            (from.domain == to).then_some(from.timestamp)
        }
        fn quality(&self) -> beatkernel::time::ClockMappingQuality {
            beatkernel::time::ClockMappingQuality::Exact
        }
    }
    #[test]
    fn actual_runtime_capture_keeps_section_seed_records_and_bytes_after_failure() {
        use beatkernel::{
            audio::command_queue,
            input::*,
            judge::JudgeEngine,
            replay::codec::{ReplayFile, encode_replay},
            runtime::Runtime,
            time::*,
            transport::{Rate, Transport},
        };
        let original = beatkernel_bms::parse(
            "#BPM 3000\n#WAV01 original.wav\n#00011:00010000\n",
            Default::default(),
        )
        .unwrap();
        let start = Timestamp::from_nanos(20_000_000);
        let source = crate::section_start::source_at(&original, start).unwrap();
        let config = crate::native_judge::NativeJudgeConfig {
            early: 0,
            late: 0,
            offset: 0,
            preroll: 0,
            output: ClockDomainId(1),
            end: None,
        };
        let engine = JudgeEngine::new(
            source.compile().unwrap().chart,
            source.rules(),
            config.profile().unwrap(),
        )
        .unwrap();
        let limits = crate::native_judge::capture_limits(true, 8192, 8)
            .unwrap()
            .unwrap();
        let mut capture =
            crate::native_judge::prepare_capture(&engine, ClockDomainId(1), start, 3, Some(limits))
                .unwrap()
                .unwrap();
        let physical = PhysicalControlId::keyboard(7);
        let bindings = BindingMap::from_bindings([Binding {
            device: DeviceSelector::Exact(DeviceId(3)),
            physical,
            game_control: GameControlId(0x11),
        }])
        .unwrap();
        let (producer, _consumer) = command_queue(1).unwrap();
        let mut runtime = Runtime::new(
            ClockDomainId(1),
            ClockDomainId(1),
            Transport::new(Timestamp::ZERO, start, Rate::NORMAL),
            bindings,
            engine,
            producer,
            vec![],
            0,
        )
        .unwrap();
        let point = |n| ClockPoint {
            domain: ClockDomainId(1),
            timestamp: Timestamp::from_nanos(n),
        };
        let hit = runtime
            .process_input(
                PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(3), point(0), 0),
                    control: physical,
                    state: ButtonState::Down,
                }),
                &Identity,
                point(0),
            )
            .unwrap();
        assert_eq!(hit.judge_events.len(), 1);
        capture.record_report(&hit).unwrap();
        let advance = runtime.advance_to(point(1), &Identity, point(1)).unwrap();
        capture.record_report(&advance).unwrap();
        let before = encode_replay(
            &ReplayFile::new(capture.header().clone(), capture.records().to_vec()),
            limits,
        )
        .unwrap();
        let token = Arc::new(99);
        let path = Path::new("original:unsuffixed.bkr");
        let result = finish_solo(
            failure(&token),
            Ok(()),
            Ok(()),
            None,
            Some(capture),
            Some(path),
            |capture, actual, failed| {
                assert!(failed);
                assert_eq!(actual, Some(path));
                let file = capture.unwrap().into_file();
                assert_eq!(file.records.len(), 2);
                assert_eq!(encode_replay(&file, limits).unwrap(), before);
                let (_, recorded_start, seed) =
                    crate::replay_playback::decode_chart_setup(&file.header.options).unwrap();
                assert_eq!(recorded_start, start);
                assert_eq!(seed, 3);
                let mut replay =
                    crate::replay_playback::reconstruct(&original, file, limits).unwrap();
                replay.seek_cursor(2).unwrap();
                assert_eq!(replay.results(), hit.judge_events);
                Ok(())
            },
        );
        assert!(Arc::ptr_eq(
            &result.unwrap_err().downcast_ref::<Marker>().unwrap().0,
            &token
        ));
    }
}
