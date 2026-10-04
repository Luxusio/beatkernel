//! Deferred presentation and actual portable routing; no asset admission or GPU execution.
use crate::{
    browser_input::TouchInputSetup,
    player_chart::{PlayerChart, PlayerNote},
    playfield_layout::default_touch_bounds,
    section_start::source_at,
};
use beatkernel::{
    audio::SampleId,
    chart::{Beat, ObjectId},
    input::{
        BackendId, ContactId, DeviceId, EventMeta, GameControlId, PhysicalControlId,
        PhysicalInputEvent, Position2, TouchEvent, TouchPhase, TouchRoute,
    },
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use beatkernel_bms::{BmsChart, parse};

const ONLY: &str = "#TITLE Invisible lanes\n#ARTIST 실제 원본\n#BPM 60\n#WAV01 key\n\
    #00031:0101\n#00036:01\n#00041:01";
const VISIBLE: &str = "#TITLE Exact timeline\n#ARTIST 원본\n#BPM 120\n#BPM01 240\n\
    #STOP01 48\n#WAV01 key\n#BMP01 image\n#00002:0.75\n#00008:000100\n\
    #00009:000100\n#00004:01\n#00011:000001\n#00152:0101";
const INVISIBLE: &str = "#00036:01\n#00032:01\n#00031:000101\n#00141:01\n#00246:01";
fn ts(nanos: i64) -> Timestamp {
    Timestamp::from_nanos(nanos)
}
fn source(text: &str) -> BmsChart {
    parse(text, Default::default()).unwrap()
}
fn project(source: &BmsChart) -> PlayerChart {
    PlayerChart::from_compiled(source, &source.compile().unwrap().chart).unwrap()
}
fn mixed() -> BmsChart {
    source(&format!("{VISIBLE}\n{INVISIBLE}"))
}
fn event(contact: u64, phase: TouchPhase) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(
        DeviceId(u64::MAX),
        ClockPoint {
            domain: ClockDomainId(0x57494e),
            timestamp: ts(9_007_199_254_740_993),
        },
        contact,
    );
    meta.original_clock_point = Some(ClockPoint {
        domain: ClockDomainId(7),
        timestamp: ts(-1),
    });
    PhysicalInputEvent::Touch(TouchEvent {
        meta,
        control: PhysicalControlId::Native {
            backend: BackendId(0x57544f55),
            code: 0,
        },
        contact: ContactId(contact),
        phase,
        position: Position2 { x: -12.5, y: 900.0 },
        pressure: Some(0.375),
    })
}

#[test]
fn invisible_only_projection_exposes_real_touch_lanes_without_creating_visible_objects() {
    let source = source(ONLY);
    let before = source.clone();
    let compiled = source.compile().unwrap();
    let chart = PlayerChart::from_compiled(&source, &compiled.chart).unwrap();
    assert_eq!(chart.title, "Invisible lanes");
    assert_eq!(chart.artist, "실제 원본");
    assert_eq!(chart.lanes, [0x16, 0x11, 0x21]);
    assert_eq!(chart.duration_ns, 2_000_000_000);
    assert!(source.notes.is_empty() && source.source.objects.is_empty());
    assert!(compiled.chart.objects().is_empty() && chart.notes.is_empty());
    assert!(chart.note_by_object(ObjectId(0)).is_none());
    assert!(chart.note_by_object(ObjectId(u64::MAX)).is_none());
    assert!(
        chart
            .visible_notes(ts(1_000_000_000), i64::MAX, i64::MAX, 2048)
            .is_empty()
    );
    let mut indices = vec![99];
    chart
        .visible_note_indices_checked(ts(0), i64::MAX, 0, &mut indices)
        .unwrap();
    assert!(indices.is_empty());
    let bounds = default_touch_bounds(&chart.lanes).unwrap();
    assert_eq!(
        bounds,
        [
            80.0, 110.0, 293.0, 634.0, 293.0, 110.0, 506.0, 634.0, 506.0, 110.0, 720.0, 634.0
        ]
    );
    let words: Vec<_> = chart
        .lanes
        .iter()
        .flat_map(|&lane| [u32::from(lane), 1, u32::MAX, u32::MAX, 1, 0x57544f55, 0])
        .collect();
    let mut setup = TouchInputSetup::new(&words, &bounds, &chart.lanes, 8).unwrap();
    for (index, (x, lane)) in [(80.0, 0x16), (293.0, 0x11), (506.0, 0x21)]
        .into_iter()
        .enumerate()
    {
        let contact = u64::MAX - index as u64;
        let original = event(contact, TouchPhase::Down);
        let TouchRoute::Bound(bound) = setup
            .router
            .route_at(&original, Position2 { x, y: 110.0 })
            .unwrap()
        else {
            panic!("actual invisible lane must admit its touch region")
        };
        assert_eq!(bound.game_control, GameControlId(lane));
        assert_eq!(
            bound.physical, original,
            "projection must preserve acquisition metadata and raw position"
        );
        for phase in [TouchPhase::Move, TouchPhase::Up] {
            let original = event(contact, phase);
            let TouchRoute::Bound(bound) = setup
                .router
                .route_at(&original, Position2 { x: 800.0, y: 700.0 })
                .unwrap()
            else {
                panic!("owned contact release")
            };
            assert_eq!(bound.game_control, GameControlId(lane));
            assert_eq!(bound.physical, original);
        }
    }
    assert_eq!(setup.router.active_contacts(), 0);
    for (contact, x, y) in [(20, 720.0, 110.0), (21, 80.0, 634.0), (22, 79.0, 110.0)] {
        assert_eq!(
            setup
                .router
                .route_at(&event(contact, TouchPhase::Down), Position2 { x, y })
                .unwrap(),
            TouchRoute::Ignored
        );
    }
    assert_eq!(source, before);

    // All eighteen source channels, deliberately reversed, use the original
    // outside-edge scratch order. Repeated selections never add extra lanes.
    let mut text = String::from("#BPM 60\n#WAV01 key\n");
    for channel in [
        0x49, 0x48, 0x47, 0x46, 0x45, 0x44, 0x43, 0x42, 0x41, 0x39, 0x38, 0x37, 0x36, 0x35, 0x34,
        0x33, 0x32, 0x31,
    ] {
        text.push_str(&format!("#{:03}{channel:02X}:0101\n", 0));
    }
    let all = project(&self::source(&text));
    assert_eq!(
        all.lanes,
        [
            0x16, 0x11, 0x12, 0x13, 0x14, 0x15, 0x17, 0x18, 0x19, 0x21, 0x22, 0x23, 0x24, 0x25,
            0x27, 0x28, 0x29, 0x26
        ]
    );
    assert!(all.notes.is_empty());
    assert_eq!(default_touch_bounds(&all.lanes).unwrap().len(), 72);
}

#[test]
fn mixed_projection_keeps_compiled_identity_exact_bpm_stop_times_and_practice_lane_availability() {
    let source = mixed();
    let visible_source = self::source(VISIBLE);
    let compiled = source.compile().unwrap();
    assert_eq!(
        compiled,
        visible_source.compile().unwrap(),
        "invisible selections do not alter gameplay or BGA"
    );
    let chart = PlayerChart::from_compiled(&source, &compiled.chart).unwrap();
    assert_eq!(chart.lanes, [0x16, 0x11, 0x12, 0x21, 0x26]);
    assert_eq!(
        chart.notes,
        [
            PlayerNote {
                object: ObjectId(1),
                lane_index: 1,
                start: ts(1_000_000_000),
                end: None
            },
            PlayerNote {
                object: ObjectId(2),
                lane_index: 2,
                start: ts(1_250_000_000),
                end: Some(ts(1_750_000_000))
            },
        ]
    );
    assert_eq!(chart.duration_ns, 2_250_000_000);
    // Beat one changes to 240 BPM and stops for 0.25s; measure zero
    // has three beats. The measure-two scratch selection is at 2.25s.
    assert_eq!(
        source
            .compile_invisible()
            .unwrap()
            .iter()
            .map(|m| (m.lane.channel(), m.at.as_nanos()))
            .collect::<Vec<_>>(),
        [
            (0x16, 0),
            (0x12, 0),
            (0x11, 500_000_000),
            (0x11, 1_000_000_000),
            (0x21, 1_250_000_000),
            (0x26, 2_250_000_000)
        ]
    );
    assert_eq!(
        chart
            .visible_notes(ts(1_000_000_000), 0, 0, 10)
            .iter()
            .map(|n| n.object)
            .collect::<Vec<_>>(),
        [ObjectId(1)]
    );
    assert_eq!(
        chart
            .visible_notes(ts(1_500_000_000), 0, 0, 10)
            .iter()
            .map(|n| n.object)
            .collect::<Vec<_>>(),
        [ObjectId(2)]
    );
    for (index, id) in [ObjectId(1), ObjectId(2)].into_iter().enumerate() {
        assert_eq!(chart.note_index_by_object(id), Some(index));
        assert_eq!(chart.note_by_object(id), Some(&chart.notes[index]));
    }
    let plain = project(&visible_source);
    assert_eq!(plain.lanes, [0x11, 0x12]);
    assert_eq!(plain.duration_ns, 1_750_000_000);
    for at in [0, 500_000_000, 1_250_000_000, i64::MAX] {
        assert_eq!(chart.bga_state(ts(at)), plain.bga_state(ts(at)));
        assert_eq!(chart.bga_opacity(ts(at)), plain.bga_opacity(ts(at)));
    }
    let selected = source_at(&source, ts(2_000_000_000)).unwrap();
    assert!(selected.notes.is_empty() && selected.source.objects.is_empty());
    assert_eq!(selected.invisible, source.invisible);
    let practice = project(&selected);
    assert_eq!(practice.lanes, chart.lanes);
    assert_eq!(
        practice.duration_ns, 2_250_000_000,
        "duration remains original song time"
    );
    assert!(practice.notes.is_empty());
    assert!(
        practice
            .visible_notes_checked(ts(2_000_000_000), 1_000_000_000, 1_000_000_000)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        default_touch_bounds(&practice.lanes).unwrap(),
        default_touch_bounds(&chart.lanes).unwrap()
    );
}

#[test]
fn malformed_nonempty_invisible_data_is_rejected_and_empty_data_keeps_existing_projection_queries()
{
    let valid = mixed();
    let compiled = valid.compile().unwrap();
    let original = valid.clone();
    let mutations: [fn(&mut BmsChart); 7] = [
        |s| s.invisible_ticks_per_beat = 0,
        |s| {
            s.source.ticks_per_beat = 2;
            s.invisible_ticks_per_beat = 3;
        },
        |s| s.invisible[0].sample = SampleId(0),
        |s| s.invisible[0].sample = SampleId(3844),
        |s| {
            s.samples.remove(&1);
        },
        |s| {
            let mut duplicate = s.invisible[0];
            duplicate.ordinal = 99;
            s.invisible.push(duplicate);
        },
        |s| {
            s.invisible[1].ordinal = s.invisible[0].ordinal;
        },
    ];
    for mutate in mutations {
        let mut invalid = valid.clone();
        mutate(&mut invalid);
        let before = invalid.clone();
        assert!(PlayerChart::from_compiled(&invalid, &compiled.chart).is_err());
        assert_eq!(
            invalid, before,
            "presentation refusal cannot repair or mutate source data"
        );
    }
    let mut overflow = valid.clone();
    overflow.invisible[0].beat = Beat::new(i64::MAX).unwrap();
    assert!(PlayerChart::from_compiled(&overflow, &compiled.chart).is_err());
    assert_eq!(valid, original);
    for text in ["#TITLE Empty\n#BPM 120", VISIBLE] {
        let source = self::source(text);
        let ordinary = project(&source);
        // With no invisible selections the extra grid is irrelevant: do not
        // run its validator or change legacy note/index/query behavior.
        let mut empty = source.clone();
        empty.invisible_ticks_per_beat = 0;
        let same = project(&empty);
        assert_eq!(
            (
                &same.title,
                &same.artist,
                &same.lanes,
                &same.notes,
                same.duration_ns,
                same.poor_bga_mode
            ),
            (
                &ordinary.title,
                &ordinary.artist,
                &ordinary.lanes,
                &ordinary.notes,
                ordinary.duration_ns,
                ordinary.poor_bga_mode
            )
        );
        for at in [0, 1_000_000_000, 1_500_000_000, i64::MAX] {
            assert_eq!(
                same.visible_notes(ts(at), 0, 0, 10),
                ordinary.visible_notes(ts(at), 0, 0, 10)
            );
            assert_eq!(same.bga_state(ts(at)), ordinary.bga_state(ts(at)));
        }
    }
    let empty = project(&self::source("#BPM 120\n#00031:000000\n#00149:00"));
    assert!(empty.lanes.is_empty() && empty.notes.is_empty());
    assert_eq!(empty.duration_ns, 0);
    assert!(default_touch_bounds(&empty.lanes).unwrap().is_empty());
}

#[cfg(feature = "graphics")]
#[test]
fn actual_scene_uses_invisible_lane_geometry_but_emits_instances_only_for_real_visible_objects() {
    use crate::{scene::Scene, ui::organisms::playfield};
    let invisible = project(&source(ONLY));
    let mut scene = Scene::new(960, 720);
    playfield(&mut scene, &invisible, ts(0), 3_000_000_000).unwrap();
    scene.status().unwrap();
    assert_eq!(scene.playfields().len(), 1);
    assert!(scene.playfields()[0].instances.is_empty());
    assert_eq!(
        (scene.playfields()[0].top, scene.playfields()[0].bottom),
        (110.0, 625.0)
    );
    for background in [
        [80.0, 110.0, 212.0, 524.0],
        [293.0, 110.0, 212.0, 524.0],
        [506.0, 110.0, 213.0, 524.0],
    ] {
        assert!(scene.rectangles().iter().any(|r| r.bounds == background));
    }
    let mixed = project(&mixed());
    scene.clear();
    playfield(&mut scene, &mixed, ts(1_000_000_000), 2_000_000_000).unwrap();
    scene.status().unwrap();
    let instances = &scene.playfields()[0].instances;
    assert_eq!(
        instances.len(),
        4,
        "one instant head and one real hold body/tail/head"
    );
    assert_eq!(
        instances
            .iter()
            .map(|i| (i.geometry[0], i.geometry[1], i.appearance[0]))
            .collect::<Vec<_>>(),
        [
            (211.0, 122.0, 2.0),
            (342.0, 116.0, 0.0),
            (339.0, 122.0, 1.0),
            (339.0, 122.0, 2.0)
        ]
    );
    let selected = project(&source_at(&self::mixed(), ts(2_000_000_000)).unwrap());
    scene.clear();
    playfield(&mut scene, &selected, ts(2_000_000_000), 2_000_000_000).unwrap();
    scene.status().unwrap();
    assert_eq!(scene.playfields().len(), 1);
    assert!(scene.playfields()[0].instances.is_empty());
    for x in [80.0, 208.0, 336.0, 464.0, 592.0] {
        assert!(
            scene
                .rectangles()
                .iter()
                .any(|r| r.bounds == [x, 110.0, 127.0, 524.0])
        );
    }
}
