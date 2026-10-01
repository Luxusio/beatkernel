//! Composed view components. Inputs are portable state; no I/O or clock ownership.
use super::{
    atoms::{rect, text},
    interaction::Bounds,
    molecules,
};
use crate::{
    competition::{OpponentKind, ScoreSummary},
    player::{CompetitionSnapshot, LocalPlayerSnapshot, NetworkStatus},
    player_chart::PlayerChart,
    scene::Scene,
};
use beatkernel::{
    judge::{JudgeEvent, JudgeOutcome},
    time::Timestamp,
};
const TOP: i64 = 110;
const LINE: i64 = 610;
#[cfg(test)]
fn note_y(time: Timestamp, now: Timestamp, lookahead: i64) -> i64 {
    project_note(time, now, lookahead, TOP, LINE).clamp(-10_000, 10_000)
}
#[cfg(test)]
fn project_note(time: Timestamp, now: Timestamp, lookahead: i64, top: i64, line: i64) -> i64 {
    let delta = i128::from(time.as_nanos()) - i128::from(now.as_nanos());
    (i128::from(line) - delta * i128::from(line - top) / i128::from(lookahead))
        .clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}
#[cfg(test)]
fn lane_bounds(index: usize, lanes: usize) -> (i64, i64) {
    partition_lane(index, lanes, 80, 640)
}
fn partition_lane(index: usize, lanes: usize, x: i64, width: i64) -> (i64, i64) {
    let left = x + (index as i128 * i128::from(width) / lanes.max(1) as i128) as i64;
    let right = x + ((index + 1) as i128 * i128::from(width) / lanes.max(1) as i128) as i64;
    (left, right)
}

pub fn playfield(
    pixels: &mut Scene,
    chart: &PlayerChart,
    now: Timestamp,
    lookahead: i64,
) -> Result<(), String> {
    playfield_in(
        pixels,
        chart,
        now,
        lookahead,
        Bounds {
            x: 80,
            y: TOP - 4,
            width: 640,
            height: LINE - TOP + 28,
        },
    )
}

/// Bounds include four pixels above the lane background for clipped note-head
/// overhang and 24 below the judgment line for labels. Projection uses only
/// actual reported song time, with wide integer arithmetic.
pub fn playfield_in(
    pixels: &mut Scene,
    chart: &PlayerChart,
    now: Timestamp,
    lookahead: i64,
    bounds: Bounds,
) -> Result<(), String> {
    if lookahead <= 0 {
        return Err("playfield lookahead must be positive".into());
    }
    let lanes = chart.lanes.len();
    let [width, height] = pixels.dimensions();
    if bounds.x < 0
        || bounds.y < 0
        || bounds.width <= 0
        || bounds.height < 32
        || i128::from(bounds.x) + i128::from(bounds.width) > width as i128
        || i128::from(bounds.y) + i128::from(bounds.height) > height as i128
        || i128::from(bounds.width) < lanes as i128 * 13
    {
        return Err("playfield bounds do not fit viewport or lane geometry".into());
    }
    let top = bounds.y + 4;
    let line = bounds.y + bounds.height - 24;
    for lane in 0..lanes {
        let (left, right) = partition_lane(lane, lanes, bounds.x, bounds.width);
        rect(
            pixels,
            left,
            top,
            right - left - 1,
            line - top + 24,
            if lane % 2 == 0 { 0x1d2734 } else { 0x18212c },
        );
        clipped_text(
            pixels,
            Bounds {
                x: left + 3,
                y: line + 10,
                width: right - left - 3,
                height: 7,
            },
            &(lane + 1).to_string(),
            1,
            0xa9bdd5,
        );
    }
    pixels.playfield(chart, now, lookahead, bounds)?;
    rect(pixels, bounds.x, line, bounds.width, 3, 0xffffff);
    pixels.status()
}

pub const LOCAL_PLAYERS_PER_PAGE: usize = 4;

fn page_range(count: usize, page: usize) -> Result<std::ops::Range<usize>, String> {
    if !(1..=crate::local_players::MAX_LOCAL_PLAYERS).contains(&count)
        || page >= count.div_ceil(LOCAL_PLAYERS_PER_PAGE)
    {
        return Err("invalid local player roster or display page".into());
    }
    let first = page * LOCAL_PLAYERS_PER_PAGE;
    Ok(first..(first + LOCAL_PLAYERS_PER_PAGE).min(count))
}

fn panel_bounds(index: usize, count: usize) -> Bounds {
    let columns = if count == 1 { 1 } else { 2 };
    let rows = if count <= 2 { 1 } else { 2 };
    let width = (912 - (columns - 1) * 12) / columns;
    let height = (540 - (rows - 1) * 12) / rows;
    Bounds {
        x: 24 + (index % columns) as i64 * (width + 12) as i64,
        y: 100 + (index / columns) as i64 * (height + 12) as i64,
        width: width as i64,
        height: height as i64,
    }
}

fn clipped_text(scene: &mut Scene, bounds: Bounds, value: &str, scale: usize, color: u32) {
    if scale == 0 || bounds.width <= 0 || bounds.height < 7 * scale as i64 {
        return;
    }
    let cells = bounds.width as usize / (6 * scale);
    let end = value
        .char_indices()
        .nth(cells)
        .map_or(value.len(), |(at, _)| at);
    text(
        scene,
        bounds.x as usize,
        bounds.y as usize,
        &value[..end],
        scale,
        color,
    );
}

/// Exact counts use one row in wide panels and separate rows in the sidebar.
fn competition_height(snapshot: &CompetitionSnapshot, width: i64) -> Result<i64, String> {
    if snapshot.ghosts.len() > 8 {
        return Err("competition display exceeds eight recorded opponents".into());
    }
    let ghost_rows = if width >= 44 * 6 { 2 } else { 3 };
    Ok(
        (snapshot.ghosts.len() as i64 * ghost_rows
            + if snapshot.network.is_some() { 4 } else { 0 })
            * 7,
    )
}

/// Passive comparison only: peer progress has its own clock and is self-reported.
/// Labels may be clipped; opponent rows, lifecycle and exact counters are retained.
pub fn competition_summary(
    scene: &mut Scene,
    snapshot: &CompetitionSnapshot,
    bounds: Bounds,
) -> Result<(), String> {
    let required = competition_height(snapshot, bounds.width)?;
    let [width, height] = scene.dimensions();
    if bounds.x < 0
        || bounds.y < 0
        || bounds.width < 186
        || bounds.height < required
        || i128::from(bounds.x) + i128::from(bounds.width) > width as i128
        || i128::from(bounds.y) + i128::from(bounds.height) > height as i128
    {
        return Err("competition summary bounds do not fit all opponent rows".into());
    }
    let wide = bounds.width >= 44 * 6;
    let mut y = bounds.y;
    let mut row = |value: &str, color| {
        clipped_text(
            scene,
            Bounds {
                y,
                height: 7,
                ..bounds
            },
            value,
            1,
            color,
        );
        y += 7;
    };
    for ghost in &snapshot.ghosts {
        let kind = match ghost.kind {
            OpponentKind::Own => "OWN",
            OpponentKind::Other => "OTHER",
        };
        row(&format!("{kind} {}", ghost.label), 0xb6cce6);
        if wide {
            row(&format!("H{} M{}", ghost.hits, ghost.misses), 0x9bb1cf);
        } else {
            row(&format!("H{}", ghost.hits), 0x9bb1cf);
            row(&format!("M{}", ghost.misses), 0x9bb1cf);
        }
    }
    if let Some(peer) = &snapshot.network {
        let status = match peer.status {
            NetworkStatus::Waiting => "WAITING",
            NetworkStatus::Connected => "CONNECTED",
            NetworkStatus::Disconnected => "DISCONNECTED",
            NetworkStatus::Stopped => "STOPPED",
        };
        if wide {
            row("PEER REPORTED", 0xe3c887);
            row(status, 0xe3c887);
            match &peer.progress {
                Some(progress) => row(
                    &format!("H{} M{}", progress.hits, progress.misses),
                    0x9bb1cf,
                ),
                None => row("NO PEER PROGRESS", 0x9bb1cf),
            }
        } else {
            row(&format!("PEER REPORTED {status}"), 0xe3c887);
            match &peer.progress {
                Some(progress) => {
                    row(&format!("H{}", progress.hits), 0x9bb1cf);
                    row(&format!("M{}", progress.misses), 0x9bb1cf);
                }
                None => {
                    row("NO PEER PROGRESS", 0x9bb1cf);
                    row("", 0x9bb1cf);
                }
            }
        }
        match &peer.progress {
            Some(progress) => row(
                &format!("SONG {:.3} S", progress.song_ns as f64 / 1e9),
                0x9bb1cf,
            ),
            None => row("SONG UNKNOWN", 0x9bb1cf),
        }
    }
    scene.status()
}

/// Four is a presentation-page budget, never a roster/gameplay limit.
pub fn local_players(
    scene: &mut Scene,
    players: &[LocalPlayerSnapshot],
    lookahead: i64,
    page: usize,
) -> Result<(), String> {
    local_players_with_competition(scene, players, lookahead, page, false)
}

/// Explicit comparison view reserves rows without changing song-time projection.
/// Normal gameplay keeps its original lanes even when comparison state exists.
pub fn local_players_with_competition(
    scene: &mut Scene,
    players: &[LocalPlayerSnapshot],
    lookahead: i64,
    page: usize,
    show: bool,
) -> Result<(), String> {
    let visible = page_range(players.len(), page)?;
    if lookahead <= 0 {
        return Err("local playfield lookahead must be positive".into());
    }
    for (index, player) in players.iter().enumerate() {
        if player.player.0 == 0
            || players[..index]
                .iter()
                .any(|other| other.player == player.player)
            || player.recent_results.len() > 128
        {
            return Err("invalid local presentation identities/result capacity".into());
        }
    }
    let count = visible.len();
    for (index, player) in players[visible].iter().enumerate() {
        let bounds = panel_bounds(index, count);
        rect(
            scene,
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            0x141d29,
        );
        let title = format!(
            "P{} {}",
            player.player.0,
            player
                .chart
                .as_ref()
                .map_or("LOADING", |chart| chart.title.as_str())
        );
        let line = |dy: i64, height: i64| Bounds {
            x: bounds.x + 10,
            y: bounds.y + dy,
            width: bounds.width - 20,
            height,
        };
        clipped_text(scene, line(8, 14), &title, 2, 0xf0f4ff);
        clipped_text(
            scene,
            line(32, 7),
            &format!("HITS {} MISSES {}", player.score.hits, player.score.misses),
            1,
            0x9bb1cf,
        );
        clipped_text(
            scene,
            line(44, 7),
            &format!(
                "COMBO {} MAX {}",
                player.score.combo, player.score.max_combo
            ),
            1,
            0x9bb1cf,
        );
        if let Some(event) = player
            .last_judge
            .as_ref()
            .or_else(|| player.recent_results.last())
        {
            let (label, color) = match event.outcome {
                JudgeOutcome::Hit { grade, .. } => {
                    (format!("G{} #{}", grade.0, event.object.0), 0x74e5c5)
                }
                JudgeOutcome::Miss { .. } => (format!("MISS #{}", event.object.0), 0xff8e8e),
            };
            clipped_text(scene, line(56, 7), &label, 1, color);
        }
        let comparisons = if show {
            player.competition.as_ref()
        } else {
            None
        };
        let summary_height = comparisons
            .map(|snapshot| competition_height(snapshot, bounds.width - 20))
            .transpose()?
            .unwrap_or(0);
        if let Some(snapshot) = comparisons {
            competition_summary(scene, snapshot, line(72, summary_height))?;
        }
        let field_offset = 72 + summary_height;
        match (player.chart.as_ref(), player.song_time) {
            (Some(chart), Some(now)) => playfield_in(
                scene,
                chart,
                now,
                lookahead,
                Bounds {
                    x: bounds.x + 10,
                    y: bounds.y + field_offset,
                    width: bounds.width - 20,
                    height: bounds.height - field_offset - 8,
                },
            )?,
            _ => clipped_text(
                scene,
                line(field_offset + 12, 7),
                "WAITING FOR GAME STATE",
                1,
                0x9bb1cf,
            ),
        }
    }
    scene.status()
}
pub fn scoreboard(pixels: &mut Scene, score: &ScoreSummary, recent_results: &[JudgeEvent]) {
    molecules::counter(pixels, 750, 145, "HITS", score.hits, 0x74e5c5);
    molecules::counter(pixels, 750, 235, "MISSES", score.misses, 0xff8e8e);
    molecules::counter(pixels, 750, 325, "COMBO", score.combo, 0x9bb1cf);
    molecules::counter(pixels, 750, 415, "MAX COMBO", score.max_combo, 0x9bb1cf);
    for (index, event) in recent_results.iter().rev().take(4).enumerate() {
        let (label, color) = match event.outcome {
            JudgeOutcome::Hit { grade, .. } => {
                (format!("G{} #{}", grade.0, event.object.0), 0x74e5c5)
            }
            JudgeOutcome::Miss { .. } => (format!("MISS #{}", event.object.0), 0xff8e8e),
        };
        text(pixels, 750, 520 + index * 22, &label, 1, color);
    }
}
/// Competition mode reserves the sidebar for every recorded/remote prefix.
pub fn competition_scoreboard(
    scene: &mut Scene,
    score: &ScoreSummary,
    snapshot: &CompetitionSnapshot,
) -> Result<(), String> {
    for (index, (label, value, color)) in [
        ("HITS", score.hits, 0x74e5c5),
        ("MISSES", score.misses, 0xff8e8e),
        ("COMBO", score.combo, 0x9bb1cf),
        ("MAX COMBO", score.max_combo, 0x9bb1cf),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 145 + index as i64 * 28;
        clipped_text(
            scene,
            Bounds {
                x: 750,
                y,
                width: 186,
                height: 7,
            },
            label,
            1,
            color,
        );
        clipped_text(
            scene,
            Bounds {
                x: 750,
                y: y + 10,
                width: 186,
                height: 7,
            },
            &value.to_string(),
            1,
            color,
        );
    }
    competition_summary(
        scene,
        snapshot,
        Bounds {
            x: 750,
            y: 280,
            width: 186,
            height: 360,
        },
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn chart() -> PlayerChart {
        let source = beatkernel_bms::parse(
            "#BPM 120\n#TITLE A LONG TITLE FOR PLAYER PANELS\n#00011:0100\n#00051:0202\n",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap()
    }
    fn inside(rectangle: &[f32; 4], bounds: Bounds) -> bool {
        rectangle[0] >= bounds.x as f32
            && rectangle[1] >= bounds.y as f32
            && rectangle[0] + rectangle[2] <= (bounds.x + bounds.width) as f32
            && rectangle[1] + rectangle[3] <= (bounds.y + bounds.height) as f32
    }
    #[test]
    fn relocated_notes_hold_caps_and_labels_remain_inside_playfield_bounds() {
        let mut scene = Scene::new(960, 720);
        let bounds = Bounds {
            x: 100,
            y: 200,
            width: 300,
            height: 160,
        };
        let chart = chart();
        playfield_in(&mut scene, &chart, Timestamp::ZERO, 1_000_000_000, bounds).unwrap();
        let field = &scene.playfields()[0];
        assert_eq!((field.top, field.bottom, field.drift), (204.0, 351.0, 0.0));
        assert_eq!(field.instances.len(), 4); // tap + hold body/tail/head
        assert!(field.instances.iter().any(|instance| {
            instance.appearance[0] == 0.0
                && instance.geometry[2] == 336.0
                && instance.geometry[3] == 204.0
        }));
        assert!(
            scene
                .rectangles()
                .iter()
                .all(|rectangle| inside(&rectangle.bounds, bounds))
        );
        for now in [Timestamp::MIN, Timestamp::MAX] {
            scene.clear();
            playfield_in(&mut scene, &chart, now, 1, bounds).unwrap();
            assert!(scene.playfields()[0].instances.is_empty());
            assert!(
                scene
                    .rectangles()
                    .iter()
                    .all(|rectangle| inside(&rectangle.bounds, bounds))
            );
        }
        assert!(playfield_in(&mut scene, &chart, Timestamp::ZERO, 0, bounds).is_err());
        assert!(
            playfield_in(
                &mut scene,
                &chart,
                Timestamp::ZERO,
                1,
                Bounds {
                    x: i64::MAX,
                    ..bounds
                }
            )
            .is_err()
        );
    }
    #[test]
    fn local_panels_contain_actual_geometry_and_paging_covers_the_whole_roster() {
        let chart = std::sync::Arc::new(chart());
        let players: Vec<_> = (1..=64)
            .map(|id| LocalPlayerSnapshot {
                player: crate::local_players::PlayerId(id),
                chart: Some(std::sync::Arc::clone(&chart)),
                song_time: Some(Timestamp::ZERO),
                score: ScoreSummary {
                    hits: u64::MAX,
                    misses: u64::MAX,
                    combo: u64::MAX,
                    max_combo: u64::MAX,
                    ..Default::default()
                },
                last_judge: None,
                recent_results: Vec::new(),
                competition: None,
            })
            .collect();
        let mut scene = Scene::new(960, 720);
        for count in [2, 3, 4] {
            scene.clear();
            local_players(&mut scene, &players[..count], 1_000_000_000, 0).unwrap();
            let panels: Vec<_> = (0..count).map(|index| panel_bounds(index, count)).collect();
            assert!(scene.rectangles().iter().all(|rectangle| {
                panels
                    .iter()
                    .any(|bounds| inside(&rectangle.bounds, *bounds))
            }));
            assert!(scene.status().is_ok());
        }
        let covered: Vec<_> = (0..16)
            .flat_map(|page| page_range(64, page).unwrap())
            .collect();
        assert_eq!(covered, (0..64).collect::<Vec<_>>());
        assert_eq!(page_range(5, 1).unwrap(), 4..5);
        assert!(page_range(64, 16).is_err());
        assert!(page_range(0, 0).is_err());
        assert!(page_range(65, 0).is_err());
        scene.clear();
        local_players(&mut scene, &players, 1_000_000_000, 15).unwrap();
        assert!(scene.status().is_ok());
    }
    fn comparisons(status: NetworkStatus, song_ns: i64) -> CompetitionSnapshot {
        CompetitionSnapshot {
            ghosts: (0..8)
                .map(|index| crate::player::GhostSnapshot {
                    kind: if index % 2 == 0 {
                        OpponentKind::Own
                    } else {
                        OpponentKind::Other
                    },
                    label: "A VERY LONG RECORDED OPPONENT LABEL".repeat(2),
                    hits: u64::MAX,
                    misses: u64::MAX,
                    combo: 0,
                    max_combo: 0,
                    recorded_until: Some(Timestamp::MAX),
                })
                .collect(),
            network: Some(crate::player::NetworkSnapshot {
                status,
                progress: Some(crate::multiplayer::Progress {
                    song_ns,
                    hits: u64::MAX,
                    misses: u64::MAX,
                    combo: 0,
                    max_combo: 0,
                }),
            }),
        }
    }
    #[test]
    fn competition_rows_keep_all_prefixes_and_exact_extreme_counts_inside_bounds() {
        let mut scene = Scene::new(960, 720);
        for status in [
            NetworkStatus::Waiting,
            NetworkStatus::Connected,
            NetworkStatus::Disconnected,
            NetworkStatus::Stopped,
        ] {
            for song in [i64::MIN, i64::MAX] {
                let snapshot = comparisons(status, song);
                assert_eq!(competition_height(&snapshot, 186).unwrap(), 196);
                assert_eq!(competition_height(&snapshot, 430).unwrap(), 140);
                for bounds in [
                    Bounds {
                        x: 750,
                        y: 280,
                        width: 186,
                        height: 360,
                    },
                    Bounds {
                        x: 34,
                        y: 172,
                        width: 430,
                        height: 140,
                    },
                ] {
                    scene.clear();
                    competition_summary(&mut scene, &snapshot, bounds).unwrap();
                    assert!(
                        scene
                            .rectangles()
                            .iter()
                            .all(|rectangle| inside(&rectangle.bounds, bounds))
                    );
                    assert!(!scene.rectangles().is_empty());
                }
            }
        }
        let mut snapshot = comparisons(NetworkStatus::Stopped, 0);
        snapshot.network.as_mut().unwrap().progress = None;
        scene.clear();
        competition_summary(
            &mut scene,
            &snapshot,
            Bounds {
                x: 750,
                y: 280,
                width: 186,
                height: 360,
            },
        )
        .unwrap();
        assert!(
            competition_summary(
                &mut scene,
                &snapshot,
                Bounds {
                    x: 750,
                    y: 280,
                    width: 186,
                    height: 195
                }
            )
            .is_err()
        );
        snapshot.ghosts.push(snapshot.ghosts[0].clone());
        assert!(competition_height(&snapshot, 430).is_err());
    }
    #[test]
    fn four_competition_panels_preserve_room_for_actual_chart_projection() {
        let chart = std::sync::Arc::new(chart());
        let players: Vec<_> = (1..=4)
            .map(|id| LocalPlayerSnapshot {
                player: crate::local_players::PlayerId(id),
                chart: Some(std::sync::Arc::clone(&chart)),
                song_time: Some(Timestamp::ZERO),
                score: ScoreSummary::default(),
                last_judge: None,
                recent_results: Vec::new(),
                competition: Some(comparisons(NetworkStatus::Connected, i64::MIN)),
            })
            .collect();
        let mut scene = Scene::new(960, 720);
        local_players(&mut scene, &players, 1_000_000_000, 0).unwrap();
        // Stored opponents do not shrink the ordinary gameplay lane background.
        assert!(
            scene
                .rectangles()
                .iter()
                .any(|rectangle| rectangle.bounds[0] == 34.0
                    && rectangle.bounds[1] == 176.0
                    && rectangle.bounds[3] == 180.0)
        );
        scene.clear();
        local_players_with_competition(&mut scene, &players, 1_000_000_000, 0, true).unwrap();
        assert!(
            scene
                .rectangles()
                .iter()
                .any(|rectangle| rectangle.bounds[0] == 34.0
                    && rectangle.bounds[1] == 316.0
                    && rectangle.bounds[3] == 40.0)
        );
        let panels: Vec<_> = (0..4).map(|index| panel_bounds(index, 4)).collect();
        assert!(scene.rectangles().iter().all(|rectangle| {
            panels
                .iter()
                .any(|bounds| inside(&rectangle.bounds, *bounds))
        }));
        assert!(scene.status().is_ok());
        let bounds = panel_bounds(0, 4);
        assert_eq!(
            bounds.height
                - 72
                - competition_height(players[0].competition.as_ref().unwrap(), bounds.width - 20)
                    .unwrap()
                - 8,
            44
        );
    }
    #[test]
    fn projection_preserves_extreme_timestamp_bounds_and_lane_partition() {
        assert_eq!(
            note_y(Timestamp::ZERO, Timestamp::ZERO, 1_000_000_000),
            LINE
        );
        assert_eq!(
            note_y(
                Timestamp::from_nanos(1_000_000_000),
                Timestamp::ZERO,
                1_000_000_000
            ),
            TOP
        );
        assert_eq!(
            note_y(Timestamp::MAX, Timestamp::MIN, 1_000_000_000),
            -10_000
        );
        assert_eq!(
            note_y(Timestamp::MIN, Timestamp::MAX, 1_000_000_000),
            10_000
        );
        for lanes in [1, 7, 16] {
            assert_eq!(lane_bounds(0, lanes).0, 80);
            assert_eq!(lane_bounds(lanes - 1, lanes).1, 720);
            for index in 1..lanes {
                assert_eq!(lane_bounds(index - 1, lanes).1, lane_bounds(index, lanes).0);
            }
        }
    }
}

/// Passive bounded catalog view; the application owns selection and hit routing.
pub fn device_list(
    scene: &mut Scene,
    catalog: &crate::device_catalog::DeviceCatalog,
    selected: Option<usize>,
    first: usize,
    max_rows: usize,
) {
    for (index, choice) in catalog
        .choices()
        .iter()
        .enumerate()
        .skip(first)
        .take(max_rows.min(10))
    {
        let y = 120 + (index - first) as i64 * 39;
        rect(
            scene,
            24,
            y,
            906,
            34,
            if selected == Some(index) {
                0x29475e
            } else {
                0x1d2734
            },
        );
        text(
            scene,
            32,
            y as usize + 9,
            &choice.label,
            2,
            if choice.selectable {
                0xf0f4ff
            } else {
                0x687485
            },
        );
    }
    if catalog.choices().is_empty() {
        text(scene, 24, 145, "NO DEVICES REPORTED", 2, 0x9bb1cf);
    }
    if let Some(choice) = selected.and_then(|index| catalog.choices().get(index)) {
        text(scene, 24, 558, &choice.id, 1, 0xf0f4ff);
        text(scene, 24, 585, &choice.detail, 1, 0x9bb1cf);
    }
}
