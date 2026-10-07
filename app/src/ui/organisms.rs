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
    playfield_layout::{DEFAULT_BOUNDS, partition_lane},
    scene::Scene,
};
use beatkernel::{
    judge::{JudgeEvent, JudgeOutcome},
    time::Timestamp,
};
#[cfg(test)]
const TOP: i64 = 110;
#[cfg(test)]
const LINE: i64 = 610;

/// Room comparisons occupy only the existing footer; note and touch geometry
/// above y=640 is untouched. All text is cached by the retained model.
pub fn room_opponent_footer(
    scene: &mut Scene,
    hud: &crate::room_opponent_hud::RoomOpponentHud,
) -> Result<(), String> {
    use crate::scene::ClipRect;
    let footer = ClipRect::new([0, 646, 960, 74])?;
    super::atoms::text_clipped(
        scene,
        12,
        646,
        hud.heading(),
        1,
        if hud.failed() { 0xff8e8e } else { 0x9bb1cf },
        footer,
    )?;
    for (index, row) in hud.page().iter().enumerate() {
        let x = 12 + (index % 2) * 480;
        let y = 663 + (index / 2) * 27;
        let clip = ClipRect::new([x as i64, y as i64, 456, 27])?;
        super::atoms::text_clipped(scene, x, y, row.label(), 1, 0xf0f4ff, clip)?;
        for (line, text) in row.counters().iter().enumerate() {
            super::atoms::text_clipped(scene, x, y + 9 * (line + 1), text, 1, 0x9bb1cf, clip)?;
        }
    }
    Ok(())
}

/// Native room page leaves x=620..960 available for existing local-player
/// controls. Only the selected four cached rows are read; field geometry stays fixed.
pub fn room_presentation_footer(
    scene: &mut Scene,
    room: &crate::room_presentation::RoomPresentation,
) -> Result<(), String> {
    use crate::scene::ClipRect;
    let heading = ClipRect::new([0, 646, 300, 17])?;
    super::atoms::text_clipped(
        scene,
        12,
        646,
        &room.heading,
        1,
        if room.failed { 0xff8e8e } else { 0x9bb1cf },
        heading,
    )?;
    if let Some(error) = &room.error {
        super::atoms::text_clipped(
            scene,
            312,
            646,
            error,
            1,
            0xff8e8e,
            ClipRect::new([300, 646, 300, 17])?,
        )?;
    }
    for (index, row) in room.rows.iter().take(4).enumerate() {
        let x = 12 + (index % 2) * 300;
        let y = 663 + (index / 2) * 27;
        let clip = ClipRect::new([x as i64, y as i64, 288, 27])?;
        super::atoms::text_clipped(scene, x, y, &row.label, 1, 0xf0f4ff, clip)?;
        for (line, text) in row.counters.iter().enumerate() {
            super::atoms::text_clipped(scene, x, y + 9 * (line + 1), text, 1, 0x9bb1cf, clip)?;
        }
    }
    Ok(())
}
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

pub fn playfield(
    pixels: &mut Scene,
    chart: &PlayerChart,
    now: Timestamp,
    lookahead: i64,
) -> Result<(), String> {
    playfield_with_feedback(pixels, chart, now, lookahead, &[])
}

/// Full-size playfield with the actual local member's recent judge events.
pub fn playfield_with_feedback(
    pixels: &mut Scene,
    chart: &PlayerChart,
    now: Timestamp,
    lookahead: i64,
    recent: &[JudgeEvent],
) -> Result<(), String> {
    playfield_with_state(pixels, chart, now, lookahead, recent, 0)
}

/// Actual admitted button state, independent of recent judge events.
pub fn playfield_with_state(
    pixels: &mut Scene,
    chart: &PlayerChart,
    now: Timestamp,
    lookahead: i64,
    recent: &[JudgeEvent],
    pressed_lanes: u32,
) -> Result<(), String> {
    playfield_with_progress(pixels, chart, now, lookahead, recent, pressed_lanes, None)
}

/// Full playfield with exact prepared-chart-local object progress.
pub fn playfield_with_progress(
    pixels: &mut Scene,
    chart: &PlayerChart,
    now: Timestamp,
    lookahead: i64,
    recent: &[JudgeEvent],
    pressed_lanes: u32,
    progress: Option<&crate::note_progress::NoteProgress>,
) -> Result<(), String> {
    playfield_with_background(
        pixels,
        chart,
        now,
        lookahead,
        recent,
        pressed_lanes,
        progress,
        crate::bga_render::BgaFrame::default(),
    )
}

/// Actual prepared static-image frame composed below notes and judgement layers.
pub fn playfield_with_background(
    pixels: &mut Scene,
    chart: &PlayerChart,
    now: Timestamp,
    lookahead: i64,
    recent: &[JudgeEvent],
    pressed_lanes: u32,
    progress: Option<&crate::note_progress::NoteProgress>,
    frame: crate::bga_render::BgaFrame,
) -> Result<(), String> {
    playfield_in_with_background(
        pixels,
        chart,
        now,
        lookahead,
        Bounds {
            x: DEFAULT_BOUNDS[0],
            y: DEFAULT_BOUNDS[1],
            width: DEFAULT_BOUNDS[2],
            height: DEFAULT_BOUNDS[3],
        },
        recent,
        pressed_lanes,
        progress,
        frame,
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
    playfield_in_with_feedback(pixels, chart, now, lookahead, bounds, &[])
}

/// Bounded lane feedback is admitted before any playfield geometry is painted.
pub fn playfield_in_with_feedback(
    pixels: &mut Scene,
    chart: &PlayerChart,
    now: Timestamp,
    lookahead: i64,
    bounds: Bounds,
    recent: &[JudgeEvent],
) -> Result<(), String> {
    playfield_in_with_state(pixels, chart, now, lookahead, bounds, recent, 0)
}

/// Channel masks are checked before drawing; chart order includes scratch lanes.
pub fn playfield_in_with_state(
    pixels: &mut Scene,
    chart: &PlayerChart,
    now: Timestamp,
    lookahead: i64,
    bounds: Bounds,
    recent: &[JudgeEvent],
    pressed_lanes: u32,
) -> Result<(), String> {
    playfield_in_with_progress(
        pixels,
        chart,
        now,
        lookahead,
        bounds,
        recent,
        pressed_lanes,
        None,
    )
}

/// Exact progress is checked before lane geometry or GPU note admission.
pub fn playfield_in_with_progress(
    pixels: &mut Scene,
    chart: &PlayerChart,
    now: Timestamp,
    lookahead: i64,
    bounds: Bounds,
    recent: &[JudgeEvent],
    pressed_lanes: u32,
    progress: Option<&crate::note_progress::NoteProgress>,
) -> Result<(), String> {
    playfield_in_with_background(
        pixels,
        chart,
        now,
        lookahead,
        bounds,
        recent,
        pressed_lanes,
        progress,
        crate::bga_render::BgaFrame::default(),
    )
}

/// Validates a background frame before any playfield geometry is painted.
pub fn playfield_in_with_background(
    pixels: &mut Scene,
    chart: &PlayerChart,
    now: Timestamp,
    lookahead: i64,
    bounds: Bounds,
    recent: &[JudgeEvent],
    pressed_lanes: u32,
    progress: Option<&crate::note_progress::NoteProgress>,
    frame: crate::bga_render::BgaFrame,
) -> Result<(), String> {
    frame.validate()?;
    if progress.is_some_and(|state| !state.matches_chart(chart)) {
        return Err("note progress belongs to another prepared chart".into());
    }
    crate::pressed_keys::validate_mask(pressed_lanes)?;
    let feedback = crate::judge_feedback::project(chart, now, recent)?;
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
        let background = if lane % 2 == 0 { 0x1d2734 } else { 0x18212c };
        rect(
            pixels,
            left,
            top,
            right - left - 1,
            line - top + 24,
            background,
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
    crate::bga_render::paint(
        pixels,
        frame,
        Bounds {
            x: bounds.x,
            y: top,
            width: bounds.width,
            height: line - top,
        },
    )?;
    pixels.playfield_with_progress(chart, now, lookahead, bounds, progress)?;
    for lane in 0..lanes {
        let (left, right) = partition_lane(lane, lanes, bounds.x, bounds.width);
        let background: u32 = if lane % 2 == 0 { 0x1d2734 } else { 0x18212c };
        if crate::pressed_keys::lane_bit(beatkernel::input::GameControlId(u32::from(
            chart.lanes[lane],
        )))
        .is_some_and(|bit| pressed_lanes & bit != 0)
        {
            // Draw over notes, below judgment feedback and the white line.
            rect(pixels, left + 1, line - 22, right - left - 3, 16, 0x416ca0);
        }
        if let Some(feedback) = feedback[lane] {
            let color = match feedback.event.outcome {
                JudgeOutcome::Hit { .. } => 0x74e5c5,
                JudgeOutcome::Miss { .. } => 0xef6372,
            };
            let lifetime = crate::judge_feedback::FEEDBACK_LIFETIME_NS;
            let mut faded = 0u32;
            for shift in [16, 8, 0] {
                let source = i64::from((color >> shift) & 255);
                let target = i64::from((background >> shift) & 255);
                let channel =
                    (source * (lifetime - feedback.age_ns) + target * feedback.age_ns) / lifetime;
                faded |= (channel as u32) << shift;
            }
            rect(pixels, left + 1, line - 6, right - left - 3, 6, faded);
        }
    }
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
    let [x, y, width, height] = crate::playfield_layout::local_panel_bounds(count, index)
        .expect("validated local page and visible slot");
    Bounds {
        x,
        y,
        width,
        height,
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
    competition_summary_with_peer_offset(scene, snapshot, bounds, None)
}

fn competition_summary_with_peer_offset(
    scene: &mut Scene,
    snapshot: &CompetitionSnapshot,
    bounds: Bounds,
    peer_offset: Option<i64>,
) -> Result<(), String> {
    let required = competition_height(snapshot, bounds.width)?;
    let required = if let Some(offset) = peer_offset {
        if offset < required - 28 {
            return Err("peer display overlaps recorded opponent rows".into());
        }
        offset
            .checked_add(28)
            .ok_or("peer display row extent overflow")?
    } else {
        required
    };
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
    let ghost_rows = snapshot.ghosts.len() as i64 * if wide { 2 } else { 3 };
    let mut row_index = 0;
    let mut row = |value: &str, color| {
        let offset = if row_index >= ghost_rows {
            peer_offset.map_or(row_index * 7, |offset| {
                offset + (row_index - ghost_rows) * 7
            })
        } else {
            row_index * 7
        };
        clipped_text(
            scene,
            Bounds {
                y: bounds.y + offset,
                height: 7,
                ..bounds
            },
            value,
            1,
            color,
        );
        row_index += 1;
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
    local_players_with_background(
        scene,
        players,
        lookahead,
        page,
        show,
        &[crate::bga_render::BgaFrame::default(); 4],
    )
}

/// Borrowed presentation state; composing a frame never clones chart-sized progress.
#[derive(Clone, Copy)]
pub struct LocalPlayerView<'a> {
    pub player: crate::local_players::PlayerId,
    pub chart: Option<&'a PlayerChart>,
    pub song_time: Option<Timestamp>,
    pub score: &'a ScoreSummary,
    pub bms_score: Option<crate::judgment_policy::BmsScoreSummary>,
    pub gauge: Option<&'a crate::gauge::BmsGauge>,
    pub last_judge: Option<&'a JudgeEvent>,
    pub recent_results: &'a [JudgeEvent],
    pub pressed_lanes: u32,
    pub note_progress: Option<&'a crate::note_progress::NoteProgress>,
    pub competition: Option<&'a CompetitionSnapshot>,
}
impl<'a> From<&'a LocalPlayerSnapshot> for LocalPlayerView<'a> {
    fn from(player: &'a LocalPlayerSnapshot) -> Self {
        Self {
            player: player.player,
            chart: player.chart.as_deref(),
            song_time: player.song_time,
            score: &player.score,
            bms_score: player.bms_score,
            gauge: Some(&player.gauge),
            last_judge: player.last_judge.as_ref(),
            recent_results: &player.recent_results,
            pressed_lanes: player.pressed_lanes,
            note_progress: player.note_progress.as_ref(),
            competition: player.competition.as_ref(),
        }
    }
}

/// Visible member slots share the bounded image cache without mixing clocks.
pub fn local_players_with_background(
    scene: &mut Scene,
    players: &[LocalPlayerSnapshot],
    lookahead: i64,
    page: usize,
    show: bool,
    frames: &[crate::bga_render::BgaFrame; 4],
) -> Result<(), String> {
    // Preflight the bounded roster before making the stack-only borrowed bridge.
    page_range(players.len(), page)?;
    let mut views = [LocalPlayerView::from(&players[0]); crate::local_players::MAX_LOCAL_PLAYERS];
    for (destination, player) in views.iter_mut().zip(players) {
        *destination = LocalPlayerView::from(player);
    }
    local_player_views_with_background(
        scene,
        &views[..players.len()],
        lookahead,
        page,
        show,
        frames,
    )
}

/// The common field composer accepts retained owners without copying their state.
pub fn local_player_views_with_background(
    scene: &mut Scene,
    players: &[LocalPlayerView<'_>],
    lookahead: i64,
    page: usize,
    show: bool,
    frames: &[crate::bga_render::BgaFrame; 4],
) -> Result<(), String> {
    local_player_views_with_background_impl(scene, players, lookahead, page, show, frames, None)
}

/// Fixed comparison reservations preserve contact geometry across HUD failure.
#[allow(clippy::too_many_arguments)]
pub fn local_player_views_with_reserved_comparison_space(
    scene: &mut Scene,
    players: &[LocalPlayerView<'_>],
    lookahead: i64,
    page: usize,
    show: bool,
    frames: &[crate::bga_render::BgaFrame; 4],
    reserved: &[i64],
) -> Result<(), String> {
    if reserved.len() != players.len() {
        return Err("local comparison reservations differ from the roster".into());
    }
    local_player_views_with_background_impl(
        scene,
        players,
        lookahead,
        page,
        show,
        frames,
        Some(reserved),
    )
}

#[allow(clippy::too_many_arguments)]
fn local_player_views_with_background_impl(
    scene: &mut Scene,
    players: &[LocalPlayerView<'_>],
    lookahead: i64,
    page: usize,
    show: bool,
    frames: &[crate::bga_render::BgaFrame; 4],
    reserved: Option<&[i64]>,
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
            || crate::pressed_keys::validate_mask(player.pressed_lanes).is_err()
        {
            return Err("invalid local presentation identities/result capacity".into());
        }
    }
    let count = visible.len();
    for frame in &frames[..count] {
        frame.validate()?;
    }
    for (slot, player) in players[visible.clone()].iter().enumerate() {
        if let Some(reserved) = reserved {
            let space = reserved[visible.start + slot];
            let field = crate::playfield_layout::local_field_bounds_with_comparison_space(
                count, slot, space,
            )?;
            let required = if show {
                player
                    .competition
                    .map(|snapshot| competition_height(snapshot, field[2]))
                    .transpose()?
                    .unwrap_or(0)
            } else {
                0
            };
            if required > space {
                return Err("comparison exceeds its fixed reservation".into());
            }
        }
        if let Some(score) = player.bms_score {
            score
                .validate_for(player.score.hits, player.score.misses)
                .map_err(|error| error.to_string())?;
        }
        let class_height = if player.bms_score.is_some() { 24 } else { 0 };
        let comparison_height = reserved
            .map_or_else(
                || {
                    if show {
                        player
                            .competition
                            .map(|snapshot| {
                                competition_height(snapshot, panel_bounds(slot, count).width - 20)
                            })
                            .transpose()
                    } else {
                        Ok(None)
                    }
                },
                |spaces| Ok(Some(spaces[visible.start + slot])),
            )?
            .unwrap_or(0);
        let field = crate::playfield_layout::local_field_bounds_with_comparison_space(
            count,
            slot,
            comparison_height,
        )?;
        if field[3] - class_height <= 16 {
            return Err("local class display leaves insufficient playfield height".into());
        }
        if let (Some(chart), Some(now)) = (player.chart, player.song_time) {
            if player
                .note_progress
                .is_some_and(|state| !state.matches_chart(chart))
            {
                return Err("local note progress belongs to another prepared chart".into());
            }
            crate::judge_feedback::project(chart, now, player.recent_results)?;
        }
    }
    for (index, player) in players[visible.clone()].iter().enumerate() {
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
            player.chart.map_or("LOADING", |chart| chart.title.as_str())
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
        if let Some(event) = player.last_judge.or_else(|| player.recent_results.last()) {
            let (label, color) = crate::timing_display::judge_label(event);
            let mut judge_bounds = line(56, 7);
            if player.gauge.is_some() {
                judge_bounds.width -= 100;
            }
            clipped_text(scene, judge_bounds, &label, 1, color);
        }
        if let Some(gauge) = player.gauge {
            molecules::gauge_hud(
                scene,
                gauge,
                Bounds {
                    x: bounds.x + bounds.width - 100,
                    y: bounds.y + 56,
                    width: 90,
                    height: 14,
                },
            )?;
        }
        let class_height = if let Some(score) = player.bms_score {
            bms_score_hud(scene, &score, line(72, 24));
            24
        } else {
            0
        };
        let comparisons = if show { player.competition } else { None };
        let summary_height = comparisons
            .map(|snapshot| competition_height(snapshot, bounds.width - 20))
            .transpose()?
            .unwrap_or(0);
        let summary_height =
            reserved.map_or(summary_height, |spaces| spaces[visible.start + index]);
        if let Some(snapshot) = comparisons {
            // A failed saved display clears its rows, but the admitted peer
            // remains below their immutable reservation.
            let peer_offset = reserved
                .filter(|_| snapshot.network.is_some())
                .map(|_| summary_height - 28);
            competition_summary_with_peer_offset(
                scene,
                snapshot,
                line(72 + class_height, summary_height),
                peer_offset,
            )?;
        }
        let field_offset = 72 + class_height + summary_height;
        let [field_x, field_y, field_width, field_height] =
            crate::playfield_layout::local_field_bounds(count, index)?;
        match (player.chart, player.song_time) {
            (Some(chart), Some(now)) => playfield_in_with_background(
                scene,
                chart,
                now,
                lookahead,
                Bounds {
                    x: field_x,
                    y: field_y + class_height + summary_height,
                    width: field_width,
                    height: field_height - class_height - summary_height,
                },
                player.recent_results,
                player.pressed_lanes,
                player.note_progress,
                frames[index],
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
/// Scalar-only two-row class display, clipped independently within each column.
fn bms_score_hud(
    scene: &mut Scene,
    score: &crate::judgment_policy::BmsScoreSummary,
    bounds: Bounds,
) {
    let values = [
        ("EX", score.ex_score),
        ("PG", score.pgreat),
        ("G", score.great),
        ("GOOD", score.good),
        ("BAD", score.bad),
        ("POOR", score.poor),
    ];
    for (index, (label, value)) in values.iter().enumerate() {
        let column = index % 3;
        let left = bounds.width * column as i64 / 3;
        let right = bounds.width * (column + 1) as i64 / 3;
        clipped_text(
            scene,
            Bounds {
                x: bounds.x + left,
                y: bounds.y + (index / 3) as i64 * 12,
                width: right - left,
                height: 7,
            },
            &format!("{label} {value}"),
            1,
            0x9bb1cf,
        );
    }
}

pub fn scoreboard(pixels: &mut Scene, score: &ScoreSummary, recent_results: &[JudgeEvent]) {
    scoreboard_with_bms_score(pixels, score, recent_results, None);
}

pub fn scoreboard_with_bms_score(
    pixels: &mut Scene,
    score: &ScoreSummary,
    recent_results: &[JudgeEvent],
    bms_score: Option<&crate::judgment_policy::BmsScoreSummary>,
) {
    let class_height = if let Some(bms_score) = bms_score {
        bms_score_hud(
            pixels,
            bms_score,
            Bounds {
                x: 750,
                y: 490,
                width: 186,
                height: 24,
            },
        );
        24
    } else {
        0
    };
    molecules::counter(pixels, 750, 145, "HITS", score.hits, 0x74e5c5);
    molecules::counter(pixels, 750, 235, "MISSES", score.misses, 0xff8e8e);
    molecules::counter(pixels, 750, 325, "COMBO", score.combo, 0x9bb1cf);
    molecules::counter(pixels, 750, 415, "MAX COMBO", score.max_combo, 0x9bb1cf);
    for (index, event) in recent_results.iter().rev().take(4).enumerate() {
        let (label, color) = crate::timing_display::judge_label(event);
        text(
            pixels,
            750,
            520 + class_height as usize + index * 22,
            &label,
            1,
            color,
        );
    }
    let (bias, absolute) = crate::timing_display::summary(&score.timing);
    clipped_text(
        pixels,
        Bounds {
            x: 750,
            y: 620 + class_height,
            width: 186,
            height: 7,
        },
        &bias,
        1,
        0x9bb1cf,
    );
    clipped_text(
        pixels,
        Bounds {
            x: 750,
            y: 634 + class_height,
            width: 186,
            height: 7,
        },
        &absolute,
        1,
        0x9bb1cf,
    );
}
/// Competition mode reserves the sidebar for every recorded/remote prefix.
pub fn competition_scoreboard(
    scene: &mut Scene,
    score: &ScoreSummary,
    snapshot: &CompetitionSnapshot,
) -> Result<(), String> {
    competition_scoreboard_with_bms_score(scene, score, snapshot, None)
}

pub fn competition_scoreboard_with_bms_score(
    scene: &mut Scene,
    score: &ScoreSummary,
    snapshot: &CompetitionSnapshot,
    bms_score: Option<&crate::judgment_policy::BmsScoreSummary>,
) -> Result<(), String> {
    let class_height = if let Some(bms_score) = bms_score {
        bms_score
            .validate_for(score.hits, score.misses)
            .map_err(|error| error.to_string())?;
        bms_score_hud(
            scene,
            bms_score,
            Bounds {
                x: 750,
                y: 280,
                width: 186,
                height: 24,
            },
        );
        24
    } else {
        0
    };
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

    let (bias, absolute) = crate::timing_display::summary(&score.timing);
    clipped_text(
        scene,
        Bounds {
            x: 750,
            y: 258,
            width: 186,
            height: 7,
        },
        &bias,
        1,
        0x9bb1cf,
    );
    clipped_text(
        scene,
        Bounds {
            x: 750,
            y: 268,
            width: 186,
            height: 7,
        },
        &absolute,
        1,
        0x9bb1cf,
    );
    competition_summary(
        scene,
        snapshot,
        Bounds {
            x: 750,
            y: 280 + class_height,
            width: 186,
            height: 360 - class_height,
        },
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn chart() -> PlayerChart {
        let source = beatkernel_bms::parse(
            "#BPM 120\n#TITLE A LONG TITLE FOR PLAYER PANELS\n#WAV01 tap.wav\n#WAV02 hold.wav\n#00012:0100\n#00051:0202\n",
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

    fn feedback_event(chart: &PlayerChart, miss: bool) -> JudgeEvent {
        use beatkernel::judge::{JudgeGrade, JudgeStage, MissReason};
        let note = chart.notes.iter().find(|note| note.end.is_none()).unwrap();
        JudgeEvent {
            object: note.object,
            stage: JudgeStage::Instant,
            at: Timestamp::ZERO,
            input: None,
            outcome: if miss {
                JudgeOutcome::Miss {
                    reason: MissReason::HeadTimeout,
                }
            } else {
                JudgeOutcome::Hit {
                    grade: JudgeGrade(1),
                    delta: beatkernel::time::Duration::ZERO,
                }
            },
        }
    }
    fn rgba(color: u32) -> [f32; 4] {
        [
            ((color >> 16) & 255) as f32 / 255.0,
            ((color >> 8) & 255) as f32 / 255.0,
            (color & 255) as f32 / 255.0,
            1.0,
        ]
    }

    #[test]
    fn feedback_strip_uses_actual_lane_fade_and_preserves_note_line_order() {
        let chart = chart();
        let event = feedback_event(&chart, false);
        let note = chart.note_by_object(event.object).unwrap();
        let (left, right) = partition_lane(note.lane_index, chart.lanes.len(), 80, 640);
        let mut scene = Scene::new(960, 720);
        playfield_with_feedback(&mut scene, &chart, Timestamp::ZERO, 1_000_000_000, &[event])
            .unwrap();
        let strip = scene
            .rectangles()
            .iter()
            .find(|rect| {
                rect.bounds
                    == [
                        (left + 1) as f32,
                        (LINE - 6) as f32,
                        (right - left - 3) as f32,
                        6.0,
                    ]
            })
            .unwrap();
        assert_eq!(strip.color, rgba(0x74e5c5));
        assert_eq!(
            scene.rectangles().last().unwrap().bounds,
            [80.0, LINE as f32, 640.0, 3.0]
        );
        let note_batch = scene
            .batches()
            .iter()
            .position(|batch| batch.playfield == Some(0))
            .unwrap();
        assert!(note_batch > 0);
        assert!(note_batch < scene.batches().len() - 1);
        let strip_index = scene
            .rectangles()
            .iter()
            .position(|rect| {
                rect.bounds
                    == [
                        (left + 1) as f32,
                        (LINE - 6) as f32,
                        (right - left - 3) as f32,
                        6.0,
                    ]
            })
            .unwrap();
        assert!(
            scene.batches()[note_batch + 1..]
                .iter()
                .any(|batch| batch.playfield.is_none()
                    && (batch.first as usize) <= strip_index
                    && strip_index < (batch.first + batch.count) as usize)
        );
        scene.clear();
        playfield_with_feedback(
            &mut scene,
            &chart,
            Timestamp::from_nanos(75_000_000),
            1_000_000_000,
            &[event],
        )
        .unwrap();
        let strip = scene
            .rectangles()
            .iter()
            .find(|rect| rect.bounds[1] == (LINE - 6) as f32 && rect.bounds[3] == 6.0)
            .unwrap();
        // This actual tap occupies odd lane1 (background18212c); exact integer half fade.
        assert_eq!(note.lane_index, 1);
        assert_eq!(strip.color, rgba(0x468378));
        scene.clear();
        playfield_with_feedback(
            &mut scene,
            &chart,
            Timestamp::from_nanos(crate::judge_feedback::FEEDBACK_LIFETIME_NS),
            1_000_000_000,
            &[event],
        )
        .unwrap();
        assert!(
            !scene
                .rectangles()
                .iter()
                .any(|rect| rect.bounds[1] == (LINE - 6) as f32 && rect.bounds[3] == 6.0)
        );
        scene.clear();
        playfield(&mut scene, &chart, Timestamp::ZERO, 1_000_000_000).unwrap();
        assert!(
            !scene
                .rectangles()
                .iter()
                .any(|rect| rect.bounds[1] == (LINE - 6) as f32 && rect.bounds[3] == 6.0)
        );
    }

    #[test]
    fn feedback_errors_precede_geometry_and_sparse_members_keep_own_results() {
        let chart = std::sync::Arc::new(chart());
        let hit = feedback_event(&chart, false);
        let miss = feedback_event(&chart, true);
        let mut scene = Scene::new(960, 720);
        scene.rect(0, 0, 1, 1, 0);
        let rectangles = scene.rectangles().len();
        let batches = scene.batches().len();
        assert!(
            playfield_with_feedback(
                &mut scene,
                &chart,
                Timestamp::ZERO,
                1_000_000_000,
                &vec![hit; 129]
            )
            .is_err()
        );
        assert_eq!(scene.rectangles().len(), rectangles);
        assert_eq!(scene.batches().len(), batches);
        assert!(scene.playfields().is_empty());
        let mut invalid = (*chart).clone();
        invalid
            .notes
            .iter_mut()
            .find(|note| note.object == hit.object)
            .unwrap()
            .lane_index = usize::MAX;
        assert!(
            playfield_with_feedback(&mut scene, &invalid, Timestamp::ZERO, 1_000_000_000, &[hit])
                .is_err()
        );
        assert_eq!(scene.rectangles().len(), rectangles);
        assert_eq!(scene.batches().len(), batches);
        let players: Vec<_> = [(3, hit), (u32::MAX, miss)]
            .into_iter()
            .map(|(id, event)| LocalPlayerSnapshot {
                mine_damage: Default::default(),
                gauge: Default::default(),
                bms_score: None,
                player: crate::local_players::PlayerId(id),
                chart: Some(std::sync::Arc::clone(&chart)),
                song_time: Some(Timestamp::ZERO),
                score: ScoreSummary::default(),
                last_judge: Some(event),
                recent_results: vec![event],
                competition: None,
                pressed_lanes: 0,
                note_progress: None,
            })
            .collect();
        scene.clear();
        local_players(&mut scene, &players, 1_000_000_000, 0).unwrap();
        let strips: Vec<_> = scene
            .rectangles()
            .iter()
            .filter(|rectangle| {
                rectangle.bounds[3] == 6.0
                    && [rgba(0x74e5c5), rgba(0xef6372)].contains(&rectangle.color)
            })
            .collect();
        assert_eq!(strips.len(), 2);
        assert_eq!(strips[0].color, rgba(0x74e5c5));
        assert_eq!(strips[1].color, rgba(0xef6372));
        assert!(inside(&strips[0].bounds, panel_bounds(0, 2)));
        assert!(inside(&strips[1].bounds, panel_bounds(1, 2)));
        assert_eq!(scene.playfields().len(), 2);
        let mut malformed = players.clone();
        malformed[1].recent_results = vec![miss; 129];
        scene.clear();
        assert!(local_players(&mut scene, &malformed, 1_000_000_000, 0).is_err());
        assert!(scene.rectangles().is_empty());
        assert!(scene.playfields().is_empty());
    }
    #[test]
    fn admitted_press_bands_map_scratch_channels_and_reject_masks_before_geometry() {
        let source = beatkernel_bms::parse(
            "#BPM 120\n#WAV01 tap.wav\n#00016:01\n#00011:01\n#00021:01\n",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        let chart = PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap();
        let mask = (1 << 5) | (1 << 9); // scratch 16 and second-side key 21
        let mut scene = Scene::new(960, 720);
        playfield_with_state(
            &mut scene,
            &chart,
            Timestamp::ZERO,
            1_000_000_000,
            &[],
            mask,
        )
        .unwrap();
        let bands: Vec<_> = scene
            .rectangles()
            .iter()
            .filter(|rect| rect.color == rgba(0x416ca0))
            .collect();
        assert_eq!(bands.len(), 2);
        for channel in [0x16, 0x21] {
            let lane = chart
                .lanes
                .iter()
                .position(|&candidate| candidate == channel)
                .unwrap();
            let (left, right) = lane_bounds(lane, chart.lanes.len());
            assert!(bands.iter().any(|band| band.bounds
                == [
                    (left + 1) as f32,
                    (LINE - 22) as f32,
                    (right - left - 3) as f32,
                    16.0
                ]));
        }
        assert_eq!(scene.rectangles().last().unwrap().color, rgba(0xffffff));
        scene.clear();
        assert!(
            playfield_with_state(
                &mut scene,
                &chart,
                Timestamp::ZERO,
                1_000_000_000,
                &[],
                1 << 18
            )
            .is_err()
        );
        assert!(scene.rectangles().is_empty());
        assert!(scene.playfields().is_empty());
        playfield(&mut scene, &chart, Timestamp::ZERO, 1_000_000_000).unwrap();
        assert!(
            scene
                .rectangles()
                .iter()
                .all(|rect| rect.color != rgba(0x416ca0))
        );
        let chart = std::sync::Arc::new(chart);
        let players: Vec<_> = [(3, 1 << 5), (u32::MAX, 1 << 9)]
            .into_iter()
            .map(|(id, pressed_lanes)| LocalPlayerSnapshot {
                mine_damage: Default::default(),
                gauge: Default::default(),
                bms_score: None,
                player: crate::local_players::PlayerId(id),
                chart: Some(std::sync::Arc::clone(&chart)),
                song_time: Some(Timestamp::ZERO),
                score: Default::default(),
                last_judge: None,
                recent_results: vec![],
                competition: None,
                pressed_lanes,
                note_progress: None,
            })
            .collect();
        scene.clear();
        local_players(&mut scene, &players, 1_000_000_000, 0).unwrap();
        let bands: Vec<_> = scene
            .rectangles()
            .iter()
            .filter(|rect| rect.color == rgba(0x416ca0))
            .collect();
        assert_eq!(bands.len(), 2);
        assert!(inside(&bands[0].bounds, panel_bounds(0, 2)));
        assert!(inside(&bands[1].bounds, panel_bounds(1, 2)));
        let mut invalid = players;
        invalid[1].pressed_lanes = 1 << 31;
        scene.clear();
        assert!(local_players(&mut scene, &invalid, 1_000_000_000, 0).is_err());
        assert!(scene.rectangles().is_empty());
    }
    #[test]
    fn authoritative_progress_hides_completed_notes_and_keeps_active_hold_body_tail() {
        use crate::note_progress::{NoteProgress, NoteState};
        use beatkernel::judge::JudgeStage;
        let chart = std::sync::Arc::new(chart());
        let instant = feedback_event(&chart, false);
        let hold = chart.notes.iter().find(|note| note.end.is_some()).unwrap();
        let head = JudgeEvent {
            object: hold.object,
            stage: JudgeStage::HoldHead,
            ..instant
        };
        let tail = JudgeEvent {
            stage: JudgeStage::HoldTail,
            ..head
        };
        let mut progress = NoteProgress::new(std::sync::Arc::clone(&chart)).unwrap();
        let pending = progress.clone();
        let mut scene = Scene::new(960, 720);
        playfield_with_progress(
            &mut scene,
            &chart,
            Timestamp::ZERO,
            1_000_000_000,
            &[],
            0,
            Some(&progress),
        )
        .unwrap();
        assert_eq!(scene.playfields()[0].instances.len(), 4);
        let untouched = std::sync::Arc::clone(&scene.playfields()[0].instances);
        progress.apply(&[head]);
        scene.clear();
        playfield_with_progress(
            &mut scene,
            &chart,
            Timestamp::ZERO,
            1_000_000_000,
            &[],
            0,
            Some(&progress),
        )
        .unwrap();
        assert_eq!(scene.playfields()[0].instances.len(), 3);
        assert!(!std::sync::Arc::ptr_eq(
            &untouched,
            &scene.playfields()[0].instances
        )); // Same note membership, new head flag.
        progress.apply(&[instant, head]);
        assert_eq!(
            progress.state(chart.note_index_by_object(hold.object).unwrap()),
            Some(NoteState::Holding)
        );
        scene.clear();
        playfield_with_progress(
            &mut scene,
            &chart,
            Timestamp::ZERO,
            1_000_000_000,
            &[instant, head],
            0,
            Some(&progress),
        )
        .unwrap();
        let held_instances = std::sync::Arc::clone(&scene.playfields()[0].instances);
        assert_eq!(held_instances.len(), 2);
        assert_eq!(held_instances[0].appearance[0], 0.0); // body
        assert_eq!(held_instances[1].appearance[0], 1.0); // tail
        scene.clear();
        playfield_with_progress(
            &mut scene,
            &chart,
            Timestamp::from_nanos(1),
            1_000_000_000,
            &[],
            0,
            Some(&progress),
        )
        .unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &held_instances,
            &scene.playfields()[0].instances
        ));
        progress.apply(&[tail]);
        scene.clear();
        playfield_with_progress(
            &mut scene,
            &chart,
            Timestamp::from_nanos(1),
            1_000_000_000,
            &[],
            0,
            Some(&progress),
        )
        .unwrap();
        assert!(scene.playfields()[0].instances.is_empty());
        scene.clear();
        playfield_with_progress(
            &mut scene,
            &chart,
            Timestamp::ZERO,
            1_000_000_000,
            &[],
            0,
            Some(&pending),
        )
        .unwrap();
        assert_eq!(scene.playfields()[0].instances.len(), 4); // Fresh/reconstructed old prefix.
        let foreign = std::sync::Arc::new((*chart).clone());
        scene.clear();
        assert!(
            playfield_with_progress(
                &mut scene,
                &foreign,
                Timestamp::ZERO,
                1_000_000_000,
                &[],
                0,
                Some(&progress)
            )
            .is_err()
        );
        assert!(scene.rectangles().is_empty());
        assert!(scene.playfields().is_empty());
        let mut players: Vec<_> = [(3, progress.clone()), (u32::MAX, pending)]
            .into_iter()
            .map(|(id, note_progress)| LocalPlayerSnapshot {
                mine_damage: Default::default(),
                gauge: Default::default(),
                bms_score: None,
                player: crate::local_players::PlayerId(id),
                chart: Some(std::sync::Arc::clone(&chart)),
                song_time: Some(Timestamp::ZERO),
                score: Default::default(),
                last_judge: None,
                recent_results: vec![],
                competition: None,
                pressed_lanes: 0,
                note_progress: Some(note_progress),
            })
            .collect();
        local_players(&mut scene, &players, 1_000_000_000, 0).unwrap();
        assert!(scene.playfields()[0].instances.is_empty());
        assert_eq!(scene.playfields()[1].instances.len(), 4);
        players[1].chart = Some(foreign);
        scene.clear();
        assert!(local_players(&mut scene, &players, 1_000_000_000, 0).is_err());
        assert!(scene.rectangles().is_empty());
    }
    #[test]
    fn backgrounds_precede_notes_and_follow_current_visible_sparse_member_slots() {
        use crate::bga_render::{BgaFrame, BgaSprite};
        use crate::texture::TextureId;
        let chart = std::sync::Arc::new(chart());
        let frames: [BgaFrame; 4] = std::array::from_fn(|_| BgaFrame {
            active: true,
            base: Some(BgaSprite {
                texture: TextureId::allocate().unwrap(),
                width: 2,
                height: 1,
            }),
            layer: Some(BgaSprite {
                texture: TextureId::allocate().unwrap(),
                width: 1,
                height: 2,
            }),
            poor_overlay: Some(BgaSprite {
                texture: TextureId::allocate().unwrap(),
                width: 1,
                height: 1,
            }),
            layer2: Some(BgaSprite {
                texture: TextureId::allocate().unwrap(),
                width: 3,
                height: 2,
            }),
            opacity: crate::bga_opacity::BgaOpacity {
                base: 255,
                layer: 128,
                layer2: 64,
                poor: 32,
            },
            unavailable: 0,
        });
        let mut scene = Scene::new(960, 720);
        playfield_with_background(
            &mut scene,
            &chart,
            Timestamp::ZERO,
            1_000_000_000,
            &[],
            0,
            None,
            frames[0],
        )
        .unwrap();
        let image = scene
            .batches()
            .iter()
            .position(|batch| batch.texture == frames[0].base.unwrap().texture)
            .unwrap();
        let notes = scene
            .batches()
            .iter()
            .position(|batch| batch.playfield.is_some())
            .unwrap();
        assert!(image < notes);
        let layer = scene
            .batches()
            .iter()
            .position(|batch| batch.texture == frames[0].layer.unwrap().texture)
            .unwrap();
        let poor = scene
            .batches()
            .iter()
            .position(|batch| batch.texture == frames[0].poor_overlay.unwrap().texture)
            .unwrap();
        let layer2 = scene
            .batches()
            .iter()
            .position(|batch| batch.texture == frames[0].layer2.unwrap().texture)
            .unwrap();
        assert!(image < layer && layer < layer2 && layer2 < poor && poor < notes);
        for (batch_index, alpha) in [(image, 255u8), (layer, 128), (layer2, 64), (poor, 32)] {
            assert_eq!(
                scene.rectangles()[scene.batches()[batch_index].first as usize].color[3],
                f32::from(alpha) / 255.0
            );
        }
        let image_rectangle = &scene.rectangles()[scene.batches()[image].first as usize];
        assert!(image_rectangle.bounds[1] >= TOP as f32);
        assert!(image_rectangle.bounds[1] + image_rectangle.bounds[3] <= LINE as f32);
        let players: Vec<_> = (0..64)
            .map(|index| LocalPlayerSnapshot {
                mine_damage: Default::default(),
                gauge: Default::default(),
                bms_score: None,
                player: crate::local_players::PlayerId(if index == 63 {
                    u32::MAX
                } else {
                    index * 3 + 1
                }),
                chart: Some(chart.clone()),
                song_time: Some(Timestamp::ZERO),
                score: Default::default(),
                last_judge: None,
                recent_results: vec![],
                competition: None,
                pressed_lanes: 0,
                note_progress: None,
            })
            .collect();
        scene.clear();
        local_players_with_background(&mut scene, &players, 1_000_000_000, 15, false, &frames)
            .unwrap();
        assert_eq!(scene.playfields().len(), 4);
        for frame in frames {
            assert_eq!(
                scene
                    .batches()
                    .iter()
                    .filter(|batch| batch.texture == frame.base.unwrap().texture)
                    .count(),
                1
            );
            for sprite in [frame.layer, frame.layer2, frame.poor_overlay]
                .into_iter()
                .flatten()
            {
                assert_eq!(
                    scene
                        .batches()
                        .iter()
                        .filter(|batch| batch.texture == sprite.texture)
                        .count(),
                    1
                );
            }
        }
        let mut invalid = frames;
        invalid[3].base.as_mut().unwrap().width = 0;
        scene.clear();
        assert!(
            local_players_with_background(&mut scene, &players, 1_000_000_000, 15, false, &invalid)
                .is_err()
        );
        assert!(scene.rectangles().is_empty());
    }

    #[test]
    fn accepted_timing_sidebar_stays_inside_normal_and_competition_bounds() {
        let chart = chart();
        let mut event = feedback_event(&chart, false);
        let JudgeOutcome::Hit { grade, .. } = event.outcome else {
            unreachable!()
        };
        event.outcome = JudgeOutcome::Hit {
            grade,
            delta: beatkernel::time::Duration::from_nanos(-1_234_567),
        };
        let mut score = ScoreSummary::default();
        score.observe(&[event]).unwrap();
        let mut scene = Scene::new(960, 720);
        scoreboard(&mut scene, &score, &[event]);
        let summary: Vec<_> = scene
            .rectangles()
            .iter()
            .filter(|rect| rect.bounds[1] >= 620.0)
            .collect();
        assert!(!summary.is_empty());
        assert!(summary.iter().all(|rect| inside(
            &rect.bounds,
            Bounds {
                x: 750,
                y: 620,
                width: 186,
                height: 21
            }
        )));
        assert!(
            scene
                .rectangles()
                .iter()
                .any(|rect| rect.color == rgba(0x87bfff))
        );
        scene.clear();
        competition_scoreboard(
            &mut scene,
            &score,
            &comparisons(NetworkStatus::Connected, 0),
        )
        .unwrap();
        let summary: Vec<_> = scene
            .rectangles()
            .iter()
            .filter(|rect| rect.bounds[1] >= 258.0 && rect.bounds[1] < 280.0)
            .collect();
        assert!(!summary.is_empty());
        assert!(summary.iter().all(|rect| inside(
            &rect.bounds,
            Bounds {
                x: 750,
                y: 258,
                width: 186,
                height: 19
            }
        )));
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
                mine_damage: Default::default(),
                gauge: Default::default(),
                bms_score: None,
                last_judge: None,
                recent_results: Vec::new(),
                competition: None,
                pressed_lanes: 0,
                note_progress: None,
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
                mine_damage: Default::default(),
                gauge: Default::default(),
                bms_score: None,
                last_judge: None,
                recent_results: Vec::new(),
                competition: Some(comparisons(NetworkStatus::Connected, i64::MIN)),
                pressed_lanes: 0,
                note_progress: None,
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

#[cfg(test)]
#[path = "live_class_hud_fixtures.rs"]
mod live_class_hud_fixtures;
