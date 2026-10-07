//! Immutable original-song opacity markers; no clocks, interpolation or pixel mutation.
use crate::bga::channel_index;
use beatkernel::{chart::MAX_SOURCE_ITEMS, time::Timestamp};
use beatkernel_bms::{BmsChart, ScheduledBgaOpacity};

/// Exact byte opacity for all four roles; untouched channels are fully opaque.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BgaOpacity {
    pub base: u8,
    pub layer: u8,
    pub layer2: u8,
    pub poor: u8,
}
impl Default for BgaOpacity {
    fn default() -> Self {
        Self {
            base: 255,
            layer: 255,
            layer2: 255,
            poor: 255,
        }
    }
}
/// Prepared per-channel indexes; every query reconstructs the exact time prefix.
#[derive(Clone, Debug, Default)]
pub struct BgaOpacityTimeline {
    channels: [Vec<ScheduledBgaOpacity>; 4],
}
impl BgaOpacityTimeline {
    /// Accepts nonnegative chronological markers, strictly ordered by ordinal at
    /// equal timestamps, bounded to the core source-item ceiling.
    pub fn new(events: Vec<ScheduledBgaOpacity>) -> Result<Self, String> {
        if events.len() > MAX_SOURCE_ITEMS {
            return Err("BGA opacity event capacity exceeded".into());
        }
        let mut previous = None;
        let mut counts = [0usize; 4];
        for event in &events {
            let key = (event.at, event.ordinal);
            if event.at < Timestamp::ZERO || previous.is_some_and(|old| old >= key) {
                return Err("BGA opacity timestamps and ordinals must be ordered".into());
            }
            previous = Some(key);
            counts[channel_index(event.channel)] += 1;
        }
        let mut channels: [Vec<ScheduledBgaOpacity>; 4] = std::array::from_fn(|_| Vec::new());
        for (channel, count) in channels.iter_mut().zip(counts) {
            channel
                .try_reserve_exact(count)
                .map_err(|e| e.to_string())?;
        }
        for event in events {
            channels[channel_index(event.channel)].push(event);
        }
        Ok(Self { channels })
    }
    /// Restores acquisition order for immutable visual registration.
    pub fn export_events(&self) -> Vec<ScheduledBgaOpacity> {
        let mut events: Vec<_> = self.channels.iter().flatten().copied().collect();
        events.sort_unstable_by_key(|event| (event.at, event.ordinal));
        events
    }

    /// Compiles checked source positions through the shared visual timing grid.
    pub fn from_chart(chart: &BmsChart) -> Result<Self, String> {
        Self::new(chart.compile_bga_opacity().map_err(|e| e.to_string())?)
    }
    /// Last marker at-or-before original song time; negative time is pristine.
    pub fn state_at(&self, now: Timestamp) -> BgaOpacity {
        let value = |channel: usize| {
            let events = &self.channels[channel];
            let end = events.partition_point(|event| event.at <= now);
            end.checked_sub(1).map_or(255, |index| events[index].alpha)
        };
        BgaOpacity {
            base: value(0),
            layer: value(1),
            layer2: value(3),
            poor: value(2),
        }
    }
    /// Total prepared markers across all roles.
    pub fn len(&self) -> usize {
        self.channels.iter().map(Vec::len).sum()
    }
    /// Whether every role retains default opacity at all times.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel_bms::BgaChannel;
    fn event(ns: i64, ordinal: u64, channel: BgaChannel, alpha: u8) -> ScheduledBgaOpacity {
        ScheduledBgaOpacity {
            at: Timestamp::from_nanos(ns),
            ordinal,
            channel,
            alpha,
        }
    }
    #[test]
    fn four_channel_defaults_equal_time_pause_and_backward_prefix_are_exact() {
        let timeline = BgaOpacityTimeline::new(vec![
            event(0, 0, BgaChannel::Base, 1),
            event(10, 1, BgaChannel::Layer, 2),
            event(10, 2, BgaChannel::Layer2, 3),
            event(10, 3, BgaChannel::Layer2, 128),
            event(20, 4, BgaChannel::Poor, 127),
            event(i64::MAX, 5, BgaChannel::Base, 255),
        ])
        .unwrap();
        assert_eq!(
            timeline.state_at(Timestamp::from_nanos(i64::MIN)),
            BgaOpacity::default()
        );
        assert_eq!(timeline.state_at(Timestamp::ZERO).base, 1);
        assert_eq!(timeline.state_at(Timestamp::from_nanos(9)).layer, 255);
        let at = BgaOpacity {
            base: 1,
            layer: 2,
            layer2: 128,
            poor: 255,
        };
        assert_eq!(timeline.state_at(Timestamp::from_nanos(10)), at);
        assert_eq!(timeline.state_at(Timestamp::from_nanos(10)), at);
        assert_eq!(timeline.state_at(Timestamp::from_nanos(20)).poor, 127);
        assert_eq!(timeline.state_at(Timestamp::from_nanos(i64::MAX)).base, 255);
        assert_eq!(timeline.state_at(Timestamp::from_nanos(10)), at);
        assert_eq!(timeline.len(), 6);
        assert!(!timeline.is_empty());
        assert_eq!(
            BgaOpacityTimeline::default().state_at(Timestamp::from_nanos(i64::MAX)),
            BgaOpacity::default()
        );
        assert!(BgaOpacityTimeline::default().is_empty());
    }
    #[test]
    fn parsed_rests_retain_values_and_all_bounds_are_validated_before_indexes() {
        let source = beatkernel_bms::parse(
            "#BPM 120\n#0000B:0100\n#0010B:00FF\n#0010C:80\n#0010D:7F\n#0010E:01",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        let timeline = BgaOpacityTimeline::from_chart(&source).unwrap();
        assert_eq!(
            timeline.state_at(Timestamp::from_nanos(1_999_999_999)),
            BgaOpacity {
                base: 1,
                ..BgaOpacity::default()
            }
        );
        assert_eq!(
            timeline.state_at(Timestamp::from_nanos(2_000_000_000)),
            BgaOpacity {
                base: 1,
                layer: 128,
                layer2: 127,
                poor: 1
            }
        );
        assert_eq!(
            timeline.state_at(Timestamp::from_nanos(3_000_000_000)).base,
            255
        );
        assert!(BgaOpacityTimeline::new(vec![event(-1, 0, BgaChannel::Base, 1)]).is_err());
        assert!(
            BgaOpacityTimeline::new(vec![
                event(2, 0, BgaChannel::Base, 1),
                event(1, 1, BgaChannel::Poor, 2)
            ])
            .is_err()
        );
        assert!(
            BgaOpacityTimeline::new(vec![
                event(1, 0, BgaChannel::Base, 1),
                event(1, 0, BgaChannel::Poor, 2)
            ])
            .is_err()
        );
        let too_many = vec![event(0, 0, BgaChannel::Base, 1); MAX_SOURCE_ITEMS + 1];
        assert!(BgaOpacityTimeline::new(too_many).is_err());
    }
}
