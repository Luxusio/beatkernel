//! Prepared image selections on the original song clock, independent of playback cursors.
use beatkernel::{chart::MAX_SOURCE_ITEMS, time::Timestamp};
use beatkernel_bms::{BgaChannel, BmsChart, ImageId, ScheduledBga};

/// Selected image references; a missing definition remains an explicit reference.
/// Poor is a selection only: the renderer decides when a miss overlay is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BgaState {
    /// Base channel04 selection.
    pub base: Option<ImageId>,
    /// Layer channel07 selection.
    pub layer: Option<ImageId>,
    /// Initial BMP00 or last channel06 selection.
    pub poor: Option<ImageId>,
}

/// Immutable per-channel indexes prepared before play; queries allocate nothing.
#[derive(Clone, Debug, Default)]
pub struct BgaTimeline {
    channels: [Vec<ScheduledBga>; 3],
    initial_poor: Option<ImageId>,
}

fn channel_index(channel: BgaChannel) -> usize {
    match channel {
        BgaChannel::Base => 0,
        BgaChannel::Layer => 1,
        BgaChannel::Poor => 2,
    }
}

fn check_limit(count: usize) -> Result<(), String> {
    if count > MAX_SOURCE_ITEMS {
        Err("BGA event capacity exceeded".into())
    } else {
        Ok(())
    }
}

impl BgaTimeline {
    /// Accepts chronological events ordered by timestamp and unique acquisition
    /// ordinal at equal timestamps. No partially prepared timeline is returned.
    pub fn new(events: Vec<ScheduledBga>, initial_poor: Option<ImageId>) -> Result<Self, String> {
        check_limit(events.len())?;
        let mut counts = [0usize; 3];
        let mut previous = None;
        for event in &events {
            let key = (event.at, event.ordinal);
            if event.at < Timestamp::ZERO || previous.is_some_and(|old| old >= key) {
                return Err("BGA events must have nonnegative, ordered song timestamps".into());
            }
            counts[channel_index(event.channel)] += 1;
            previous = Some(key);
        }
        let mut channels: [Vec<ScheduledBga>; 3] = std::array::from_fn(|_| Vec::new());
        for (channel, count) in channels.iter_mut().zip(counts) {
            channel
                .try_reserve_exact(count)
                .map_err(|e| e.to_string())?;
        }
        for event in events {
            channels[channel_index(event.channel)].push(event);
        }
        Ok(Self {
            channels,
            initial_poor,
        })
    }

    /// Compiles visual positions with core timing without opening image paths.
    pub fn from_chart(chart: &BmsChart) -> Result<Self, String> {
        Self::new(
            chart.compile_bga().map_err(|e| e.to_string())?,
            chart.images.contains_key(&ImageId(0)).then_some(ImageId(0)),
        )
    }

    /// Projects the exact original-song prefix, including events at `now`.
    /// Backward seeks and pauses need no reset or accumulated elapsed time.
    pub fn state_at(&self, now: Timestamp) -> BgaState {
        let last = |channel: usize| {
            let events = &self.channels[channel];
            let end = events.partition_point(|event| event.at <= now);
            end.checked_sub(1).map(|index| events[index].image)
        };
        BgaState {
            base: last(0),
            layer: last(1),
            poor: last(2).or(self.initial_poor),
        }
    }

    /// Number of prepared visual markers, independent of resource definitions.
    pub fn len(&self) -> usize {
        self.channels.iter().map(Vec::len).sum()
    }

    /// Whether there are no timed visual selections (BMP00 may still exist).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(ns: i64, ordinal: u64, channel: BgaChannel, id: u16) -> ScheduledBga {
        ScheduledBga {
            at: Timestamp::from_nanos(ns),
            ordinal,
            channel,
            image: ImageId(id),
        }
    }

    #[test]
    fn independent_channels_equal_time_last_ordinal_pause_and_backward_seek() {
        let timeline = BgaTimeline::new(
            vec![
                event(0, 0, BgaChannel::Base, 1),
                event(10, 1, BgaChannel::Layer, 2),
                event(10, 2, BgaChannel::Base, 3),
                event(10, 3, BgaChannel::Base, 4),
                event(20, 4, BgaChannel::Poor, 1295),
                event(i64::MAX, 5, BgaChannel::Layer, 9),
            ],
            Some(ImageId(0)),
        )
        .unwrap();
        let pristine = BgaState {
            poor: Some(ImageId(0)),
            ..BgaState::default()
        };
        assert_eq!(timeline.state_at(Timestamp::from_nanos(i64::MIN)), pristine);
        assert_eq!(timeline.state_at(Timestamp::from_nanos(-1)), pristine);
        assert_eq!(timeline.state_at(Timestamp::ZERO).base, Some(ImageId(1)));
        assert_eq!(timeline.state_at(Timestamp::from_nanos(9)).layer, None);
        let at_ten = BgaState {
            base: Some(ImageId(4)),
            layer: Some(ImageId(2)),
            poor: Some(ImageId(0)),
        };
        assert_eq!(timeline.state_at(Timestamp::from_nanos(10)), at_ten);
        assert_eq!(timeline.state_at(Timestamp::from_nanos(10)), at_ten); // Paused.
        assert_eq!(
            timeline.state_at(Timestamp::from_nanos(20)).poor,
            Some(ImageId(1295))
        );
        assert_eq!(
            timeline.state_at(Timestamp::from_nanos(i64::MAX)).layer,
            Some(ImageId(9))
        );
        assert_eq!(timeline.state_at(Timestamp::from_nanos(10)), at_ten); // Seek backwards.
        assert_eq!(timeline.len(), 6);
    }

    #[test]
    fn rejection_and_empty_pristine_state_do_not_require_resources() {
        assert!(BgaTimeline::new(vec![event(-1, 0, BgaChannel::Base, 1)], None).is_err());
        assert!(
            BgaTimeline::new(
                vec![
                    event(2, 0, BgaChannel::Base, 1),
                    event(1, 1, BgaChannel::Layer, 1)
                ],
                None
            )
            .is_err()
        );
        assert!(
            BgaTimeline::new(
                vec![
                    event(1, 0, BgaChannel::Base, 1),
                    event(1, 0, BgaChannel::Layer, 1)
                ],
                None
            )
            .is_err()
        );
        assert!(check_limit(MAX_SOURCE_ITEMS).is_ok());
        assert!(check_limit(MAX_SOURCE_ITEMS + 1).is_err());
        let empty = BgaTimeline::default();
        assert!(empty.is_empty());
        assert_eq!(
            empty.state_at(Timestamp::from_nanos(i64::MAX)),
            BgaState::default()
        );
    }

    #[test]
    fn parsed_zero_rests_preserve_selections_and_undefined_resource_identity() {
        let source = beatkernel_bms::parse(
            "#BPM 120\n#BMP00 poor.png\n#BMP01 背景.PNG\n#00004:0100\n#00104:00ZZ\n#00106:02\n#00107:03\n",
            beatkernel_bms::ParseOptions::default(),
        ).unwrap();
        let timeline = BgaTimeline::from_chart(&source).unwrap();
        assert_eq!(
            timeline.state_at(Timestamp::from_nanos(1_999_999_999)).base,
            Some(ImageId(1))
        );
        let at_two = timeline.state_at(Timestamp::from_nanos(2_000_000_000));
        assert_eq!(
            at_two,
            BgaState {
                base: Some(ImageId(1)),
                poor: Some(ImageId(2)),
                layer: Some(ImageId(3))
            }
        );
        assert_eq!(
            timeline.state_at(Timestamp::from_nanos(3_000_000_000)).base,
            Some(ImageId(1295))
        );
        assert!(!source.images.contains_key(&ImageId(1295)));
    }
}
