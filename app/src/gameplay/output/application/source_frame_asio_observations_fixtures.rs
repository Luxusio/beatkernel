//! Supplied original ASIO intervals test the actual cache without driver timing claims.
use super::*;
use crate::{
    gameplay::output::ports::NativeSourcePlaybackQuery,
    native_audio_presentation::NativeAudioSnapshot,
};
use beatkernel::{
    audio::{AudioCounters, RenderReport, SourcePlaybackSpan, SourcePlaybackState},
    time::{ClockPair, Timestamp},
};
use beatkernel_platform::audio::asio::{AsioPresentationObservation, MultimediaHostInterval};
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
    render: RenderReport,
    first: u64,
    frames: usize,
    before: ClockPoint,
    after: ClockPoint,
    revision: u64,
    pulled: u64,
    submitted: u64,
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn header(index: u64) -> Header {
    let first = 17 + index * 2;
    Header {
        epoch: 8,
        basis: OutputFrameBasis::new(point(2, 100), 1000, 17).unwrap(),
        identity: SourcePlaybackIdentity::new(1000, 5, 3).unwrap(),
        render: RenderReport {
            start_frame: first,
            frames: 2,
            playback_start_frame: 7,
            playback_frames: 0,
            paused: true,
            playback_end_physical_frame: None,
            active_voices: 0,
            pending_commands: 0,
            song_position: Timestamp::ZERO,
            producer_disconnected: false,
            counters: AudioCounters::default(),
        },
        first,
        frames: 2,
        before: point(1, 1000 + index as i64 * 100),
        after: point(1, 1080 + index as i64 * 100),
        revision: (index + 1) * 2,
        pulled: first + 2,
        submitted: first + 2,
    }
}
fn observation(h: Header, drops: &Rc<Cell<usize>>) -> NativePlaybackObservation<Evidence> {
    let native = AsioPresentationObservation::from_delivery(
        h.render,
        h.first,
        h.frames,
        h.basis.sample_rate(),
        MultimediaHostInterval {
            before: h.before,
            after: h.after,
        },
        0,
        0,
        h.basis.origin(),
    )
    .unwrap();
    let coordinate = CoordinateNativeAudioSnapshot::new(
        NativeAudioSnapshot {
            epoch: h.epoch,
            basis: h.basis,
            evidence: OriginalNativePresentationEvidence::Asio {
                observation: native,
                basis: Some(h.basis),
            },
        },
        NativeOutputCoordinate::StreamFrame {
            frame: h.first - h.basis.start_physical_frame(),
        },
    )
    .unwrap();
    packet(h, coordinate, drops)
}
fn packet(
    h: Header,
    coordinate: CoordinateNativeAudioSnapshot,
    drops: &Rc<Cell<usize>>,
) -> NativePlaybackObservation<Evidence> {
    NativePlaybackObservation::new(
        coordinate,
        NativeSourcePlaybackQuery::new(
            h.epoch,
            h.basis,
            h.identity,
            coordinate.source_query().unwrap(),
            SourcePlaybackFacts {
                playback: SourcePlaybackSpan::new(9, None, 7, SourcePlaybackState::Frozen).unwrap(),
                pulled_source_end: h.pulled,
                submitted_source_end: h.submitted,
                render: Some(h.render),
                revision: h.revision,
            },
            Some(Evidence {
                words: Box::new([h.revision, h.first]),
                drops: drops.clone(),
            }),
        )
        .unwrap(),
    )
    .unwrap()
}
fn cache() -> PendingSourceFrameObservations<Evidence> {
    let h = header(0);
    PendingSourceFrameObservations::new_asio(h.epoch, h.basis, h.identity, ClockDomainId(1))
        .unwrap()
}
fn take(
    cache: &mut PendingSourceFrameObservations<Evidence>,
    index: u64,
) -> NativePlaybackObservation<Evidence> {
    cache.take_covered(header(index).after).unwrap().unwrap()
}
fn raw(value: &NativePlaybackObservation<Evidence>) -> AsioPresentationObservation {
    let OriginalNativePresentationEvidence::Asio { observation, .. } =
        value.coordinate().snapshot().evidence
    else {
        panic!("expected original ASIO interval")
    };
    observation
}

#[test]
fn asio_future_keeps_original_first_two_nonclone_packets_until_their_after_bounds_are_covered() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = cache();
    let a = observation(header(0), &drops);
    let b = observation(header(1), &drops);
    let coordinates = [a.coordinate(), b.coordinate()];
    let pointers = [
        a.source().native_evidence().unwrap().words.as_ptr(),
        b.source().native_evidence().unwrap().words.as_ptr(),
    ];
    cache.push(a).unwrap();
    cache.push(b).unwrap();
    for index in 2..20 {
        cache.push(observation(header(index), &drops)).unwrap();
        assert!(
            cache.take_covered(point(1, 1040)).unwrap().is_none(),
            "midpoint is not upper coverage"
        );
        assert!(cache.take_covered(point(1, 1079)).unwrap().is_none());
    }
    let a = take(&mut cache, 0);
    assert_eq!(a.coordinate(), coordinates[0]);
    assert_eq!(raw(&a).host.after, header(0).after);
    assert_eq!(
        a.source().native_evidence().unwrap().words.as_ptr(),
        pointers[0]
    );
    assert!(cache.take_covered(point(1, 1179)).unwrap().is_none());
    let b = take(&mut cache, 1);
    assert_eq!(b.coordinate(), coordinates[1]);
    assert_eq!(
        b.source().native_evidence().unwrap().words.as_ptr(),
        pointers[1]
    );
    assert_eq!(drops.get(), 18);
    assert!(cache.take_covered(point(1, 9999)).unwrap().is_none());
    drop((a, b));
    assert_eq!(drops.get(), 20);
}

#[test]
fn asio_full_queue_fresh_c_is_eligible_on_identical_read_after_a_release() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = cache();
    for index in 0..3 {
        cache.push(observation(header(index), &drops)).unwrap();
    }
    assert_eq!(raw(&take(&mut cache, 0)).delivered_first_frame, 17);
    let c = observation(header(2), &drops);
    let pointer = c.source().native_evidence().unwrap().words.as_ptr();
    cache.push(c).unwrap();
    assert_eq!(raw(&take(&mut cache, 1)).delivered_first_frame, 19);
    let c = take(&mut cache, 2);
    assert_eq!(
        c.source().native_evidence().unwrap().words.as_ptr(),
        pointer
    );
    assert_eq!(
        raw(&c).host,
        MultimediaHostInterval {
            before: header(2).before,
            after: header(2).after
        }
    );
    assert!(cache.take_covered(point(1, 9999)).unwrap().is_none());
}

#[test]
fn asio_adjacent_deliveries_from_one_original_render_are_distinct_anchors() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = cache();
    let mut a = header(0);
    a.render.frames = 8;
    a.pulled = 25;
    let mut b = header(1);
    b.render = a.render;
    b.pulled = 25;
    cache.push(observation(a, &drops)).unwrap();
    cache.push(observation(b, &drops)).unwrap();
    let first = take(&mut cache, 0);
    let second = take(&mut cache, 1);
    assert_eq!(raw(&first).render, raw(&second).render);
    assert_eq!(
        (
            raw(&first).delivered_first_frame,
            raw(&second).delivered_first_frame
        ),
        (17, 19)
    );
    assert_eq!(first.source().facts().render, Some(a.render));
    assert_eq!(second.source().facts().render, Some(a.render));
}

#[test]
fn asio_duplicate_with_later_host_does_not_refresh_or_return_an_original_twice() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = cache();
    cache.push(observation(header(0), &drops)).unwrap();
    let mut later = header(0);
    later.before = point(1, 9000);
    later.after = point(1, 9080);
    later.revision = 4;
    cache.push(observation(later, &drops)).unwrap();
    let original = take(&mut cache, 0);
    assert_eq!(raw(&original).host.after, header(0).after);
    cache.push(observation(later, &drops)).unwrap();
    assert!(cache.take_covered(point(1, 9999)).unwrap().is_none());
    let mut next = header(1);
    next.revision = 6;
    cache.push(observation(next, &drops)).unwrap();
    assert_eq!(raw(&take(&mut cache, 1)).delivered_first_frame, 19);
}

#[test]
fn asio_equal_midpoint_waits_for_real_host_progress_without_replacing_original_anchor() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = cache();
    cache.push(observation(header(0), &drops)).unwrap();
    let mut tied = header(1);
    tied.before = point(1, 1020);
    tied.after = point(1, 1060);
    cache.push(observation(tied, &drops)).unwrap();
    assert_eq!(raw(&take(&mut cache, 0)).host.after, header(0).after);
    assert!(cache.take_covered(point(1, 9999)).unwrap().is_none());
    let mut progressed = header(1);
    progressed.revision = 6;
    cache.push(observation(progressed, &drops)).unwrap();
    assert_eq!(raw(&take(&mut cache, 1)).host.after, header(1).after);
}

#[test]
fn asio_invalid_first_future_and_full_headers_never_poison_valid_selected_values() {
    let drops = Rc::new(Cell::new(0));
    for occupied in 0..=2 {
        for invalid in 0..7 {
            let mut cache = cache();
            for index in 0..occupied {
                cache.push(observation(header(index), &drops)).unwrap();
            }
            let mut h = header(2);
            match invalid {
                0 => h.epoch += 1,
                1 => h.basis = OutputFrameBasis::new(point(2, 101), 1000, 17).unwrap(),
                2 => h.identity = SourcePlaybackIdentity::new(1000, 5, 4).unwrap(),
                3 => {
                    h.before.domain = ClockDomainId(77);
                    h.after.domain = ClockDomainId(77);
                }
                4 => h.basis = OutputFrameBasis::new(point(77, 100), 1000, 17).unwrap(),
                5 => h.revision = 0,
                _ => h.basis = OutputFrameBasis::new(point(2, 100), 1000, 18).unwrap(),
            }
            assert!(
                cache.push(observation(h, &drops)).is_err(),
                "occupied={occupied}, invalid={invalid}"
            );
            for index in 0..occupied {
                assert_eq!(
                    raw(&take(&mut cache, index)).delivered_first_frame,
                    header(index).first
                );
            }
            assert!(cache.take_covered(point(1, 9999)).unwrap().is_none());
            cache.push(observation(header(2), &drops)).unwrap();
            assert_eq!(
                raw(&take(&mut cache, 2)).delivered_first_frame,
                header(2).first
            );
        }
    }
}

#[test]
fn asio_cache_rejects_pair_family_on_first_or_full_read_and_pair_cache_rejects_asio() {
    let drops = Rc::new(Cell::new(0));
    for occupied in 0..=2 {
        let mut cache = cache();
        for index in 0..occupied {
            cache.push(observation(header(index), &drops)).unwrap();
        }
        let h = header(2);
        let coordinate = CoordinateNativeAudioSnapshot::new(
            NativeAudioSnapshot {
                epoch: h.epoch,
                basis: h.basis,
                evidence: OriginalNativePresentationEvidence::SuppliedPair(ClockPair {
                    source: h.basis.point_at_stream_frame(h.first - 17).unwrap(),
                    target: h.before,
                }),
            },
            NativeOutputCoordinate::StreamFrame {
                frame: h.first - 17,
            },
        )
        .unwrap();
        assert!(cache.push(packet(h, coordinate, &drops)).is_err());
        for index in 0..occupied {
            take(&mut cache, index);
        }
        cache.push(observation(h, &drops)).unwrap();
        assert_eq!(raw(&take(&mut cache, 2)).delivered_first_frame, h.first);
    }
    let h = header(0);
    let mut pair =
        PendingSourceFrameObservations::new(h.epoch, h.basis, h.identity, ClockDomainId(1));
    assert!(pair.push(observation(h, &drops)).is_err());
}

#[test]
fn asio_actual_delivered_extent_and_same_publication_render_are_checked_before_queue_mutation() {
    let drops = Rc::new(Cell::new(0));
    for occupied in 0..=2 {
        for invalid in 0..2 {
            let mut cache = cache();
            for index in 0..occupied {
                cache.push(observation(header(index), &drops)).unwrap();
            }
            let h = header(2);
            let original = observation(h, &drops);
            let mut malformed = h;
            if invalid == 0 {
                malformed.submitted = h.first + 1;
            } else {
                malformed.render.song_position = Timestamp::from_nanos(1);
            }
            assert!(cache
                .push(packet(malformed, original.coordinate(), &drops))
                .is_err());
            for index in 0..occupied {
                take(&mut cache, index);
            }
            cache.push(observation(h, &drops)).unwrap();
            assert_eq!(raw(&take(&mut cache, 2)).render, h.render);
        }
    }
}

#[test]
fn asio_wrong_poll_domain_and_clear_preserve_then_release_owned_originals() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = cache();
    cache.push(observation(header(1), &drops)).unwrap();
    assert!(cache.take_covered(point(77, 9999)).is_err());
    assert!(cache.take_covered(point(1, 1179)).unwrap().is_none());
    cache.clear();
    assert_eq!(drops.get(), 1);
    cache.push(observation(header(0), &drops)).unwrap();
    assert_eq!(raw(&take(&mut cache, 0)).delivered_first_frame, 17);
}

#[test]
fn asio_invalid_revision_prefix_chronology_or_changed_duplicate_refuses_even_when_full() {
    let drops = Rc::new(Cell::new(0));
    for invalid in 0..7 {
        let mut cache = cache();
        let mut a = header(0);
        a.pulled = 100;
        a.submitted = 90;
        let mut b = header(1);
        b.pulled = 102;
        b.submitted = 92;
        cache.push(observation(a, &drops)).unwrap();
        cache.push(observation(b, &drops)).unwrap();
        let mut bad = header(2);
        bad.pulled = 104;
        bad.submitted = 94;
        match invalid {
            0 => bad.revision = 2,
            1 => bad.revision = 4,
            2 => bad.pulled = 101,
            3 => bad.submitted = 91,
            4 => {
                bad.before = point(1, 1000);
                bad.after = point(1, 1080);
            }
            5 => {
                bad = b;
                bad.revision += 2;
                bad.frames = 1;
            }
            _ => {
                bad = b;
                bad.revision += 2;
                bad.render.song_position = Timestamp::from_nanos(1);
            }
        }
        assert!(
            cache.push(observation(bad, &drops)).is_err(),
            "invalid={invalid}"
        );
        assert_eq!(raw(&take(&mut cache, 0)).delivered_first_frame, a.first);
        assert_eq!(raw(&take(&mut cache, 1)).delivered_first_frame, b.first);
        let mut valid = header(2);
        valid.pulled = 104;
        valid.submitted = 94;
        cache.push(observation(valid, &drops)).unwrap();
        assert_eq!(raw(&take(&mut cache, 2)).delivered_first_frame, valid.first);
    }
}
