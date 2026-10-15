//! Hot mathematical mapping heap requests; not CPU/native latency evidence.
use beatkernel::{
    audio::{
        ConvertedPhaseSpan, InterpolatedTargetTime, OutputFrameBasis, SourcePlaybackSpan,
        SourcePlaybackState, TargetFrameBasis, TargetTime,
    },
    time::{ClockDomainId, ClockPoint, Timestamp},
    transport::OutputPhaseBasis,
};
use beatkernel_bms_runtime::{
    audio_authority::{
        AudioAuthority, AudioAuthorityConfig, AudioAuthorityCoordinateBasis, AudioAuthorityEpoch,
    },
    gameplay::output::ports::{
        NativeSourcePlaybackQuery, SourcePlaybackFacts, SourcePlaybackIdentity,
    },
    native_audio_presentation::NativeAudioPresentation,
    native_playback_phase::{
        ConvertedPlaybackFacts, NativePlaybackMapping, NativePlaybackPhaseError,
    },
};
use beatkernel_platform::audio::presentation::validation::NativePresentationValidator;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
thread_local! {static TRACK:Cell<bool>=const{Cell::new(false)};static REQUESTS:Cell<usize>=const{Cell::new(0)};}
fn request() {
    let _ = TRACK.try_with(|active| {
        if active.get() {
            let _ = REQUESTS.try_with(|count| count.set(count.get() + 1));
        }
    });
}
struct Allocator;
// SAFETY: Every allocation argument passes unchanged to System; TLS holds scalars.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        request();
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        request();
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        request();
        unsafe { System.realloc(p, l, n) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        request();
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        TRACK.with(|active| active.set(false));
    }
}
fn measure<T>(f: impl FnOnce() -> T) -> (T, usize) {
    TRACK.with(|active| assert!(!active.get()));
    REQUESTS.with(|count| count.set(0));
    TRACK.with(|active| active.set(true));
    let guard = Guard;
    let result = f();
    drop(guard);
    (result, REQUESTS.with(Cell::get))
}
fn calibrate() {
    let (_, count) = measure(|| {
        let mut known = Vec::<u64>::with_capacity(std::hint::black_box(4));
        known.push(std::hint::black_box(1));
        known.reserve(std::hint::black_box(128));
        drop(known);
    });
    assert!(
        count >= 3,
        "allocation/reallocation/deallocation must be observable"
    );
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn presentation(descriptor: AudioAuthorityCoordinateBasis) -> NativeAudioPresentation {
    let origin = match descriptor {
        AudioAuthorityCoordinateBasis::SourceFrame(b)
        | AudioAuthorityCoordinateBasis::SourceCounter { basis: b, .. } => {
            b.point_at_stream_frame(0).unwrap()
        }
        AudioAuthorityCoordinateBasis::TargetFrame(b) => b.point_at_stream_frame(0).unwrap(),
        AudioAuthorityCoordinateBasis::SourceTimestamp(b) => {
            // Cold origin arithmetic grants no native admission or frame evidence.
            b.point_at_stream_frame(0).unwrap()
        }
    };
    NativeAudioPresentation::new(
        AudioAuthority::new_with_coordinates(
            AudioAuthorityConfig::default(),
            AudioAuthorityEpoch {
                id: 7,
                stream_origin: origin,
                logical_origin: point(3, 500),
                host_domain: ClockDomainId(1),
            },
            descriptor,
        )
        .unwrap(),
        NativePresentationValidator::new(7, origin, ClockDomainId(1)),
    )
    .unwrap()
}
fn id() -> SourcePlaybackIdentity {
    SourcePlaybackIdentity::new(3, 0, 0).unwrap()
}
fn attempt() -> OutputPhaseBasis {
    OutputPhaseBasis::new(ClockDomainId(3), 11, 17, 1, 3).unwrap()
}
fn running() -> SourcePlaybackSpan {
    SourcePlaybackSpan::new(0, None, 0, SourcePlaybackState::Running).unwrap()
}

#[test]
fn borrowed_source_mapping_exact_phase_and_refusals_request_no_heap() {
    calibrate();
    let basis = OutputFrameBasis::new(point(2, 100), 3, 1).unwrap();
    let p = presentation(AudioAuthorityCoordinateBasis::SourceFrame(basis));
    let q = InterpolatedTargetTime::new(
        TargetTime::new(0, 3_333_333_348, 10_000_000_000).unwrap(),
        0,
        0,
        1,
        0,
        1,
    )
    .unwrap();
    // Deliberately non-Copy/non-Clone evidence remains cold-owned and borrowed.
    struct Evidence(Vec<u64>);
    let packet = NativeSourcePlaybackQuery::new(
        7,
        basis,
        id(),
        q,
        SourcePlaybackFacts {
            playback: running(),
            pulled_source_end: 4,
            submitted_source_end: 4,
            render: None,
            revision: 2,
        },
        Some(Evidence(vec![7, 13])),
    )
    .unwrap();
    let pointer = packet.native_evidence().unwrap().0.as_ptr();
    let ((elapsed, rejected), requests) = measure(|| {
        let mut elapsed = 0;
        let mut rejected = 0;
        for _ in 0..64 {
            let mapping = NativePlaybackMapping::from_source(std::hint::black_box(&packet));
            elapsed += mapping
                .phase(&p, id(), &q, attempt(), point(3, 501))
                .unwrap()
                .elapsed_nanos();
            rejected += usize::from(
                mapping
                    .phase(&p, id(), &q, attempt(), point(3, 502))
                    .is_err(),
            );
        }
        (elapsed, rejected)
    });
    assert_eq!((elapsed, rejected), (64, 64));
    assert_eq!(requests, 0);
    assert_eq!(packet.native_evidence().unwrap().0.as_ptr(), pointer);
}
#[test]
fn converted_mapping_checked_bounds_phase_and_refusals_request_no_heap() {
    calibrate();
    let basis =
        TargetFrameBasis::new(point(2, 100), TargetTime::from_frames(1, 3).unwrap(), 7).unwrap();
    let p = presentation(AudioAuthorityCoordinateBasis::TargetFrame(basis));
    let q = InterpolatedTargetTime::new(TargetTime::new(0, 1, 2).unwrap(), 0, 0, 1, 0, 1).unwrap();
    let facts = ConvertedPlaybackFacts {
        phase: ConvertedPhaseSpan::from_words([3, 1, 1, 2, 0, 1, 3, 0]).unwrap(),
        segment_end: TargetTime::new(1, 0, 1).unwrap(),
        generated_target_end: TargetTime::new(1, 0, 1).unwrap(),
        submitted_target_end: TargetTime::new(0, 2, 3).unwrap(),
        playback: running(),
        pulled_source_end: 4,
        revision: 2,
    };
    let mut invalid = facts;
    invalid.segment_end = TargetTime::new(0, 1, 2).unwrap();
    let ((elapsed, rejected), requests) = measure(|| {
        let mut elapsed = 0;
        let mut rejected = 0;
        for _ in 0..64 {
            let mapping =
                NativePlaybackMapping::converted(7, basis, id(), q, std::hint::black_box(facts))
                    .unwrap();
            elapsed += mapping
                .phase(&p, id(), &q, attempt(), point(3, 166_667_167))
                .unwrap()
                .elapsed_nanos();
            rejected +=
                usize::from(NativePlaybackMapping::converted(7, basis, id(), q, invalid).is_err());
        }
        (elapsed, rejected)
    });
    assert_eq!((elapsed, rejected), (64 * 333_333_333, 64));
    assert_eq!(requests, 0);
}

#[test]
fn raw_timestamp_descriptor_cannot_qualify_native_source_mapping_without_heap_requests() {
    calibrate();
    let basis = OutputFrameBasis::new(point(2, 100), 3, 0).unwrap();
    let p = presentation(AudioAuthorityCoordinateBasis::SourceTimestamp(basis));
    let q = InterpolatedTargetTime::new(TargetTime::new(0, 1, 3).unwrap(), 0, 0, 1, 0, 1).unwrap();
    let packet = NativeSourcePlaybackQuery::new(
        7,
        basis,
        id(),
        q,
        SourcePlaybackFacts {
            playback: running(),
            pulled_source_end: 4,
            submitted_source_end: 4,
            render: None,
            revision: 2,
        },
        None::<()>,
    )
    .unwrap();
    let mapping = NativePlaybackMapping::from_source(&packet);
    let (_, requests) = measure(|| {
        for _ in 0..64 {
            assert_eq!(
                mapping
                    .phase(&p, id(), &q, attempt(), point(3, 333_333_833))
                    .unwrap_err(),
                NativePlaybackPhaseError::Descriptor,
            );
        }
    });
    assert_eq!(requests, 0);
    assert_eq!(p.latest_record(), None);
    assert_eq!(p.basis(), None);
    assert_eq!(p.target_basis(), None);
    assert_eq!(p.authority().history_len(), 0);
    assert_eq!(p.authority().latest_coordinate_observation(), None);
    assert!(p.authority().committed_coordinate_operation().is_none());
    assert!(p.authority().committed_coordinate_presentation().is_none());
}
