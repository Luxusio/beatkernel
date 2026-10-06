//! Native presentation registration consumes the actual validated replay owner.
use crate::{
    gauge::{BmsGauge, GaugeProfile, GaugeFailure},
    mine_audio_consumers_fixtures::{data, recorded, replay_limits, Action},
    player::{self, PauseState, PlayerViewer, PlayerSnapshot},
    replay_visual::ReplayVisual,
    replay_gauge_policy::wrap_header,
};
use beatkernel::time::Timestamp;
use beatkernel_bms::BmsInputMode;
use std::sync::atomic::{AtomicU64, Ordering};
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn policy() -> GaugeProfile {
    GaugeProfile::new(20_000_000, 0, 1_000_000, -30_000_000, true, vec![]).unwrap()
}
fn snapshot(viewer: &PlayerViewer) -> PlayerSnapshot {
    player::publish_pause(PauseState::Paused);
    player::publish_pause(PauseState::Running);
    viewer.take_latest().unwrap()
}
fn unchanged(a: &PlayerSnapshot, b: &PlayerSnapshot) {
    assert_eq!(a.gauge, b.gauge);
    assert_eq!(a.players[0].gauge, b.players[0].gauge);
    assert_eq!(a.score, b.score);
    assert_eq!(a.pressed_lanes, b.pressed_lanes);
    assert_eq!(a.song_time, b.song_time);
    assert_eq!(a.players[0].mine_damage, b.players[0].mine_damage);
}
struct ChartFile(std::path::PathBuf);
impl ChartFile {
    fn new(text: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "beatkernel-replay-policy-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("chart.bms");
        std::fs::write(&path, text).unwrap();
        Self(path)
    }
}
impl Drop for ChartFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_dir(self.0.parent().unwrap());
    }
}
#[test]
fn validated_native_replay_registers_policy_once_and_rejects_changed_policy_or_revival_atomically()
{
    let text = "#BPM 60\n#WAV01 note\n#00011:01010101";
    let chart_file = ChartFile::new(text);
    let prepared = data(text, false);
    let original = recorded(
        &prepared,
        &[
            Action::Press(0, 91),
            Action::Release(0, 91),
            Action::Advance(1_500_000_000),
        ],
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    let mut file = original.file;
    file.header = wrap_header(file.header, &policy(), replay_limits()).unwrap();
    let mut visual = ReplayVisual::new_section(&prepared.source, &file, replay_limits()).unwrap();
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_native_replay_chart(
            &chart_file.0,
            &prepared.source,
            &prepared.compiled.chart,
            &visual,
        )
        .unwrap();
        let initial = snapshot(&viewer);
        assert_eq!(initial.gauge.profile(), &policy());
        assert_eq!(initial.players[0].gauge, initial.gauge);
        assert!(
            player::publish_native_replay_chart(
                &chart_file.0,
                &prepared.source,
                &prepared.compiled.chart,
                &visual
            )
            .is_err()
        );
        unchanged(&initial, &snapshot(&viewer));
        let events = visual.advance_to(ts(0)).unwrap();
        player::publish_replay_prefix_with_gauge(
            ts(0),
            &events,
            visual.pressed_lanes(),
            *visual.mine_damage(),
            visual.gauge(),
        )
        .unwrap();
        let hit = snapshot(&viewer);
        assert_eq!(hit.gauge, *visual.gauge());
        let changed = BmsGauge::new(
            GaugeProfile::new(21_000_000, 0, 1_000_000, -30_000_000, true, vec![]).unwrap(),
        );
        assert!(
            player::publish_replay_prefix_with_gauge(
                ts(1),
                &[],
                1,
                *visual.mine_damage(),
                &changed
            )
            .is_err()
        );
        unchanged(&hit, &snapshot(&viewer));
        let events = visual.advance_to(ts(1_500_000_000)).unwrap();
        player::publish_replay_prefix_with_gauge(
            ts(1_500_000_000),
            &events,
            1,
            *visual.mine_damage(),
            visual.gauge(),
        )
        .unwrap();
        let failed = snapshot(&viewer);
        assert_eq!(
            failed.gauge.snapshot().failure,
            Some(GaugeFailure::Depleted)
        );
        assert_eq!(failed.pressed_lanes, 0);
        assert!(
            player::publish_replay_prefix_with_gauge(
                ts(1_500_000_001),
                &[],
                0,
                *visual.mine_damage(),
                &BmsGauge::new(policy())
            )
            .is_err()
        );
        unchanged(&failed, &snapshot(&viewer));
        player::publish_replay_prefix_with_gauge(
            ts(1_500_000_000),
            &[],
            1,
            *visual.mine_damage(),
            visual.gauge(),
        )
        .unwrap();
        assert_eq!(snapshot(&viewer).gauge, failed.gauge);
        Ok(())
    })
    .unwrap();
    assert!(
        player::publish_native_replay_chart(
            &chart_file.0,
            &prepared.source,
            &prepared.compiled.chart,
            &visual
        )
        .is_err()
    );
}
#[test]
fn depletion_before_later_mine_death_keeps_first_failure_and_pristine_chart_is_required() {
    let text = "#BPM 60\n#WAV01 note\n#00011:01000100\n#000D1:0000ZZ00";
    let chart_file = ChartFile::new(text);
    let prepared = data(text, false);
    let original = recorded(
        &prepared,
        &[
            Action::Advance(1_500_000_000),
            Action::Press(2_000_000_000, 91),
        ],
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    let mut file = original.file;
    file.header = wrap_header(file.header, &policy(), replay_limits()).unwrap();
    let mut visual = ReplayVisual::new_section(&prepared.source, &file, replay_limits()).unwrap();
    let other = data("#BPM 120\n#WAV01 note\n#00011:01", false);
    assert!(
        player::publish_native_replay_chart(
            &chart_file.0,
            &prepared.source,
            &other.compiled.chart,
            &visual
        )
        .is_err()
    );
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_native_replay_chart(
            &chart_file.0,
            &prepared.source,
            &prepared.compiled.chart,
            &visual,
        )
        .unwrap();
        let events = visual.advance_to(ts(2_000_000_000)).unwrap();
        assert!(visual.mine_damage().instant_death);
        assert_eq!(
            visual.gauge().snapshot().failure,
            Some(GaugeFailure::Depleted)
        );
        player::publish_replay_prefix_with_gauge(
            ts(2_000_000_000),
            &events,
            1,
            *visual.mine_damage(),
            visual.gauge(),
        )
        .unwrap();
        let observed = snapshot(&viewer);
        assert_eq!(observed.gauge, *visual.gauge());
        assert!(observed.players[0].mine_damage.instant_death);
        assert_eq!(observed.pressed_lanes, 0);
        Ok(())
    })
    .unwrap();
    // Unattached publication is validated but requires no native assets/display.
    player::publish_replay_prefix_with_gauge(
        ts(2_000_000_000),
        &[],
        1,
        *visual.mine_damage(),
        visual.gauge(),
    )
    .unwrap();
}

#[test]
fn native_policy_asset_loader_accepts_recorded_setup_and_refuses_mismatch_before_missing_pcm() {
    let text = "#BPM 60\n#WAV01 note\n#00011:01";
    let chart_file = ChartFile::new(text);
    let prepared = data(text, false);
    let original = recorded(
        &prepared,
        &[Action::Advance(1_500_000_000)],
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    let mut file = original.file;
    file.header = wrap_header(file.header, &policy(), replay_limits()).unwrap();
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&40u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&10u32.to_le_bytes());
    wav.extend_from_slice(&20u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&4u32.to_le_bytes());
    wav.extend_from_slice(&[0; 4]);
    let asset = chart_file.0.parent().unwrap().join("note.wav");
    std::fs::write(&asset, wav).unwrap();
    let limits = beatkernel::audio::PcmLimits::new(256, 2048, 8).unwrap();
    let loaded = crate::load_prepared_for_section_replay(
        &chart_file.0,
        prepared.bank.format(),
        limits,
        crate::ChannelPolicy::Exact,
        &file,
        replay_limits(),
    )
    .unwrap();
    assert_eq!(loaded.compiled.chart, prepared.compiled.chart);
    assert!(loaded.bank.get(beatkernel::audio::SampleId(1)).is_some());
    std::fs::remove_file(&asset).unwrap();
    file.header.chart_identity[0] ^= 1;
    let error = crate::load_prepared_for_section_replay(
        &chart_file.0,
        prepared.bank.format(),
        limits,
        crate::ChannelPolicy::Exact,
        &file,
        replay_limits(),
    )
    .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<crate::replay_playback::PlaybackError>(),
        Some(crate::replay_playback::PlaybackError::IdentityMismatch(_))
    ));
}
