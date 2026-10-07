//! Portable visual transfer uses compiled identities and committed judge output.
use crate::browser_render_state::*;
use crate::{competition::ScoreSummary, gauge::BmsGauge, note_progress::NoteState};
use beatkernel::{
    input::{
        ButtonEvent, ButtonState, DeviceId, EventMeta, GameInputEvent, PhysicalControlId,
        PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};
use std::sync::Arc;

struct JudgedChart {
    chart: Arc<crate::player_chart::PlayerChart>,
    events: Vec<beatkernel::judge::JudgeEvent>,
}

fn judged_chart(instants: usize) -> JudgedChart {
    let text = format!(
        "#BPM 60\n#WAV01 key.wav\n#00011:{}\n#00152:0101\n",
        "01".repeat(instants)
    );
    let source = beatkernel_bms::parse(&text, Default::default()).unwrap();
    let compiled = source.compile().unwrap();
    let chart = Arc::new(
        crate::player_chart::PlayerChart::from_compiled(&source, &compiled.chart).unwrap(),
    );
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
    let hold = chart.notes.last().unwrap();
    let mut events = judge.advance_to(hold.start).unwrap();
    assert_eq!(events.len(), instants);
    let mut key = GameInputEvent {
        game_control: source
            .notes
            .iter()
            .find(|note| note.object == hold.object)
            .unwrap()
            .lane
            .control(),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(
                DeviceId(1),
                ClockPoint {
                    domain: ClockDomainId(1),
                    timestamp: hold.start,
                },
                1,
            ),
            control: PhysicalControlId::keyboard(4),
            state: ButtonState::Down,
        }),
    };
    let heads = judge.push_input(&key, hold.start).unwrap();
    assert_eq!(heads.len(), 1);
    events.extend(heads);
    let end = hold.end.unwrap();
    let PhysicalInputEvent::Button(button) = &mut key.physical else {
        unreachable!()
    };
    button.state = ButtonState::Up;
    button.meta = EventMeta::new(
        DeviceId(1),
        ClockPoint {
            domain: ClockDomainId(1),
            timestamp: end,
        },
        2,
    );
    let tails = judge.push_input(&key, end).unwrap();
    assert_eq!(tails.len(), 1);
    events.extend(tails);
    JudgedChart { chart, events }
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

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}

// Completion is obtained through genuine output reports, never numeric gauge inference.
fn completed_game() -> crate::step_gameplay::StepGameplay {
    use beatkernel::{
        audio::{
            AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank, command_queue,
        },
        input::BindingMap,
    };
    let source = beatkernel_bms::parse("#BPM 120\n", Default::default()).unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    let prepared = crate::PreparedBms {
        compiled: source.compile().unwrap(),
        source,
        bank: SampleBank::new(format, PcmLimits::new(64, 256, 1).unwrap()).unwrap(),
        sounds: vec![],
        bgm_commands: vec![],
    };
    let config = crate::step_gameplay::StepGameplayConfig {
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
    let (mut game, bank) = crate::step_gameplay::StepGameplay::new_section(
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

fn scalars(events: &[beatkernel::judge::JudgeEvent], song_ns: i64) -> RenderMemberScalars {
    let mut score = ScoreSummary::default();
    score.observe(events).unwrap();
    let mut gauge = BmsGauge::default();
    gauge.observe(events, &[]).unwrap();
    RenderMemberScalars {
        song_ns,
        pressed: 3,
        recent: events
            .iter()
            .rev()
            .take(128)
            .rev()
            .map(RenderJudgeEvent::from_event)
            .collect(),
        score: Some(RenderScore::from_summary(&score)),
        gauge: Some(RenderGauge::from_gauge(&gauge)),
        competition: None,
        saved_comparison_height: 0,
        peer_admitted: false,
        saved_failed: false,
        peer_failed: false,
    }
}

fn receiver(f: &JudgedChart, roster: &[crate::local_players::PlayerId]) -> BrowserRenderState {
    BrowserRenderState::import_visual(
        7,
        19,
        f.chart.export_visual(),
        crate::image_assets::ImageAssets::default().export_visual(),
        roster.to_vec(),
    )
    .unwrap()
}

fn frame(page: u32, sequence: u64, members: Vec<RenderMemberUpdate>) -> RenderFrame {
    RenderFrame {
        generation: 7,
        content: 19,
        sequence,
        page,
        lookahead_ns: 1_000_000_000,
        members,
        room: None,
        room_disabled: false,
    }
}

#[test]
fn reconstructed_common_visual_state_preserves_committed_values_and_opaque_grades() {
    let f = judged_chart(33);
    let id = crate::local_players::PlayerId(u32::MAX);
    let mut producer = crate::note_progress::NoteProgress::new(f.chart.clone()).unwrap();
    let pending = producer.clone();
    producer.apply(&f.events[..34]);
    let mut value = scalars(&f.events[..34], 604_800_000_000_001);
    value.competition = Some(comparison());
    value.saved_comparison_height = 14;
    let update = RenderMemberUpdate::from_progress(id, value.clone(), &producer, &pending).unwrap();
    {
        let mut render = receiver(&f, &[id]);
        assert!(!Arc::ptr_eq(render.chart(), &f.chart));
        render
            .apply_frame(&frame(0, 1, vec![update.clone()]))
            .unwrap();
        let member = render.member(id).unwrap();
        let actual = member.scalars.as_ref().unwrap();
        assert_eq!(actual.song_ns, 604_800_000_000_001);
        assert_eq!(actual.pressed, 3);
        assert_eq!(actual.score, value.score);
        assert_eq!(actual.score.unwrap().hits, 1);
        assert_eq!(actual.score.unwrap().misses, 33);
        assert_eq!(actual.score.unwrap().combo, 1);
        assert_eq!(actual.score.unwrap().max_combo, 1);
        assert_eq!(actual.score.unwrap().timing.count, 1);
        assert_eq!(actual.score.unwrap().timing.exact, 1);
        assert_eq!(actual.gauge, value.gauge);
        assert_eq!(actual.competition, value.competition);
        assert_eq!(actual.saved_comparison_height, 14);
        assert_eq!(actual.recent.len(), 34);
        for (wire, event) in actual.recent.iter().zip(&f.events) {
            let restored = wire.event();
            assert_eq!(restored.object, event.object);
            assert_eq!(restored.stage, event.stage);
            assert_eq!(restored.outcome, event.outcome);
            assert_eq!(restored.at, event.at);
            assert!(restored.input.is_none());
        }
        assert!(matches!(
            actual.recent.last().unwrap().event().outcome,
            beatkernel::judge::JudgeOutcome::Hit {
                grade: JudgeGrade(u32::MAX),
                ..
            }
        ));
        assert_eq!(member.progress.last_miss(), producer.last_miss());
        for index in 0..34 {
            assert_eq!(member.progress.state(index), producer.state(index));
        }
        assert_eq!(member.progress.state(33), Some(NoteState::Holding));
    }
}

fn assert_retained(
    render: &BrowserRenderState,
    roster: &[crate::local_players::PlayerId],
    expected: &[(
        Option<RenderMemberScalars>,
        Vec<Option<NoteState>>,
        Option<Timestamp>,
    )],
    sequence: u64,
    page: u32,
) {
    assert_eq!(render.sequence(), sequence);
    assert_eq!(render.page(), page);
    for (id, (scalars, states, miss)) in roster.iter().zip(expected) {
        let actual = render.member(*id).unwrap();
        assert_eq!(&actual.scalars, scalars);
        assert_eq!(actual.progress.last_miss(), *miss);
        for (index, state) in states.iter().enumerate() {
            assert_eq!(actual.progress.state(index), *state);
        }
    }
}

#[test]
fn bad_final_member_page_or_scalar_keeps_every_prior_player_and_frame_unchanged() {
    let f = judged_chart(8193);
    let ids = [
        crate::local_players::PlayerId(91),
        crate::local_players::PlayerId(u32::MAX),
    ];
    let mut producer = crate::note_progress::NoteProgress::new(f.chart.clone()).unwrap();
    let pending = producer.clone();
    producer.apply(&f.events[..1]);
    let initial = ids
        .iter()
        .map(|id| {
            RenderMemberUpdate::from_progress(*id, scalars(&f.events[..1], -9), &producer, &pending)
                .unwrap()
        })
        .collect();
    let mut render = receiver(&f, &ids);
    render.apply_frame(&frame(0, 1, initial)).unwrap();
    let retained: Vec<_> = ids
        .iter()
        .map(|id| {
            let member = render.member(*id).unwrap();
            (
                member.scalars.clone(),
                (0..8194).map(|i| member.progress.state(i)).collect(),
                member.progress.last_miss(),
            )
        })
        .collect();
    // An unacknowledged middle update must still be included against the old baseline.
    let acknowledged = producer.clone();
    producer.apply(&f.events[4096..4097]);
    producer.apply(&f.events[1..8194]);
    let updates: Vec<_> = ids
        .iter()
        .map(|id| {
            RenderMemberUpdate::from_progress(
                *id,
                scalars(&f.events[..8194], i64::MAX),
                &producer,
                &acknowledged,
            )
            .unwrap()
        })
        .collect();
    assert_eq!(
        updates[0]
            .pages
            .iter()
            .map(|page| page.index)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    let valid = frame(0, 2, updates);
    let mut malformed = Vec::new();
    let mut unknown = valid.clone();
    unknown.members.last_mut().unwrap().player = crate::local_players::PlayerId(17);
    malformed.push(unknown);
    let mut duplicate = valid.clone();
    duplicate.members.last_mut().unwrap().player = ids[0];
    malformed.push(duplicate);
    let mut bad_page = valid.clone();
    bad_page
        .members
        .last_mut()
        .unwrap()
        .pages
        .last_mut()
        .unwrap()
        .packed_states[127] = 1;
    malformed.push(bad_page);
    let mut wrong_page = valid.clone();
    wrong_page
        .members
        .last_mut()
        .unwrap()
        .pages
        .last_mut()
        .unwrap()
        .index = 3;
    malformed.push(wrong_page);
    let mut wrong_extent = valid.clone();
    wrong_extent
        .members
        .last_mut()
        .unwrap()
        .pages
        .last_mut()
        .unwrap()
        .packed_states
        .push(0);
    malformed.push(wrong_extent);
    let mut duplicate_page = valid.clone();
    let duplicate = duplicate_page.members[1].pages[0].clone();
    duplicate_page.members[1].pages.insert(1, duplicate);
    malformed.push(duplicate_page);
    let mut bad_scalar = valid.clone();
    bad_scalar
        .members
        .last_mut()
        .unwrap()
        .scalars
        .score
        .as_mut()
        .unwrap()
        .combo = u64::MAX;
    malformed.push(bad_scalar);
    let mut bad_gauge = valid.clone();
    bad_gauge
        .members
        .last_mut()
        .unwrap()
        .scalars
        .gauge
        .as_mut()
        .unwrap()
        .snapshot
        .level_units = crate::gauge::MAX_GAUGE_UNITS + 1;
    malformed.push(bad_gauge);
    let mut bad_content = valid.clone();
    bad_content.content += 1;
    malformed.push(bad_content);
    let mut bad_generation = valid.clone();
    bad_generation.generation += 1;
    malformed.push(bad_generation);
    let mut bad_visible_page = valid.clone();
    bad_visible_page.page = 1;
    malformed.push(bad_visible_page);
    let mut bad_lookahead = valid.clone();
    bad_lookahead.lookahead_ns = 0;
    malformed.push(bad_lookahead);
    let mut wrong_pressed = valid.clone();
    wrong_pressed.members.last_mut().unwrap().scalars.pressed = u32::MAX;
    malformed.push(wrong_pressed);
    let mut oversized_recent = valid.clone();
    oversized_recent
        .members
        .last_mut()
        .unwrap()
        .scalars
        .recent
        .resize(129, valid.members[0].scalars.recent[0].clone());
    malformed.push(oversized_recent);
    for (case, rejected) in malformed.iter().enumerate() {
        assert!(render.apply_frame(rejected).is_err(), "case {case}");
        assert_retained(&render, &ids, &retained, 1, 0);
        assert_eq!(render.lookahead_ns(), 1_000_000_000);
        assert!(render.room().is_none());
        assert!(!render.room_disabled());
    }
    render.apply_frame(&valid).unwrap();
    for id in ids {
        let actual = render.member(id).unwrap();
        assert_eq!(actual.scalars.as_ref().unwrap().song_ns, i64::MAX);
        assert_eq!(actual.progress.state(4096), Some(NoteState::Completed));
        assert_eq!(actual.progress.state(8193), Some(NoteState::Holding));
        for index in 0..8194 {
            assert_eq!(actual.progress.state(index), producer.state(index));
        }
    }
}

#[test]
fn maximum_actual_roster_has_exact_four_member_pages_and_rejects_bad_bounds() {
    let f = judged_chart(1);
    let ids: Vec<_> = (0..64)
        .map(|n| crate::local_players::PlayerId(u32::MAX - n))
        .collect();
    let mut render = receiver(&f, &ids);
    assert_eq!(render.roster(), ids);
    let producer = crate::note_progress::NoteProgress::new(f.chart.clone()).unwrap();
    for (page, visible) in ids.chunks(4).enumerate() {
        let updates = visible
            .iter()
            .map(|id| {
                RenderMemberUpdate::from_progress(
                    *id,
                    scalars(&[], -(page as i64)),
                    &producer,
                    &producer,
                )
                .unwrap()
            })
            .collect();
        let update = frame(page as u32, page as u64 + 1, updates);
        assert!(update.encoded_bytes().unwrap() <= MAX_RENDER_FRAME_BYTES);
        render.apply_frame(&update).unwrap();
        for id in visible {
            assert_eq!(
                render
                    .member(*id)
                    .unwrap()
                    .scalars
                    .as_ref()
                    .unwrap()
                    .song_ns,
                -(page as i64)
            );
        }
    }
    let invalid_rosters = [
        Vec::new(),
        vec![crate::local_players::PlayerId(0)],
        vec![ids[0], ids[0]],
        (1..=65).map(crate::local_players::PlayerId).collect(),
    ];
    for roster in invalid_rosters {
        assert!(
            BrowserRenderState::new(
                7,
                19,
                f.chart.clone(),
                Arc::new(crate::image_assets::ImageAssets::default()),
                roster
            )
            .is_err()
        );
    }
    assert!(render.apply_frame(&frame(16, 17, vec![])).is_err());
    assert_eq!(render.sequence(), 16);
    assert_eq!(render.page(), 15);
    let members = ids[..5]
        .iter()
        .map(|id| {
            RenderMemberUpdate::from_progress(*id, scalars(&[], 0), &producer, &producer).unwrap()
        })
        .collect();
    assert!(frame(0, 17, members).encoded_bytes().is_err());
    assert!(
        BrowserRenderState::peak_progress_bytes(beatkernel::chart::MAX_SOURCE_ITEMS, 64).is_ok()
    );
    assert!(
        BrowserRenderState::peak_progress_bytes(beatkernel::chart::MAX_SOURCE_ITEMS + 1, 64)
            .is_err()
    );
    assert!(BrowserRenderState::peak_progress_bytes(1, 65).is_err());
}

// Histories come from the real judge above; every retained member has admitted
// scalars, a holding long note, and its own immutable comparison reservation.
fn populated_local_render_state(count: usize) -> (BrowserRenderState, JudgedChart) {
    let f = judged_chart(129);
    let ids: Vec<_> = (0..count)
        .map(|index| crate::local_players::PlayerId(u32::MAX - index as u32))
        .collect();
    let mut state = receiver(&f, &ids);
    let pending = crate::note_progress::NoteProgress::new(f.chart.clone()).unwrap();
    let mut producer = pending.clone();
    producer.apply(&f.events[..130]);
    let song_ns = f.chart.notes.last().unwrap().start.as_nanos();
    for (page, visible) in ids.chunks(MAX_RENDER_VISIBLE).enumerate() {
        let members = visible
            .iter()
            .enumerate()
            .map(|(slot, id)| {
                let index = page * MAX_RENDER_VISIBLE + slot;
                let mut scalar = scalars(&f.events[..130], song_ns);
                scalar.recent.drain(..index % 3);
                scalar.competition = Some(comparison());
                scalar.saved_comparison_height = if index % 2 == 0 { 14 } else { 28 };
                scalar.peer_admitted = index % 2 != 0;
                RenderMemberUpdate::from_progress(*id, scalar, &producer, &pending).unwrap()
            })
            .collect();
        state
            .apply_frame(&frame(page as u32, page as u64 + 1, members))
            .unwrap();
    }
    (state, f)
}

fn select_retained_render_page(state: &mut BrowserRenderState, page: u32) {
    let start = page as usize * MAX_RENDER_VISIBLE;
    let end = (start + MAX_RENDER_VISIBLE).min(state.roster().len());
    let members = state.roster()[start..end]
        .iter()
        .map(|id| {
            let member = state.member(*id).unwrap();
            RenderMemberUpdate::from_progress(
                *id,
                member.scalars.clone().unwrap(),
                &member.progress,
                &member.progress,
            )
            .unwrap()
        })
        .collect();
    state.apply_frame(&frame(page, state.sequence() + 1, members)).unwrap();
}

// Independent baseline keeps the original full-roster adapter. It deliberately
// converts all retained histories so a visible-only bridge cannot hide a page,
// roster, reservation or feedback mapping error by sharing its selection code.
fn paint_full_roster_baseline(state: &BrowserRenderState) -> crate::scene::Scene {
    use crate::ui::organisms::VisualLocalPlayerView;
    let recent: Vec<Vec<_>> = state.roster()
        .iter()
        .map(|id| state.member(*id).unwrap().scalars.as_ref().unwrap().recent
            .iter().map(RenderJudgeEvent::event).collect())
        .collect();
    let views: Vec<_> = state.roster()
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let member = state.member(*id).unwrap();
            let scalar = member.scalars.as_ref().unwrap();
            VisualLocalPlayerView {
                player: *id,
                chart: Some(state.chart()),
                song_time: Some(Timestamp::from_nanos(scalar.song_ns)),
                score: scalar.score.unwrap(),
                bms_score: None,
                gauge: scalar.gauge,
                last_judge: recent[index].last(),
                recent_results: &recent[index],
                pressed_lanes: scalar.pressed,
                note_progress: Some(&member.progress),
                competition: scalar.competition.as_ref(),
            }
        })
        .collect();
    let reservations: Vec<_> = state.roster()
        .iter()
        .map(|id| {
            let scalar = state.member(*id).unwrap().scalars.as_ref().unwrap();
            i64::from(scalar.saved_comparison_height) + if scalar.peer_admitted { 28 } else { 0 }
        })
        .collect();
    let mut scene = crate::scene::Scene::new(960, 720);
    crate::ui::organisms::visual_local_player_views(
        &mut scene, &views, state.lookahead_ns(), state.page() as usize,
        true, &[crate::bga_render::BgaFrame::default(); 4], Some(&reservations),
    ).unwrap();
    scene
}

fn assert_same_local_scene(actual: &crate::scene::Scene, expected: &crate::scene::Scene) {
    assert_eq!(
        bytemuck::cast_slice::<_, u8>(actual.rectangles()),
        bytemuck::cast_slice::<_, u8>(expected.rectangles()),
    );
    let batches = |scene: &crate::scene::Scene| scene.batches()
        .iter().map(|batch| (batch.texture, batch.first, batch.count, batch.playfield))
        .collect::<Vec<_>>();
    assert_eq!(batches(actual), batches(expected));
    assert_eq!(actual.playfields().len(), expected.playfields().len());
    for (actual, expected) in actual.playfields().iter().zip(expected.playfields()) {
        assert_eq!(bytemuck::cast_slice::<_, u8>(&actual.instances),
            bytemuck::cast_slice::<_, u8>(&expected.instances));
        assert_eq!((actual.drift, actual.top, actual.bottom),
            (expected.drift, expected.top, expected.bottom));
    }
}

#[test]
fn borrowed_local_render_bridge_preserves_full_roster_pages_and_visible_only_histories() {
    let (mut state, _f) = populated_local_render_state(64);
    let histories: Vec<_> = state.roster().iter()
        .map(|id| state.member(*id).unwrap().scalars.as_ref().unwrap().recent.clone())
        .collect();
    let mut scratch: [Vec<beatkernel::judge::JudgeEvent>; MAX_RENDER_VISIBLE] =
        std::array::from_fn(|_| Vec::with_capacity(MAX_RENDER_RECENT));
    let storage = scratch.each_ref().map(|events| (events.as_ptr(), events.capacity()));
    let mut scene = crate::scene::Scene::new(960, 720);
    for page in [15, 0, 15] {
        select_retained_render_page(&mut state, page);
        scene.clear();
        crate::ui::organisms::visual_local_render_state(
            &mut scene, &state, &[crate::bga_render::BgaFrame::default(); 4], &mut scratch,
        ).unwrap();
        assert_same_local_scene(&scene, &paint_full_roster_baseline(&state));
        assert_eq!(scratch.each_ref().map(|events| (events.as_ptr(), events.capacity())), storage);
        for (slot, events) in scratch.iter().enumerate() {
            let index = page as usize * MAX_RENDER_VISIBLE + slot;
            let expected: Vec<_> = histories[index].iter().map(RenderJudgeEvent::event).collect();
            assert_eq!(events, &expected);
        }
        // All 64 histories survive, while the converted buffer contains only
        // the current four. Hidden histories become available on the next page.
        assert_eq!(scratch.iter().map(Vec::len).sum::<usize>(),
            histories[page as usize * 4..page as usize * 4 + 4].iter().map(Vec::len).sum::<usize>());
        for (index, id) in state.roster().iter().enumerate() {
            assert_eq!(state.member(*id).unwrap().scalars.as_ref().unwrap().recent, histories[index]);
        }
    }
    let (mut partial, _f) = populated_local_render_state(5);
    select_retained_render_page(&mut partial, 1);
    scene.clear();
    crate::ui::organisms::visual_local_render_state(
        &mut scene, &partial, &[crate::bga_render::BgaFrame::default(); 4], &mut scratch,
    ).unwrap();
    assert_same_local_scene(&scene, &paint_full_roster_baseline(&partial));
    assert_eq!(scene.playfields().len(), 1);
    assert_eq!(scratch[0].len(), MAX_RENDER_RECENT - 1);
    assert!(scratch[1..].iter().all(Vec::is_empty));
    assert_eq!(scratch.each_ref().map(|events| (events.as_ptr(), events.capacity())), storage);
}

#[test]
fn borrowed_local_render_bridge_reuses_scratch_note_instances_and_progress_on_scalar_redraw() {
    let (mut state, _f) = populated_local_render_state(64);
    let mut scratch: [Vec<beatkernel::judge::JudgeEvent>; MAX_RENDER_VISIBLE] =
        std::array::from_fn(|_| Vec::with_capacity(MAX_RENDER_RECENT));
    let storage = scratch.each_ref().map(|events| (events.as_ptr(), events.capacity()));
    let mut scene = crate::scene::Scene::new(960, 720);
    let frames = [crate::bga_render::BgaFrame::default(); 4];
    crate::ui::organisms::visual_local_render_state(&mut scene, &state, &frames, &mut scratch).unwrap();
    let instances: Vec<_> = scene.playfields().iter().map(|field| Arc::clone(&field.instances)).collect();
    assert!(instances.iter().all(|notes| !notes.is_empty()));
    let progress: Vec<_> = state.roster().iter().map(|id| state.member(*id).unwrap().progress.clone()).collect();
    let pristine = crate::note_progress::NoteProgress::new(state.chart().clone()).unwrap();
    let page_storage: Vec<Vec<_>> = progress.iter().map(|value| value.changed_pages_since(&pristine)
        .unwrap().map(|page| page.packed_states().as_ptr()).collect()).collect();
    assert!(page_storage.iter().all(|pages| !pages.is_empty()));
    for scalar_only in [false, true, true] {
        if scalar_only {
            let start = state.page() as usize * MAX_RENDER_VISIBLE;
            let members = state.roster()[start..].iter().map(|id| {
                let member = state.member(*id).unwrap();
                let mut scalar = member.scalars.clone().unwrap();
                scalar.pressed = 0;
                RenderMemberUpdate::from_progress(*id, scalar, &member.progress, &member.progress).unwrap()
            }).collect();
            state.apply_frame(&frame(state.page(), state.sequence() + 1, members)).unwrap();
        }
        scene.clear();
        crate::ui::organisms::visual_local_render_state(&mut scene, &state, &frames, &mut scratch).unwrap();
        assert_same_local_scene(&scene, &paint_full_roster_baseline(&state));
        assert_eq!(scratch.each_ref().map(|events| (events.as_ptr(), events.capacity())), storage);
        for (field, retained) in scene.playfields().iter().zip(&instances) {
            assert!(Arc::ptr_eq(&field.instances, retained));
        }
        for (index, id) in state.roster().iter().enumerate() {
            let member = state.member(*id).unwrap();
            assert_eq!(member.progress.changed_pages_since(&progress[index]).unwrap().count(), 0);
            let pages: Vec<_> = member.progress.changed_pages_since(&pristine).unwrap()
                .map(|page| page.packed_states().as_ptr()).collect();
            assert_eq!(pages, page_storage[index]);
        }
    }
}

fn comparison() -> crate::competition_presentation::CompetitionSnapshot {
    use crate::competition_presentation::{CompetitionSnapshot, GhostSnapshot};
    CompetitionSnapshot {
        ghosts: vec![GhostSnapshot {
            kind: crate::competition::OpponentKind::Other,
            label: "stored prefix".into(),
            hits: u64::MAX,
            misses: 0,
            combo: u64::MAX,
            max_combo: u64::MAX,
            recorded_until: Some(Timestamp::from_nanos(604_800_000_000_001)),
        }],
        network: None,
    }
}

#[test]
fn frozen_historical_grade_and_comparison_pages_preserve_full_supported_custom_domain() {
    use crate::{
        historical_record_presentation::HistoricalRecordPresentation,
        result_archive::{ArchivedScore, MAX_SCORE_GRADES, decode_archive},
        timing::TimingRecord,
    };
    let archive = decode_archive(&crate::result_archive::fixtures::golden()).unwrap();
    let entry = &archive.entries()[0];
    let mut grades: Vec<_> = (0..MAX_SCORE_GRADES - 1).map(|n| (n as u32, 1)).collect();
    grades.push((u32::MAX, 1));
    let mut score = ArchivedScore {
        hits: MAX_SCORE_GRADES as u64,
        misses: 7,
        combo: 0,
        max_combo: 3,
        grades,
        timing: TimingRecord::default(),
    };
    score.validate().unwrap();
    let mut stored_comparison = Some(comparison());
    let mut authoritative = HistoricalRecordPresentation::from_record_with_comparisons(
        (entry.player, entry.result),
        Some(&score),
        Some(&stored_comparison),
    )
    .unwrap();
    let grade_pages = MAX_SCORE_GRADES.div_ceil(4);
    assert_eq!(authoritative.grade_page_count(), grade_pages + 1);
    authoritative.set_grade_page(grade_pages - 1).unwrap();
    let frozen = authoritative.export_visual();
    score.grades.clear();
    stored_comparison.as_mut().unwrap().ghosts.clear();
    drop(authoritative);
    let mut renderer = HistoricalRecordPresentation::import_visual(frozen.clone()).unwrap();
    assert_eq!(renderer.value(), (entry.player, entry.result));
    assert_eq!(renderer.grade_page(), grade_pages - 1);
    assert_eq!(renderer.score().unwrap().grades.len(), MAX_SCORE_GRADES);
    assert_eq!(
        renderer.score().unwrap().grades.last(),
        Some(&(u32::MAX, 1))
    );
    assert_eq!(
        renderer.comparison().unwrap().as_ref().unwrap().ghosts[0].hits,
        u64::MAX
    );
    for page in 0..grade_pages + 1 {
        renderer.set_grade_page(page).unwrap();
    }
    let before = renderer.export_visual();
    assert!(renderer.set_grade_page(grade_pages + 1).is_err());
    assert_eq!(renderer.grade_page(), before.grade_page);
    let mut bad_final_grade = frozen.clone();
    bad_final_grade
        .score
        .as_mut()
        .unwrap()
        .grades
        .last_mut()
        .unwrap()
        .1 = 0;
    assert!(HistoricalRecordPresentation::import_visual(bad_final_grade).is_err());
    let mut bad_page = frozen;
    bad_page.grade_page = grade_pages + 1;
    assert!(HistoricalRecordPresentation::import_visual(bad_page).is_err());
}

#[test]
fn frozen_completed_details_and_comparisons_export_only_actual_mixer_completion() {
    use crate::{
        completed_results_presentation::CompletedResultsPresentation,
        ui::results::FrozenResultsView,
    };
    let game = completed_game();
    let proof = *game.completed_result().unwrap();
    let mut original_comparison = comparison();
    let mut completed = CompletedResultsPresentation::default();
    assert!(
        completed
            .capture_solo(&game, Some(&original_comparison))
            .unwrap()
    );
    let source = completed.view().unwrap();
    let pages = [source.page_count_for(false), source.page_count_for(true)];
    let frozen = completed.export_visual().unwrap().unwrap();
    original_comparison.ghosts.clear();
    drop(game);
    drop(completed);
    let renderer = FrozenResultsView::from_model(frozen.clone()).unwrap();
    assert_eq!(renderer.model().roster, [crate::local_players::PlayerId(1)]);
    assert_eq!(renderer.model().rows[0].result.scope, proof.scope());
    assert_eq!(renderer.model().rows[0].result.outcome, proof.outcome());
    assert_eq!(renderer.model().rows[0].result.gauge, proof.gauge());
    assert_eq!(renderer.model().details[0].score.hits, 0);
    assert_eq!(
        renderer.model().details[0]
            .competition
            .as_ref()
            .unwrap()
            .ghosts[0]
            .hits,
        u64::MAX
    );
    assert_eq!(renderer.page_count_for(false), pages[0]);
    assert_eq!(renderer.page_count_for(true), pages[1]);
    assert!(renderer.has_comparisons());
    let mut bad_member = frozen.clone();
    bad_member.details[0].player = crate::local_players::PlayerId(2);
    assert!(FrozenResultsView::from_model(bad_member).is_err());
    let mut bad_scalar = frozen;
    bad_scalar.details[0].score.max_combo = 1;
    assert!(FrozenResultsView::from_model(bad_scalar).is_err());
    assert!(
        CompletedResultsPresentation::default()
            .export_visual()
            .unwrap()
            .is_none()
    );
}

#[test]
fn frozen_room_pages_match_actual_builder_projection_in_standalone_and_combined_state() {
    use crate::{
        local_players::PlayerId,
        multiplayer_group_rooms::{GroupRoomPolicy, GroupRoomRegistry},
        room_results_builder::RoomResultsBuilder,
    };
    let mut registry = GroupRoomRegistry::new(GroupRoomPolicy::new(1, 3, 8, 100).unwrap());
    let roster = [PlayerId(u32::MAX), PlayerId(71), PlayerId(9)];
    let ids: Vec<_> = (0..3)
        .map(|_| {
            registry
                .join("render", b"shared identity", &roster, 0)
                .unwrap()
                .id
        })
        .collect();
    registry.seal(ids[0], 1).unwrap();
    for id in &ids {
        registry.ready(*id, 2).unwrap();
    }
    let members = registry.room("render").unwrap().members.to_vec();
    let words: Vec<_> = members
        .iter()
        .flat_map(|member| {
            let mut row = vec![
                member.id.0 as u32,
                (member.id.0 >> 32) as u32,
                member.players.len() as u32,
            ];
            row.extend(member.players.iter().map(|player| player.0));
            row
        })
        .collect();
    let mut builder = RoomResultsBuilder::new(ids[0], &words).unwrap();
    assert!(builder.export_visual().unwrap().is_none());
    builder
        .freeze(1, true, Some("accepted prefix retained".into()), false)
        .unwrap();
    let frozen = builder.export_visual().unwrap().unwrap();
    frozen.validate().unwrap();
    assert_eq!(frozen.pages.len(), 2);
    let f = judged_chart(1);
    let mut combined = receiver(&f, &[roster[0]]);
    let producer = crate::note_progress::NoteProgress::new(f.chart.clone()).unwrap();
    let update =
        RenderMemberUpdate::from_progress(roster[0], scalars(&[], 0), &producer, &producer)
            .unwrap();
    let mut actual_ids = Vec::new();
    for page in 0..2 {
        builder.set_page(page).unwrap();
        let standalone = frozen.project(page).unwrap();
        assert_eq!(standalone, builder.presentation().unwrap());
        actual_ids.extend(
            standalone
                .rows
                .iter()
                .map(|row| (row.participant, row.player)),
        );
        let mut state = frame(0, page as u64 + 1, vec![update.clone()]);
        state.room = Some(standalone.clone());
        combined.apply_frame(&state).unwrap();
        assert_eq!(combined.room(), Some(standalone));
    }
    assert_eq!(
        actual_ids,
        members
            .iter()
            .filter(|member| member.id != ids[0])
            .flat_map(|member| member
                .players
                .iter()
                .map(move |player| (member.id, *player)))
            .collect::<Vec<_>>()
    );
    assert!(frozen.project(2).is_err());
    let mut bad_final = frozen.clone();
    bad_final
        .pages
        .last_mut()
        .unwrap()
        .rows
        .last_mut()
        .unwrap()
        .player = PlayerId(0);
    assert!(bad_final.validate().is_err());
    let before_scalars = combined.member(roster[0]).unwrap().scalars.clone();
    let before_room = combined.room().unwrap().clone();
    let mut bad_combined = frame(0, 3, vec![update]);
    bad_combined.members[0].scalars.song_ns = i64::MAX;
    bad_combined.room = Some(bad_final.pages.last().unwrap().clone());
    assert!(combined.apply_frame(&bad_combined).is_err());
    assert_eq!(combined.sequence(), 2);
    assert_eq!(combined.member(roster[0]).unwrap().scalars, before_scalars);
    assert_eq!(combined.room(), Some(&before_room));
}

#[test]
fn explicit_image_bank_limits_accept_exact_budget_and_refuse_candidate_without_publication() {
    use crate::image_assets::{ImageAssetLimits, ImageAssetsTransfer, MAX_IMAGE_BANK_BYTES};
    use beatkernel_bms::ImageId;
    let f = judged_chart(1);
    let ids = [crate::local_players::PlayerId(73)];
    let pixels = vec![1, 2, 3, 255, 4, 5, 6, 128];
    let images = ImageAssetsTransfer {
        resources: vec![Arc::new(
            crate::texture::RgbaImage::new(2, 1, pixels.clone()).unwrap(),
        )],
        sources: vec![0],
        source_ids: vec![(ImageId(1), Some(0))],
        images: vec![(ImageId(1), 0)],
        layers: vec![],
        unavailable: vec![],
    };
    let chart = f.chart.export_visual();
    let cold = visual_registration_bytes(&chart, &images, &ids).unwrap();
    assert!(cold <= MAX_RENDER_REGISTRATION_BYTES);
    let exact = ImageAssetLimits {
        max_decoded_bytes: 8,
        ..Default::default()
    };
    let mut admitted = BrowserRenderState::import_visual_with_limits(
        7,
        19,
        chart.clone(),
        images.clone(),
        ids.to_vec(),
        exact,
    )
    .unwrap();
    assert_eq!(admitted.images().decoded_bytes(), 8);
    assert_eq!(admitted.images().get(ImageId(1)).unwrap().pixels(), pixels);
    let producer = crate::note_progress::NoteProgress::new(f.chart.clone()).unwrap();
    admitted
        .apply_frame(&frame(
            0,
            1,
            vec![
                RenderMemberUpdate::from_progress(ids[0], scalars(&[], -31), &producer, &producer)
                    .unwrap(),
            ],
        ))
        .unwrap();
    let old_chart = admitted.chart().clone();
    let old_images = admitted.images().clone();
    let old_scalars = admitted.member(ids[0]).unwrap().scalars.clone();
    for budget in [0, 7, MAX_IMAGE_BANK_BYTES + 1] {
        let invalid = ImageAssetLimits {
            max_decoded_bytes: budget,
            ..exact
        };
        assert!(
            BrowserRenderState::import_visual_with_limits(
                8,
                20,
                chart.clone(),
                images.clone(),
                ids.to_vec(),
                invalid
            )
            .is_err()
        );
        assert!(Arc::ptr_eq(admitted.chart(), &old_chart));
        assert!(Arc::ptr_eq(admitted.images(), &old_images));
        assert_eq!(admitted.sequence(), 1);
        assert_eq!(admitted.member(ids[0]).unwrap().scalars, old_scalars);
    }
    for budget in [
        9,
        ImageAssetLimits::default().max_decoded_bytes,
        MAX_IMAGE_BANK_BYTES,
    ] {
        let configured = ImageAssetLimits {
            max_decoded_bytes: budget,
            ..exact
        };
        let receiver = BrowserRenderState::import_visual_with_limits(
            8,
            20,
            chart.clone(),
            images.clone(),
            ids.to_vec(),
            configured,
        )
        .unwrap();
        assert_eq!(receiver.images().decoded_bytes(), 8);
    }
    let default = BrowserRenderState::import_visual(8, 20, chart, images, ids.to_vec()).unwrap();
    assert_eq!(default.images().decoded_bytes(), 8);
    let peak = peak_visual_transport_bytes(cold, f.chart.notes.len(), ids.len()).unwrap();
    assert!(peak >= 6 * cold as u64 + 2 * MAX_IMAGE_BANK_BYTES);
    assert!(peak_visual_transport_bytes(cold + 1, f.chart.notes.len(), ids.len()).unwrap() > peak);
    assert!(peak_visual_transport_bytes(MAX_RENDER_REGISTRATION_BYTES + 1, 1, 1).is_ok());
    if usize::BITS == 64 {
        assert!(peak_visual_transport_bytes(usize::MAX, 1, 1).is_err());
    }
    assert!(peak_visual_transport_bytes(cold, beatkernel::chart::MAX_SOURCE_ITEMS + 1, 1).is_err());
    assert!(peak_visual_transport_bytes(cold, 1, 65).is_err());
}

#[test]
fn diagnostic_transport_budget_counts_exact_aggregate_utf8_without_truncation() {
    use crate::image_assets::{ImageAssetLimits, ImageAssets, ImageUnavailable};
    use beatkernel_bms::ImageId;
    let f = judged_chart(1);
    let ids = [crate::local_players::PlayerId(73)];
    let chart = f.chart.export_visual();
    let mut images = ImageAssets::default().export_visual();
    let first = "가ab".to_owned();
    assert_eq!(first.len(), 5);
    images
        .unavailable
        .push((ImageId(1), ImageUnavailable::InvalidData(first.clone())));
    let limits = ImageAssetLimits::default();
    let admitted = BrowserRenderState::import_visual_with_budget(
        7,
        19,
        chart.clone(),
        images.clone(),
        ids.to_vec(),
        limits,
        first.len(),
    )
    .unwrap();
    assert_eq!(
        admitted.images().unavailable(ImageId(1)),
        Some(&ImageUnavailable::InvalidData(first.clone()))
    );
    assert!(
        BrowserRenderState::import_visual_with_budget(
            8,
            20,
            chart.clone(),
            images.clone(),
            ids.to_vec(),
            limits,
            first.len() - 1
        )
        .is_err()
    );
    let second = "é".to_owned();
    images
        .unavailable
        .push((ImageId(2), ImageUnavailable::InvalidData(second.clone())));
    let aggregate = first.len() + second.len();
    let cold = visual_registration_bytes(&chart, &images, &ids).unwrap();
    let exact = BrowserRenderState::import_visual_with_budget(
        8,
        20,
        chart.clone(),
        images.clone(),
        ids.to_vec(),
        limits,
        aggregate,
    )
    .unwrap();
    for (id, text) in [(ImageId(1), &first), (ImageId(2), &second)] {
        assert_eq!(
            exact.images().unavailable(id),
            Some(&ImageUnavailable::InvalidData(text.clone()))
        );
    }
    for allowance in [0, first.len(), aggregate - 1] {
        assert!(
            BrowserRenderState::import_visual_with_budget(
                8,
                20,
                chart.clone(),
                images.clone(),
                ids.to_vec(),
                limits,
                allowance
            )
            .is_err()
        );
        assert_eq!(
            admitted.images().unavailable(ImageId(1)),
            Some(&ImageUnavailable::InvalidData(first.clone()))
        );
        assert_eq!(admitted.images().unavailable(ImageId(2)), None);
    }
    // The compatibility convenience preserves diagnostics; only an explicit
    // transport admission applies a caller's diagnostic byte allowance.
    let convenience =
        BrowserRenderState::import_visual(8, 20, chart.clone(), images.clone(), ids.to_vec())
            .unwrap();
    assert_eq!(
        convenience.images().unavailable(ImageId(2)),
        Some(&ImageUnavailable::InvalidData(second))
    );
    for (_, reason) in &mut images.unavailable {
        let ImageUnavailable::InvalidData(reason) = reason else {
            unreachable!()
        };
        reason.clear();
    }
    let without_diagnostics = visual_registration_bytes(&chart, &images, &ids).unwrap();
    assert_eq!(cold - without_diagnostics, aggregate);
    let with_peak = peak_visual_transport_bytes(cold, f.chart.notes.len(), ids.len()).unwrap();
    let without_peak =
        peak_visual_transport_bytes(without_diagnostics, f.chart.notes.len(), ids.len()).unwrap();
    assert_eq!(with_peak - without_peak, 6 * aggregate as u64);
}
