//! Independent whole-timeline expectations, separate from the workload oracle.
#[path = "../examples/long_duration_player/workload.rs"]
mod workload;

use beatkernel::{
    input::{
        ButtonEvent, ButtonState, CodecLimits, DeviceId, EventMeta, PhysicalControlId,
        PhysicalInputEvent,
    },
    judge::{JudgeGrade, JudgeOutcome, JudgeStage},
    replay::{
        ReplayOperation,
        codec::{ReplayCodecLimits, decode_replay, encode_replay},
    },
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms_runtime::{
    local_input::{InputMerger, MergeError},
    native_gameplay_host::PauseState,
    replay_playback::{decode_section_setup, reconstruct_section},
};

const HOUR_FRAMES: u64 = 3_600_000;
const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

#[test]
fn numeric_and_combined_limits_are_checked_before_player_allocation() {
    let defaults = workload::Options::default();
    assert_eq!((defaults.hours, defaults.block_frames), (20, 8192));
    for hours in [1, 2, 20, 168] {
        for block_frames in [128, 129, 8191, 8192, 65536] {
            let frames = hours as u64 * HOUR_FRAMES;
            let blocks = frames.div_ceil(block_frames as u64);
            let admitted = 2 * blocks + 8 * hours as u64 + 64 <= 200_000;
            assert_eq!(
                workload::Options {
                    hours,
                    block_frames
                }
                .validate()
                .is_ok(),
                admitted,
                "hours={hours}, block_frames={block_frames}"
            );
        }
    }
    for hours in [0, 169, usize::MAX] {
        assert!(workload::Options { hours, ..defaults }.validate().is_err());
    }
    for block_frames in [0, 1, 127, 65537, usize::MAX] {
        assert!(
            workload::Options {
                block_frames,
                ..defaults
            }
            .validate()
            .is_err()
        );
    }
    assert!(
        workload::Options {
            hours: 168,
            block_frames: 128
        }
        .validate()
        .is_err()
    );
    assert_eq!(168 * HOUR_FRAMES, 604_800_000);
}

fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(
        32 * 1024 * 1024,
        200_000,
        4096,
        CodecLimits::new(4096, 1024).unwrap(),
    )
    .unwrap()
}

fn fnv_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    })
}

// Zero f32 samples contribute four zero bytes. Exponentiation skips repeated
// multiplication, not verification: the expected event positions are literal.
fn zero_samples(hash: u64, samples: u64) -> u64 {
    let mut exponent = samples * 4;
    let mut factor = FNV_PRIME;
    let mut product = 1u64;
    while exponent != 0 {
        if exponent & 1 != 0 {
            product = product.wrapping_mul(factor);
        }
        exponent >>= 1;
        factor = factor.wrapping_mul(factor);
    }
    hash.wrapping_mul(product)
}

fn expected_pcm_hash(hours: usize, physical_frames: u64) -> u64 {
    let pause_frame = hours as u64 * HOUR_FRAMES / 2 + 123;
    let mut sounds = Vec::new();
    for hour in 0..hours as u64 {
        let start = hour * HOUR_FRAMES;
        sounds.push((start + 900_000, 0.25f32));
        sounds.push((start + 1_800_000, 0.125f32));
        if hour % 2 == 1 {
            sounds.push((start + 2_700_000, 0.25f32));
        }
    }
    sounds.sort_by_key(|item| item.0);
    let mut hash = FNV_OFFSET;
    let mut cursor = 0;
    for (playback, sample) in sounds {
        let physical = playback + if playback >= pause_frame { 3 } else { 0 };
        hash = zero_samples(hash, physical - cursor);
        for byte in sample.to_bits().to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME);
        }
        cursor = physical + 1;
    }
    assert!(physical_frames >= cursor);
    zero_samples(hash, physical_frames - cursor)
}

fn check_whole_player(hours: usize, block_frames: usize) -> workload::Report {
    let report = workload::run(workload::Options {
        hours,
        block_frames,
    })
    .unwrap();
    let horizon = hours as u64 * HOUR_FRAMES;
    let end = Timestamp::from_nanos(horizon as i64 * 1_000_000);
    assert!(report.completed);
    assert_eq!(report.section_end, end);
    assert_eq!(report.playback_frames, horizon);
    assert!((horizon + 3..=horizon + 8).contains(&report.physical_frames));
    assert_eq!(report.verified_samples, report.physical_frames);
    assert_eq!(report.pause_render_count, 3);
    assert_eq!(report.input_edges, 2 * hours);
    assert_eq!(report.key_samples, hours + hours / 2);
    assert_eq!(report.bgm_samples, hours);
    assert_eq!(report.repair_records, 0);
    assert_eq!(report.pending_inputs, 0);
    assert_eq!(report.pending_commands, 0);
    assert_eq!(report.active_voices, 0);
    assert_ne!(report.mixer_identity_before, 0);
    assert_eq!(report.mixer_identity_before, report.mixer_identity_after);
    assert!(report.render_calls > 0);
    assert!(report.render_calls + report.record_operations <= 200_000);
    assert!(report.cached_report_reads > report.render_calls);
    assert!(report.checkpoints.len() <= 512);
    assert!(!report.checkpoints.is_empty());
    for pair in report.checkpoints.windows(2) {
        assert!(pair[1].physical_frame >= pair[0].physical_frame);
        assert!(pair[1].playback_frame >= pair[0].playback_frame);
    }
    assert!(report.pause_states.contains(&PauseState::Paused));
    assert!(report.pause_states.contains(&PauseState::Running));
    assert_eq!(
        report.pcm_hash,
        expected_pcm_hash(hours, report.physical_frames)
    );
    assert_eq!(report.replay_hash, fnv_bytes(&report.bytes));

    let compiled = report.source.compile().unwrap();
    assert_eq!(compiled.chart.objects().len(), hours);
    assert_eq!(compiled.bgm.len(), hours);
    for (hour, object) in compiled.chart.objects().iter().enumerate() {
        assert_eq!(
            object.time.start,
            Timestamp::from_nanos((hour as i64 * 3600 + 900) * 1_000_000_000)
        );
        assert_eq!(
            object.time.end,
            if hour % 2 == 1 {
                Some(Timestamp::from_nanos(
                    (hour as i64 * 3600 + 2700) * 1_000_000_000,
                ))
            } else {
                None
            }
        );
    }
    for (hour, bgm) in compiled.bgm.iter().enumerate() {
        assert_eq!(bgm.sample, beatkernel::audio::SampleId(2));
        assert_eq!(
            bgm.at,
            Timestamp::from_nanos((hour as i64 * 3600 + 1800) * 1_000_000_000)
        );
    }
    let canonical = encode_replay(&report.file, limits()).unwrap();
    assert_eq!(report.bytes, canonical);
    let decoded = decode_replay(&report.bytes, limits()).unwrap();
    assert_eq!(decoded, report.file);
    assert_eq!(decoded.header.seed, 0);
    assert_eq!(decoded.header.normalized_clock, ClockDomainId(3));
    assert_eq!(report.record_operations, decoded.records.len());
    let setup = decode_section_setup(&decoded.header.options).unwrap();
    assert_eq!(setup.start, Timestamp::ZERO);
    assert_eq!(setup.end, Some(end));
    assert_eq!(decoded.records.last().unwrap().song_time, end);
    let replay = reconstruct_section(&report.source, decoded, limits()).unwrap();
    assert_eq!(replay.results(), report.reconstructed_results);
    assert_eq!(report.live_results, report.reconstructed_results);
    let stages: Vec<_> = (0..hours)
        .flat_map(|hour| {
            let start = hour as i64 * 3600;
            if hour % 2 == 0 {
                vec![(JudgeStage::Instant, start + 900)]
            } else {
                vec![
                    (JudgeStage::HoldHead, start + 900),
                    (JudgeStage::HoldTail, start + 2700),
                ]
            }
        })
        .collect();
    assert_eq!(report.live_results.len(), hours + hours / 2);
    for (event, (stage, seconds)) in report.live_results.iter().zip(stages) {
        assert_eq!(event.stage, stage);
        assert_eq!(event.at, Timestamp::from_nanos(seconds * 1_000_000_000));
        assert_eq!(
            event.outcome,
            JudgeOutcome::Hit {
                grade: JudgeGrade(1),
                delta: Duration::ZERO
            }
        );
        assert!(event.input.is_some());
    }
    let inputs: Vec<_> = report
        .file
        .records
        .iter()
        .filter_map(|record| match &record.operation {
            ReplayOperation::Input(input) => Some((record, input)),
            ReplayOperation::Advance => None,
        })
        .collect();
    assert_eq!(inputs.len(), 2 * hours);
    assert_eq!(
        report.advance_records,
        report.file.records.len() - inputs.len()
    );
    assert!(report.advance_records > 0);
    for hour in 0..hours {
        let down_ns = (hour as i64 * 3600 + 900) * 1_000_000_000;
        let up_ns = if hour % 2 == 0 {
            down_ns + 1_000_000
        } else {
            (hour as i64 * 3600 + 2700) * 1_000_000_000
        };
        for (edge, expected_ns) in [down_ns, up_ns].into_iter().enumerate() {
            let (record, input) = inputs[hour * 2 + edge];
            assert_eq!(record.song_time, Timestamp::from_nanos(expected_ns));
            let PhysicalInputEvent::Button(button) = &input.physical else {
                panic!("original hourly button edges must survive capture")
            };
            assert_eq!(button.meta.sequence, (hour * 2 + edge) as u64);
            assert_eq!(
                button.state,
                if edge == 0 {
                    ButtonState::Down
                } else {
                    ButtonState::Up
                }
            );
            assert_eq!(button.meta.source, DeviceId(1001));
            assert_eq!(button.control, PhysicalControlId::keyboard(7));
            assert_eq!(button.meta.clock_domain, ClockDomainId(3));
            let pause_ns = horizon as i64 * 1_000_000 / 2 + 123_000_000;
            let original_ns = expected_ns
                + if expected_ns >= pause_ns {
                    3_000_000
                } else {
                    0
                };
            assert_eq!(
                button.meta.original_clock_point,
                Some(ClockPoint {
                    domain: ClockDomainId(1),
                    timestamp: Timestamp::from_nanos(original_ns),
                })
            );
            assert_eq!(
                button.meta.timestamp,
                Timestamp::from_nanos(5_000_000_000 + original_ns)
            );
            if hour > 0 || edge > 0 {
                assert!(record.song_time > inputs[hour * 2 + edge - 1].0.song_time);
            }
        }
    }
    report
}

#[test]
fn complete_one_hour_including_the_final_idle_interval() {
    check_whole_player(1, 8191);
}

#[test]
fn complete_two_hours_preserve_hold_bgm_and_postpause_output() {
    check_whole_player(2, 65536);
}

#[test]
fn smallest_crossing_blocks_have_the_same_literal_pcm() {
    let small = check_whole_player(1, 128);
    let large = check_whole_player(1, 65536);
    assert_eq!(small.pcm_hash, large.pcm_hash);
    assert_eq!(small.live_results, large.live_results);
    assert_eq!(small.physical_frames, large.physical_frames);
    assert!(small.render_calls > large.render_calls);
}

#[test]
fn original_port_failures_cannot_publish_a_complete_traversal() {
    for fault in [
        workload::Fault::LateInput,
        workload::Fault::IncompleteInput,
        workload::Fault::NativeRegression,
        workload::Fault::MissingProgress,
        workload::Fault::UnacknowledgedPause,
        workload::Fault::HostPublication,
        workload::Fault::OperationBudget,
    ] {
        let prefix = workload::run_fault(
            workload::Options {
                hours: 1,
                block_frames: 8192,
            },
            fault,
        )
        .unwrap();
        assert!(!prefix.error.is_empty(), "{fault:?}");
        assert!(prefix.playback_frames < HOUR_FRAMES, "{fault:?}");
        assert!(
            prefix.playback_frames <= prefix.physical_frames,
            "{fault:?}"
        );
        assert!(
            prefix.render_calls + prefix.record_operations <= 200_001,
            "{fault:?}"
        );
        assert_eq!(
            prefix.input_edges,
            prefix
                .records
                .iter()
                .filter(|record| { matches!(record.operation, ReplayOperation::Input(_)) })
                .count(),
            "{fault:?}"
        );
        assert!(prefix.live_results.len() <= 1, "{fault:?}");
        assert!(
            prefix
                .records
                .windows(2)
                .all(|pair| pair[1].song_time >= pair[0].song_time)
        );
        match fault {
            workload::Fault::LateInput => {
                assert_eq!(
                    prefix.input_edges, 0,
                    "first original edge was not accepted"
                );
                assert_eq!(prefix.acquired_input_edges, 1);
                assert_eq!(prefix.pending_inputs, 0);
                assert!(prefix.live_results.is_empty());
            }
            workload::Fault::IncompleteInput => {
                assert!(prefix.acquisition_cut_missing);
                assert_eq!(prefix.acquired_input_edges, 1);
                assert_eq!(
                    prefix.pending_inputs, 1,
                    "retain the original uncommitted edge"
                );
                assert_eq!(prefix.input_edges, 0);
                assert_eq!(prefix.playback_frames, 900_000);
                assert!(prefix.live_results.is_empty());
            }
            workload::Fault::NativeRegression => {
                assert!(
                    prefix.render_calls > 0,
                    "must retain an actual output prefix"
                );
                assert_eq!(prefix.input_edges, 0);
            }
            workload::Fault::MissingProgress => {
                assert!(prefix.native_observation_missing);
                assert!(
                    prefix.render_calls > 2,
                    "actual callback prefix continues without correlation"
                );
                assert_eq!(prefix.acquired_input_edges, 0);
                assert_eq!(prefix.input_edges, 0);
                assert_eq!(prefix.pending_inputs, 0);
            }
            workload::Fault::UnacknowledgedPause => {
                assert!(prefix.pause_callback_missing);
                assert_eq!(prefix.playback_frames, HOUR_FRAMES / 2 + 123);
                assert_eq!(prefix.pause_render_count, 0, "no invented acknowledgment");
                assert_eq!(prefix.input_edges, 2);
                assert_eq!(prefix.acquired_input_edges, 2);
                assert_eq!(prefix.pending_inputs, 0);
                assert_eq!(prefix.live_results.len(), 1);
            }
            workload::Fault::HostPublication => {
                assert!(prefix.render_calls > 0);
                assert_eq!(prefix.input_edges, 0);
            }
            workload::Fault::OperationBudget => {
                assert!(prefix.error.contains("budget"), "{}", prefix.error);
                assert!(prefix.render_calls > 0);
                assert_eq!(prefix.input_edges, 0);
            }
        }
    }
}

#[test]
fn original_merger_preserves_week_scale_incomplete_and_late_prefixes() {
    let point = |ns| ClockPoint {
        domain: ClockDomainId(1),
        timestamp: Timestamp::from_nanos(ns),
    };
    let late_week = 604_799_000_000_000;
    let edge = PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(17), point(late_week), 99),
        control: PhysicalControlId::keyboard(7),
        state: ButtonState::Down,
    });
    let mut merger = InputMerger::new(ClockDomainId(1), point(0), vec![DeviceId(17)], 1).unwrap();
    merger
        .admit(edge.clone(), point(late_week + 1_000_000))
        .unwrap();
    assert_eq!(
        merger.commit(point(late_week)),
        Err(MergeError::PendingBeforeFrontier {
            timestamp: Timestamp::from_nanos(late_week),
            frontier: Timestamp::from_nanos(late_week),
        })
    );
    assert_eq!(merger.pending(), 1);
    assert_eq!(
        merger.pop_ready(point(late_week)).unwrap(),
        Some(edge.clone())
    );
    merger.commit(point(late_week)).unwrap();
    assert_eq!(
        merger.admit(edge, point(late_week + 1_000_000)),
        Err(MergeError::LateInput {
            timestamp: Timestamp::from_nanos(late_week),
            committed: Timestamp::from_nanos(late_week),
        })
    );
    assert_eq!(merger.pending(), 0);
}
