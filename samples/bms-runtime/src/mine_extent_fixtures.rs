//! Deferred portable mine extent fixtures. Typed preparation below does not
//! change the playable-source guard or claim native/GPU/audio-device execution.
use crate::{
    PreparedBms,
    bgm::BgmFeedReport,
    browser_input::TouchInputSetup,
    completion::SongCompletion,
    native_judge::NativeJudgeConfig,
    player_chart::{PlayerChart, PlayerNote},
    playfield_layout::default_touch_bounds,
    section_start::{prepare_at, source_at},
};
use beatkernel::{
    audio::{
        AudioFormat, AudioLimits, CommandProducer, Mixer, MixerConfig, PcmLimits, RenderReport,
        SampleBank, command_queue,
    },
    chart::{Beat, ObjectId},
    input::{
        BackendId, ContactId, DeviceId, EventMeta, GameControlId, PhysicalControlId,
        PhysicalInputEvent, Position2, TouchEvent, TouchPhase, TouchRoute,
    },
    judge::{HazardOutcome, HazardTimeline, JudgeEngine},
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use beatkernel_bms::{BmsChart, parse};

fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn source(text: &str) -> BmsChart {
    parse(text, Default::default()).unwrap()
}
fn project(source: &BmsChart) -> PlayerChart {
    PlayerChart::from_compiled(source, &source.compile().unwrap().chart).unwrap()
}
fn bounds() -> PcmLimits {
    PcmLimits::new(64, 128, 4).unwrap()
}
fn prepared(source: BmsChart) -> PreparedBms {
    let compiled = source.compile().unwrap();
    assert!(compiled.chart.objects().is_empty() && compiled.bgm.is_empty());
    PreparedBms {
        source,
        compiled,
        bank: SampleBank::new(AudioFormat::new(10, 1).unwrap(), bounds()).unwrap(),
        sounds: vec![],
        bgm_commands: vec![],
    }
}
fn judge(source: &BmsChart, late: i64, offset: i64) -> JudgeEngine {
    NativeJudgeConfig {
        early: 0,
        late,
        offset,
        preroll: 0,
        output: ClockDomainId(7),
        end: None,
    }
    .judge(source, source.compile().unwrap().chart)
    .unwrap()
}
fn mixer(bank: SampleBank) -> (CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(8).unwrap();
    let config = MixerConfig::new(
        bank.format(),
        ClockDomainId(7),
        ts(0),
        AudioLimits::new(8, 4, 8, 16, 16).unwrap(),
    );
    (producer, Mixer::new(config, bank, consumer).unwrap())
}
fn render(mixer: &mut Mixer) -> RenderReport {
    let mut pcm = [1.0; 10];
    let report = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0; 10]);
    report
}
fn presented(ns: i64) -> Option<ClockPoint> {
    Some(ClockPoint {
        domain: ClockDomainId(7),
        timestamp: ts(ns),
    })
}
fn touch(contact: u64, phase: TouchPhase) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(
            DeviceId(u64::MAX),
            ClockPoint {
                domain: ClockDomainId(99),
                timestamp: ts(9_007_199_254_740_993),
            },
            contact,
        ),
        control: PhysicalControlId::Native {
            backend: BackendId(0x57544f55),
            code: 0,
        },
        contact: ContactId(contact),
        phase,
        position: Position2 { x: -8.5, y: 900.0 },
        pressure: Some(0.5),
    })
}

#[test]
fn actual_timing_projects_a_unique_lane_union_and_routes_default_touch_without_fake_notes() {
    let ordinary = source(
        "#BPM 120\n#BPM01 240\n#STOP01 48\n#WAV01 key\n#00002:0.75\n#00008:000100\n#00009:000100\n#00011:000001\n#00032:01",
    );
    let mixed = source(
        "#BPM 120\n#BPM01 240\n#STOP01 48\n#WAV01 key\n#00002:0.75\n#00008:000100\n#00009:000100\n#00011:000001\n#00032:01\n#000D6:1E1E00\n#001E1:01\n#002E6:ZZ\n#002D1:01",
    );
    assert_eq!(mixed.compile().unwrap(), ordinary.compile().unwrap());
    let actual = project(&mixed);
    assert_eq!(actual.lanes, [0x16, 0x11, 0x12, 0x21, 0x26]);
    assert_eq!(actual.duration_ns, 2_250_000_000);
    assert_eq!(
        actual.notes,
        [PlayerNote {
            object: ObjectId(1),
            lane_index: 1,
            start: ts(1_000_000_000),
            end: None
        }]
    );
    assert_eq!(
        mixed
            .compile_mines()
            .unwrap()
            .iter()
            .map(|m| (m.lane.channel(), m.at.as_nanos()))
            .collect::<Vec<_>>(),
        [
            (0x16, 0),
            (0x16, 500_000_000),
            (0x21, 1_250_000_000),
            (0x26, 2_250_000_000),
            (0x11, 2_250_000_000),
        ]
    );
    let regions = default_touch_bounds(&actual.lanes).unwrap();
    assert_eq!(
        regions,
        [
            80.0, 110.0, 208.0, 634.0, 208.0, 110.0, 336.0, 634.0, 336.0, 110.0, 464.0, 634.0,
            464.0, 110.0, 592.0, 634.0, 592.0, 110.0, 720.0, 634.0,
        ]
    );
    let words: Vec<_> = actual
        .lanes
        .iter()
        .flat_map(|&lane| [u32::from(lane), 1, u32::MAX, u32::MAX, 1, 0x57544f55, 0])
        .collect();
    let mut setup = TouchInputSetup::new(&words, &regions, &actual.lanes, 8).unwrap();
    for (index, (x, lane)) in [
        (80.0, 0x16),
        (208.0, 0x11),
        (336.0, 0x12),
        (464.0, 0x21),
        (592.0, 0x26),
    ]
    .into_iter()
    .enumerate()
    {
        let contact = u64::MAX - index as u64;
        let raw = touch(contact, TouchPhase::Down);
        let TouchRoute::Bound(bound) = setup
            .router
            .route_at(&raw, Position2 { x, y: 110.0 })
            .unwrap()
        else {
            panic!("prepared lane must route")
        };
        assert_eq!(bound.game_control, GameControlId(lane));
        assert_eq!(bound.physical, raw);
        let release = touch(contact, TouchPhase::Cancel);
        let TouchRoute::Bound(bound) = setup
            .router
            .route_at(&release, Position2 { x: 900.0, y: -1.0 })
            .unwrap()
        else {
            panic!("owned release outside field")
        };
        assert_eq!(bound.game_control, GameControlId(lane));
        assert_eq!(bound.physical, release);
    }
    assert_eq!(setup.router.active_contacts(), 0);
    let only = project(&source("#BPM 60\n#000D6:1E\n#000D1:01\n#001E1:ZZ"));
    assert_eq!(only.lanes, [0x16, 0x11, 0x21]);
    assert_eq!(only.duration_ns, 4_000_000_000);
    assert!(only.notes.is_empty() && only.note_by_object(ObjectId(1)).is_none());
    let baseline = project(&ordinary);
    let mut unused = ordinary;
    unused.mine_ticks_per_beat = 0;
    let unchanged = project(&unused);
    assert_eq!(unchanged.lanes, baseline.lanes);
    assert_eq!(unchanged.notes, baseline.notes);
    assert_eq!(unchanged.duration_ns, baseline.duration_ns);
    let empty = project(&source("#BPM 120\n#000D1:00"));
    assert!(empty.lanes.is_empty() && empty.notes.is_empty());
    assert_eq!(empty.duration_ns, 0);
}

#[test]
fn core_count_accessors_read_the_actual_cursor_without_changing_hash_snapshot_or_failure_state() {
    let source = source("#BPM 60\n#000D1:1EZZ");
    let mut actual = judge(&source, 0, 0);
    let pristine = actual.snapshot().unwrap();
    let hash = actual.stable_hash().unwrap();
    for _ in 0..64 {
        assert_eq!((actual.hazard_count(), actual.remaining_hazards()), (2, 2));
    }
    assert_eq!(actual.stable_hash().unwrap(), hash);
    assert!(actual.hazard_events().is_empty());
    actual.advance_to(ts(0)).unwrap();
    assert_eq!((actual.hazard_count(), actual.remaining_hazards()), (2, 1));
    assert_eq!(actual.hazard_events()[0].outcome, HazardOutcome::Avoided);
    let prefix = actual.snapshot().unwrap();
    let prefix_hash = actual.stable_hash().unwrap();
    assert!(actual.advance_to(ts(-1)).is_err());
    assert_eq!((actual.hazard_count(), actual.remaining_hazards()), (2, 1));
    assert_eq!(actual.stable_hash().unwrap(), prefix_hash);
    let restored = JudgeEngine::from_snapshot(&prefix).unwrap();
    assert_eq!(
        (restored.hazard_count(), restored.remaining_hazards()),
        (2, 1)
    );
    assert_eq!(restored.stable_hash().unwrap(), prefix_hash);
    actual.advance_to(ts(2_000_000_000)).unwrap();
    assert_eq!((actual.hazard_count(), actual.remaining_hazards()), (2, 0));
    actual.advance_to(ts(2_000_000_000)).unwrap();
    assert!(actual.hazard_events().is_empty());
    assert_eq!((actual.hazard_count(), actual.remaining_hazards()), (2, 0));
    actual.restore(&pristine).unwrap();
    assert_eq!((actual.hazard_count(), actual.remaining_hazards()), (2, 2));
    assert_eq!(actual.stable_hash().unwrap(), hash);
    let mut plain = source.clone();
    plain.mines.clear();
    plain.mine_ticks_per_beat = 0;
    let mut legacy = judge(&plain, 0, 0);
    let legacy_hash = legacy.stable_hash().unwrap();
    assert_eq!((legacy.hazard_count(), legacy.remaining_hazards()), (0, 0));
    assert!(legacy.restore(&prefix).is_err());
    assert_eq!(legacy.stable_hash().unwrap(), legacy_hash);
    legacy
        .configure_hazards(HazardTimeline::new(vec![], 0).unwrap())
        .unwrap();
    let configured_empty_hash = legacy.stable_hash().unwrap();
    assert_eq!((legacy.hazard_count(), legacy.remaining_hazards()), (0, 0));
    assert_eq!(legacy.stable_hash().unwrap(), configured_empty_hash);
}

#[test]
fn full_song_waits_for_offset_correct_strict_mine_frontier_consumption_and_real_software_drain() {
    for (offset, deadline, seconds) in [
        (500_000_000, 1_500_000_001, 2),
        (0, 2_000_000_001, 3),
        (-500_000_000, 2_500_000_001, 3),
    ] {
        let prepared = prepared(source("#BPM 60\n#000D1:001E"));
        let mut actual = judge(&prepared.source, 7_000_000_000, offset);
        let pristine = actual.snapshot().unwrap();
        let mut completion =
            SongCompletion::prepare(&prepared, 7_000_000_000, offset, 0, ClockDomainId(7)).unwrap();
        assert_eq!(
            completion.calibration_seconds(),
            seconds,
            "ordinary late windows do not extend mines"
        );
        let (_producer, mut mixer) = mixer(prepared.bank);
        for end in [1_000_000_000, 2_000_000_000] {
            let report = render(&mut mixer);
            assert!(
                !completion
                    .observe(
                        &actual,
                        ts(99_000_000_000),
                        BgmFeedReport::default(),
                        Some(report),
                        presented(end)
                    )
                    .unwrap(),
                "song time alone must not consume the configured hazard"
            );
        }
        actual.advance_to(ts(deadline - 1)).unwrap();
        assert_eq!(actual.remaining_hazards(), 0);
        let boundary = render(&mut mixer);
        assert!(
            !completion
                .observe(
                    &actual,
                    ts(deadline - 1),
                    BgmFeedReport::default(),
                    Some(boundary),
                    presented(3_000_000_000)
                )
                .unwrap()
        );
        let barrier = render(&mut mixer);
        assert!(
            !completion
                .observe(
                    &actual,
                    ts(deadline),
                    BgmFeedReport::default(),
                    Some(barrier),
                    None
                )
                .unwrap()
        );
        let idle = render(&mut mixer);
        assert!(
            !completion
                .observe(
                    &actual,
                    ts(deadline),
                    BgmFeedReport::default(),
                    Some(idle),
                    None
                )
                .unwrap()
        );
        let later = render(&mut mixer);
        assert!(
            !completion
                .observe(
                    &actual,
                    ts(deadline),
                    BgmFeedReport::default(),
                    Some(later),
                    presented(4_999_999_999)
                )
                .unwrap()
        );
        assert!(
            completion
                .observe(
                    &actual,
                    ts(deadline),
                    BgmFeedReport::default(),
                    Some(later),
                    presented(5_000_000_000)
                )
                .unwrap()
        );
        // Restoring an earlier judge cursor must revoke drain readiness even
        // after this completion owner has already observed a terminal prefix.
        actual.restore(&pristine).unwrap();
        assert_eq!(actual.remaining_hazards(), 1);
        let restored = render(&mut mixer);
        assert!(
            !completion
                .observe(
                    &actual,
                    ts(deadline),
                    BgmFeedReport::default(),
                    Some(restored),
                    presented(7_000_000_000)
                )
                .unwrap()
        );
        actual.advance_to(ts(deadline)).unwrap();
        let barrier = render(&mut mixer);
        assert!(
            !completion
                .observe(
                    &actual,
                    ts(deadline),
                    BgmFeedReport::default(),
                    Some(barrier),
                    presented(8_000_000_000)
                )
                .unwrap()
        );
        let idle = render(&mut mixer);
        assert!(
            completion
                .observe(
                    &actual,
                    ts(deadline),
                    BgmFeedReport::default(),
                    Some(idle),
                    presented(9_000_000_000)
                )
                .unwrap()
        );
    }
}

#[test]
fn completion_rejects_mismatched_owners_before_drain_and_checks_malformed_or_unrepresentable_extents()
 {
    let setup = prepared(source("#BPM 60\n#000D1:001E"));
    let mut actual = judge(&setup.source, 0, 0);
    actual.advance_to(ts(2_000_000_001)).unwrap();
    let mut plain = setup.source.clone();
    plain.mines.clear();
    let wrong = judge(&plain, 0, 0);
    let mut completion = SongCompletion::prepare(&setup, 0, 0, 0, ClockDomainId(7)).unwrap();
    let (_producer, mut mixer) = mixer(setup.bank);
    let rejected = render(&mut mixer);
    assert!(
        completion
            .observe(
                &wrong,
                ts(9_000_000_000),
                BgmFeedReport::default(),
                Some(rejected),
                presented(1_000_000_000)
            )
            .is_err()
    );
    let barrier = render(&mut mixer);
    assert!(
        !completion
            .observe(
                &actual,
                ts(2_000_000_001),
                BgmFeedReport::default(),
                Some(barrier),
                presented(2_000_000_000)
            )
            .unwrap()
    );
    let idle = render(&mut mixer);
    assert!(
        completion
            .observe(
                &actual,
                ts(2_000_000_001),
                BgmFeedReport::default(),
                Some(idle),
                presented(3_000_000_000)
            )
            .unwrap()
    );
    let later = render(&mut mixer);
    assert!(
        completion
            .observe(
                &wrong,
                ts(9_000_000_000),
                BgmFeedReport::default(),
                Some(later),
                presented(4_000_000_000)
            )
            .is_err()
    );
    assert!(
        completion
            .observe(
                &actual,
                ts(2_000_000_001),
                BgmFeedReport::default(),
                Some(later),
                presented(4_000_000_000)
            )
            .unwrap()
    );

    let long = prepared(self::source("#BPM 0.001\n#003D1:1E"));
    assert_eq!(project(&long.source).duration_ns, 720_000_000_000_000);
    assert_eq!(
        SongCompletion::prepare(&long, 0, 0, 0, ClockDomainId(7))
            .unwrap()
            .calibration_seconds(),
        720_001
    );
    assert!(SongCompletion::prepare(&long, 0, i64::MIN, 0, ClockDomainId(7)).is_err());
    assert!(SongCompletion::prepare(&long, 0, 0, i64::MAX, ClockDomainId(7)).is_err());
    let original = self::source("#BPM 60\n#000D1:1EZZ");
    let mut grid = original.clone();
    grid.mine_ticks_per_beat = 0;
    let mut duplicate = original.clone();
    duplicate.mines[1].ordinal = duplicate.mines[0].ordinal;
    let mut overflow = original;
    overflow.mines[1].beat = Beat::new(i64::MAX).unwrap();
    for invalid in [grid, duplicate, overflow] {
        assert!(PlayerChart::from_compiled(&invalid, &invalid.compile().unwrap().chart).is_err());
        assert!(SongCompletion::prepare(&prepared(invalid), 0, 0, 0, ClockDomainId(7)).is_err());
    }
    let mut empty = self::source("#BPM 60\n#000D1:00");
    empty.mine_ticks_per_beat = 0;
    assert_eq!(
        SongCompletion::prepare(&prepared(empty), i64::MAX, i64::MIN, 0, ClockDomainId(7))
            .unwrap()
            .calibration_seconds(),
        1
    );
}

#[test]
fn mine_only_practice_requires_a_future_or_equal_marker_and_keeps_original_identity_and_times() {
    let original = source("#BPM 60\n#000D6:1EZZ");
    let original_markers = original.compile_mines().unwrap();
    for start in [1, 1_000_000_000, 2_000_000_000] {
        let selected_source = source_at(&original, ts(start)).unwrap();
        assert_eq!(selected_source.mines, original.mines);
        assert_eq!(selected_source.compile_mines().unwrap(), original_markers);
        assert_eq!(
            source_at(&selected_source, ts(start)).unwrap(),
            selected_source
        );
        let (selected, report) =
            prepare_at(prepared(original.clone()), ts(start), bounds()).unwrap();
        assert_eq!(selected.source, selected_source);
        assert_eq!(report.start, ts(start));
        assert_eq!(
            (
                report.excluded_objects,
                report.excluded_crossing_holds,
                report.retired_bgm
            ),
            (0, 0, 0)
        );
        assert!(report.tails.is_empty() && selected.compiled.chart.objects().is_empty());
        assert_eq!(selected.bank.len(), 0);
        assert_eq!(project(&selected.source).lanes, [0x16]);
        assert_eq!(project(&selected.source).duration_ns, 2_000_000_000);
        assert_eq!(
            judge(&selected.source, 0, 0).stable_hash().unwrap(),
            judge(&original, 0, 0).stable_hash().unwrap()
        );
        let (again, again_report) = prepare_at(selected, ts(start), bounds()).unwrap();
        assert_eq!(again.source, selected_source);
        assert_eq!(again.source.compile_mines().unwrap(), original_markers);
        assert!(again_report.tails.is_empty());
    }
    for start in [2_000_000_001, 9_000_000_000, i64::MAX] {
        assert_eq!(
            source_at(&original, ts(start)).unwrap().mines,
            original.mines
        );
        assert!(prepare_at(prepared(original.clone()), ts(start), bounds()).is_err());
    }
    assert!(prepare_at(prepared(source("#BPM 60")), ts(1), bounds()).is_err());
    assert!(prepare_at(prepared(original.clone()), ts(-1), bounds()).is_err());
    let (whole, report) = prepare_at(prepared(original.clone()), ts(0), bounds()).unwrap();
    assert_eq!(whole.source, original);
    assert_eq!(report.start, ts(0));
    let mut fresh = judge(&whole.source, 0, 0);
    fresh.advance_to(ts(2_000_000_000)).unwrap();
    assert_eq!(
        fresh
            .hazard_events()
            .iter()
            .map(|e| (e.id.0, e.at, e.outcome))
            .collect::<Vec<_>>(),
        [
            (0, ts(0), HazardOutcome::Avoided),
            (1, ts(2_000_000_000), HazardOutcome::Avoided),
        ]
    );
}
