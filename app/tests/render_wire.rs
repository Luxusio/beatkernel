#![cfg(feature = "graphics")]
//! The byte boundary uses genuine chart, judge, completion and room owners.
pub use beatkernel_bms_runtime::{
    browser_render_state, competition, competition_presentation, gauge,
    historical_record_presentation, image_assets, image_decode, judgment_policy, local_players,
    multiplayer, multiplayer_group_rooms, multiplayer_protocol, multiplayer_rooms, play_result,
    player_chart, result_archive, room_presentation, room_results_builder, texture, timing, ui,
};
#[path = "../src/render_wire.rs"]
mod wire;

use beatkernel::{
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};
use browser_render_state::*;
use local_players::PlayerId;
use std::sync::Arc;
use wire::*;

fn limits() -> WireLimits {
    WireLimits {
        max_packet_bytes: 512 * 1024 * 1024,
        image: image_assets::ImageAssetLimits::default(),
        max_diagnostic_bytes: 1024 * 1024,
    }
}
fn header(kind: u16) -> WireHeader {
    WireHeader {
        kind,
        generation: u64::MAX,
        content: u64::MAX - 1,
        sequence: 1,
        payload_len: 0,
    }
}
fn chart_and_events() -> (
    Arc<player_chart::PlayerChart>,
    Vec<beatkernel::judge::JudgeEvent>,
) {
    use beatkernel::input::{
        ButtonEvent, ButtonState, DeviceId, EventMeta, GameInputEvent, PhysicalControlId,
        PhysicalInputEvent,
    };
    let source = beatkernel_bms::parse("#TITLE 별빛\n#ARTIST 作曲家\n#BPM 60\n#WAV01 key.wav\n#00011:010101\n#00152:0101\n#000D2:1E\n#00004:01\n#00007:02\n#0010B:80\n", Default::default()).unwrap();
    let compiled = source.compile().unwrap();
    let chart =
        Arc::new(player_chart::PlayerChart::from_compiled(&source, &compiled.chart).unwrap());
    let mut judge = JudgeEngine::new(
        compiled.chart,
        source.rules(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(u32::MAX),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    let hold = chart.notes.iter().find(|note| note.end.is_some()).unwrap();
    let mut events = judge.advance_to(hold.start).unwrap();
    let mut input = GameInputEvent {
        game_control: source
            .notes
            .iter()
            .find(|note| note.object == hold.object)
            .unwrap()
            .lane
            .control(),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(1), point(1, hold.start.as_nanos()), 1),
            control: PhysicalControlId::keyboard(4),
            state: ButtonState::Down,
        }),
    };
    events.extend(judge.push_input(&input, hold.start).unwrap());
    let end = hold.end.unwrap();
    let PhysicalInputEvent::Button(button) = &mut input.physical else {
        unreachable!()
    };
    button.state = ButtonState::Up;
    button.meta = EventMeta::new(DeviceId(1), point(1, end.as_nanos()), 2);
    events.extend(judge.push_input(&input, end).unwrap());
    assert!(events.iter().any(|event| matches!(
        event.outcome,
        beatkernel::judge::JudgeOutcome::Hit {
            grade: JudgeGrade(u32::MAX),
            ..
        }
    )));
    (chart, events)
}
fn images() -> image_assets::ImageAssets {
    use beatkernel_bms_runtime::asset_source::{MemoryAssetLimits, MemoryFiles};
    use image::ImageEncoder;
    let text = "#CANVASSIZE 2 1\n#BMP00 art.png\n#BMP01 ./art.png\n#BMP02 art.png\n#BGA03 01 0 0 1 1 1 0\n#BGA04 02 0 0 1 1 1 0\n#BMP05 absent.bmp\n#BMP08 damaged.png\n#00004:010203040508\n#00007:0103\n";
    let source = beatkernel_bms::parse(text, Default::default()).unwrap();
    let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    files
        .insert("song/chart.bms", text.as_bytes().to_vec())
        .unwrap();
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &[0, 0, 0, 255, 9, 8, 7, 128],
            2,
            1,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    files.insert("song/art.png", png).unwrap();
    files
        .insert("song/damaged.png", b"\x89PNG\r\n\x1a\n".to_vec())
        .unwrap();
    image_assets::ImageAssets::prepare_from_source(
        &files.scope("song/chart.bms").unwrap(),
        &source,
        image_assets::ImageAssetLimits::default(),
    )
    .unwrap()
}
fn live_frame(
    chart: &Arc<player_chart::PlayerChart>,
    events: &[beatkernel::judge::JudgeEvent],
) -> RenderFrame {
    use beatkernel_bms_runtime::note_progress::NoteProgress;
    let mut progress = NoteProgress::new(chart.clone()).unwrap();
    let baseline = progress.clone();
    progress.apply(events);
    let mut score = competition::ScoreSummary::default();
    score.observe(events).unwrap();
    let mut gauge = gauge::BmsGauge::default();
    gauge.observe(events, &[]).unwrap();
    let scalars = RenderMemberScalars {
        song_ns: i64::MIN,
        pressed: 1,
        recent: events.iter().map(RenderJudgeEvent::from_event).collect(),
        score: Some(RenderScore::from_summary(&score)),
        gauge: Some(RenderGauge::from_gauge(&gauge)),
        ..Default::default()
    };
    let updates = [PlayerId(7), PlayerId(u32::MAX)]
        .into_iter()
        .map(|id| {
            RenderMemberUpdate::from_progress(id, scalars.clone(), &progress, &baseline).unwrap()
        })
        .collect();
    RenderFrame {
        generation: u64::MAX,
        content: u64::MAX - 1,
        sequence: 1,
        page: 0,
        lookahead_ns: i64::MAX,
        members: updates,
        room: None,
        room_disabled: false,
    }
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
struct SameDomain;
impl beatkernel::time::ClockMapper for SameDomain {
    fn map(&self, point: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        (point.domain == target).then_some(point.timestamp)
    }
    fn quality(&self) -> beatkernel::time::ClockMappingQuality {
        beatkernel::time::ClockMappingQuality::Unknown
    }
}
fn completed() -> beatkernel_bms_runtime::step_gameplay::StepGameplay {
    use beatkernel::{
        audio::{
            AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank, command_queue,
        },
        input::BindingMap,
    };
    use beatkernel_bms_runtime::{
        PreparedBms,
        step_gameplay::{StepGameplay, StepGameplayConfig},
    };
    let source = beatkernel_bms::parse("#BPM 120\n", Default::default()).unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    let prepared = PreparedBms {
        compiled: source.compile().unwrap(),
        source,
        bank: SampleBank::new(format, PcmLimits::new(64, 256, 1).unwrap()).unwrap(),
        sounds: vec![],
        bgm_commands: vec![],
    };
    let config = StepGameplayConfig {
        host_origin: point(1, 0),
        output_origin: point(2, 0),
        preroll: Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 8,
        bgm_pending: 4,
        bgm_lookahead: Duration::from_nanos(1_000_000_000),
        telemetry_capacity: 8,
    };
    let (mut game, bank) = StepGameplay::new_section(
        prepared,
        config,
        BindingMap::from_bindings([]).unwrap(),
        Timestamp::ZERO,
        None,
    )
    .unwrap();
    game.activate(point(1, 0)).unwrap();
    game.advance_to(point(1, 1_000_000), &SameDomain, point(2, 0))
        .unwrap();
    let (_producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 16, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let first = mixer.render(&mut [0.0; 10]).unwrap();
    game.observe_completion(Some(first), Some(point(2, 10_000_000)))
        .unwrap();
    let second = mixer.render(&mut [0.0; 10]).unwrap();
    assert!(
        game.observe_completion(Some(second), Some(point(2, 20_000_000)))
            .unwrap()
    );
    game
}
fn history_and_results() -> (
    historical_record_presentation::FrozenHistoricalRecord,
    ui::results::FrozenResultsModel,
) {
    use beatkernel_bms_runtime::completed_results_presentation::CompletedResultsPresentation;
    let game = completed();
    let proof = *game.completed_result().unwrap();
    let mut results = CompletedResultsPresentation::default();
    assert!(results.capture_solo(&game, None).unwrap());
    let frozen = results.export_visual().unwrap().unwrap();
    // A real archive is encoded/decoded before historical projection.
    let mut options = b"bms-judge-profile/v1:".to_vec();
    options.extend_from_slice(&0i64.to_le_bytes());
    options.extend_from_slice(&1u64.to_le_bytes());
    options.extend_from_slice(&1u32.to_le_bytes());
    options.extend_from_slice(&0i64.to_le_bytes());
    options.extend_from_slice(&0i64.to_le_bytes());
    let identity = beatkernel::replay::ReplayHeader {
        version: 1,
        chart_identity: b"empty".to_vec(),
        rules_identity: b"bms".to_vec(),
        options,
        seed: 9,
        normalized_clock: ClockDomainId(7),
    };
    let archive = result_archive::ResultArchive::from_completed_with_scores(
        &[(PlayerId(1), proof)],
        &[(PlayerId(1), identity, game.gauge().profile().clone())],
        &[(PlayerId(1), game.score())],
    )
    .unwrap();
    let archive =
        result_archive::decode_archive(&result_archive::encode_archive(&archive).unwrap()).unwrap();
    let entry = &archive.entries()[0];
    let history =
        historical_record_presentation::HistoricalRecordPresentation::from_record_with_comparisons(
            (entry.player, entry.result),
            entry.score.as_ref(),
            Some(&None),
        )
        .unwrap()
        .export_visual();
    (history, frozen)
}
fn room() -> room_results_builder::FrozenRoomResults {
    use multiplayer_group_rooms::{GroupRoomPolicy, GroupRoomRegistry};
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 3, 8, 100).unwrap());
    let roster = [PlayerId(7), PlayerId(u32::MAX), PlayerId(9)];
    let ids: Vec<_> = (0..3)
        .map(|_| {
            registry
                .join("render", b"chart identity", &roster, 0)
                .unwrap()
                .id
        })
        .collect();
    registry.seal(ids[0], 1).unwrap();
    for id in &ids {
        registry.ready(*id, 2).unwrap();
    }
    let words: Vec<_> = registry
        .room("render")
        .unwrap()
        .members
        .iter()
        .flat_map(|member| {
            let mut words = vec![
                member.id.0 as u32,
                (member.id.0 >> 32) as u32,
                member.players.len() as u32,
            ];
            words.extend(member.players.iter().map(|player| player.0));
            words
        })
        .collect();
    let mut builder = room_results_builder::RoomResultsBuilder::new(ids[0], &words).unwrap();
    builder
        .freeze(1, true, Some("accepted prefix retained 가".into()), false)
        .unwrap();
    builder.export_visual().unwrap().unwrap()
}
fn packets() -> Vec<(WireHeader, WirePacket)> {
    let (chart, events) = chart_and_events();
    let (history, results) = history_and_results();
    let room = room();
    let mut frame = live_frame(&chart, &events);
    frame.room = Some(room.pages[0].clone());
    vec![
        (
            header(REGISTRATION),
            WirePacket::Registration(VisualRegistration {
                chart: chart.export_visual(),
                images: images().export_visual(),
                roster: vec![PlayerId(7), PlayerId(u32::MAX)],
            }),
        ),
        (header(FRAME), WirePacket::Frame(frame)),
        (
            header(PREVIEW),
            WirePacket::Preview(PreviewState {
                song_ns: 0,
                lookahead_ns: i64::MAX,
            }),
        ),
        (header(HISTORY), WirePacket::History(history)),
        (header(RESULTS), WirePacket::Results(results)),
        (header(ROOM), WirePacket::Room(room)),
    ]
}

#[test]
fn genuine_all_kind_roundtrip_and_receiver_local_image_aliases() {
    for (h, packet) in packets() {
        let encoded = encode_packet(h, &packet, limits()).unwrap();
        assert_eq!(&encoded[..4], b"BKRV");
        assert_eq!(u16::from_le_bytes(encoded[4..6].try_into().unwrap()), 1);
        let (actual, decoded) = decode_packet(&encoded, limits()).unwrap();
        assert_eq!(actual.kind, h.kind);
        assert_eq!(
            (actual.generation, actual.content, actual.sequence),
            (h.generation, h.content, h.sequence)
        );
        assert_eq!(actual.payload_len as usize, encoded.len() - 40);
        assert_eq!(encode_packet(actual, &decoded, limits()).unwrap(), encoded);
        match (packet, decoded) {
            (WirePacket::Registration(original), WirePacket::Registration(received)) => {
                let left = player_chart::PlayerChart::import_visual(original.chart).unwrap();
                let right = player_chart::PlayerChart::import_visual(received.chart).unwrap();
                assert_eq!(left.notes, right.notes);
                assert_eq!(left.mines(), right.mines());
                for ns in [i64::MIN, 0, 1_000_000_000, i64::MAX] {
                    assert_eq!(
                        left.bga_state(Timestamp::from_nanos(ns)),
                        right.bga_state(Timestamp::from_nanos(ns))
                    );
                    assert_eq!(
                        left.bga_opacity(Timestamp::from_nanos(ns)),
                        right.bga_opacity(Timestamp::from_nanos(ns))
                    );
                }
                for (old, new) in original
                    .images
                    .resources
                    .iter()
                    .zip(&received.images.resources)
                {
                    assert!(!Arc::ptr_eq(old, new));
                    assert_ne!(old.pixels().as_ptr(), new.pixels().as_ptr());
                    assert_eq!(old.pixels(), new.pixels());
                }
                let bank =
                    image_assets::ImageAssets::import_visual(received.images, limits().image)
                        .unwrap();
                assert!(Arc::ptr_eq(
                    bank.get(beatkernel_bms::ImageId(1)).unwrap(),
                    bank.get(beatkernel_bms::ImageId(2)).unwrap()
                ));
                assert!(Arc::ptr_eq(
                    bank.get(beatkernel_bms::ImageId(3)).unwrap(),
                    bank.get(beatkernel_bms::ImageId(4)).unwrap()
                ));
                assert!(bank.get_layer(beatkernel_bms::ImageId(1)).is_some());
            }
            (WirePacket::Frame(left), WirePacket::Frame(right)) => assert_eq!(left, right),
            (WirePacket::History(left), WirePacket::History(right)) => assert_eq!(left, right),
            (WirePacket::Results(left), WirePacket::Results(right)) => assert_eq!(left, right),
            (WirePacket::Room(left), WirePacket::Room(right)) => assert_eq!(left, right),
            (WirePacket::Preview(left), WirePacket::Preview(right)) => {
                assert_eq!(left.song_ns, right.song_ns);
                assert_eq!(left.lookahead_ns, right.lookahead_ns);
            }
            _ => panic!("wire kind changed"),
        }
    }
}

#[test]
fn every_kind_truncation_overflow_unknown_and_trailing_bytes_refuse() {
    for (h, packet) in packets() {
        let bytes = encode_packet(h, &packet, limits()).unwrap();
        for end in 0..bytes.len() {
            assert!(
                decode_packet(&bytes[..end], limits()).is_err(),
                "kind {} truncated {end}",
                h.kind
            );
        }
        for (offset, replacement) in [
            (0, vec![0]),
            (4, 2u16.to_le_bytes().to_vec()),
            (6, 99u16.to_le_bytes().to_vec()),
            (8, 0u64.to_le_bytes().to_vec()),
            (16, 0u64.to_le_bytes().to_vec()),
            (32, u64::MAX.to_le_bytes().to_vec()),
        ] {
            let mut bad = bytes.clone();
            bad[offset..offset + replacement.len()].copy_from_slice(&replacement);
            assert!(
                decode_packet(&bad, limits()).is_err(),
                "kind {} offset {offset}",
                h.kind
            );
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode_packet(&trailing, limits()).is_err());
        let mut wrong_kind = bytes.clone();
        wrong_kind[6..8]
            .copy_from_slice(&(if h.kind == PREVIEW { FRAME } else { PREVIEW }).to_le_bytes());
        assert!(decode_packet(&wrong_kind, limits()).is_err());
        let mut exact = limits();
        exact.max_packet_bytes = bytes.len();
        assert!(decode_packet(&bytes, exact).is_ok());
        assert!(encode_packet(h, &packet, exact).is_ok());
        exact.max_packet_bytes -= 1;
        assert!(decode_packet(&bytes, exact).is_err());
        assert!(encode_packet(h, &packet, exact).is_err());
        assert!(preflight_header(&bytes[..40], bytes.len(), limits()).is_ok());
        assert!(preflight_header(&bytes[..39], bytes.len(), limits()).is_err());
        if h.kind == FRAME {
            // The frame's first collection follows page, lookahead and boolean.
            let mut bad = bytes.clone();
            bad[HEADER_BYTES + 13..HEADER_BYTES + 17].copy_from_slice(&u32::MAX.to_le_bytes());
            assert!(
                decode_packet(&bad, limits()).is_err(),
                "overflowing collection extent"
            );
        }
    }
}

#[test]
fn empty_preview_has_no_fabricated_member_and_header_context_is_enforced() {
    let source = beatkernel_bms::parse("#TITLE empty\n#BPM 120\n", Default::default()).unwrap();
    let chart = player_chart::PlayerChart::from_compiled(&source, &source.compile().unwrap().chart)
        .unwrap();
    let packet = WirePacket::Registration(VisualRegistration {
        chart: chart.export_visual(),
        images: image_assets::ImageAssets::default().export_visual(),
        roster: vec![],
    });
    let encoded = encode_packet(header(REGISTRATION), &packet, limits()).unwrap();
    let (_, decoded) = decode_packet(&encoded, limits()).unwrap();
    let WirePacket::Registration(reg) = decoded else {
        panic!("registration expected")
    };
    assert!(reg.roster.is_empty());
    assert!(reg.chart.notes.is_empty());
    let (chart, events) = chart_and_events();
    let packet = WirePacket::Frame(live_frame(&chart, &events));
    for change in 0..4 {
        let mut h = header(FRAME);
        match change {
            0 => h.generation -= 1,
            1 => h.content -= 1,
            2 => h.sequence += 1,
            _ => h.kind = HISTORY,
        }
        assert!(encode_packet(h, &packet, limits()).is_err());
    }
}

#[test]
fn explicit_image_and_aggregate_diagnostic_budgets_are_exact() {
    let (chart, _) = chart_and_events();
    let bank = images();
    let mut images = bank.export_visual();
    // Distinct diagnostics retain their exact aggregate UTF8 byte admission.
    images.unavailable.push((
        beatkernel_bms::ImageId(12),
        image_assets::ImageUnavailable::InvalidData("가é".into()),
    ));
    let diagnostics: usize = images
        .unavailable
        .iter()
        .map(|(_, reason)| match reason {
            image_assets::ImageUnavailable::InvalidData(text) => text.len(),
            _ => 0,
        })
        .sum();
    assert!(diagnostics > 0);
    let packet = WirePacket::Registration(VisualRegistration {
        chart: chart.export_visual(),
        images,
        roster: vec![PlayerId(7)],
    });
    let mut exact = limits();
    exact.image.max_decoded_bytes = bank.decoded_bytes();
    exact.max_diagnostic_bytes = diagnostics;
    let bytes = encode_packet(header(REGISTRATION), &packet, exact).unwrap();
    assert!(decode_packet(&bytes, exact).is_ok());
    let mut too_small = exact;
    too_small.image.max_decoded_bytes -= 1;
    assert!(decode_packet(&bytes, too_small).is_err());
    assert!(encode_packet(header(REGISTRATION), &packet, too_small).is_err());
    too_small = exact;
    too_small.max_diagnostic_bytes -= 1;
    assert!(decode_packet(&bytes, too_small).is_err());
    assert!(encode_packet(header(REGISTRATION), &packet, too_small).is_err());
}

#[test]
fn decoded_whole_model_rejects_final_member_atomically_and_accepts_genuine_progress() {
    let (chart, events) = chart_and_events();
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    let mut state = BrowserRenderState::import_visual(
        u64::MAX,
        u64::MAX - 1,
        chart.export_visual(),
        images().export_visual(),
        ids.to_vec(),
    )
    .unwrap();
    let initial = live_frame(&chart, &events[..1]);
    let bytes = encode_packet(header(FRAME), &WirePacket::Frame(initial), limits()).unwrap();
    let (_, WirePacket::Frame(initial)) = decode_packet(&bytes, limits()).unwrap() else {
        panic!("frame")
    };
    state.apply_frame(&initial).unwrap();
    let before: Vec<_> = ids
        .iter()
        .map(|id| {
            let member = state.member(*id).unwrap();
            (
                member.scalars.clone(),
                (0..chart.notes.len())
                    .map(|n| member.progress.state(n))
                    .collect::<Vec<_>>(),
                member.progress.last_miss(),
            )
        })
        .collect();
    let mut final_frame = live_frame(&chart, &events);
    final_frame.sequence = 2;
    final_frame.members[0].scalars.song_ns = i64::MAX;
    let mut bad_frames = Vec::new();
    let mut foreign = final_frame.clone();
    foreign.members[1].player = PlayerId(17);
    bad_frames.push(foreign);
    let mut duplicate = final_frame.clone();
    duplicate.members[1].player = ids[0];
    let mut duplicate_header = header(FRAME);
    duplicate_header.sequence = 2;
    assert!(encode_packet(duplicate_header, &WirePacket::Frame(duplicate), limits()).is_err());
    let mut pressed = final_frame.clone();
    pressed.members[1].scalars.pressed = u32::MAX;
    bad_frames.push(pressed);
    let mut object = final_frame.clone();
    object.members[1].scalars.recent.last_mut().unwrap().object = u64::MAX;
    bad_frames.push(object);
    let mut shape = final_frame.clone();
    shape.members[1].scalars.recent.last_mut().unwrap().stage = 0;
    bad_frames.push(shape);
    let mut last_page = final_frame.clone();
    last_page.members[1].pages.last_mut().unwrap().valid_count = 4096;
    // Generic byte validity cannot know the exact receiver chart page extent.
    bad_frames.push(last_page);
    for (case, candidate) in bad_frames.into_iter().enumerate() {
        let mut h = header(FRAME);
        h.sequence = 2;
        let encoded = encode_packet(h, &WirePacket::Frame(candidate), limits()).unwrap();
        let (_, WirePacket::Frame(decoded)) = decode_packet(&encoded, limits()).unwrap() else {
            panic!("frame")
        };
        assert!(state.apply_frame(&decoded).is_err(), "case {case}");
        assert_eq!(state.sequence(), 1);
        assert_eq!(state.page(), 0);
        assert_eq!(state.lookahead_ns(), i64::MAX);
        assert!(state.room().is_none());
        for (id, retained) in ids.iter().zip(&before) {
            let member = state.member(*id).unwrap();
            assert_eq!(member.scalars, retained.0);
            assert_eq!(member.progress.last_miss(), retained.2);
            assert_eq!(
                (0..chart.notes.len())
                    .map(|n| member.progress.state(n))
                    .collect::<Vec<_>>(),
                retained.1
            );
        }
    }
    // A valid payload under a foreign envelope cannot change this registration.
    let mut h = header(FRAME);
    h.sequence = 2;
    let valid_bytes = encode_packet(h, &WirePacket::Frame(final_frame.clone()), limits()).unwrap();
    for (offset, identity) in [(8, 91u64), (16, 92u64), (24, 1u64)] {
        let mut foreign = valid_bytes.clone();
        foreign[offset..offset + 8].copy_from_slice(&identity.to_le_bytes());
        let (_, WirePacket::Frame(decoded)) = decode_packet(&foreign, limits()).unwrap() else {
            panic!("frame")
        };
        assert!(state.apply_frame(&decoded).is_err());
        assert_eq!(state.sequence(), 1);
        for (id, retained) in ids.iter().zip(&before) {
            assert_eq!(state.member(*id).unwrap().scalars, retained.0);
        }
    }
    let mut h = header(FRAME);
    h.sequence = 2;
    let encoded = encode_packet(h, &WirePacket::Frame(final_frame), limits()).unwrap();
    let (_, WirePacket::Frame(decoded)) = decode_packet(&encoded, limits()).unwrap() else {
        panic!("frame")
    };
    state.apply_frame(&decoded).unwrap();
    assert_eq!(state.sequence(), 2);
    for id in ids {
        let member = state.member(id).unwrap();
        assert!(member.progress.all_completed(0, chart.notes.len()));
        assert!(
            member
                .scalars
                .as_ref()
                .unwrap()
                .recent
                .iter()
                .any(|event| event.grade_or_reason == u32::MAX)
        );
    }
}

#[test]
fn supported_wide_timing_counts_and_opaque_grade_domain_roundtrip_without_narrowing() {
    let (chart, events) = chart_and_events();
    for delta in [i64::MIN, i64::MAX] {
        let mut frame = live_frame(&chart, &events);
        for member in &mut frame.members {
            let event = member.scalars.recent.last_mut().unwrap();
            event.stage = 3;
            event.custom_stage = u32::MAX;
            event.outcome = 0;
            event.grade_or_reason = u32::MAX;
            event.delta_ns = delta;
            event.at_ns = delta;
            member.last_miss_ns = Some(delta);
        }
        let bytes =
            encode_packet(header(FRAME), &WirePacket::Frame(frame.clone()), limits()).unwrap();
        let (_, WirePacket::Frame(decoded)) = decode_packet(&bytes, limits()).unwrap() else {
            panic!("frame")
        };
        assert_eq!(decoded, frame);
        let mut receiver = BrowserRenderState::import_visual(
            u64::MAX,
            u64::MAX - 1,
            chart.export_visual(),
            images().export_visual(),
            vec![PlayerId(7), PlayerId(u32::MAX)],
        )
        .unwrap();
        receiver.apply_frame(&decoded).unwrap();
        assert_eq!(
            receiver.member(PlayerId(7)).unwrap().progress.last_miss(),
            Some(Timestamp::from_nanos(delta))
        );
    }
    let (mut history, mut results) = history_and_results();
    for delta in [i64::MIN, i64::MAX] {
        let count = u64::MAX;
        let negative = delta < 0;
        let record = timing::TimingRecord {
            count,
            early: if negative { count } else { 0 },
            late: if negative { 0 } else { count },
            exact: 0,
            sum: i128::from(delta) * i128::from(count),
            absolute_sum: u128::from(delta.unsigned_abs()) * u128::from(count),
            last: Some(delta),
            min: Some(delta),
            max: Some(delta),
        };
        record.validate().unwrap();
        let score = result_archive::ArchivedScore {
            hits: count,
            misses: 0,
            combo: count,
            max_combo: count,
            grades: vec![(u32::MAX, count)],
            timing: record,
        };
        score.validate().unwrap();
        history.score = Some(score.clone());
        results.details[0].score = score;
        for (kind, packet) in [
            (HISTORY, WirePacket::History(history.clone())),
            (RESULTS, WirePacket::Results(results.clone())),
        ] {
            let encoded = encode_packet(header(kind), &packet, limits()).unwrap();
            let (_, decoded) = decode_packet(&encoded, limits()).unwrap();
            match decoded {
                WirePacket::History(model) => assert_eq!(model, history),
                WirePacket::Results(model) => assert_eq!(model, results),
                _ => panic!("frozen kind"),
            }
        }
    }
    let mut grades: Vec<_> = (0..result_archive::MAX_SCORE_GRADES - 1)
        .map(|n| (n as u32, 1))
        .collect();
    grades.push((u32::MAX, 1));
    let score = result_archive::ArchivedScore {
        hits: grades.len() as u64,
        misses: u64::MAX,
        combo: 0,
        max_combo: 0,
        grades,
        timing: Default::default(),
    };
    score.validate().unwrap();
    history.score = Some(score.clone());
    history.grade_page = result_archive::MAX_SCORE_GRADES.div_ceil(4) - 1;
    let encoded = encode_packet(
        header(HISTORY),
        &WirePacket::History(history.clone()),
        limits(),
    )
    .unwrap();
    let (_, WirePacket::History(decoded)) = decode_packet(&encoded, limits()).unwrap() else {
        panic!("history")
    };
    assert_eq!(decoded, history);
    assert_eq!(decoded.score.unwrap().grades.last(), Some(&(u32::MAX, 1)));
}

#[test]
fn invalid_utf8_final_fields_and_optional_tags_refuse_before_publication() {
    for (h, packet) in packets() {
        let bytes = encode_packet(h, &packet, limits()).unwrap();
        if h.kind == REGISTRATION {
            let position = bytes
                .windows("별빛".len())
                .position(|chunk| chunk == "별빛".as_bytes())
                .unwrap();
            let mut bad = bytes.clone();
            bad[position] = 0xff;
            assert!(decode_packet(&bad, limits()).is_err());
        }
        if h.kind == ROOM {
            let mut bad = bytes.clone();
            *bad.last_mut().unwrap() = 0xff;
            assert!(
                decode_packet(&bad, limits()).is_err(),
                "final room field UTF8"
            );
        }
        if h.kind == FRAME {
            // Frame begins page:u32, lookahead:i64, room_disabled:u8.
            let mut bad = bytes.clone();
            bad[HEADER_BYTES + 4 + 8] = 2;
            assert!(
                decode_packet(&bad, limits()).is_err(),
                "reserved frame boolean"
            );
        }
    }
    let (history, mut results) = history_and_results();
    results.details[0].score.max_combo = 1;
    assert!(encode_packet(header(RESULTS), &WirePacket::Results(results), limits()).is_err());
    let mut history = history;
    history.grade_page = usize::MAX;
    assert!(encode_packet(header(HISTORY), &WirePacket::History(history), limits()).is_err());
    let mut room = room();
    room.pages
        .last_mut()
        .unwrap()
        .rows
        .last_mut()
        .unwrap()
        .player = PlayerId(0);
    assert!(encode_packet(header(ROOM), &WirePacket::Room(room), limits()).is_err());
}

#[test]
fn cancelled_room_diagnostics_count_every_utf8_page_before_replacement() {
    let cancelled = room();
    assert!(cancelled.pages.len() > 1);
    assert!(cancelled.pages.iter().all(|page| {
        page.status == room_presentation::RoomStatus::Closed
            && !page.failed
            && page.heading.contains("CANCELLED")
            && page.error.as_deref() == Some("accepted prefix retained 가")
            && !page.rows.is_empty()
    }));
    let diagnostic_bytes: usize = cancelled
        .pages
        .iter()
        .map(|page| page.error.as_ref().unwrap().len())
        .sum();
    let first_error = cancelled.pages[0].error.as_ref().unwrap();
    assert!(first_error.len() > first_error.chars().count());
    assert!(diagnostic_bytes > first_error.len());
    let packet = WirePacket::Room(cancelled.clone());
    // Hostile receiving allowances must be tested against an otherwise valid packet.
    let encoded = encode_packet(header(ROOM), &packet, limits()).unwrap();
    let mut exact = limits();
    exact.max_diagnostic_bytes = diagnostic_bytes;
    assert!(encode_packet(header(ROOM), &packet, exact).is_ok());
    let (_, WirePacket::Room(decoded)) = decode_packet(&encoded, exact).unwrap() else {
        panic!("room packet")
    };
    assert_eq!(decoded, cancelled);
    for allowance in [0, first_error.len(), diagnostic_bytes - 1] {
        let mut insufficient = exact;
        insufficient.max_diagnostic_bytes = allowance;
        assert!(encode_packet(header(ROOM), &packet, insufficient).is_err());
        assert!(decode_packet(&encoded, insufficient).is_err());
    }

    // Headings, identities and counters remain display text in a cancelled room.
    let mut without_errors = cancelled.clone();
    for page in &mut without_errors.pages {
        page.error = None;
    }
    without_errors.validate().unwrap();
    assert!(
        without_errors
            .pages
            .iter()
            .all(|page| !page.heading.is_empty())
    );
    assert!(
        without_errors
            .pages
            .iter()
            .flat_map(|page| &page.rows)
            .all(|row| !row.label.is_empty())
    );
    let mut zero = limits();
    zero.max_diagnostic_bytes = 0;
    let bytes = encode_packet(
        header(ROOM),
        &WirePacket::Room(without_errors.clone()),
        zero,
    )
    .unwrap();
    let (_, WirePacket::Room(decoded)) = decode_packet(&bytes, zero).unwrap() else {
        panic!("room packet")
    };
    assert_eq!(decoded, without_errors);

    // Promotion of a fully decoded genuine model occurs only after admission.
    let mut retained = without_errors.clone();
    for allowance in [0, diagnostic_bytes - 1] {
        let mut insufficient = limits();
        insufficient.max_diagnostic_bytes = allowance;
        let admission = (|| -> Result<(), String> {
            let (_, WirePacket::Room(candidate)) = decode_packet(&encoded, insufficient)? else {
                return Err("room packet expected".into());
            };
            candidate.validate()?;
            retained = candidate;
            Ok(())
        })();
        assert!(admission.is_err());
        assert_eq!(retained, without_errors);
    }
}

#[test]
fn frame_room_error_budget_refuses_before_atomic_committed_state_changes() {
    let (chart, events) = chart_and_events();
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    let mut state = BrowserRenderState::import_visual(
        u64::MAX,
        u64::MAX - 1,
        chart.export_visual(),
        images().export_visual(),
        ids.to_vec(),
    )
    .unwrap();
    let initial = live_frame(&chart, &events[..1]);
    let initial_bytes =
        encode_packet(header(FRAME), &WirePacket::Frame(initial), limits()).unwrap();
    let (_, WirePacket::Frame(initial)) = decode_packet(&initial_bytes, limits()).unwrap() else {
        panic!("frame packet")
    };
    state.apply_frame(&initial).unwrap();
    let retained_chart = state.chart().clone();
    let retained_images = state.images().clone();
    let retained: Vec<_> = ids
        .iter()
        .map(|id| {
            let member = state.member(*id).unwrap();
            (
                member.scalars.clone(),
                member.progress.last_miss(),
                (0..chart.notes.len())
                    .map(|n| member.progress.state(n))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let mut candidate = live_frame(&chart, &events);
    candidate.sequence = 2;
    candidate.members[0].scalars.song_ns = i64::MAX;
    candidate.room = Some(room().pages[0].clone());
    let error = candidate.room.as_ref().unwrap().error.as_ref().unwrap();
    let diagnostic_bytes = error.len();
    assert!(diagnostic_bytes > error.chars().count());
    let mut h = header(FRAME);
    h.sequence = 2;
    let encoded = encode_packet(h, &WirePacket::Frame(candidate.clone()), limits()).unwrap();
    let mut exact = limits();
    exact.max_diagnostic_bytes = diagnostic_bytes;
    assert!(encode_packet(h, &WirePacket::Frame(candidate.clone()), exact).is_ok());
    let (_, WirePacket::Frame(decoded)) = decode_packet(&encoded, exact).unwrap() else {
        panic!("frame packet")
    };
    assert_eq!(decoded, candidate);

    for allowance in [0, diagnostic_bytes - 1] {
        let mut insufficient = limits();
        insufficient.max_diagnostic_bytes = allowance;
        assert!(encode_packet(h, &WirePacket::Frame(candidate.clone()), insufficient).is_err());
        assert!(decode_packet(&encoded, insufficient).is_err());
        let applied = (|| -> Result<(), String> {
            let (_, WirePacket::Frame(frame)) = decode_packet(&encoded, insufficient)? else {
                return Err("frame packet expected".into());
            };
            state.apply_frame(&frame)
        })();
        assert!(applied.is_err());
        assert_eq!(state.sequence(), 1);
        assert_eq!(state.page(), 0);
        assert_eq!(state.lookahead_ns(), i64::MAX);
        assert!(state.room().is_none());
        assert!(!state.room_disabled());
        assert!(Arc::ptr_eq(state.chart(), &retained_chart));
        assert!(Arc::ptr_eq(state.images(), &retained_images));
        for (id, before) in ids.iter().zip(&retained) {
            let member = state.member(*id).unwrap();
            assert_eq!(member.scalars, before.0);
            assert_eq!(member.progress.last_miss(), before.1);
            assert_eq!(
                (0..chart.notes.len())
                    .map(|n| member.progress.state(n))
                    .collect::<Vec<_>>(),
                before.2
            );
        }
    }
    // Removing just diagnostics does not charge ordinary room display labels.
    let mut no_error = candidate.clone();
    no_error.room.as_mut().unwrap().error = None;
    let mut zero = limits();
    zero.max_diagnostic_bytes = 0;
    let bytes = encode_packet(h, &WirePacket::Frame(no_error.clone()), zero).unwrap();
    let (_, WirePacket::Frame(decoded_no_error)) = decode_packet(&bytes, zero).unwrap() else {
        panic!("frame packet")
    };
    assert_eq!(decoded_no_error, no_error);
    state.apply_frame(&decoded).unwrap();
    assert_eq!(state.sequence(), 2);
    assert_eq!(state.room(), candidate.room.as_ref());
    assert!(
        state
            .member(ids[0])
            .unwrap()
            .progress
            .all_completed(0, chart.notes.len())
    );
}
