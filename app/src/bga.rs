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
    /// Layer2 channel0A selection.
    pub layer2: Option<ImageId>,
    /// Initial BMP00/BGA00 or last channel06 selection.
    pub poor: Option<ImageId>,
}

/// Exact marker identity used to restart a movie on the original song clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BgaActivation {
    pub channel: BgaChannel,
    pub image: ImageId,
    pub activated_at: Timestamp,
    /// Initial Poor is a selection without a timed marker.
    pub ordinal: Option<u64>,
}

/// Immutable per-channel indexes prepared before play; queries allocate nothing.
#[derive(Clone, Debug, Default)]
pub struct BgaTimeline {
    channels: [Vec<ScheduledBga>; 4],
    initial_poor: Option<ImageId>,
}

pub(crate) fn channel_index(channel: BgaChannel) -> usize {
    match channel {
        BgaChannel::Base => 0,
        BgaChannel::Layer => 1,
        BgaChannel::Poor => 2,
        BgaChannel::Layer2 => 3,
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
        let mut counts = [0usize; 4];
        let mut previous = None;
        for event in &events {
            let key = (event.at, event.ordinal);
            if event.at < Timestamp::ZERO || previous.is_some_and(|old| old >= key) {
                return Err("BGA events must have nonnegative, ordered song timestamps".into());
            }
            counts[channel_index(event.channel)] += 1;
            previous = Some(key);
        }
        let mut channels: [Vec<ScheduledBga>; 4] = std::array::from_fn(|_| Vec::new());
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

    /// Restores acquisition order for immutable visual registration.
    pub fn export_events(&self) -> Vec<ScheduledBga> {
        let mut events: Vec<_> = self.channels.iter().flatten().copied().collect();
        events.sort_unstable_by_key(|event| (event.at, event.ordinal));
        events
    }

    /// Initial BMP00/BGA00 selection, independent of timed markers.
    pub fn initial_poor(&self) -> Option<ImageId> {
        self.initial_poor
    }

    /// Compiles visual positions with core timing without opening image paths.
    pub fn from_chart(chart: &BmsChart) -> Result<Self, String> {
        Self::new(
            chart.compile_bga().map_err(|e| e.to_string())?,
            (chart.images.contains_key(&ImageId(0)) || chart.bga_crops.contains_key(&ImageId(0)))
                .then_some(ImageId(0)),
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
            layer2: last(3),
            poor: last(2).or(self.initial_poor),
        }
    }

    /// Exact activations in Base, Layer, Poor, Layer2 order. Repeated image
    /// references retain their distinct timestamps and acquisition ordinals.
    pub fn activations_at(&self, now: Timestamp) -> [Option<BgaActivation>; 4] {
        let kinds = [
            BgaChannel::Base,
            BgaChannel::Layer,
            BgaChannel::Poor,
            BgaChannel::Layer2,
        ];
        std::array::from_fn(|channel| {
            let events = &self.channels[channel];
            let end = events.partition_point(|event| event.at <= now);
            end.checked_sub(1)
                .map(|index| {
                    let event = events[index];
                    BgaActivation {
                        channel: event.channel,
                        image: event.image,
                        activated_at: event.at,
                        ordinal: Some(event.ordinal),
                    }
                })
                .or_else(|| {
                    (channel == 2)
                        .then_some(self.initial_poor)
                        .flatten()
                        .map(|image| BgaActivation {
                            channel: kinds[channel],
                            image,
                            activated_at: Timestamp::ZERO,
                            ordinal: None,
                        })
                })
        })
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
            layer2: None,
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
        assert!(BgaTimeline::new(
            vec![
                event(2, 0, BgaChannel::Base, 1),
                event(1, 1, BgaChannel::Layer, 1)
            ],
            None
        )
        .is_err());
        assert!(BgaTimeline::new(
            vec![
                event(1, 0, BgaChannel::Base, 1),
                event(1, 0, BgaChannel::Layer, 1)
            ],
            None
        )
        .is_err());
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
                layer: Some(ImageId(3)),
                layer2: None
            }
        );
        assert_eq!(
            timeline.state_at(Timestamp::from_nanos(3_000_000_000)).base,
            Some(ImageId(1295))
        );
        assert!(!source.images.contains_key(&ImageId(1295)));
    }
    #[test]
    fn fourth_channel_last_equal_time_selection_is_independent_and_seekable() {
        let timeline = BgaTimeline::new(
            vec![
                event(0, 0, BgaChannel::Base, 1),
                event(10, 1, BgaChannel::Layer, 2),
                event(10, 2, BgaChannel::Layer2, 3),
                event(10, 3, BgaChannel::Layer2, 4),
                event(20, 4, BgaChannel::Poor, 5),
                event(i64::MAX, 5, BgaChannel::Layer2, 1295),
            ],
            Some(ImageId(0)),
        )
        .unwrap();
        let at = timeline.state_at(Timestamp::from_nanos(10));
        assert_eq!(
            at,
            BgaState {
                base: Some(ImageId(1)),
                layer: Some(ImageId(2)),
                layer2: Some(ImageId(4)),
                poor: Some(ImageId(0))
            }
        );
        assert_eq!(timeline.state_at(Timestamp::from_nanos(10)), at);
        assert_eq!(timeline.state_at(Timestamp::from_nanos(9)).layer2, None);
        assert_eq!(
            timeline.state_at(Timestamp::from_nanos(i64::MAX)).layer2,
            Some(ImageId(1295))
        );
        assert_eq!(timeline.state_at(Timestamp::from_nanos(10)), at);
        assert_eq!(
            timeline.state_at(Timestamp::from_nanos(i64::MIN)).layer2,
            None
        );
        assert_eq!(timeline.len(), 6);
        assert_eq!(timeline.channels.len(), 4);
        let parsed = beatkernel_bms::parse(
            "#BPM 120\n#0000a:0100\n#0010A:00ZZ",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        let parsed = BgaTimeline::from_chart(&parsed).unwrap();
        assert_eq!(
            parsed.state_at(Timestamp::from_nanos(2_999_999_999)).layer2,
            Some(ImageId(1))
        );
        assert_eq!(
            parsed.state_at(Timestamp::from_nanos(3_000_000_000)).layer2,
            Some(ImageId(1295))
        );
    }
}
