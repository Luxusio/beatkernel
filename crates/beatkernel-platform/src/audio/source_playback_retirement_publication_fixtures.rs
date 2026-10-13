//! Actual owner retirement and payload selection; no native admission is inferred.
use super::*;
use crate::audio::count_heap_calls;
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};

fn rig(capacity: Option<usize>) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(3, 1).unwrap();
    let limits = PcmLimits::new(1024, 1024, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 256], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(3),
            Timestamp::ZERO,
            AudioLimits::new(8, 4, 8, 64, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.0; 3]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.0; 2]).unwrap();
    producer.request_pause(false);
    if let Some(capacity) = capacity {
        mixer.enable_source_playback_history(capacity).unwrap();
    }
    (producer, mixer)
}
fn three_spans() -> (CommandProducer, Mixer) {
    let (mut producer, mut mixer) = rig(Some(3));
    for paused in [false, true, false] {
        producer.request_pause(paused);
        mixer.render(&mut [0.0; 2]).unwrap();
    }
    assert_eq!(history(&mixer).len(), 3);
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (11, 7)
    );
    (producer, mixer)
}
fn history(mixer: &Mixer) -> &SourcePlaybackHistory {
    mixer.source_playback_history().unwrap()
}
fn query(numerator: u64, denominator: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(
        TargetTime::new(
            numerator / denominator,
            numerator % denominator,
            denominator,
        )
        .unwrap(),
        0,
        0,
        1,
        0,
        1,
    )
    .unwrap()
}
fn select(
    publication: &SourcePlaybackPublication,
    q: InterpolatedTargetTime,
) -> Option<PublishedSourcePlayback> {
    publication.select(3, 5, 3, |frame| q.compare_source_time_frame(frame, 3).ok())
}
fn header(publication: &SourcePlaybackPublication) -> [u64; 5] {
    std::array::from_fn(|index| publication.test_header()[index].load(Ordering::SeqCst))
}
fn available(publication: &SourcePlaybackPublication) -> bool {
    publication.test_available().load(Ordering::SeqCst)
}

#[test]
fn genuine_retirement_republishes_remaining_spans_and_preserves_original_creation() {
    let (_, mut mixer) = three_spans();
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    let original = header(&publication);
    let expected: Vec<_> = [21, 22, 26, 27, 28, 33]
        .into_iter()
        .map(|n| {
            let q = query(n, 9);
            (q, history(&mixer).lookup_at_source_time(q).unwrap())
        })
        .collect();
    let (removed, calls) = count_heap_calls(|| {
        let removed = mixer.retire_source_playback_before(7).unwrap();
        publication.publish(Some(history(&mixer)));
        removed
    });
    assert_eq!((removed, calls), (1, 0));
    assert!(available(&publication));
    assert_eq!(
        header(&publication),
        [2, original[1], original[2], original[3], original[4]]
    );
    assert_eq!(publication.initial_playback_frame(), 3);
    assert!(publication.unchanged(history(&mixer)));
    for (q, expected) in expected {
        let (selected, calls) = count_heap_calls(|| select(&publication, q));
        assert_eq!(calls, 0);
        let selected = selected.unwrap();
        assert_eq!(selected.span, expected);
        assert_eq!(selected.pulled_end_frame, 11);
        assert_eq!(
            selected.span.elapsed_nanos_at_source_time(q, 3, 3),
            expected.elapsed_nanos_at_source_time(q, 3, 3)
        );
    }
    assert!(select(&publication, query(5, 3)).is_none());
}

#[test]
fn exact_retained_lower_rejects_fractional_floor_alias_and_keeps_closed_frontier() {
    let (_, mut mixer) = three_spans();
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    let below = query(70_000_000_000 - 1, 30_000_000_000);
    let equal = query(7, 3);
    let above = query(70_000_000_000 + 1, 30_000_000_000);
    assert_eq!(
        select(&publication, below).unwrap().span.state(),
        SourcePlaybackState::Running
    );
    mixer.retire_source_playback_before(7).unwrap();
    publication.publish(Some(history(&mixer)));
    assert!(select(&publication, below).is_none());
    for q in [equal, above] {
        let selected = select(&publication, q).unwrap();
        assert_eq!(
            (selected.span.physical_start(), selected.span.state()),
            (7, SourcePlaybackState::Frozen)
        );
    }
    assert_eq!(
        select(&publication, query(11, 3)).unwrap().pulled_end_frame,
        11
    );
    assert!(select(&publication, query(110_000_000_001, 30_000_000_000)).is_none());
}

#[test]
fn cold_seed_of_already_retired_owner_keeps_original_id_not_retained_id() {
    let (_, mut mixer) = three_spans();
    mixer.retire_source_playback_before(9).unwrap();
    let mut moved = mixer;
    let publication = SourcePlaybackPublication::new(history(&moved)).unwrap();
    assert_eq!(header(&publication), [1, 3, 5, 3, 11]);
    assert!(publication.unchanged(history(&moved)));
    assert!(select(&publication, query(8, 3)).is_none());
    assert_eq!(
        select(&publication, query(9, 3))
            .unwrap()
            .span
            .playback_start(),
        5
    );
    for (rate, physical, playback) in [(7, 5, 3), (3, 9, 5), (3, 5, 5), (3, 9, 3)] {
        assert!(publication
            .select(rate, physical, playback, |frame| Some(10u64.cmp(&frame)))
            .is_none());
    }
    moved.render(&mut [0.0; 2]).unwrap();
    publication.publish(Some(history(&moved)));
    assert!(available(&publication));
    assert_eq!(
        select(&publication, query(13, 3)).unwrap().pulled_end_frame,
        13
    );
    assert_eq!(header(&publication)[2..4], [5, 3]);
}

#[test]
fn retirement_nonce_prevents_unchanged_even_when_scalar_count_and_end_match() {
    let (_, mut mixer) = three_spans();
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    mixer.retire_source_playback_before(7).unwrap();
    // Isolate the nonce from the count/end optimization; this is not a publishable header.
    publication.test_header()[0].store(history(&mixer).len() as u64, Ordering::SeqCst);
    assert_eq!(header(&publication)[4], history(&mixer).pulled_end_frame());
    assert!(!publication.unchanged(history(&mixer)));
}

#[test]
fn retire_and_real_append_restore_count_without_reusing_stale_slots() {
    let (mut producer, mut mixer) = three_spans();
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    mixer.retire_source_playback_before(7).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.0; 2]).unwrap();
    assert_eq!(history(&mixer).len(), header(&publication)[0] as usize);
    assert!(!publication.unchanged(history(&mixer)));
    let ((), calls) = count_heap_calls(|| publication.publish(Some(history(&mixer))));
    assert_eq!(calls, 0);
    assert!(available(&publication));
    for n in 7..=13 {
        let q = query(n, 3);
        assert_eq!(
            select(&publication, q).unwrap().span,
            history(&mixer).lookup_at_source_time(q).unwrap()
        );
    }
    assert!(select(&publication, query(6, 3)).is_none());
    assert!(publication.unchanged(history(&mixer)));
}

#[test]
fn fixed_capacity_repeated_pause_resume_retire_publish_matches_actual_reference_pcm() {
    let (mut producer, mut mixer) = rig(Some(3));
    let (mut reference_producer, mut reference) = rig(None);
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    let mut nonce = 0;
    for _ in 0..24 {
        for paused in [false, true] {
            producer.request_pause(paused);
            reference_producer.request_pause(paused);
            let mut actual_pcm = [99.0; 2];
            let mut reference_pcm = [99.0; 2];
            let (report, calls) = count_heap_calls(|| {
                let report = mixer.render(&mut actual_pcm).unwrap();
                // Only latest-span queries survive in this mathematical fixture.
                let pin = history(&mixer)
                    .span(history(&mixer).len() - 1)
                    .unwrap()
                    .physical_start();
                if mixer.retire_source_playback_before(pin).unwrap() > 0 {
                    nonce += 1;
                }
                publication.publish(Some(history(&mixer)));
                report
            });
            assert_eq!(calls, 0);
            assert_eq!(report, reference.render(&mut reference_pcm).unwrap());
            assert_eq!(actual_pcm, reference_pcm);
            assert!(available(&publication));
            assert!(publication.unchanged(history(&mixer)));
            assert_eq!(history(&mixer).capacity(), 3);
            assert_eq!(history(&mixer).len(), 1);
            assert_eq!(history(&mixer).retirement_revision(), nonce);
            let first = history(&mixer).retained_start_frame();
            for frame in [first, mixer.frame_cursor()] {
                let q = query(frame, 3);
                assert_eq!(
                    select(&publication, q).unwrap().span,
                    history(&mixer).lookup_at_source_time(q).unwrap()
                );
            }
            if first > 5 {
                assert!(select(&publication, query(first - 1, 3)).is_none());
            }
            assert_eq!(header(&publication)[1..4], [3, 5, 3]);
        }
    }
}

#[test]
fn noop_retirement_preserves_nonce_payload_and_availability_without_heap() {
    let (_, mut mixer) = three_spans();
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    mixer.retire_source_playback_before(7).unwrap();
    publication.publish(Some(history(&mixer)));
    let before = header(&publication);
    let nonce = history(&mixer).retirement_revision();
    let (removed, calls) = count_heap_calls(|| {
        let removed = mixer.retire_source_playback_before(8).unwrap();
        publication.publish(Some(history(&mixer)));
        removed
    });
    assert_eq!((removed, calls), (0, 0));
    assert_eq!(history(&mixer).retirement_revision(), nonce);
    assert_eq!(header(&publication), before);
    assert!(available(&publication));
    assert!(publication.unchanged(history(&mixer)));
}

#[test]
fn stale_real_owner_nonce_and_unchanged_nonce_header_corruption_close_permanently() {
    let (_, mut mixer) = three_spans();
    let (_, stale) = three_spans();
    mixer.retire_source_playback_before(7).unwrap();
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    publication.publish(Some(history(&stale)));
    assert!(!available(&publication));
    publication.publish(Some(history(&mixer)));
    assert!(select(&publication, query(8, 3)).is_none());
    for (index, bad) in [(0, 3), (4, 12)] {
        let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
        let old = header(&publication)[index];
        publication.test_header()[index].store(bad, Ordering::SeqCst);
        let ((), calls) = count_heap_calls(|| publication.publish(Some(history(&mixer))));
        assert_eq!(calls, 0);
        assert!(!available(&publication));
        publication.test_header()[index].store(old, Ordering::SeqCst);
        publication.publish(Some(history(&mixer)));
        assert!(select(&publication, query(8, 3)).is_none());
    }
}

#[test]
fn real_converter_projection_selects_same_remaining_source_spans() {
    let (_, raw) = rig(None);
    let mut converter = ConvertedMixer::new(
        raw,
        AudioFormat::new(7, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        64,
    )
    .unwrap_or_else(|failure| panic!("fixture: {:?}", failure.error()));
    converter.enable_phase_history(3).unwrap();
    let report = converter.render(&mut [0.0; 28]).unwrap();
    let span = converter.phase_history().unwrap().span(0).unwrap();
    let (_, mut mixer) = three_spans();
    let publication = SourcePlaybackPublication::new(history(&mixer)).unwrap();
    // Converter starts at absolute time 5/3 with actual source cursor five.
    let projected = |q| {
        publication.select(3, 5, 3, |frame| {
            span.compare_source_frame_at_interpolated_time(q, report.target_end_time, frame)
                .ok()
        })
    };
    let q = query(7, 3);
    let expected = history(&mixer)
        .lookup_at_interpolated_time(span, q, report.target_end_time)
        .unwrap();
    assert_eq!(projected(q).unwrap().span, expected);
    mixer.retire_source_playback_before(7).unwrap();
    publication.publish(Some(history(&mixer)));
    assert_eq!(projected(q).unwrap().span, expected);
    assert!(projected(query(69, 30)).is_none());
    for q in [query(7, 3), query(3, 1), query(11, 3)] {
        assert_eq!(
            projected(q).unwrap().span,
            history(&mixer)
                .lookup_at_interpolated_time(span, q, report.target_end_time)
                .unwrap()
        );
    }
}
