//! Pure bounded presentation of actual local results; no gameplay ownership.
use crate::player_chart::PlayerChart;
use beatkernel::{
    judge::{JudgeEvent, JudgeStage},
    time::Timestamp,
};
/// Maximum supported displayed lane count.
pub const MAX_FEEDBACK_LANES: usize = 18;
/// Exclusive lifetime in actual reported song nanoseconds.
pub const FEEDBACK_LIFETIME_NS: i64 = 150_000_000;
/// The actual event and its bounded age, preserving stage/outcome/provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaneFeedback {
    /// Original event including effective song time and input provenance.
    pub event: JudgeEvent,
    /// Nonnegative age strictly below the feedback lifetime.
    pub age_ns: i64,
}
/// Project recent results through the prepared object index. Successful calls
/// use fixed stack storage; future/expired/unknown/mismatched events do not flash.
/// Newest event wins per lane; later slice order wins equal-time ties.
pub fn project(
    chart: &PlayerChart,
    now: Timestamp,
    recent: &[JudgeEvent],
) -> Result<[Option<LaneFeedback>; MAX_FEEDBACK_LANES], String> {
    if recent.len() > 128 || chart.lanes.len() > MAX_FEEDBACK_LANES {
        return Err("judge feedback exceeds result or lane capacity".into());
    }
    let mut lanes: [Option<LaneFeedback>; MAX_FEEDBACK_LANES] = [None; MAX_FEEDBACK_LANES];
    for event in recent {
        let Some(note) = chart.note_by_object(event.object) else {
            continue;
        };
        if note.lane_index >= chart.lanes.len() {
            return Err("judge feedback references an unavailable lane".into());
        }
        let matching = match event.stage {
            JudgeStage::Instant => note.end.is_none(),
            JudgeStage::HoldHead | JudgeStage::HoldTail => note.end.is_some(),
            JudgeStage::Custom(_) => false,
        };
        if !matching {
            continue;
        }
        let age = i128::from(now.as_nanos()) - i128::from(event.at.as_nanos());
        if age < 0 || age >= i128::from(FEEDBACK_LIFETIME_NS) {
            continue;
        }
        let slot = &mut lanes[note.lane_index];
        if slot.is_none_or(|old| event.at >= old.event.at) {
            *slot = Some(LaneFeedback {
                event: *event,
                age_ns: age as i64,
            });
        }
    }
    Ok(lanes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatkernel::{
        chart::ObjectId,
        judge::{JudgeGrade, JudgeOutcome, MissReason},
        time::Duration,
    };
    fn chart() -> PlayerChart {
        let source = beatkernel_bms::parse(
            "#BPM 60\n#WAV01 head.wav\n#00051:0101\n#00016:01\n#00021:01",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap()
    }
    fn hit(object: ObjectId, at: i64, stage: JudgeStage) -> JudgeEvent {
        JudgeEvent {
            object,
            at: Timestamp::from_nanos(at),
            stage,
            outcome: JudgeOutcome::Hit {
                grade: JudgeGrade(1),
                delta: Duration::ZERO,
            },
            input: None,
        }
    }
    #[test]
    fn compiled_object_index_preserves_scratch_double_and_hold_stage_mapping() {
        let chart = chart();
        assert_eq!(chart.lanes, vec![0x16, 0x11, 0x21]);
        for note in &chart.notes {
            assert_eq!(chart.note_by_object(note.object), Some(note));
            let stages = if note.end.is_some() {
                vec![JudgeStage::HoldHead, JudgeStage::HoldTail]
            } else {
                vec![JudgeStage::Instant]
            };
            for stage in stages {
                let event = hit(note.object, 0, stage);
                let feedback = project(&chart, Timestamp::ZERO, &[event]).unwrap();
                assert_eq!(
                    feedback[note.lane_index],
                    Some(LaneFeedback { event, age_ns: 0 })
                );
                assert_eq!(feedback.iter().flatten().count(), 1);
            }
        }
        assert!(chart.note_by_object(ObjectId(u64::MAX)).is_none());
        let first = chart.notes[0].object;
        let mut stale = chart.clone();
        stale.notes.clear();
        assert!(stale.note_by_object(first).is_none());
        let mut stale = chart.clone();
        stale.notes[0].object = ObjectId(u64::MAX);
        assert!(stale.note_by_object(first).is_none());
    }
    #[test]
    fn exact_lifetime_future_extremes_pause_and_seek_use_only_song_time() {
        let chart = chart();
        let note = chart.notes.iter().find(|note| note.end.is_none()).unwrap();
        let event = hit(note.object, 0, JudgeStage::Instant);
        for age in [0, 1, FEEDBACK_LIFETIME_NS - 1] {
            let now = Timestamp::from_nanos(age);
            let first = project(&chart, now, &[event]).unwrap();
            assert_eq!(first[note.lane_index].unwrap().age_ns, age);
            assert_eq!(project(&chart, now, &[event]).unwrap(), first); // paused exact time
        }
        for now in [-1, FEEDBACK_LIFETIME_NS, i64::MAX] {
            assert!(
                project(&chart, Timestamp::from_nanos(now), &[event])
                    .unwrap()
                    .iter()
                    .all(Option::is_none)
            );
        }
        let minimum = hit(note.object, i64::MIN, JudgeStage::Instant);
        assert_eq!(
            project(&chart, Timestamp::from_nanos(i64::MIN + 1), &[minimum]).unwrap()
                [note.lane_index]
                .unwrap()
                .age_ns,
            1
        );
        assert!(
            project(&chart, Timestamp::MAX, &[minimum])
                .unwrap()
                .iter()
                .all(Option::is_none)
        );
        let maximum = hit(note.object, i64::MAX, JudgeStage::Instant);
        assert!(
            project(&chart, Timestamp::MIN, &[maximum])
                .unwrap()
                .iter()
                .all(Option::is_none)
        );
        assert_eq!(
            project(&chart, Timestamp::MAX, &[maximum]).unwrap()[note.lane_index]
                .unwrap()
                .age_ns,
            0
        );
    }
    #[test]
    fn newest_event_wins_independent_order_and_equal_time_uses_last_slice_event() {
        let chart = chart();
        let note = chart.notes.iter().find(|note| note.end.is_none()).unwrap();
        let old = hit(note.object, 4, JudgeStage::Instant);
        let mut newest = hit(note.object, 5, JudgeStage::Instant);
        newest.outcome = JudgeOutcome::Miss {
            reason: MissReason::HeadTimeout,
        };
        assert_eq!(
            project(&chart, Timestamp::from_nanos(5), &[newest, old]).unwrap()[note.lane_index]
                .unwrap()
                .event,
            newest
        );
        let tie = hit(note.object, 5, JudgeStage::Instant);
        assert_eq!(
            project(&chart, Timestamp::from_nanos(5), &[newest, tie]).unwrap()[note.lane_index]
                .unwrap()
                .event,
            tie
        );
        assert_eq!(
            project(&chart, Timestamp::from_nanos(4), &[old, newest, tie]).unwrap()
                [note.lane_index]
                .unwrap()
                .event,
            old
        );
    }
    #[test]
    fn unknown_custom_mismatched_shapes_and_caps_never_fabricate_feedback() {
        let chart = chart();
        let instant = chart.notes.iter().find(|note| note.end.is_none()).unwrap();
        let hold = chart.notes.iter().find(|note| note.end.is_some()).unwrap();
        let ignored = [
            hit(ObjectId(u64::MAX), 0, JudgeStage::Instant),
            hit(instant.object, 0, JudgeStage::HoldHead),
            hit(instant.object, 0, JudgeStage::HoldTail),
            hit(hold.object, 0, JudgeStage::Instant),
            hit(hold.object, 0, JudgeStage::Custom(1)),
        ];
        assert!(
            project(&chart, Timestamp::ZERO, &ignored)
                .unwrap()
                .iter()
                .all(Option::is_none)
        );
        assert!(
            project(
                &chart,
                Timestamp::ZERO,
                &vec![hit(instant.object, 0, JudgeStage::Instant); 129]
            )
            .is_err()
        );
        let mut lanes = chart.clone();
        lanes.lanes.resize(MAX_FEEDBACK_LANES + 1, 0x11);
        assert!(project(&lanes, Timestamp::ZERO, &[]).is_err());
        let mut invalid = chart.clone();
        let object = invalid.notes[0].object;
        invalid.notes[0].lane_index = MAX_FEEDBACK_LANES;
        assert!(
            project(
                &invalid,
                Timestamp::ZERO,
                &[hit(object, 0, JudgeStage::HoldHead)]
            )
            .is_err()
        );
    }
}
