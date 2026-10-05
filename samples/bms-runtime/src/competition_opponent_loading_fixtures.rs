use super::*;
use crate::{
    competition::{Competition, OpponentKind, CompetitionError},
    replay_capture::LiveReplayCapture,
};
use beatkernel::{
    input::{
        ButtonEvent, ButtonState, CodecLimits, DeviceId, EventMeta, GameInputEvent,
        PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeEvent, JudgeGrade, JudgeProfile, JudgeWindow},
    replay::{
        ReplaySession,
        codec::{ReplayCodecLimits, ReplayFile},
    },
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};
use std::{collections::VecDeque, sync::Arc};

// Neither key nor error requires Clone, Debug, Display or Error.
struct Key(u32);
struct Refusal(Arc<u8>);
struct Loader {
    replies: VecDeque<std::result::Result<LoadedOpponent, Refusal>>,
    calls: Vec<(usize, u32, [usize; 5])>,
}
impl OpponentReplayPort for Loader {
    type Key = Key;
    type Error = Refusal;
    fn load(
        &mut self,
        key: &Key,
        limits: ReplayCodecLimits,
    ) -> std::result::Result<LoadedOpponent, Refusal> {
        self.calls
            .push((key as *const Key as usize, key.0, limit_values(limits)));
        self.replies
            .pop_front()
            .expect("unexpected opponent resource access")
    }
}
fn limit_values(limits: ReplayCodecLimits) -> [usize; 5] {
    [
        limits.max_file_bytes(),
        limits.max_records(),
        limits.max_header_bytes(),
        limits.input_limits().max_encoded_bytes(),
        limits.input_limits().max_payload_bytes(),
    ]
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(1 << 20, 127, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn recording(domain: u32) -> (beatkernel_bms::BmsChart, ReplayFile, Vec<JudgeEvent>) {
    let source = beatkernel_bms::parse(
        "#BPM 60\n#WAV01 key.wav\n#00011:00010001",
        Default::default(),
    )
    .unwrap();
    let judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    let domain = ClockDomainId(domain);
    let header = LiveReplayCapture::new(&judge, domain, limits())
        .unwrap()
        .into_file()
        .header;
    let mut session = ReplaySession::new(header.clone(), judge).unwrap();
    session
        .push_input(
            GameInputEvent {
                game_control: source.notes[0].lane.control(),
                physical: PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(
                        DeviceId(u64::MAX),
                        ClockPoint {
                            domain,
                            timestamp: Timestamp::from_nanos(17),
                        },
                        37,
                    ),
                    control: PhysicalControlId::keyboard(4),
                    state: ButtonState::Down,
                }),
            },
            Timestamp::from_nanos(1_000_000_000),
        )
        .unwrap();
    let events = session.results().to_vec();
    (
        source,
        ReplayFile::new(header, session.records().to_vec()),
        events,
    )
}
fn loaded(file: ReplayFile, label: &str) -> std::result::Result<LoadedOpponent, Refusal> {
    Ok(LoadedOpponent {
        file,
        label: label.into(),
    })
}
fn requests(keys: &[Key]) -> Vec<OpponentRequest<'_, Key>> {
    keys.iter()
        .enumerate()
        .map(|(i, key)| OpponentRequest {
            kind: if i == 0 {
                OpponentKind::Own
            } else {
                OpponentKind::Other
            },
            key,
        })
        .collect()
}
fn loader(
    replies: impl IntoIterator<Item = std::result::Result<LoadedOpponent, Refusal>>,
) -> Loader {
    Loader {
        replies: replies.into_iter().collect(),
        calls: vec![],
    }
}

#[test]
fn complete_capacity_preflight_and_empty_batches_never_touch_resources_or_local_progress() {
    let (source, file, events) = recording(17);
    for (capacity, existing, count) in [(0, 0, 1), (2, 2, 1), (2, 1, 2), (2, 0, 3)] {
        let mut comparison = Competition::new(file.header.clone(), capacity).unwrap();
        comparison
            .observe(&events, Timestamp::from_nanos(604_800_000_000_001))
            .unwrap();
        for _ in 0..existing {
            comparison
                .add_replay(
                    &source,
                    file.clone(),
                    limits(),
                    OpponentKind::Other,
                    "existing",
                )
                .unwrap();
        }
        let before = comparison.score().clone();
        let time = comparison.song_time();
        let mut loader = loader([]);
        let keys: Vec<_> = (0..count).map(Key).collect();
        assert!(matches!(
            load_opponents(
                &mut loader,
                &source,
                &mut comparison,
                &requests(&keys),
                limits()
            ),
            Err(OpponentLoadError::Competition(
                CompetitionError::TooManyOpponents
            ))
        ));
        assert!(loader.calls.is_empty());
        assert_eq!(comparison.opponents().len(), existing);
        assert_eq!(
            comparison.remaining_opponent_capacity(),
            capacity - existing
        );
        assert_eq!(comparison.score(), &before);
        assert_eq!(comparison.song_time(), time);
        assert!(load_opponents(&mut loader, &source, &mut comparison, &[], limits()).is_ok());
        assert!(loader.calls.is_empty());
        assert_eq!(comparison.opponents().len(), existing);
    }
}

#[test]
fn original_opaque_keys_limits_categories_and_adapter_labels_preserve_request_order() {
    let (source, file, _) = recording(17);
    let mut comparison = Competition::new(file.header.clone(), 3).unwrap();
    let keys = [Key(91), Key(7), Key(u32::MAX)];
    let labels = ["/private/own.bkr", "다른 기록", "adapter supplied third"];
    let mut loader = loader(labels.iter().map(|label| loaded(file.clone(), label)));
    assert!(
        load_opponents(
            &mut loader,
            &source,
            &mut comparison,
            &requests(&keys),
            limits()
        )
        .is_ok()
    );
    for (i, key) in keys.iter().enumerate() {
        assert_eq!(
            loader.calls[i],
            (key as *const Key as usize, key.0, limit_values(limits()))
        );
        assert_eq!(comparison.opponents()[i].label(), labels[i]);
        assert_eq!(
            comparison.opponents()[i].kind(),
            if i == 0 {
                OpponentKind::Own
            } else {
                OpponentKind::Other
            }
        );
        assert_eq!(comparison.opponents()[i].score().hits, 0);
        assert_eq!(
            comparison.opponents()[i].recorded_until(),
            Some(Timestamp::from_nanos(1_000_000_000))
        );
    }
    assert_eq!(comparison.remaining_opponent_capacity(), 0);
    assert_eq!(comparison.song_time(), None);
}

#[test]
fn every_loader_refusal_position_retains_original_error_and_only_the_accepted_prefix() {
    let (source, file, events) = recording(17);
    for failed in 0..3 {
        let token = Arc::new(53);
        let keys = [Key(11), Key(23), Key(37)];
        let mut comparison = Competition::new(file.header.clone(), 3).unwrap();
        comparison
            .observe(&events, Timestamp::from_nanos(604_800_000_000_001))
            .unwrap();
        let before = comparison.score().clone();
        let mut loader = loader((0..3).map(|i| {
            if i == failed {
                Err(Refusal(token.clone()))
            } else {
                loaded(file.clone(), "accepted")
            }
        }));
        match load_opponents(
            &mut loader,
            &source,
            &mut comparison,
            &requests(&keys),
            limits(),
        ) {
            Err(OpponentLoadError::Load(Refusal(actual))) => assert!(Arc::ptr_eq(&actual, &token)),
            _ => panic!("loader error must remain the original opaque token"),
        }
        assert_eq!(loader.calls.len(), failed + 1);
        assert_eq!(loader.replies.len(), 2 - failed);
        assert_eq!(comparison.opponents().len(), failed);
        assert_eq!(comparison.score(), &before);
        assert_eq!(
            comparison.song_time(),
            Some(Timestamp::from_nanos(604_800_000_000_001))
        );
        assert!(
            comparison
                .opponents()
                .iter()
                .all(|opponent| opponent.score().hits == 1 && opponent.score().misses == 0)
        );
    }
}

#[test]
fn malformed_later_decoded_files_fail_real_admission_without_loading_the_third_resource() {
    let (source, file, events) = recording(17);
    for corruption in 0..7 {
        let mut invalid = file.clone();
        match corruption {
            0 => invalid.header.chart_identity.push(99),
            1 => invalid.header.rules_identity.push(99),
            2 => invalid.header.options.push(99),
            3 => invalid.header.seed += 1,
            4 => invalid.header.version += 1,
            5 => invalid.runtime_version.push_str("-unsupported"),
            _ => invalid.records[0].ordinal = u64::MAX,
        }
        let mut comparison = Competition::new(file.header.clone(), 3).unwrap();
        comparison
            .observe(&events, Timestamp::from_nanos(604_800_000_000_001))
            .unwrap();
        let before = comparison.score().clone();
        let keys = [Key(19), Key(29), Key(39)];
        let mut loader = loader([
            loaded(file.clone(), "first"),
            loaded(invalid, "must refuse"),
            loaded(file.clone(), "never loaded"),
        ]);
        let result = load_opponents(
            &mut loader,
            &source,
            &mut comparison,
            &requests(&keys),
            limits(),
        );
        if corruption < 5 {
            assert!(matches!(
                result,
                Err(OpponentLoadError::Competition(
                    CompetitionError::IncompatibleSetup
                ))
            ));
        } else {
            assert!(matches!(
                result,
                Err(OpponentLoadError::Competition(CompetitionError::Playback(
                    _
                )))
            ));
        }
        assert_eq!(loader.calls.len(), 2);
        assert_eq!(loader.replies.len(), 1);
        assert_eq!(comparison.opponents().len(), 1);
        assert_eq!(comparison.opponents()[0].label(), "first");
        assert_eq!(comparison.opponents()[0].score().hits, 1);
        assert_eq!(comparison.opponents()[0].score().misses, 0);
        assert_eq!(comparison.score(), &before);
        assert_eq!(
            comparison.song_time(),
            Some(Timestamp::from_nanos(604_800_000_000_001))
        );
    }
}

#[test]
fn active_loading_accepts_original_capture_domains_without_inventing_unrecorded_tail_misses() {
    let (source, local, events) = recording(17);
    let (_, remote, _) = recording(91);
    let empty = ReplayFile::new(remote.header.clone(), vec![]);
    let mut comparison = Competition::new(local.header, 2).unwrap();
    comparison
        .observe(&events, Timestamp::from_nanos(72_000_000_000_001))
        .unwrap();
    let before = comparison.score().clone();
    let keys = [Key(91), Key(92)];
    let mut loader = loader([
        loaded(remote, "truncated hit"),
        loaded(empty, "empty prefix"),
    ]);
    assert!(
        load_opponents(
            &mut loader,
            &source,
            &mut comparison,
            &requests(&keys),
            limits()
        )
        .is_ok()
    );
    assert_eq!(comparison.score(), &before);
    assert_eq!(
        comparison.song_time(),
        Some(Timestamp::from_nanos(72_000_000_000_001))
    );
    assert_eq!(comparison.opponents()[0].score().hits, 1);
    assert_eq!(comparison.opponents()[0].score().misses, 0);
    assert_eq!(comparison.opponents()[1].score().hits, 0);
    assert_eq!(comparison.opponents()[1].score().misses, 0);
    assert_eq!(comparison.opponents()[1].recorded_until(), None);
    comparison
        .observe(&[], Timestamp::from_nanos(604_800_000_000_001))
        .unwrap();
    assert_eq!(comparison.score(), &before);
    assert!(
        comparison
            .opponents()
            .iter()
            .all(|opponent| opponent.score().misses == 0)
    );
    assert_eq!(
        comparison.opponents()[0].recorded_until(),
        Some(Timestamp::from_nanos(1_000_000_000))
    );
}
