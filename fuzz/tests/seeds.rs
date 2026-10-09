use std::collections::BTreeSet;

use beatkernel::{
    chart::ChartError,
    input::{decode_event, PhysicalInputEvent},
    replay::{codec::decode_replay, ReplayOperation},
};
use beatkernel_bms::parse_seeded;
use beatkernel_bms_runtime::{
    multiplayer_room_wire::{decode_message, RoomMessage},
    multiplayer_start::StartMessage,
};
use beatkernel_layer_fuzz::{
    bms::{check_bms, parser_options, BMS_MAX_BYTES},
    chart::{chart_from_bytes, check_chart, CHART_MAX_BYTES},
    codecs::{check_input, check_replay, check_room, input_limits, replay_limits},
    seeds::seed_cases,
};

fn cases(target: &str) -> Vec<Vec<u8>> {
    let selected: Vec<_> = seed_cases()
        .into_iter()
        .filter_map(|(name, bytes)| (name == target).then_some(bytes))
        .collect();
    assert!(!selected.is_empty(), "missing {target} corpus");
    selected
}

#[derive(Default)]
struct PhysicalCoverage {
    variants: BTreeSet<u8>,
    floats: BTreeSet<u32>,
    original: bool,
    native: bool,
}

impl PhysicalCoverage {
    fn observe(&mut self, event: &PhysicalInputEvent) {
        // Literal fixture provenance, independently decoded by production APIs.
        let meta = event.meta();
        assert_eq!(meta.source.0, 42);
        assert_eq!(meta.timestamp.as_nanos(), -17);
        assert_eq!(meta.clock_domain.0, 3);
        assert_eq!(meta.sequence, 99);
        let native = meta
            .native
            .expect("native provenance must survive decoding");
        assert_eq!(native.backend.0, 12);
        assert_eq!(native.code, Some(255));
        let point = native.timestamp.unwrap();
        assert_eq!((point.domain.0, point.timestamp.as_nanos()), (5, 1234));
        let original = meta.original_clock_point.unwrap();
        assert_eq!(
            (original.domain.0, original.timestamp.as_nanos()),
            (9, -987)
        );
        self.original |= event.meta().original_clock_point.is_some();
        self.native |= event.meta().native.is_some();
        let (tag, values) = match event {
            PhysicalInputEvent::Button(_) => (0, vec![]),
            PhysicalInputEvent::Axis(e) => (1, vec![e.value]),
            PhysicalInputEvent::Touch(e) => {
                let mut values = vec![e.position.x, e.position.y];
                values.extend(e.pressure);
                (2, values)
            }
            PhysicalInputEvent::Pointer(e) => (3, vec![e.position.x, e.position.y]),
            PhysicalInputEvent::Pose(e) => (
                4,
                vec![
                    e.position.x,
                    e.position.y,
                    e.position.z,
                    e.orientation.x,
                    e.orientation.y,
                    e.orientation.z,
                    e.orientation.w,
                ],
            ),
            PhysicalInputEvent::RawHidReport(_) => (5, vec![]),
            PhysicalInputEvent::Custom(_) => (6, vec![]),
        };
        self.variants.insert(tag);
        self.floats.extend(values.into_iter().map(f32::to_bits));
    }

    fn assert_complete(&self) {
        assert_eq!(self.variants, (0..7).collect());
        assert!(self.original, "original clock metadata is absent");
        assert!(self.native, "native event metadata is absent");
        assert!(self.floats.contains(&0), "positive zero is absent");
        assert!(
            self.floats.contains(&0x8000_0000),
            "negative zero is absent"
        );
        assert!(
            self.floats.contains(&0x7fc0_0073),
            "quiet NaN payload changed"
        );
        assert!(
            self.floats.contains(&0xffa0_0042),
            "negative signaling NaN payload changed"
        );
    }
}

#[test]
fn corpus_is_reproducible_and_has_exactly_the_five_campaigns() {
    let first = seed_cases();
    assert_eq!(first, seed_cases());
    let names: BTreeSet<_> = first.iter().map(|(target, _)| *target).collect();
    assert_eq!(
        names,
        BTreeSet::from([
            "input_codec",
            "replay_codec",
            "room_codec",
            "bms_parser",
            "chart_compiler",
        ])
    );
    assert!(first.iter().all(|(_, bytes)| !bytes.is_empty()));
}

#[test]
fn input_corpus_reaches_every_real_variant_and_raw_float_metadata_family() {
    let mut coverage = PhysicalCoverage::default();
    for bytes in cases("input_codec") {
        let event = decode_event(&bytes, input_limits()).expect("seed must actually decode");
        coverage.observe(&event);
        assert!(check_input(&bytes));
    }
    coverage.assert_complete();
}

#[test]
fn replay_corpus_reaches_both_operations_and_nested_physical_families() {
    let mut coverage = PhysicalCoverage::default();
    let mut advance = false;
    for bytes in cases("replay_codec") {
        let replay = decode_replay(&bytes, replay_limits()).expect("seed must actually decode");
        for record in &replay.records {
            match &record.operation {
                ReplayOperation::Input(event) => coverage.observe(&event.physical),
                ReplayOperation::Advance => advance = true,
            }
        }
        assert!(check_replay(&bytes));
    }
    coverage.assert_complete();
    assert!(advance, "Advance operation is absent");
}

fn room_tag(message: &RoomMessage) -> u8 {
    match message {
        RoomMessage::Join { .. } => 1,
        RoomMessage::Admitted { .. } => 2,
        RoomMessage::Snapshot { .. } => 3,
        RoomMessage::Seal => 4,
        RoomMessage::Ready => 5,
        RoomMessage::Leave => 6,
        RoomMessage::ClockPing { .. } => 7,
        RoomMessage::ClockPong { .. } => 8,
        RoomMessage::Start(StartMessage::ClockReady(_)) => 9,
        RoomMessage::Start(StartMessage::Propose(_)) => 10,
        RoomMessage::Start(StartMessage::Accept(_)) => 11,
        RoomMessage::Start(StartMessage::Commit(_)) => 12,
        RoomMessage::Progress(_) => 13,
        RoomMessage::PeerProgress { .. } => 14,
        RoomMessage::FinalAck { .. } => 15,
        RoomMessage::DrainReady { .. } => 16,
        RoomMessage::DrainComplete { .. } => 17,
    }
}

#[test]
fn room_corpus_reaches_all_room_and_nested_start_variants() {
    let mut tags = BTreeSet::new();
    for bytes in cases("room_codec") {
        let message = decode_message(&bytes).expect("seed must actually decode");
        let tag = room_tag(&message);
        assert_eq!(bytes[6], tag, "decoded family disagrees with wire tag");
        tags.insert(tag);
        assert!(check_room(&bytes));
    }
    assert_eq!(tags, (1..=17).collect());
}

#[test]
fn room_corpus_contains_the_actual_maximum_join_frame() {
    let bytes = cases("room_codec")
        .into_iter()
        .find(|bytes| bytes.len() == 65_808)
        .expect("maximum Join frame must be in corpus");
    let RoomMessage::Join { identity, players } = decode_message(&bytes).unwrap() else {
        panic!("maximum frame must actually be Join")
    };
    assert_eq!(identity.len(), 65_536);
    assert_eq!(players.len(), 64);
    assert_eq!(u32::from_le_bytes(bytes[7..11].try_into().unwrap()), 65_797);
    assert_eq!(
        u32::from_le_bytes(bytes[11..15].try_into().unwrap()),
        65_536
    );
    assert!(check_room(&bytes));
}

#[test]
fn bms_corpus_reaches_real_main_and_auxiliary_schedules_for_both_seeds() {
    for random_seed in [0, 73] {
        // Objects, holds, tempo, STOP, BGM, BGA, opacity, invisible and mines.
        let mut covered = [false; 9];
        for bytes in cases("bms_parser") {
            assert!(bytes.len() <= BMS_MAX_BYTES);
            let text = std::str::from_utf8(&bytes).expect("BMS corpus is actual UTF-8 text");
            let Ok(parsed) = parse_seeded(text, parser_options(), random_seed) else {
                continue;
            };
            let Ok(compiled) = parsed.compile() else {
                continue;
            };
            let invisible = parsed.compile_invisible().unwrap();
            let mines = parsed.compile_mines().unwrap();
            let witnessed = [
                !compiled.chart.objects().is_empty(),
                compiled
                    .chart
                    .objects()
                    .iter()
                    .any(|o| o.time.end.is_some()),
                !compiled.chart.bpm_changes().is_empty(),
                !compiled.chart.stops().is_empty(),
                !compiled.bgm.is_empty(),
                !compiled.bga.is_empty(),
                !compiled.bga_opacity.is_empty(),
                !invisible.is_empty(),
                !mines.is_empty(),
            ];
            for (seen, witness) in covered.iter_mut().zip(witnessed) {
                *seen |= witness;
            }
            assert!(check_bms(&bytes));
        }
        assert_eq!(
            covered, [true; 9],
            "schedule family missing for seed {random_seed}"
        );
    }
}

#[test]
fn bms_corpus_contains_a_real_seed_zero_vs_seventy_three_conditional() {
    let mut different = false;
    for bytes in cases("bms_parser") {
        let text = std::str::from_utf8(&bytes).unwrap();
        if let (Ok(zero), Ok(other)) = (
            parse_seeded(text, parser_options(), 0),
            parse_seeded(text, parser_options(), 73),
        ) {
            // A parser accepting directives with identical output is no branch witness.
            different |= zero.source != other.source || zero.metadata != other.metadata;
        }
    }
    assert!(
        different,
        "conditional corpus never changes actual parsed output"
    );
}

#[test]
fn chart_corpus_has_all_eight_actual_compiler_mode_witnesses() {
    let mut modes = BTreeSet::new();
    let mut timed_object = false;
    for bytes in cases("chart_compiler") {
        assert!((18..=CHART_MAX_BYTES).contains(&bytes.len()));
        let mode = bytes[0] % 8;
        let source =
            chart_from_bytes(&bytes).expect("seed must reach compile, not just reject schema");
        let result = source.compile();
        let correct_branch = matches!(
            (&result, mode),
            (Ok(_), 0)
                | (Err(ChartError::Overflow), 1)
                | (Err(ChartError::NegativeStop { .. }), 2)
                | (Err(ChartError::DuplicateObjectId { .. }), 3)
                | (Err(ChartError::ReversedRange { .. }), 4)
                | (Err(ChartError::DuplicateBpm { .. }), 5)
                | (Err(ChartError::DuplicateStop { .. }), 6)
                | (Err(ChartError::InvalidResolution), 7)
        );
        assert!(correct_branch, "mode {mode}: {result:?}");
        assert_eq!(check_chart(&bytes), mode == 0);
        if let Ok(compiled) = result {
            timed_object |= !compiled.objects().is_empty();
            // Independently calculate the constant-tempo subdomain with i128.
            if source.bpm_changes.is_empty() && source.stops.is_empty() {
                for object in compiled.objects() {
                    let original = source.objects.iter().find(|o| o.id == object.id).unwrap();
                    let expected = i128::from(original.start.ticks())
                        * 60_000_000_000
                        * i128::from(source.initial_bpm.denominator())
                        / (i128::from(source.ticks_per_beat)
                            * i128::from(source.initial_bpm.numerator()));
                    assert_eq!(i128::from(object.time.start.as_nanos()), expected);
                    assert_eq!(object.metadata, original.metadata);
                }
            }
        }
        modes.insert(mode);
    }
    assert_eq!(modes, (0..8).collect());
    assert!(timed_object, "valid corpus never schedules an object");
}
