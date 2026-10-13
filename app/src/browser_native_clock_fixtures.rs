//! Independent integer-domain and real output-owner tests; no browser timestamps.
use super::{MAX_NATIVE_FRAME, NATIVE_PERIODS, NativeClock, NativeClockError, joint_offset};
use crate::worklet_audio::{
    WorkletAudio, WorkletAudioBuilder, WorkletAudioConfig, WorkletAudioError,
};
use beatkernel::{
    audio::{AudioCommand, AudioFormat, AudioLimits, PcmLimits, SampleId, VoiceId},
    time::Timestamp,
};

// Deliberately use the general CRT sum and an extended-Euclidean inverse in u128,
// rather than production's fixed mixed-radix recurrence and inverse constant.
fn inverse(value: u128, modulus: u128) -> u128 {
    let (mut remainder, mut next_remainder) = (modulus as i128, value as i128);
    let (mut coefficient, mut next_coefficient) = (0i128, 1i128);
    while next_remainder != 0 {
        let quotient = remainder / next_remainder;
        (remainder, next_remainder) = (next_remainder, remainder - quotient * next_remainder);
        (coefficient, next_coefficient) =
            (next_coefficient, coefficient - quotient * next_coefficient);
    }
    assert_eq!(remainder, 1, "fixture periods must be coprime");
    coefficient.rem_euclid(modulus as i128) as u128
}

fn crt_oracle(residues: [u32; 3]) -> u128 {
    let modulus = NATIVE_PERIODS
        .iter()
        .map(|&p| u128::from(p))
        .product::<u128>();
    let mut result = 0u128;
    for (&residue, &period) in residues.iter().zip(&NATIVE_PERIODS) {
        let period = u128::from(period);
        assert!(u128::from(residue) < period);
        let complement = modulus / period;
        result = (result + u128::from(residue) * complement * inverse(complement % period, period))
            % modulus;
    }
    result
}

fn phases(offset: u64) -> [u32; 3] {
    NATIVE_PERIODS.map(|period| (offset % u64::from(period)) as u32)
}

fn horizons(origin: u64, turns: [u64; 3]) -> [u64; 3] {
    std::array::from_fn(|lane| origin + turns[lane] * u64::from(NATIVE_PERIODS[lane]))
}

fn block(clock: &mut NativeClock, origin: u64, base: u64, frames: u32) -> u64 {
    clock
        .frame(
            phases(base - origin),
            phases(base - origin + u64::from(frames) - 1),
            0,
            frames,
        )
        .unwrap()
}

fn ready_clock(origin: u64, turns: [u64; 3]) -> (NativeClock, u64) {
    let mut clock = NativeClock::new(origin, horizons(origin, turns)).unwrap();
    assert!(!clock.ready());
    assert_eq!(
        clock.frame(phases(0), phases(63), 64, 128).unwrap(),
        origin - 64
    );
    assert!(!clock.ready());
    assert_eq!(block(&mut clock, origin, origin + 64, 128), origin + 64);
    assert!(clock.ready());
    (clock, origin + 192)
}

#[test]
fn frozen_periods_and_safe_domain_match_the_native_program() {
    assert_eq!(NATIVE_PERIODS, [1_048_576, 1_048_575, 1_048_573]);
    assert_eq!(MAX_NATIVE_FRAME, 9_007_199_254_740_991);
    let product = NATIVE_PERIODS
        .iter()
        .map(|&p| u128::from(p))
        .product::<u128>();
    assert_eq!(product, 1_152_917_106_563_481_600);
    assert!(product > u128::from(MAX_NATIVE_FRAME));
}

#[test]
fn crt_matches_independent_general_oracle_at_carries_and_integer_limits() {
    let mut offsets = vec![0, 1, 2, MAX_NATIVE_FRAME - 1, MAX_NATIVE_FRAME];
    for period in NATIVE_PERIODS {
        for multiple in [1, 2, 3, 17, 1024, 4096] {
            let boundary = u64::from(period) * multiple;
            offsets.extend([boundary - 1, boundary, boundary + 1]);
        }
    }
    for left in NATIVE_PERIODS {
        for right in NATIVE_PERIODS {
            let boundary = u64::from(left) * u64::from(right);
            offsets.extend([boundary - 1, boundary, boundary + 1]);
        }
    }
    for boundary in [1u64 << 32, 1u64 << 48, 1u64 << 52] {
        offsets.extend([boundary - 1, boundary, boundary + 1]);
    }
    for offset in offsets {
        let encoded = phases(offset);
        assert_eq!(crt_oracle(encoded), u128::from(offset));
        assert_eq!(joint_offset(encoded), Ok(offset), "offset {offset}");
    }
}

#[test]
fn crt_matches_general_oracle_for_independent_residues_and_seeded_coordinates() {
    // Arbitrary residues exercise the refusal domain as well as valid frames.
    let mut seed = 0x91e1_0da5_c79e_7b1du64;
    for _ in 0..4096 {
        let residues = std::array::from_fn(|lane| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % u64::from(NATIVE_PERIODS[lane])) as u32
        });
        let expected = crt_oracle(residues);
        if expected <= u128::from(MAX_NATIVE_FRAME) {
            assert_eq!(joint_offset(residues), Ok(expected as u64));
        } else {
            assert_eq!(joint_offset(residues), Err(NativeClockError::Overflow));
        }
        let offset = seed % (MAX_NATIVE_FRAME + 1);
        assert_eq!(crt_oracle(phases(offset)), u128::from(offset));
        assert_eq!(joint_offset(phases(offset)), Ok(offset));
    }
}

#[test]
fn third_lane_distinguishes_two_lane_aliases_but_residues_do_not_certify_epoch() {
    let two_lane_period = u64::from(NATIVE_PERIODS[0]) * u64::from(NATIVE_PERIODS[1]);
    for offset in [0, 1, 127, 65_535, two_lane_period - 1] {
        let ordinary = phases(offset);
        let alias = phases(offset + two_lane_period);
        assert_eq!(ordinary[..2], alias[..2]);
        assert_ne!(ordinary[2], alias[2]);
        assert_eq!(joint_offset(alias), Ok(offset + two_lane_period));
    }
    let full_period = NATIVE_PERIODS
        .iter()
        .map(|&p| u64::from(p))
        .product::<u64>();
    assert_eq!(phases(full_period), phases(0));
    assert_eq!(joint_offset(phases(full_period)), Ok(0));
    // This explicitly preserves the ceiling: whole-cycle age needs native
    // origin/provenance/horizons, and cannot be inferred from residue equality.
}

#[test]
fn crt_rejects_out_of_lane_residues_and_unsafe_alias_domain() {
    for lane in 0..3 {
        for value in [NATIVE_PERIODS[lane], u32::MAX] {
            let mut residues = [0; 3];
            residues[lane] = value;
            assert_eq!(joint_offset(residues), Err(NativeClockError::Invalid));
        }
    }
    let product = NATIVE_PERIODS
        .iter()
        .map(|&p| u64::from(p))
        .product::<u64>();
    for offset in [MAX_NATIVE_FRAME + 1, MAX_NATIVE_FRAME + 2, product - 1] {
        assert_eq!(crt_oracle(phases(offset)), u128::from(offset));
        assert_eq!(
            joint_offset(phases(offset)),
            Err(NativeClockError::Overflow)
        );
    }
}

#[test]
fn constructor_accepts_fixed_lane_association_in_either_horizon_order() {
    let origin = 8192;
    for turns in [[3, 3, 3], [1, 2, 3], [3, 2, 1], [7, 11, 5]] {
        let clock = NativeClock::new(origin, horizons(origin, turns)).unwrap();
        assert!(!clock.ready());
    }
    let descending = horizons(origin, [3; 3]);
    assert!(descending[0] > descending[1] && descending[1] > descending[2]);
}

#[test]
fn constructor_rejects_zero_unsafe_nonintegral_and_swapped_geometry() {
    let origin = 8192;
    assert_eq!(
        NativeClock::new(0, horizons(0, [3; 3])).unwrap_err(),
        NativeClockError::Invalid
    );
    for bad_origin in [MAX_NATIVE_FRAME + 1, u64::MAX] {
        assert_eq!(
            NativeClock::new(bad_origin, [bad_origin; 3]).unwrap_err(),
            NativeClockError::Overflow
        );
    }
    for lane in 0..3 {
        for bad_horizon in [origin - 1, origin, origin + 1] {
            let mut geometry = horizons(origin, [3; 3]);
            geometry[lane] = bad_horizon;
            assert_eq!(
                NativeClock::new(origin, geometry).unwrap_err(),
                NativeClockError::Invalid
            );
        }
        let mut unsafe_geometry = horizons(origin, [3; 3]);
        unsafe_geometry[lane] = MAX_NATIVE_FRAME + 1;
        assert_eq!(
            NativeClock::new(origin, unsafe_geometry).unwrap_err(),
            NativeClockError::Overflow
        );
    }
    let mut swapped = horizons(origin, [3; 3]);
    swapped.swap(0, 1);
    assert_eq!(
        NativeClock::new(origin, swapped).unwrap_err(),
        NativeClockError::Invalid
    );
}

#[test]
fn first_interior_block_requires_next_contiguous_nonempty_block_for_readiness() {
    let origin = 8192;
    let mut clock = NativeClock::new(origin, horizons(origin, [3; 3])).unwrap();
    assert_eq!(
        clock.frame(phases(0), phases(114), 13, 128),
        Ok(origin - 13)
    );
    assert!(!clock.ready());
    assert_eq!(block(&mut clock, origin, origin + 115, 257), origin + 115);
    assert!(clock.ready());
    assert_eq!(block(&mut clock, origin, origin + 372, 1), origin + 372);
    assert!(clock.ready());
}

#[test]
fn one_active_first_sample_and_authenticated_one_frame_callbacks_are_valid() {
    let origin = 8192;
    let mut clock = NativeClock::new(origin, horizons(origin, [3; 3])).unwrap();
    assert_eq!(
        clock.frame(phases(0), phases(0), 4095, 4096),
        Ok(origin - 4095)
    );
    assert!(!clock.ready());
    assert_eq!(block(&mut clock, origin, origin + 1, 1), origin + 1);
    assert!(clock.ready());
    for offset in 2..34 {
        assert_eq!(
            block(&mut clock, origin, origin + offset, 1),
            origin + offset
        );
    }
}

#[test]
fn initial_zero_index_one_frame_zero_extent_and_underflow_never_establish_proof() {
    let origin = 8192;
    for (edge, frames) in [(0, 128), (0, 1), (1, 1), (128, 128), (129, 128), (0, 0)] {
        let mut clock = NativeClock::new(origin, horizons(origin, [3; 3])).unwrap();
        let before = clock;
        assert_eq!(
            clock.frame(phases(0), phases(0), edge, frames),
            Err(NativeClockError::Invalid)
        );
        assert_eq!(clock, before);
        assert!(!clock.ready());
    }
    let mut clock = NativeClock::new(7, horizons(7, [3; 3])).unwrap();
    let before = clock;
    assert_eq!(
        clock.frame(phases(0), phases(7), 8, 16),
        Err(NativeClockError::Overflow)
    );
    assert_eq!(clock, before);
}

#[test]
fn nonzero_initial_phase_and_wrong_partial_endpoints_leave_admission_unmodified() {
    let origin = 8192;
    let mut clock = NativeClock::new(origin, horizons(origin, [3; 3])).unwrap();
    let before = clock;
    for (first, last) in [(1, 64), (0, 62), (0, 64)] {
        assert!(clock.frame(phases(first), phases(last), 64, 128).is_err());
        assert_eq!(clock, before);
    }
    let alias = u64::from(NATIVE_PERIODS[0]) * u64::from(NATIVE_PERIODS[1]);
    assert!(
        clock
            .frame(phases(alias), phases(alias + 63), 64, 128)
            .is_err()
    );
    assert_eq!(clock, before);
    assert_eq!(clock.frame(phases(0), phases(63), 64, 128), Ok(origin - 64));
    assert!(!clock.ready());
}

#[test]
fn malformed_phase_in_any_endpoint_is_rejected_without_advancing_clock() {
    let origin = 8192;
    for initial in [true, false] {
        for lane in 0..3 {
            for last_endpoint in [false, true] {
                let (mut clock, base, edge, frames) = if initial {
                    (
                        NativeClock::new(origin, horizons(origin, [3; 3])).unwrap(),
                        origin,
                        64,
                        128,
                    )
                } else {
                    let (clock, base) = ready_clock(origin, [3; 3]);
                    (clock, base, 0, 128)
                };
                let before = clock;
                let first_offset = base - origin;
                let mut first = phases(first_offset);
                let mut last = phases(first_offset + u64::from(frames - edge) - 1);
                if last_endpoint {
                    last[lane] = NATIVE_PERIODS[lane];
                } else {
                    first[lane] = NATIVE_PERIODS[lane];
                }
                assert_eq!(
                    clock.frame(first, last, edge, frames),
                    Err(NativeClockError::Invalid)
                );
                assert_eq!(clock, before);
            }
        }
    }
}

#[test]
fn second_callback_gap_or_overlap_cannot_publish_ready_or_advance() {
    let origin = 8192;
    let mut clock = NativeClock::new(origin, horizons(origin, [3; 3])).unwrap();
    clock.frame(phases(0), phases(63), 64, 128).unwrap();
    let before = clock;
    for offset in [63, 65, 1000] {
        assert_eq!(
            clock.frame(phases(offset), phases(offset + 127), 0, 128),
            Err(NativeClockError::Chronology)
        );
        assert_eq!(clock, before);
        assert!(!clock.ready());
    }
    assert_eq!(block(&mut clock, origin, origin + 64, 128), origin + 64);
    assert!(clock.ready());
}

#[test]
fn certified_variable_extents_remain_on_exact_original_coordinates() {
    let origin = 8192;
    let (mut clock, mut next) = ready_clock(origin, [16; 3]);
    for frames in [1, 2, 3, 127, 128, 257, 4096, 65_536, 1_048_576] {
        assert_eq!(block(&mut clock, origin, next, frames), next);
        next += u64::from(frames);
    }
    assert!(clock.ready());
}

#[test]
fn certified_wrong_index_zero_extent_and_wrong_tail_are_atomic_refusals() {
    let origin = 8192;
    let (mut clock, next) = ready_clock(origin, [3; 3]);
    let before = clock;
    for (edge, frames) in [(1, 128), (127, 128), (128, 128), (0, 0)] {
        assert_eq!(
            clock.frame(
                phases(next - origin),
                phases(next - origin + 127),
                edge,
                frames
            ),
            Err(NativeClockError::Invalid)
        );
        assert_eq!(clock, before);
    }
    for wrong_tail in [126, 128] {
        assert_eq!(
            clock.frame(
                phases(next - origin),
                phases(next - origin + wrong_tail),
                0,
                128
            ),
            Err(NativeClockError::Chronology)
        );
        assert_eq!(clock, before);
    }
    assert_eq!(block(&mut clock, origin, next, 128), next);
}

#[test]
fn certified_endpoints_decode_across_all_distinct_period_resets() {
    let origin = 8192;
    for period in NATIVE_PERIODS {
        let (mut clock, _) = ready_clock(origin, [8; 3]);
        let base = origin + u64::from(period) - 4;
        assert_eq!(block(&mut clock, origin, base, 9), base);
        assert_eq!(block(&mut clock, origin, base + 9, 1), base + 9);
    }
    let mut clock = NativeClock::new(2_000_000, horizons(2_000_000, [8; 3])).unwrap();
    let frames = NATIVE_PERIODS[0] + 123;
    assert_eq!(
        clock.frame(phases(0), phases(u64::from(frames - 65)), 64, frames),
        Ok(1_999_936)
    );
    assert_eq!(
        block(&mut clock, 2_000_000, 2_000_000 + u64::from(frames - 64), 3),
        2_000_000 + u64::from(frames - 64)
    );
    assert!(clock.ready());
}

#[test]
fn certified_forward_gap_is_observable_but_overlap_and_backtrack_refuse() {
    let origin = 8192;
    let (mut clock, next) = ready_clock(origin, [3; 3]);
    let gap_base = next + 1000;
    assert_eq!(block(&mut clock, origin, gap_base, 17), gap_base);
    let before = clock;
    for base in [gap_base - 1, gap_base, gap_base + 16] {
        assert_eq!(
            clock.frame(phases(base - origin), phases(base - origin + 7), 0, 8),
            Err(NativeClockError::Chronology)
        );
        assert_eq!(clock, before);
    }
    assert_eq!(block(&mut clock, origin, gap_base + 17, 1), gap_base + 17);
}

#[test]
fn each_lane_can_own_common_horizon_and_only_the_active_extent_must_precede_it() {
    let origin = 8192;
    for turns in [[1, 2, 3], [2, 1, 3], [3, 2, 1]] {
        let committed = horizons(origin, turns);
        let common = *committed.iter().min().unwrap();
        let (mut clock, _) = ready_clock(origin, turns);
        let before = clock;
        assert_eq!(
            clock.frame(
                phases(common - origin - 4),
                phases(common - origin + 3),
                0,
                8
            ),
            Err(NativeClockError::Chronology)
        );
        assert_eq!(clock, before);
        assert_eq!(block(&mut clock, origin, common - 4, 4), common - 4);
        let expired = clock;
        assert_eq!(
            clock.frame(phases(common - origin), phases(common - origin), 0, 1),
            Err(NativeClockError::Chronology)
        );
        assert_eq!(clock, expired);
        let extended =
            std::array::from_fn(|lane| committed[lane] + u64::from(NATIVE_PERIODS[lane]));
        assert_eq!(clock.extend(extended), Err(NativeClockError::Chronology));
        assert_eq!(clock, expired);
    }
}

#[test]
fn first_partial_tail_crossing_horizon_is_not_admitted() {
    let origin = 8192;
    let mut clock = NativeClock::new(origin, horizons(origin, [1; 3])).unwrap();
    let before = clock;
    let common_offset = u64::from(NATIVE_PERIODS[2]);
    let frames = NATIVE_PERIODS[2] + 65;
    assert_eq!(
        clock.frame(phases(0), phases(common_offset), 64, frames),
        Err(NativeClockError::Chronology)
    );
    assert_eq!(clock, before);
    assert!(!clock.ready());
}

#[test]
fn whole_extension_before_native_readiness_is_rejected_without_mutation() {
    let origin = 8192;
    let mut clock = NativeClock::new(origin, horizons(origin, [3; 3])).unwrap();
    for after_first in [false, true] {
        if after_first {
            clock.frame(phases(0), phases(63), 64, 128).unwrap();
        }
        let before = clock;
        assert_eq!(
            clock.extend(horizons(origin, [4; 3])),
            Err(NativeClockError::State)
        );
        assert_eq!(clock, before);
    }
}

#[test]
fn whole_extension_preserves_origin_and_allows_native_blocks_beyond_old_horizons() {
    let origin = 8192;
    let old = horizons(origin, [3; 3]);
    let (mut clock, next) = ready_clock(origin, [3; 3]);
    clock.extend(horizons(origin, [5, 4, 6])).unwrap();
    assert!(clock.ready());
    assert_eq!(block(&mut clock, origin, next, 128), next);
    let past_every_old = *old.iter().max().unwrap() + 17;
    assert_eq!(
        block(&mut clock, origin, past_every_old, 128),
        past_every_old
    );
    assert_eq!(
        block(&mut clock, origin, past_every_old + 128, 1),
        past_every_old + 128
    );
}

#[test]
fn invalid_whole_extensions_are_atomic_and_cannot_raise_only_one_lane() {
    let origin = 8192;
    let old = horizons(origin, [3; 3]);
    let valid = horizons(origin, [4; 3]);
    let mut cases = vec![old];
    for lane in 0..3 {
        let mut partial = old;
        partial[lane] = valid[lane];
        cases.push(partial);
        let mut backward = valid;
        backward[lane] = old[lane] - u64::from(NATIVE_PERIODS[lane]);
        cases.push(backward);
        let mut fractional = valid;
        fractional[lane] += 1;
        cases.push(fractional);
        let mut unsafe_geometry = valid;
        unsafe_geometry[lane] = origin
            + ((MAX_NATIVE_FRAME - origin) / u64::from(NATIVE_PERIODS[lane]) + 1)
                * u64::from(NATIVE_PERIODS[lane]);
        cases.push(unsafe_geometry);
    }
    let common = *old.iter().min().unwrap();
    for proposed in cases {
        let (mut clock, _) = ready_clock(origin, [3; 3]);
        let before = clock;
        assert!(clock.extend(proposed).is_err(), "geometry {proposed:?}");
        assert_eq!(clock, before);
        assert_eq!(
            clock.frame(phases(common - origin), phases(common - origin), 0, 1),
            Err(NativeClockError::Chronology)
        );
        assert_eq!(clock, before);
        clock.extend(valid).unwrap();
        assert_eq!(block(&mut clock, origin, common, 1), common);
    }
}

#[test]
fn repeated_whole_extensions_preserve_coordinate_association_across_own_boundaries() {
    let origin = 8192;
    let (mut clock, _) = ready_clock(origin, [3; 3]);
    for turn in 4..12 {
        let old = horizons(origin, [turn - 1; 3]);
        let new = horizons(origin, [turn; 3]);
        clock.extend(new).unwrap();
        let base = *old.iter().max().unwrap() + 1;
        assert_eq!(block(&mut clock, origin, base, 16), base);
        assert!(clock.ready());
    }
}

#[test]
fn number_safe_absolute_origin_does_not_get_narrowed_at_u32_or_f64_boundaries() {
    for origin in [
        1u64 << 32,
        (1u64 << 48) + 65_535,
        MAX_NATIVE_FRAME - 3 * u64::from(NATIVE_PERIODS[0]),
    ] {
        let (mut clock, next) = ready_clock(origin, [3; 3]);
        assert_eq!(block(&mut clock, origin, next, 4096), next);
        let before = clock;
        let unsafe_offset = MAX_NATIVE_FRAME;
        assert_eq!(
            clock.frame(phases(unsafe_offset), phases(unsafe_offset), 0, 1),
            Err(NativeClockError::Overflow)
        );
        assert_eq!(clock, before);
    }
}

#[test]
fn twenty_hour_and_week_coordinates_use_exact_integer_frames_without_claiming_aged_native_io() {
    for sample_rate in [44_100u64, 48_000] {
        for seconds in [20 * 60 * 60, 7 * 24 * 60 * 60] {
            let absolute = sample_rate * seconds;
            assert_eq!(joint_offset(phases(absolute)), Ok(absolute));
            assert_eq!(crt_oracle(phases(absolute)), u128::from(absolute));
            let (mut aged_origin, next) = ready_clock(absolute, [3; 3]);
            assert_eq!(block(&mut aged_origin, absolute, next, 128), next);
            let origin = 8192;
            let turns = NATIVE_PERIODS.map(|p| absolute / u64::from(p) + 2);
            let (mut long_extent, _) = ready_clock(origin, turns);
            assert_eq!(block(&mut long_extent, origin, absolute, 4096), absolute);
        }
    }
}

fn real_audio() -> WorkletAudio {
    let format = AudioFormat::new(1000, 1).unwrap();
    let mut builder = WorkletAudioBuilder::new(WorkletAudioConfig {
        format,
        pcm_limits: PcmLimits::new(64, 64, 1).unwrap(),
        audio_limits: AudioLimits::new(4, 2, 4, 4096, 4).unwrap(),
    })
    .unwrap();
    builder
        .insert_sample(SampleId(1), format, vec![0.25, 0.5, 0.75, -0.25])
        .unwrap();
    let mut audio = builder.finish().unwrap();
    audio
        .enqueue(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    audio
}

fn join_ready(clock: &mut NativeClock, audio: &mut WorkletAudio, origin: u64) -> u64 {
    let first = clock.frame(phases(0), phases(63), 64, 128).unwrap();
    audio.render(first, 128).unwrap();
    assert!(!clock.ready());
    assert!(audio.report().is_none());
    assert!(audio.output().iter().all(|&sample| sample == 0.0));
    let second = block(clock, origin, origin + 64, 128);
    audio.render(second, 128).unwrap();
    assert!(clock.ready());
    assert!(audio.report().is_none());
    assert_eq!(audio.context_frame(), Some(origin + 192));
    origin + 192
}

#[test]
fn actual_worklet_owner_arms_at_decoded_callback_and_preserves_original_report_grid() {
    let origin = (1u64 << 32) + 8192;
    let mut clock = NativeClock::new(origin, horizons(origin, [3; 3])).unwrap();
    let mut audio = real_audio();
    let next = join_ready(&mut clock, &mut audio, origin);
    let native_base = block(&mut clock, origin, next, 8);
    let start = native_base + 3;
    audio.arm(start, native_base).unwrap();
    audio.render(native_base, 8).unwrap();
    assert_eq!(
        &audio.output()[..8],
        &[0.0, 0.0, 0.0, 0.25, 0.5, 0.75, -0.25, 0.0]
    );
    let report = audio.report().unwrap();
    assert_eq!(report.start_frame, 0);
    assert_eq!(report.frames, 5);
    assert_eq!(report.counters.commands_consumed, 1);
    assert_eq!(report.counters.commands_applied, 1);
    assert_eq!(audio.start_frame(), Some(start));
    assert_eq!(audio.context_frame(), Some(native_base + 8));
    assert_eq!(
        start + report.start_frame + report.frames as u64,
        audio.context_frame().unwrap()
    );
    audio.render(native_base + 8, 0).unwrap();
    assert_eq!(audio.context_frame(), Some(native_base + 8));
    let following = block(&mut clock, origin, native_base + 8, 1);
    audio.render(following, 1).unwrap();
    assert_eq!(audio.report().unwrap().start_frame, 5);
    assert_eq!(audio.context_frame(), Some(following + 1));
}

#[test]
fn actual_armed_worklet_owner_fences_native_forward_gap_without_clock_repair() {
    let origin = 8192;
    let mut clock = NativeClock::new(origin, horizons(origin, [3; 3])).unwrap();
    let mut audio = real_audio();
    let next = join_ready(&mut clock, &mut audio, origin);
    let base = block(&mut clock, origin, next, 4);
    audio.arm(base, base).unwrap();
    audio.render(base, 4).unwrap();
    let report = audio.report();
    let context = audio.context_frame();
    let gap = block(&mut clock, origin, base + 5, 4);
    assert_eq!(
        gap,
        base + 5,
        "native frame cannot be silently made contiguous"
    );
    assert_eq!(audio.render(gap, 4), Err(WorkletAudioError::Chronology));
    assert!(audio.failed());
    assert!(audio.output().iter().all(|&sample| sample == 0.0));
    assert_eq!(audio.context_frame(), context);
    assert_eq!(audio.report(), report);
    assert_eq!(audio.start_frame(), Some(base));
    let following = block(&mut clock, origin, gap + 4, 4);
    assert_eq!(audio.render(following, 4), Err(WorkletAudioError::Failed));
    assert_eq!(
        audio.arm(following, following),
        Err(WorkletAudioError::Failed)
    );
}

#[test]
fn actual_unarmed_worklet_owner_accepts_visible_native_gap_then_arms_new_actual_callback() {
    let origin = 8192;
    let mut clock = NativeClock::new(origin, horizons(origin, [3; 3])).unwrap();
    let mut audio = real_audio();
    let next = join_ready(&mut clock, &mut audio, origin);
    let gap = block(&mut clock, origin, next + 1000, 17);
    audio.render(gap, 17).unwrap();
    assert!(!audio.failed());
    assert!(audio.report().is_none());
    assert!(audio.output().iter().all(|&sample| sample == 0.0));
    let native_base = block(&mut clock, origin, gap + 17, 4);
    audio.arm(native_base, native_base).unwrap();
    audio.render(native_base, 4).unwrap();
    assert_eq!(&audio.output()[..4], &[0.25, 0.5, 0.75, -0.25]);
    assert_eq!(audio.start_frame(), Some(native_base));
    assert_eq!(audio.context_frame(), Some(native_base + 4));
}
