//! The production bounded cache retains original non-Clone evidence, not a clock.
use super::*;
use crate::{
    gameplay::output::ports::{NativeSourcePlaybackQuery, SourcePlaybackFacts},
    native_audio_presentation::NativeAudioSnapshot,
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
};
use beatkernel::{
    audio::{InterpolatedTargetTime, SourcePlaybackSpan, SourcePlaybackState, TargetTime},
    time::{ClockPair, Timestamp},
};
use beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence;
use std::{cell::Cell, rc::Rc};

struct Evidence {
    words: Box<[u64; 2]>,
    drops: Rc<Cell<usize>>,
}
impl Drop for Evidence {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}
#[derive(Clone, Copy)]
struct Header {
    epoch: u64,
    basis: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    frame: u64,
    host: ClockPoint,
    revision: u64,
    pulled: u64,
    submitted: u64,
}
fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn header(frame: u64) -> Header {
    Header {
        epoch: 8,
        basis: OutputFrameBasis::new(point(2, 100), 1000, 17).unwrap(),
        identity: SourcePlaybackIdentity::new(1000, 5, 3).unwrap(),
        frame,
        host: point(1, 1000 + frame as i64 * 100),
        revision: frame * 2,
        pulled: 100 + frame,
        submitted: 80 + frame,
    }
}
fn observation(h: Header, drops: &Rc<Cell<usize>>) -> NativePlaybackObservation<Evidence> {
    let coordinate = CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: h.epoch,
            basis: h.basis,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                source: h.basis.point_at_stream_frame(h.frame).unwrap(),
                target: h.host,
            }),
        },
        NativeOutputCoordinate::StreamFrame { frame: h.frame },
    )
    .unwrap();
    let query = InterpolatedTargetTime::new(
        TargetTime::from_frames(h.basis.start_physical_frame() + h.frame, h.identity.hz()).unwrap(),
        0,
        0,
        1,
        0,
        1,
    )
    .unwrap();
    NativePlaybackObservation::new(
        coordinate,
        NativeSourcePlaybackQuery::new(
            h.epoch,
            h.basis,
            h.identity,
            query,
            SourcePlaybackFacts {
                playback: SourcePlaybackSpan::new(9, None, 7, SourcePlaybackState::Frozen).unwrap(),
                pulled_source_end: h.pulled,
                submitted_source_end: h.submitted,
                render: None,
                revision: h.revision,
            },
            Some(Evidence {
                words: Box::new([h.revision, h.frame]),
                drops: drops.clone(),
            }),
        )
        .unwrap(),
    )
    .unwrap()
}
fn cache() -> PendingSourceFrameObservations<Evidence> {
    let h = header(1);
    PendingSourceFrameObservations::new(h.epoch, h.basis, h.identity, ClockDomainId(1))
}
fn take(
    cache: &mut PendingSourceFrameObservations<Evidence>,
    frame: u64,
) -> NativePlaybackObservation<Evidence> {
    cache.take_covered(header(frame).host).unwrap().unwrap()
}

#[test]
fn continuous_future_callbacks_retain_original_two_values_until_their_own_host_is_covered() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = cache();
    let a = observation(header(1), &drops);
    let b = observation(header(2), &drops);
    let a_pointer = a.source().native_evidence().unwrap().words.as_ptr();
    let b_pointer = b.source().native_evidence().unwrap().words.as_ptr();
    let a_coordinate = a.coordinate();
    let b_coordinate = b.coordinate();
    cache.push(a).unwrap();
    cache.push(b).unwrap();
    for frame in 3..=20 {
        cache.push(observation(header(frame), &drops)).unwrap();
        assert!(cache.take_covered(point(1, 1099)).unwrap().is_none());
    }
    let a = take(&mut cache, 1);
    assert_eq!(a.coordinate(), a_coordinate);
    assert_eq!(a.source().identity(), header(1).identity);
    assert_eq!(
        a.source().native_evidence().unwrap().words.as_ptr(),
        a_pointer
    );
    assert_eq!(*a.source().native_evidence().unwrap().words, [2, 1]);
    assert!(cache.take_covered(point(1, 1199)).unwrap().is_none());
    let b = take(&mut cache, 2);
    assert_eq!(b.coordinate(), b_coordinate);
    assert_eq!(
        b.source().native_evidence().unwrap().words.as_ptr(),
        b_pointer
    );
    assert_eq!(b.source().facts().revision, 4);
    assert!(cache.take_covered(point(1, 9999)).unwrap().is_none());
    assert_eq!(drops.get(), 18);
    drop((a, b));
    assert_eq!(drops.get(), 20);
}

#[test]
fn full_queue_dropped_c_remains_eligible_on_identical_reread_after_a_is_released() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = cache();
    for frame in 1..=3 {
        cache.push(observation(header(frame), &drops)).unwrap();
    }
    let a = take(&mut cache, 1);
    let c = observation(header(3), &drops);
    let c_pointer = c.source().native_evidence().unwrap().words.as_ptr();
    cache.push(c).unwrap();
    let b = take(&mut cache, 2);
    let c = take(&mut cache, 3);
    assert_eq!(
        c.source().native_evidence().unwrap().words.as_ptr(),
        c_pointer
    );
    assert_eq!(*c.source().native_evidence().unwrap().words, [6, 3]);
    assert_eq!(a.coordinate(), observation_coordinate(header(1)));
    assert_eq!(b.coordinate(), observation_coordinate(header(2)));
    assert!(cache.take_covered(point(1, 9999)).unwrap().is_none());
}
fn observation_coordinate(h: Header) -> CoordinateNativeAudioSnapshot {
    let drops = Rc::new(Cell::new(0));
    observation(h, &drops).coordinate()
}

#[test]
fn queued_and_delivered_duplicates_are_never_returned_twice_or_refresh_a_timestamp() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = cache();
    for _ in 0..4 {
        cache.push(observation(header(1), &drops)).unwrap();
    }
    assert!(cache.take_covered(point(1, 1099)).unwrap().is_none());
    let first = take(&mut cache, 1);
    for _ in 0..4 {
        cache.push(observation(header(1), &drops)).unwrap();
    }
    assert!(cache.take_covered(point(1, 9999)).unwrap().is_none());
    cache.push(observation(header(2), &drops)).unwrap();
    assert_eq!(take(&mut cache, 2).source().facts().revision, 4);
    assert_eq!(first.coordinate(), observation_coordinate(header(1)));
}

#[test]
fn invalid_first_future_or_full_cache_header_refuses_atomically() {
    let drops = Rc::new(Cell::new(0));
    for occupied in 0..=2 {
        for invalid in 0..7 {
            let mut cache = cache();
            for frame in 1..=occupied {
                cache.push(observation(header(frame), &drops)).unwrap();
            }
            let mut bad = header(3);
            match invalid {
                0 => bad.epoch += 1,
                1 => bad.basis = OutputFrameBasis::new(point(2, 101), 1000, 17).unwrap(),
                2 => bad.identity = SourcePlaybackIdentity::new(1000, 5, 4).unwrap(),
                3 => bad.host.domain = ClockDomainId(77),
                4 => bad.basis = OutputFrameBasis::new(point(77, 100), 1000, 17).unwrap(),
                5 => bad.revision = 0,
                _ => bad.basis = OutputFrameBasis::new(point(2, 100), 1000, 18).unwrap(),
            }
            assert!(
                cache.push(observation(bad, &drops)).is_err(),
                "occupied={occupied}, invalid={invalid}"
            );
            for frame in 1..=occupied {
                assert_eq!(
                    take(&mut cache, frame).coordinate(),
                    observation_coordinate(header(frame))
                );
            }
            assert!(cache.take_covered(point(1, 9999)).unwrap().is_none());
            // A rejected read must not poison a later valid read.
            cache.push(observation(header(3), &drops)).unwrap();
            assert_eq!(take(&mut cache, 3).source().facts().revision, 6);
        }
    }
}

#[test]
fn incoherent_revision_prefix_source_and_host_progress_refuse_even_while_full() {
    let drops = Rc::new(Cell::new(0));
    for invalid in 0..8 {
        let mut cache = cache();
        cache.push(observation(header(1), &drops)).unwrap();
        cache.push(observation(header(2), &drops)).unwrap();
        let mut bad = header(3);
        match invalid {
            0 => bad.revision = 2,
            1 => bad.revision = 4,
            2 => bad.pulled = header(2).pulled - 1,
            3 => bad.submitted = header(2).submitted - 1,
            4 => bad.frame = 1,
            5 => bad.host.timestamp = header(1).host.timestamp,
            6 => bad.host.timestamp = header(2).host.timestamp,
            _ => {
                bad = header(2);
                bad.pulled += 1;
            }
        }
        assert!(
            cache.push(observation(bad, &drops)).is_err(),
            "invalid={invalid}"
        );
        assert_eq!(
            take(&mut cache, 1).coordinate(),
            observation_coordinate(header(1))
        );
        assert_eq!(
            take(&mut cache, 2).coordinate(),
            observation_coordinate(header(2))
        );
        cache.push(observation(header(3), &drops)).unwrap();
        assert_eq!(
            take(&mut cache, 3).coordinate(),
            observation_coordinate(header(3))
        );
    }
}

#[test]
fn later_host_without_source_progress_cannot_replace_or_add_a_selected_anchor() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = cache();
    cache.push(observation(header(2), &drops)).unwrap();
    let mut unchanged = header(3);
    unchanged.frame = 2;
    unchanged.host.timestamp = Timestamp::from_nanos(1250);
    cache.push(observation(unchanged, &drops)).unwrap();
    let original = take(&mut cache, 2);
    assert_eq!(original.coordinate(), observation_coordinate(header(2)));
    assert!(cache.take_covered(point(1, 9999)).unwrap().is_none());
    let mut progressed = header(4);
    progressed.frame = 3;
    progressed.host.timestamp = Timestamp::from_nanos(1300);
    cache.push(observation(progressed, &drops)).unwrap();
    assert_eq!(
        take(&mut cache, 3).coordinate(),
        observation_coordinate(progressed)
    );
}

#[test]
fn wrong_poll_domain_is_atomic_and_clear_releases_owned_evidence_and_validation_history() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = cache();
    cache.push(observation(header(2), &drops)).unwrap();
    assert!(cache.take_covered(point(77, 9999)).is_err());
    assert!(cache.take_covered(point(1, 1199)).unwrap().is_none());
    cache.clear();
    assert_eq!(drops.get(), 1);
    assert!(cache.take_covered(point(1, 9999)).unwrap().is_none());
    cache.push(observation(header(1), &drops)).unwrap();
    assert_eq!(take(&mut cache, 1).source().identity(), header(1).identity);
}
