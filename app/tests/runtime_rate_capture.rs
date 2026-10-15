//! Applied source-rate metadata overlays real Runtime reports without rewriting them.
//! This exercises the capture owner, not the audio/presentation qualification path.
use beatkernel::{
    audio::command_queue,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent, encode_event,
    },
    judge::JudgeEngine,
    replay::{
        ReplayOperation,
        codec::{ReplayCodecLimits, ReplayFile, decode_replay, encode_replay},
    },
    runtime::{Runtime, RuntimeReport},
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockPair, ClockPoint, Duration, Timestamp,
    },
};
use beatkernel_bms::{BmsChart, BmsInputMode};
use beatkernel_bms_runtime::{
    live_speed::LiveSpeedSetup,
    play_policy::ResolvedPlayPolicy,
    replay_capture::LiveReplayCapture,
    replay_playback::{decode_section_setup, reconstruct_section},
    replay_speed::ReplaySpeed,
    replay_speed_policy::{SourceRateCut, split_schedule},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<[usize; 4]> = const { Cell::new([0; 4]) };
}
struct CountingAllocator;
fn count(kind: usize, bytes: usize) {
    let _ = TRACK.try_with(|on| {
        if on.get() {
            let _ = COUNTS.try_with(|c| {
                let mut counts = c.get();
                counts[kind] += 1;
                counts[3] += bytes;
                c.set(counts);
            });
        }
    });
}
// SAFETY: Every pointer/layout operation is forwarded unchanged to System;
// bookkeeping uses allocation-free thread-local scalar cells.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(0, layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(0, layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, n: usize) -> *mut u8 {
        count(1, n);
        unsafe { System.realloc(ptr, layout, n) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count(2, 0);
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
fn tracked<T>(f: impl FnOnce() -> T) -> (T, [usize; 4]) {
    COUNTS.with(|c| c.set([0; 4]));
    TRACK.with(|on| on.set(true));
    let result = f();
    TRACK.with(|on| on.set(false));
    (result, COUNTS.with(Cell::get))
}
fn limits() -> ReplayCodecLimits {
    bounded(1 << 20, 4096)
}
fn bounded(file: usize, header: usize) -> ReplayCodecLimits {
    ReplayCodecLimits::new(file, 100, header, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(domain: u32, n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(n),
    }
}
fn speed(s: &str) -> ReplaySpeed {
    s.parse().unwrap()
}
fn source() -> BmsChart {
    beatkernel_bms::parse(
        "#BPM 80\n#WAV01 note.wav\n#00011:01\n#00111:01\n",
        Default::default(),
    )
    .unwrap()
}
fn judge() -> JudgeEngine {
    let source = source();
    let policy = ResolvedPlayPolicy::builtin(0, 0, 0).unwrap();
    JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        policy.judge().clone(),
    )
    .unwrap()
}
fn capture_at(initial: &str, limits: ReplayCodecLimits) -> LiveReplayCapture {
    LiveReplayCapture::new_with_policy_and_speed(
        &judge(),
        ClockDomainId(1),
        limits,
        Timestamp::ZERO,
        0,
        None,
        BmsInputMode::ButtonOnly,
        None,
        &ResolvedPlayPolicy::builtin(0, 0, 0).unwrap(),
        speed(initial),
    )
    .unwrap()
}
fn cut(whole: i128, n: u64, d: u64, rate: &str) -> SourceRateCut {
    SourceRateCut::new(whole, n, d, speed(rate)).unwrap()
}
fn apply(capture: &mut LiveReplayCapture, c: SourceRateCut) {
    let mut prepared = capture.prepare_source_rate_change(c.speed()).unwrap();
    capture
        .prepare_applied_source_rate_change(&mut prepared, c)
        .unwrap();
    capture.commit_source_rate_change(prepared).unwrap();
}
fn reports() -> Vec<RuntimeReport> {
    let mapper = AffineClockMapper::exact_offset(
        ClockPair {
            source: point(9, 0),
            target: point(1, 0),
        },
        ClockInterval {
            start: Timestamp::ZERO,
            end: ts(8_000_000_000),
        },
    )
    .unwrap();
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(7),
        game_control: GameControlId(0x11),
    }])
    .unwrap();
    let (producer, _consumer) = command_queue(8).unwrap();
    let setup = LiveSpeedSetup::new(Timestamp::ZERO, Duration::ZERO, speed("1/1"), None).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        setup.transport(Timestamp::ZERO).unwrap(),
        bindings,
        judge(),
        producer,
        vec![],
        0,
    )
    .unwrap();
    let mut reports: Vec<_> = [
        (0, ButtonState::Down),
        (1, ButtonState::Up),
        (1_000_000_000, ButtonState::Down),
        (1_000_000_001, ButtonState::Up),
        (3_000_000_000, ButtonState::Down),
    ]
    .into_iter()
    .enumerate()
    .map(|(seq, (at, state))| {
        let mut meta = EventMeta::new(DeviceId(17), point(9, at), seq as u64);
        meta.original_clock_point = Some(point(11, at + 7));
        runtime
            .process_input(
                PhysicalInputEvent::Button(ButtonEvent {
                    meta,
                    control: PhysicalControlId::keyboard(7),
                    state,
                }),
                &mapper,
                point(2, 17),
            )
            .unwrap()
    })
    .collect();
    reports.push(
        runtime
            .advance_to(point(9, 4_000_000_000), &mapper, point(2, 17))
            .unwrap(),
    );
    reports
}
fn encoded_snapshot(c: &LiveReplayCapture) -> Vec<u8> {
    encode_replay(
        &ReplayFile::new(c.header().clone(), c.records().to_vec()),
        limits(),
    )
    .unwrap()
}
fn exact(c: &LiveReplayCapture) {
    assert_eq!(c.encoded_bytes(), encoded_snapshot(c).len());
}
// First NORMAL->variable growth is prefix + u32 extent + i64/u64 initial
// + u32 count + one 48-byte exact cut. Each later cut adds exactly 48 bytes.
const FIRST_GROWTH: usize = b"bms-speed-setup/v2:".len() + 4 + 16 + 4 + 48;

#[test]
fn real_reports_survive_two_changes_and_return_to_original_byte_for_byte() {
    let reports = reports();
    let mut actual = capture_at("1/1", limits());
    let mut control = capture_at("1/1", limits());
    actual.record_report(&reports[0]).unwrap();
    control.record_report(&reports[0]).unwrap();
    let retained_prefix = actual.records().to_vec();
    let mut first = actual.prepare_source_rate_change(speed("3/2")).unwrap();
    assert_eq!(actual.header(), control.header());
    assert_eq!(actual.records(), retained_prefix);
    // Operations may arrive after reservation and before the actual cut.
    actual.record_report(&reports[1]).unwrap();
    control.record_report(&reports[1]).unwrap();
    actual
        .prepare_applied_source_rate_change(&mut first, cut(1, 1, 2, "3/2"))
        .unwrap();
    actual.commit_source_rate_change(first).unwrap();
    exact(&actual);
    assert_eq!(
        actual.encoded_bytes(),
        control.encoded_bytes() + FIRST_GROWTH
    );
    for r in &reports[2..4] {
        actual.record_report(r).unwrap();
        control.record_report(r).unwrap();
    }
    let mut second = actual.prepare_source_rate_change(speed("1/1")).unwrap();
    actual.record_report(&reports[4]).unwrap();
    control.record_report(&reports[4]).unwrap();
    actual
        .prepare_applied_source_rate_change(&mut second, cut(3_000_000_000, 1, 3, "1/1"))
        .unwrap();
    actual.commit_source_rate_change(second).unwrap();
    actual.record_report(&reports[5]).unwrap();
    control.record_report(&reports[5]).unwrap();
    exact(&actual);
    assert_eq!(actual.records(), control.records());
    assert_eq!(
        actual.encoded_bytes(),
        control.encoded_bytes() + FIRST_GROWTH + 48
    );
    let (_, schedule) = split_schedule(&actual.header().options).unwrap();
    assert!(schedule.is_variable());
    assert_eq!(schedule.initial_speed(), speed("1/1"));
    assert_eq!(schedule.current_speed(), speed("1/1"));
    assert_eq!(schedule.cuts().len(), 2);
    for (ordinal, record) in actual.records().iter().enumerate() {
        assert_eq!(record.ordinal, ordinal as u64);
        match &record.operation {
            ReplayOperation::Input(input) => {
                let original = &reports[ordinal].bound_inputs[0].physical;
                assert_eq!(input.physical, *original);
                assert_eq!(
                    encode_event(&input.physical, limits().input_limits()).unwrap(),
                    encode_event(original, limits().input_limits()).unwrap()
                );
            }
            ReplayOperation::Advance => assert!(reports[ordinal].input.is_none()),
        }
        assert_eq!(record.song_time, reports[ordinal].song_time);
    }
    let effective = actual.header().clone();
    let byte_len = actual.encoded_bytes();
    let file = actual.into_file();
    assert_eq!(file.header, effective);
    let bytes = encode_replay(&file, limits()).unwrap();
    assert_eq!(bytes.len(), byte_len);
    let decoded = decode_replay(&bytes, limits()).unwrap();
    assert_eq!(decoded.records, control.records());
    assert_eq!(
        decode_section_setup(&decoded.header.options)
            .unwrap()
            .source_speed,
        speed("1/1")
    );
    let expected_events: Vec<_> = reports
        .iter()
        .flat_map(|r| r.judge_events.iter().cloned())
        .collect();
    let replay = reconstruct_section(&source(), decoded, limits()).unwrap();
    assert_eq!(replay.results(), expected_events);
}

#[test]
fn no_op_reservation_has_zero_growth_and_never_turns_fixed_into_variable() {
    let reports = reports();
    let mut c = capture_at("3/2", limits());
    c.record_report(&reports[0]).unwrap();
    let header = c.header().clone();
    let bytes = c.encoded_bytes();
    apply(&mut c, cut(0, 0, 1, "6/4"));
    assert_eq!(c.header(), &header);
    assert_eq!(c.encoded_bytes(), bytes);
    assert!(!split_schedule(&c.header().options).unwrap().1.is_variable());
    exact(&c);
}

#[test]
fn non_normal_initial_capture_replaces_v1_once_and_preserves_initial_view() {
    let mut c = capture_at("3/2", limits());
    let report = reports().remove(0);
    c.record_report(&report).unwrap();
    let before = c.encoded_bytes();
    let records = c.records().to_vec();
    apply(&mut c, cut(0, 1, 2, "2/1"));
    let v1_growth = b"bms-speed-setup/v1:".len() + 4 + 16;
    assert_eq!(c.encoded_bytes(), before + FIRST_GROWTH - v1_growth);
    assert_eq!(
        decode_section_setup(&c.header().options)
            .unwrap()
            .source_speed,
        speed("3/2")
    );
    apply(&mut c, cut(1, 1, 2, "6/4"));
    let (_, schedule) = split_schedule(&c.header().options).unwrap();
    assert_eq!(schedule.initial_speed(), speed("3/2"));
    assert_eq!(schedule.current_speed(), speed("3/2"));
    assert_eq!(schedule.cuts().len(), 2);
    assert_eq!(c.records(), records);
    exact(&c);
}

#[test]
fn pending_reservation_is_charged_to_reports_at_below_equal_and_above_file_cap() {
    let report = reports().remove(0);
    let empty = capture_at("1/1", limits()).encoded_bytes();
    let mut probe = capture_at("1/1", limits());
    probe.record_report(&report).unwrap();
    let record_cost = probe.encoded_bytes() - empty;
    let required = empty + FIRST_GROWTH + record_cost;
    for (cap, accepts) in [
        (required - 1, false),
        (required, true),
        (required + 1, true),
    ] {
        let mut c = capture_at("1/1", bounded(cap, 4096.min(cap)));
        let mut prepared = c.prepare_source_rate_change(speed("3/2")).unwrap();
        let before = encoded_snapshot(&c);
        assert_eq!(c.record_report(&report).is_ok(), accepts);
        if !accepts {
            assert_eq!(encoded_snapshot(&c), before);
            assert!(c.records().is_empty());
        }
        c.prepare_applied_source_rate_change(&mut prepared, cut(0, 1, 2, "3/2"))
            .unwrap();
        c.commit_source_rate_change(prepared).unwrap();
        exact(&c);
        assert_eq!(
            c.encoded_bytes(),
            empty + FIRST_GROWTH + if accepts { record_cost } else { 0 }
        );
    }
}

#[test]
fn pending_second_change_uses_current_body_size_and_reserves_only_one_more_cut() {
    let reports = reports();
    let mut probe = capture_at("1/1", limits());
    probe.record_report(&reports[0]).unwrap();
    apply(&mut probe, cut(0, 1, 2, "3/2"));
    probe.record_report(&reports[1]).unwrap();
    let before = probe.encoded_bytes();
    probe.record_report(&reports[2]).unwrap();
    let cost = probe.encoded_bytes() - before;
    for (cap, accepts) in [(before + 48 + cost - 1, false), (before + 48 + cost, true)] {
        let mut c = capture_at("1/1", bounded(cap, 4096.min(cap)));
        c.record_report(&reports[0]).unwrap();
        apply(&mut c, cut(0, 1, 2, "3/2"));
        c.record_report(&reports[1]).unwrap();
        let mut token = c.prepare_source_rate_change(speed("2/1")).unwrap();
        assert_eq!(c.record_report(&reports[2]).is_ok(), accepts);
        c.prepare_applied_source_rate_change(&mut token, cut(1_000_000_000, 1, 2, "2/1"))
            .unwrap();
        c.commit_source_rate_change(token).unwrap();
        exact(&c);
        assert_eq!(
            c.encoded_bytes(),
            before + 48 + if accepts { cost } else { 0 }
        );
    }
}

#[test]
fn prepare_checks_header_and_file_exact_limits_before_reserving_or_mutating() {
    let base = capture_at("1/1", limits());
    let bytes = base.encoded_bytes() + FIRST_GROWTH;
    let h = base.header().chart_identity.len()
        + base.header().rules_identity.len()
        + base.header().options.len()
        + env!("CARGO_PKG_VERSION").len()
        + FIRST_GROWTH;
    for (file_limit, header_limit, accepts) in [
        (bytes - 1, h, false),
        (bytes, h - 1, false),
        (bytes, h, true),
        (bytes + 1, h + 1, true),
    ] {
        let mut c = capture_at("1/1", bounded(file_limit, header_limit));
        let before = encoded_snapshot(&c);
        let result = c.prepare_source_rate_change(speed("3/2"));
        assert_eq!(result.is_ok(), accepts);
        assert_eq!(encoded_snapshot(&c), before);
        if let Ok(token) = result {
            c.cancel_source_rate_change(token).unwrap();
        }
        // A normalized no-op fits even when no growth can be reserved.
        let mut noop = c.prepare_source_rate_change(speed("2/2")).unwrap();
        c.prepare_applied_source_rate_change(&mut noop, cut(0, 0, 1, "1/1"))
            .unwrap();
        c.commit_source_rate_change(noop).unwrap();
        assert_eq!(encoded_snapshot(&c), before);
    }
}

#[test]
fn cancellation_releases_reserved_bytes_without_altering_effective_header_or_operations() {
    let report = reports().remove(0);
    let mut probe = capture_at("1/1", limits());
    probe.record_report(&report).unwrap();
    let cap = probe.encoded_bytes() + FIRST_GROWTH - 1;
    let mut c = capture_at("1/1", bounded(cap, 4096.min(cap)));
    let before = encoded_snapshot(&c);
    let token = c.prepare_source_rate_change(speed("3/2")).unwrap();
    assert!(c.prepare_source_rate_change(speed("2/1")).is_err());
    assert!(c.record_report(&report).is_err());
    assert_eq!(encoded_snapshot(&c), before);
    c.cancel_source_rate_change(token).unwrap();
    assert_eq!(encoded_snapshot(&c), before);
    c.record_report(&report).unwrap();
    exact(&c);
}

#[test]
fn invalid_actual_cut_and_cross_owner_tokens_fail_before_mutation() {
    let mut a = capture_at("1/1", limits());
    let mut b = capture_at("1/1", limits());
    let original = encoded_snapshot(&a);
    let other = encoded_snapshot(&b);
    let mut token = a.prepare_source_rate_change(speed("3/2")).unwrap();
    assert!(
        b.prepare_applied_source_rate_change(&mut token, cut(0, 1, 2, "3/2"))
            .is_err()
    );
    assert_eq!(encoded_snapshot(&a), original);
    assert_eq!(encoded_snapshot(&b), other);
    assert!(
        a.prepare_applied_source_rate_change(&mut token, cut(0, 1, 2, "2/1"))
            .is_err()
    );
    assert_eq!(encoded_snapshot(&a), original);
    a.prepare_applied_source_rate_change(&mut token, cut(0, 1, 2, "3/2"))
        .unwrap();
    a.commit_source_rate_change(token).unwrap();
    let applied = encoded_snapshot(&a);
    let mut backwards = a.prepare_source_rate_change(speed("2/1")).unwrap();
    assert!(
        a.prepare_applied_source_rate_change(&mut backwards, cut(0, 1, 3, "2/1"))
            .is_err()
    );
    assert_eq!(encoded_snapshot(&a), applied);
    a.cancel_source_rate_change(backwards).unwrap();
    for cancel in [false, true] {
        let mut owner = capture_at("1/1", limits());
        let token = owner.prepare_source_rate_change(speed("3/2")).unwrap();
        let result = if cancel {
            b.cancel_source_rate_change(token)
        } else {
            b.commit_source_rate_change(token)
        };
        assert!(result.is_err());
        assert_eq!(encoded_snapshot(&b), other);
    }
}

#[test]
fn actual_cut_fill_and_commit_allocate_nothing_after_cold_reservation() {
    // Calibrate against an actual allocation so a disabled counter cannot pass.
    let (allocation, count) = tracked(|| Box::new(std::hint::black_box(7u64)));
    assert!(count[0] > 0);
    drop(allocation);
    let mut c = capture_at("1/1", limits());
    for (i, requested) in ["3/2", "2/1", "1/1"].into_iter().enumerate() {
        let mut token = c.prepare_source_rate_change(speed(requested)).unwrap();
        let exact_cut = cut(i as i128, 1, 2, requested);
        let (result, counts) = tracked(|| {
            c.prepare_applied_source_rate_change(&mut token, exact_cut)?;
            c.commit_source_rate_change(token)
        });
        result.unwrap();
        assert_eq!(
            &counts[..2],
            &[0, 0],
            "actual-cut fill/commit allocated at {i}"
        );
        exact(&c);
    }
}

#[test]
fn per_report_allocation_volume_does_not_scale_with_growing_variable_history() {
    // Measure allocated bytes as well as counts: cloning a larger options Vec
    // can retain the same allocation count while copying the complete history.
    let report = reports().remove(0);
    let mut short = capture_at("1/1", limits());
    apply(&mut short, cut(-2, 0, 1, "3/2"));
    let mut long = capture_at("1/1", limits());
    for i in 0..24 {
        apply(
            &mut long,
            cut(i - 48, 0, 1, if i % 2 == 0 { "3/2" } else { "2/1" }),
        );
    }
    let (s, short_counts) = tracked(|| short.record_report(&report));
    let (l, long_counts) = tracked(|| long.record_report(&report));
    s.unwrap();
    l.unwrap();
    assert_eq!(&long_counts[..2], &short_counts[..2]);
    assert_eq!(long_counts[3], short_counts[3]);
    assert_eq!(long.records(), short.records());
    exact(&short);
    exact(&long);
}

#[test]
fn dropped_prepared_clone_keeps_original_reservation_charged_to_later_reports() {
    let report = reports().remove(0);
    let empty = capture_at("1/1", limits()).encoded_bytes();
    let mut probe = capture_at("1/1", limits());
    probe.record_report(&report).unwrap();
    let record_cost = probe.encoded_bytes() - empty;
    let required = empty + FIRST_GROWTH + record_cost;
    for (cap, accepts_report) in [(required - 1, false), (required, true)] {
        let mut capture = capture_at("1/1", bounded(cap, 4096.min(cap)));
        let original = capture.prepare_source_rate_change(speed("3/2")).unwrap();
        let before = encoded_snapshot(&capture);
        let mut discarded = original.clone();
        capture
            .prepare_applied_source_rate_change(&mut discarded, cut(0, 1, 2, "3/2"))
            .unwrap();
        drop(discarded);
        assert_eq!(encoded_snapshot(&capture), before);
        assert!(capture.prepare_source_rate_change(speed("2/1")).is_err());
        assert_eq!(capture.record_report(&report).is_ok(), accepts_report);
        if !accepts_report {
            assert_eq!(encoded_snapshot(&capture), before);
            assert!(capture.records().is_empty());
        }
        // The retained original permits another independently filled candidate
        // for the identical actual cut after the first candidate was abandoned.
        let mut retry = original.clone();
        capture
            .prepare_applied_source_rate_change(&mut retry, cut(0, 1, 2, "3/2"))
            .unwrap();
        capture.commit_source_rate_change(retry).unwrap();
        drop(original);
        assert_eq!(
            capture.encoded_bytes(),
            empty + FIRST_GROWTH + if accepts_report { record_cost } else { 0 }
        );
        assert_eq!(capture.source_rate_schedule().cuts(), [cut(0, 1, 2, "3/2")]);
        if accepts_report {
            assert_eq!(capture.records(), probe.records());
        }
        exact(&capture);
    }
}

#[test]
fn commit_or_explicit_cancel_invalidates_every_clone_without_cancelling_a_later_reservation() {
    for cancel in [false, true] {
        let mut capture = capture_at("1/1", limits());
        let before = encoded_snapshot(&capture);
        let original = capture.prepare_source_rate_change(speed("3/2")).unwrap();
        let mut applied_clone = original.clone();
        capture
            .prepare_applied_source_rate_change(&mut applied_clone, cut(0, 1, 2, "3/2"))
            .unwrap();
        let mut stale_filled = applied_clone.clone();
        let mut stale_original = original.clone();
        if cancel {
            capture.cancel_source_rate_change(applied_clone).unwrap();
            assert_eq!(encoded_snapshot(&capture), before);
            assert_eq!(capture.source_rate_schedule().current_speed(), speed("1/1"));
        } else {
            capture.commit_source_rate_change(applied_clone).unwrap();
            assert_ne!(encoded_snapshot(&capture), before);
            assert_eq!(capture.source_rate_schedule().current_speed(), speed("3/2"));
        }
        let completed = encoded_snapshot(&capture);
        let requested = if cancel { "3/2" } else { "2/1" };
        let actual_cut = cut(1, 1, 2, requested);
        let mut next = capture
            .prepare_source_rate_change(speed(requested))
            .unwrap();
        assert!(
            capture
                .prepare_applied_source_rate_change(&mut stale_original, cut(0, 1, 2, "3/2"))
                .is_err()
        );
        assert!(
            capture
                .prepare_applied_source_rate_change(&mut stale_filled, cut(0, 1, 2, "3/2"))
                .is_err()
        );
        assert!(capture.commit_source_rate_change(stale_filled).is_err());
        assert!(capture.cancel_source_rate_change(stale_original).is_err());
        assert!(capture.cancel_source_rate_change(original).is_err());
        assert_eq!(encoded_snapshot(&capture), completed);
        assert!(
            capture.prepare_source_rate_change(speed("5/2")).is_err(),
            "stale clones must not cancel the newer reservation"
        );
        capture
            .prepare_applied_source_rate_change(&mut next, actual_cut)
            .unwrap();
        capture.commit_source_rate_change(next).unwrap();
        assert_eq!(
            capture.source_rate_schedule().current_speed(),
            speed(requested)
        );
        assert_eq!(
            capture.source_rate_schedule().cuts().last().copied(),
            Some(actual_cut)
        );
        assert_eq!(
            capture.source_rate_schedule().cuts().len(),
            if cancel { 1 } else { 2 }
        );
        exact(&capture);
    }
}
