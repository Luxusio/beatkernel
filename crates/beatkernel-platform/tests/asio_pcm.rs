//! Handwritten planar PCM fixtures; no native buffer or driver is constructed.
use beatkernel_platform::audio::asio::{
    encode_asio_channel, AsioPcmEncoding, AsioPcmEncoding::*, AsioPcmError,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<[usize; 3]> = const { Cell::new([0; 3]) };
}
struct Allocator;
fn count(index: usize) {
    let _ = TRACK.try_with(|track| {
        if track.get() {
            let _ = COUNTS.try_with(|counts| {
                let mut values = counts.get();
                values[index] += 1;
                counts.set(values);
            });
        }
    });
}
// SAFETY: The allocator forwards unchanged pointers/layouts to System;
// instrumentation only accesses allocation-free thread-local scalar cells.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(0);
        // SAFETY: Caller supplies a valid allocation layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(0);
        // SAFETY: Caller supplies a valid allocation layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(1);
        // SAFETY: Caller supplies a live System allocation, matching layout,
        // and valid nonzero replacement size under GlobalAlloc's contract.
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count(2);
        // SAFETY: Caller supplies the live original allocation and layout.
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn tracked<T>(operation: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|counts| counts.set([0; 3]));
    TRACK.with(|track| track.set(true));
    let result = operation();
    TRACK.with(|track| track.set(false));
    (result, COUNTS.with(Cell::get))
}

const ENCODINGS: [AsioPcmEncoding; 18] = [
    Int16Msb, Int24Msb, Int32Msb, Float32Msb, Float64Msb, Int32Msb16, Int32Msb18, Int32Msb20,
    Int32Msb24, Int16Lsb, Int24Lsb, Int32Lsb, Float32Lsb, Float64Lsb, Int32Lsb16, Int32Lsb18,
    Int32Lsb20, Int32Lsb24,
];

#[test]
fn native_pcm_identities_and_widths_are_literal_and_dsd_is_explicitly_unsupported() {
    let identities = [
        0, 1, 2, 3, 4, 8, 9, 10, 11, 16, 17, 18, 19, 20, 24, 25, 26, 27,
    ];
    let widths = [2, 3, 4, 4, 8, 4, 4, 4, 4, 2, 3, 4, 4, 8, 4, 4, 4, 4];
    for ((encoding, identity), width) in ENCODINGS.into_iter().zip(identities).zip(widths) {
        assert_eq!(AsioPcmEncoding::from_native(identity), Ok(encoding));
        assert_eq!(encoding.native_type(), identity);
        assert_eq!(encoding.bytes_per_sample(), width);
    }
    for sample_type in [-1, i32::MIN, 5, 7, 12, 15, 21, 28, 32, 33, 40, i32::MAX] {
        assert_eq!(
            AsioPcmEncoding::from_native(sample_type),
            Err(AsioPcmError::UnsupportedSampleType { sample_type })
        );
    }
}

#[test]
fn full_width_integer_native_bytes_quantize_signed_extrema_and_half_scale() {
    let cases: &[(AsioPcmEncoding, &[u8])] = &[
        (Int16Msb, &[0x80, 0, 0xc0, 0, 0, 0, 0x40, 0, 0x7f, 0xff]),
        (Int16Lsb, &[0, 0x80, 0, 0xc0, 0, 0, 0, 0x40, 0xff, 0x7f]),
        (
            Int24Msb,
            &[
                0x80, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0x40, 0, 0, 0x7f, 0xff, 0xff,
            ],
        ),
        (
            Int24Lsb,
            &[
                0, 0, 0x80, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0x40, 0xff, 0xff, 0x7f,
            ],
        ),
        (
            Int32Msb,
            &[
                0x80, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0, 0x40, 0, 0, 0, 0x7f, 0xff, 0xff, 0xff,
            ],
        ),
        (
            Int32Lsb,
            &[
                0, 0, 0, 0x80, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0, 0x40, 0xff, 0xff, 0xff, 0x7f,
            ],
        ),
    ];
    for &(encoding, expected) in cases {
        let mut output = vec![0xa5; expected.len()];
        encode_asio_channel(encoding, &[-1.0, -0.5, 0.0, 0.5, 1.0], 1, 0, &mut output).unwrap();
        assert_eq!(output, expected, "{encoding:?}");
        let width = encoding.bytes_per_sample();
        let mut clamped = vec![0xa5; 2 * width];
        encode_asio_channel(encoding, &[-2.0, 2.0], 1, 0, &mut clamped).unwrap();
        assert_eq!(&clamped[..width], &expected[..width]);
        assert_eq!(&clamped[width..], &expected[expected.len() - width..]);
    }
}

#[test]
fn reduced_valid_bits_are_low_aligned_with_zero_unused_high_bits() {
    let cases: &[(AsioPcmEncoding, &[u8])] = &[
        (
            Int32Msb16,
            &[
                0, 0, 0x80, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0, 0x40, 0, 0, 0, 0x7f, 0xff,
            ],
        ),
        (
            Int32Lsb16,
            &[
                0, 0x80, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0, 0x40, 0, 0, 0xff, 0x7f, 0, 0,
            ],
        ),
        (
            Int32Msb18,
            &[
                0, 2, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0xff, 0xff,
            ],
        ),
        (
            Int32Lsb18,
            &[
                0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0xff, 0xff, 1, 0,
            ],
        ),
        (
            Int32Msb20,
            &[
                0, 8, 0, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 7, 0xff, 0xff,
            ],
        ),
        (
            Int32Lsb20,
            &[
                0, 0, 8, 0, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0xff, 0xff, 7, 0,
            ],
        ),
        (
            Int32Msb24,
            &[
                0, 0x80, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0, 0x40, 0, 0, 0, 0x7f, 0xff, 0xff,
            ],
        ),
        (
            Int32Lsb24,
            &[
                0, 0, 0x80, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0, 0x40, 0, 0xff, 0xff, 0x7f, 0,
            ],
        ),
    ];
    for &(encoding, expected) in cases {
        let mut output = [0xa5; 20];
        encode_asio_channel(encoding, &[-1.0, -0.5, 0.0, 0.5, 1.0], 1, 0, &mut output).unwrap();
        assert_eq!(output.as_slice(), expected, "{encoding:?}");
    }
}

#[test]
fn quantization_halfway_ties_round_away_from_zero_without_sign_extending_reduced_words() {
    for (encoding, expected) in [
        (Int16Msb, &[0, 1, 0xff, 0xff][..]),
        (Int16Lsb, &[1, 0, 0xff, 0xff][..]),
        (Int32Msb16, &[0, 0, 0, 1, 0, 0, 0xff, 0xff][..]),
        (Int32Lsb16, &[1, 0, 0, 0, 0xff, 0xff, 0, 0][..]),
    ] {
        let mut output = vec![0xa5; expected.len()];
        encode_asio_channel(
            encoding,
            &[1.0 / 65536.0, -1.0 / 65536.0],
            1,
            0,
            &mut output,
        )
        .unwrap();
        assert_eq!(output, expected);
    }
}

#[test]
fn float_native_bytes_preserve_signed_zero_overrange_and_exact_widening() {
    let cases: &[(AsioPcmEncoding, &[u8])] = &[
        (
            Float32Msb,
            &[0x80, 0, 0, 0, 0x40, 0x20, 0, 0, 0xc0, 0x20, 0, 0],
        ),
        (
            Float32Lsb,
            &[0, 0, 0, 0x80, 0, 0, 0x20, 0x40, 0, 0, 0x20, 0xc0],
        ),
        (
            Float64Msb,
            &[
                0x80, 0, 0, 0, 0, 0, 0, 0, 0x40, 4, 0, 0, 0, 0, 0, 0, 0xc0, 4, 0, 0, 0, 0, 0, 0,
            ],
        ),
        (
            Float64Lsb,
            &[
                0, 0, 0, 0, 0, 0, 0, 0x80, 0, 0, 0, 0, 0, 0, 4, 0x40, 0, 0, 0, 0, 0, 0, 4, 0xc0,
            ],
        ),
    ];
    for &(encoding, expected) in cases {
        let mut output = vec![0xa5; expected.len()];
        encode_asio_channel(encoding, &[-0.0, 2.5, -2.5], 1, 0, &mut output).unwrap();
        assert_eq!(output, expected);
    }
    let mut widened = [0xa5; 8];
    encode_asio_channel(
        Float64Msb,
        &[f32::from_bits(0x3f80_0001)],
        1,
        0,
        &mut widened,
    )
    .unwrap();
    assert_eq!(widened, [0x3f, 0xf0, 0, 0, 0x20, 0, 0, 0]);
    encode_asio_channel(
        Float64Lsb,
        &[f32::from_bits(0x3f80_0001)],
        1,
        0,
        &mut widened,
    )
    .unwrap();
    assert_eq!(widened, [0, 0, 0, 0x20, 0, 0, 0xf0, 0x3f]);
}

#[test]
fn explicit_channel_selection_ignores_unselected_nan_and_never_mixes_channels() {
    let input = [f32::NAN, -1.0, f32::INFINITY, 0.5];
    let mut output = [0xa5; 4];
    encode_asio_channel(Int16Lsb, &input, 2, 1, &mut output).unwrap();
    assert_eq!(output, [0, 0x80, 0, 0x40]);
    let mut output = [0xa5; 6];
    encode_asio_channel(
        Int16Msb,
        &[0.5, -0.5, 0.0, 1.0, -1.0, 0.0],
        2,
        0,
        &mut output,
    )
    .unwrap();
    assert_eq!(output, [0x40, 0, 0, 0, 0x80, 0]);
}

#[test]
fn selected_nonfinite_preflight_is_atomic_even_when_bad_sample_is_last() {
    for encoding in ENCODINGS {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut output = vec![0xa5; encoding.bytes_per_sample() * 3];
            assert_eq!(
                encode_asio_channel(encoding, &[0.25, 0.5, bad], 1, 0, &mut output),
                Err(AsioPcmError::NonFiniteSample)
            );
            assert!(output.iter().all(|&byte| byte == 0xa5));
        }
    }
}

#[test]
fn invalid_layout_and_output_extent_preserve_destination_while_empty_valid_input_succeeds() {
    for (input, channels, channel) in [
        (&[0.5][..], 0, 0),
        (&[0.5][..], 1, 1),
        (&[0.5, 0.0, 1.0][..], 2, 0),
    ] {
        let mut output = [0xa5; 8];
        assert_eq!(
            encode_asio_channel(Int16Lsb, input, channels, channel, &mut output),
            Err(AsioPcmError::InvalidLayout)
        );
        assert_eq!(output, [0xa5; 8]);
    }
    for size in [0, 1, 3, 4] {
        let mut output = vec![0xa5; size];
        assert_eq!(
            encode_asio_channel(Int16Lsb, &[0.5], 1, 0, &mut output),
            Err(AsioPcmError::OutputSize)
        );
        assert!(output.iter().all(|&byte| byte == 0xa5));
    }
    for encoding in ENCODINGS {
        encode_asio_channel(encoding, &[], 3, 2, &mut []).unwrap();
    }
}

#[test]
fn allocation_observer_calibration_counts_alloc_realloc_and_dealloc() {
    let (_, counts) = tracked(|| {
        let mut bytes = Vec::with_capacity(1);
        bytes.push(3_u8);
        bytes.reserve_exact(1024);
        std::hint::black_box(&bytes);
        drop(bytes);
    });
    assert!(counts.into_iter().all(|count| count > 0));
}

#[test]
fn successful_conversion_and_preflight_errors_do_not_allocate_reallocate_or_free() {
    for encoding in ENCODINGS {
        let width = encoding.bytes_per_sample();
        let mut output = [0xa5; 24];
        let (result, counts) = tracked(|| {
            encode_asio_channel(encoding, &[-0.5, 0.0, 0.5], 1, 0, &mut output[..3 * width])
        });
        assert_eq!(result, Ok(()));
        assert_eq!(counts, [0, 0, 0]);
        let (result, counts) = tracked(|| {
            encode_asio_channel(
                encoding,
                &[0.5, 0.0, f32::NAN],
                1,
                0,
                &mut output[..3 * width],
            )
        });
        assert_eq!(result, Err(AsioPcmError::NonFiniteSample));
        assert_eq!(counts, [0, 0, 0]);
    }
}
