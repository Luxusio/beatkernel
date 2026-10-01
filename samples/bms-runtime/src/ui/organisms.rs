//! Composed view components. Inputs are portable state; no I/O or clock ownership.
use super::{
    atoms::{rect, text},
    molecules,
};
use crate::{
    competition::ScoreSummary,
    player_chart::{PlayerChart, MAX_VISIBLE_NOTES},
    scene::Scene,
};
use beatkernel::{
    judge::{JudgeEvent, JudgeOutcome},
    time::Timestamp,
};
const TOP: i64 = 110;
const LINE: i64 = 610;
fn note_y(time: Timestamp, now: Timestamp, lookahead: i64) -> i64 {
    let delta = i128::from(time.as_nanos()) - i128::from(now.as_nanos());
    (i128::from(LINE) - delta * i128::from(LINE - TOP) / i128::from(lookahead))
        .clamp(-10_000, 10_000) as i64
}
fn lane_bounds(index: usize, lanes: usize) -> (i64, i64) {
    let left = 80 + (index * 640 / lanes.max(1)) as i64;
    let right = 80 + ((index + 1) * 640 / lanes.max(1)) as i64;
    (left, right)
}

pub fn playfield(
    pixels: &mut Scene,
    chart: &PlayerChart,
    now: Timestamp,
    lookahead: i64,
) -> Result<(), String> {
    if lookahead <= 0 {
        return Err("playfield lookahead must be positive".into());
    }
    let lanes = chart.lanes.len();
    for lane in 0..lanes {
        let (left, right) = lane_bounds(lane, lanes);
        rect(
            pixels,
            left,
            TOP,
            right - left - 1,
            LINE - TOP + 24,
            if lane % 2 == 0 { 0x1d2734 } else { 0x18212c },
        );
        text(
            pixels,
            left.max(0) as usize + 3,
            620,
            &(lane + 1).to_string(),
            1,
            0xa9bdd5,
        );
    }
    for note in chart.visible_notes(now, lookahead, 150_000_000, MAX_VISIBLE_NOTES) {
        let (left, right) = lane_bounds(note.lane_index, lanes);
        let head = note_y(note.start, now, lookahead);
        let tail = note.end.map(|end| note_y(end, now, lookahead));
        molecules::note(pixels, (left, right), head, tail, TOP..=LINE + 15);
    }
    rect(pixels, 80, LINE, 640, 3, 0xffffff);
    Ok(())
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
#[cfg(test)]
mod tests {
    use super::*;
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
