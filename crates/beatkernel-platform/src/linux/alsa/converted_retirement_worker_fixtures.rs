//! Scripted native admission exercises the production converted worker and its
//! real complete owner; retirement receipts are never native acknowledgments.
use super::*;
use super::super::converted_fixtures::rig_boundaries;
use beatkernel::audio::*;
use std::cell::Cell;

fn rig(
    source: u32,
    target: u32,
    period: usize,
    capacity: usize,
) -> (
    CommandProducer,
    ConvertedNativeOutputState,
    AlsaAppliedConfig,
) {
    let (producer, mut owner, config) = rig_boundaries(period, source, target, None, None);
    owner.enable_source_playback_history(capacity).unwrap();
    owner.enable_phase_history(capacity).unwrap();
    (producer, owner, config)
}
fn telemetry(owner: &ConvertedNativeOutputState) -> ConvertedTelemetry {
    ConvertedTelemetry::with_phase_history(owner, owner.target_frame_basis()).unwrap()
}
fn identity(owner: &ConvertedNativeOutputState) -> (u32, u64, u64) {
    let h = owner.converter_owner().source_playback_history().unwrap();
    (
        h.source_rate(),
        h.initial_physical_frame(),
        h.initial_playback_frame(),
    )
}
fn exact(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}
fn generate(
    owner: &mut ConvertedNativeOutputState,
    t: &ConvertedTelemetry,
    held: bool,
    frames: usize,
) -> ConvertedRenderReport {
    let report = if held {
        owner.render_held_pending(frames)
    } else {
        owner.render_pending(frames)
    }
    .unwrap();
    t.publish_generated(Some(report), owner);
    owner.admit(frames).unwrap();
    t.publish_submitted(owner.admitted_target_time().unwrap());
    report
}
fn hook(owner: &mut ConvertedNativeOutputState, t: &ConvertedTelemetry, completed: &mut u64) {
    let mut output = ConvertedPump {
        state: owner,
        telemetry: t,
        retirement_completed: *completed,
    };
    output.before_pending().unwrap();
    *completed = output.retirement_completed;
}

#[derive(Clone, Copy)]
enum Step {
    Full,
    Count(usize),
    Again,
}
#[derive(Clone, Copy)]
enum Mode {
    None,
    Held,
    SourcePause,
}
struct Pcm<'a> {
    shared: &'a Shared,
    telemetry: &'a ConvertedTelemetry,
    producer: &'a mut CommandProducer,
    basis: TargetFrameBasis,
    identity: (u32, u64, u64),
    mode: Mode,
    enqueue: bool,
    steps: &'a [Step],
    stop_after: usize,
    writes: usize,
    waits: usize,
    timings: usize,
    drops: usize,
    length: usize,
    pcm: [f32; 512],
    reports: [Option<ConvertedRenderReport>; 64],
    revisions: [u64; 64],
    queued: [u64; 64],
}
impl PcmOperations for Pcm<'_> {
    fn write(&mut self, bytes: &[u8], frames: usize) -> Result<Option<usize>, LinuxError> {
        // Admission runs with the existing outer update held, even on a retry.
        assert_eq!(self.shared.timing.sequence() & 1, 1);
        let index = self.writes;
        let report = self.telemetry.read().unwrap().0;
        self.reports[index] = Some(report);
        self.revisions[index] = self.shared.timing.sequence();
        let source_pin = if matches!(self.mode, Mode::SourcePause) {
            let source = self.telemetry.last_real_source_report().unwrap();
            source.start_frame + source.frames as u64
        } else {
            self.identity.1
        };
        if self.enqueue && !matches!(self.mode, Mode::None) {
            self.queued[index] = self
                .telemetry
                .request_history_retirement(
                    self.basis,
                    self.identity,
                    source_pin,
                    report.target_end_time,
                )
                .unwrap()
                .expect("uncontended bounded enqueue");
        }
        match self.mode {
            Mode::Held => self.telemetry.set_held(index % 2 == 0),
            Mode::SourcePause => {
                self.producer.request_pause(index % 2 == 0);
            }
            Mode::None => {}
        }
        self.writes += 1;
        if self.writes == self.stop_after {
            self.shared.stop.store(true, Ordering::Release);
        }
        match self.steps.get(index).copied().unwrap_or(Step::Full) {
            Step::Again => Ok(None),
            step => {
                let count = match step {
                    Step::Count(n) => n,
                    _ => frames,
                };
                assert!(count <= frames);
                for word in bytes[..count * 4].chunks_exact(4) {
                    self.pcm[self.length] = f32::from_le_bytes(word.try_into().unwrap());
                    self.length += 1;
                }
                Ok(Some(count))
            }
        }
    }
    fn wait(&mut self) -> Result<bool, LinuxError> {
        self.waits += 1;
        // The actual worker must release its outer publication before waiting.
        assert_eq!(self.shared.timing.sequence() & 1, 0);
        assert!(self.telemetry.retirement_request().is_some() || matches!(self.mode, Mode::None));
        Ok(true)
    }
    fn timing<C: WorkerClock>(
        &mut self,
        _: &C,
        _: u64,
    ) -> Result<Option<AlsaTimingSnapshot>, LinuxError> {
        self.timings += 1;
        assert_eq!(self.shared.timing.sequence() & 1, 1);
        Ok(None)
    }
    fn drop_stream(&mut self) -> Result<(), LinuxError> {
        self.drops += 1;
        Ok(())
    }
}
impl<'a> Pcm<'a> {
    fn new(
        shared: &'a Shared,
        telemetry: &'a ConvertedTelemetry,
        producer: &'a mut CommandProducer,
        basis: TargetFrameBasis,
        identity: (u32, u64, u64),
        mode: Mode,
        steps: &'a [Step],
        stop_after: usize,
    ) -> Self {
        Self {
            shared,
            telemetry,
            producer,
            basis,
            identity,
            mode,
            enqueue: true,
            steps,
            stop_after,
            writes: 0,
            waits: 0,
            timings: 0,
            drops: 0,
            length: 0,
            pcm: [0.0; 512],
            reports: [None; 64],
            revisions: [0; 64],
            queued: [0; 64],
        }
    }
}
struct Clock(Cell<usize>);
impl WorkerClock for Clock {
    fn now(&self) -> Result<ClockPoint, LinuxError> {
        self.0.set(self.0.get() + 1);
        Ok(ClockPoint {
            domain: ClockDomainId(8),
            timestamp: Timestamp::from_nanos(self.0.get() as i64),
        })
    }
}
struct Encoder;
impl PcmEncoder for Encoder {
    fn encode(
        &mut self,
        format: DeviceFormat,
        samples: &[f32],
        bytes: &mut [u8],
    ) -> Result<(), LinuxError> {
        encode_pcm(format, samples, bytes).map_err(LinuxError::Conversion)
    }
}
fn run(
    pcm: &mut Pcm<'_>,
    owner: &mut ConvertedNativeOutputState,
    config: &AlsaAppliedConfig,
    clock: &Clock,
) -> Result<(), LinuxError> {
    let mut bytes = [0_u8; 128];
    let shared = pcm.shared;
    let t = pcm.telemetry;
    run_converted_worker(
        pcm,
        owner,
        config,
        &mut bytes[..config.period_frames as usize * 4],
        shared,
        clock,
        &mut Encoder,
        t,
    )
}

#[test]
fn actual_worker_retires_before_capacity_preflight_for_repeated_held_cycles_without_heap() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (mut producer, mut owner, config) = rig(source, target, 2, 3);
        let (_reference_producer, mut reference, _) = rig(source, target, 2, 64);
        let t = telemetry(&owner);
        let shared = Shared::with_converted_history(&t);
        let mut pcm = Pcm::new(
            &shared,
            &t,
            &mut producer,
            owner.target_frame_basis(),
            identity(&owner),
            Mode::Held,
            &[],
            31,
        );
        let clock = Clock(Cell::new(0));
        let (result, allocations) =
            crate::audio::count_heap_calls(|| run(&mut pcm, &mut owner, &config, &clock));
        result.unwrap();
        assert_eq!(allocations, 0);
        for index in 0..31 {
            let report = if index % 2 == 1 {
                reference.render_held_pending(2)
            } else {
                reference.render_pending(2)
            }
            .unwrap();
            assert_eq!(pcm.reports[index], Some(report));
            assert_eq!(
                &pcm.pcm[index * 2..index * 2 + 2],
                reference.pending_samples()
            );
            reference.admit(2).unwrap();
            assert!(pcm.queued[index] > 0);
        }
        assert_eq!(owner.phase_history().unwrap().capacity(), 3);
        assert!(owner.phase_history().unwrap().len() <= 2);
        assert!(owner.phase_history().unwrap().retirement_revision() >= 28);
        assert_eq!(
            owner.admitted_target_time(),
            reference.admitted_target_time()
        );
        assert_eq!(
            owner.converter_owner().source_position(),
            reference.converter_owner().source_position()
        );
        assert_eq!(
            owner.last_real_source_report(),
            reference.last_real_source_report()
        );
        assert_eq!(
            (pcm.writes, pcm.waits, pcm.timings, pcm.drops, clock.0.get()),
            (31, 0, 31, 1, 62)
        );
        assert_eq!(shared.renders.load(Ordering::SeqCst), 31);
        assert_eq!(shared.submitted.load(Ordering::SeqCst), 62);
    }
}

#[test]
fn actual_capacity_three_worker_without_request_refuses_the_fourth_transition() {
    let (mut producer, mut owner, config) = rig(3, 7, 2, 3);
    let t = telemetry(&owner);
    let shared = Shared::with_converted_history(&t);
    let mut pcm = Pcm::new(
        &shared,
        &t,
        &mut producer,
        owner.target_frame_basis(),
        identity(&owner),
        Mode::Held,
        &[],
        8,
    );
    pcm.enqueue = false;
    assert!(matches!(
        run(&mut pcm, &mut owner, &config, &Clock(Cell::new(0))),
        Err(LinuxError::Mixer(AudioError::RenderCapacity))
    ));
    assert_eq!(pcm.writes, 3);
    assert_eq!(owner.phase_history().unwrap().len(), 3);
    assert_eq!(owner.phase_history().unwrap().retirement_revision(), 0);
    assert_eq!(owner.pending_frames(), 0);
    assert_eq!(shared.renders.load(Ordering::SeqCst), 3);
    assert_eq!(shared.submitted.load(Ordering::SeqCst), 6);
    assert!(t.retirement_request().is_none());
}

#[test]
fn actual_worker_repeated_source_pause_resume_reclaims_real_source_spans() {
    for (source, target) in [(3, 7), (7, 3)] {
        let (mut producer, mut owner, config) = rig(source, target, target as usize, 3);
        let (mut reference_producer, mut reference, _) = rig(source, target, target as usize, 64);
        let t = telemetry(&owner);
        let shared = Shared::with_converted_history(&t);
        let mut pcm = Pcm::new(
            &shared,
            &t,
            &mut producer,
            owner.target_frame_basis(),
            identity(&owner),
            Mode::SourcePause,
            &[],
            24,
        );
        let (result, allocations) = crate::audio::count_heap_calls(|| {
            run(&mut pcm, &mut owner, &config, &Clock(Cell::new(0)))
        });
        result.unwrap();
        assert_eq!(allocations, 0);
        for index in 0..24 {
            if index != 0 {
                reference_producer.request_pause(index % 2 == 1);
            }
            let report = reference.render_pending(target as usize).unwrap();
            assert_eq!(pcm.reports[index], Some(report));
            assert_eq!(
                &pcm.pcm[index * target as usize..(index + 1) * target as usize],
                reference.pending_samples()
            );
            reference.admit(target as usize).unwrap();
        }
        let history = owner.converter_owner().source_playback_history().unwrap();
        assert_eq!(history.capacity(), 3);
        assert!(history.len() <= 3);
        assert!(history.retirement_revision() >= 20);
        assert_eq!(history.initial_physical_frame(), 0);
        assert_eq!(
            owner.mixer().frame_cursor(),
            reference.mixer().frame_cursor()
        );
        assert_eq!(owner.mixer().counters(), reference.mixer().counters());
        assert_eq!(owner.boundaries(), reference.boundaries());
    }
}

#[test]
fn no_request_keeps_original_native_render_admission_and_wait_cadence() {
    let (mut producer, mut owner, config) = rig(3, 7, 4, 3);
    let (_reference_producer, mut reference, _) = rig(3, 7, 4, 3);
    let t = telemetry(&owner);
    let shared = Shared::with_converted_history(&t);
    let steps = [Step::Count(1), Step::Again, Step::Full, Step::Full];
    let mut pcm = Pcm::new(
        &shared,
        &t,
        &mut producer,
        owner.target_frame_basis(),
        identity(&owner),
        Mode::None,
        &steps,
        4,
    );
    let clock = Clock(Cell::new(0));
    run(&mut pcm, &mut owner, &config, &clock).unwrap();
    let first = reference.render_pending(4).unwrap();
    assert_eq!(&pcm.pcm[..4], reference.pending_samples());
    reference.admit(4).unwrap();
    let second = reference.render_pending(4).unwrap();
    assert_eq!(&pcm.pcm[4..8], reference.pending_samples());
    reference.admit(4).unwrap();
    assert_eq!(
        pcm.reports[..4],
        [Some(first), Some(first), Some(first), Some(second)]
    );
    assert_eq!(
        (pcm.writes, pcm.waits, pcm.timings, pcm.drops, clock.0.get()),
        (4, 1, 4, 1, 6)
    );
    assert_eq!(shared.renders.load(Ordering::SeqCst), 2);
    assert_eq!(shared.submitted.load(Ordering::SeqCst), 8);
    assert_eq!(owner.phase_history().unwrap().retirement_revision(), 0);
    assert_eq!(
        owner
            .converter_owner()
            .source_playback_history()
            .unwrap()
            .retirement_revision(),
        0
    );
    assert!(t.retirement_request().is_none());
}

#[test]
fn partial_and_eagain_retain_original_report_then_same_request_completes_later() {
    let (mut producer, mut owner, config) = rig(3, 7, 4, 3);
    let t = telemetry(&owner);
    let basis = owner.target_frame_basis();
    generate(&mut owner, &t, false, 2);
    generate(&mut owner, &t, true, 2);
    let pending = owner.render_pending(4).unwrap();
    t.publish_generated(Some(pending), &owner);
    let samples: [f32; 4] = owner.pending_samples().try_into().unwrap();
    let receipt = t
        .request_history_retirement(basis, identity(&owner), 0, pending.target_end_time)
        .unwrap()
        .unwrap();
    let shared = Shared::with_converted_history(&t);
    shared.submitted.store(4, Ordering::SeqCst);
    let steps = [Step::Count(1), Step::Again, Step::Full, Step::Full];
    let mut pcm = Pcm::new(
        &shared,
        &t,
        &mut producer,
        basis,
        identity(&owner),
        Mode::None,
        &steps,
        4,
    );
    let clock = Clock(Cell::new(0));
    let (result, allocations) =
        crate::audio::count_heap_calls(|| run(&mut pcm, &mut owner, &config, &clock));
    result.unwrap();
    assert_eq!(allocations, 0);
    assert_eq!(&pcm.pcm[..4], &samples);
    assert_eq!(pcm.reports[..3], [Some(pending); 3]);
    assert_eq!(t.retirement_request().unwrap().revision, receipt);
    assert_eq!(pcm.waits, 1);
    assert_eq!(owner.phase_history().unwrap().retirement_revision(), 1);
    assert_eq!(
        owner.phase_history().unwrap().retained_start_time(),
        pending.target_start_time
    );
    assert_eq!(shared.submitted.load(Ordering::SeqCst), 12);
    assert_eq!(shared.timing.sequence() & 1, 0);
    // The request was deferred by the original pending block, then accepted
    // unchanged after its complete admission, without a fresh producer write.
    let mut completed = 0;
    hook(&mut owner, &t, &mut completed);
    assert_eq!(completed, receipt);
    assert_eq!(owner.pending_frames(), 0);
}

#[test]
fn actual_pending_hook_clamps_exact_target_and_source_pins_without_render_or_admit() {
    let (mut producer, mut owner, _) = rig(3, 7, 7, 8);
    let t = telemetry(&owner);
    let basis = owner.target_frame_basis();
    generate(&mut owner, &t, false, 7);
    producer.request_pause(true);
    generate(&mut owner, &t, false, 7);
    producer.request_pause(false);
    let pending = owner.render_pending(7).unwrap();
    t.publish_generated(Some(pending), &owner);
    owner.admit(1).unwrap();
    t.publish_submitted(owner.admitted_target_time().unwrap());
    let pulled = owner
        .converter_owner()
        .source_playback_history()
        .unwrap()
        .pulled_end_frame();
    let receipt = t
        .request_history_retirement(
            basis,
            identity(&owner),
            pulled.min(pending.source_position.frame),
            pending.target_end_time,
        )
        .unwrap()
        .unwrap();
    let pcm = owner.pending_samples().to_vec();
    let physical = owner.mixer().frame_cursor();
    let position = owner.converter_owner().source_position();
    let admitted = owner.admitted_target_time();
    let counters = owner.mixer().counters();
    let mut completed = 0;
    let (_, allocations) = crate::audio::count_heap_calls(|| hook(&mut owner, &t, &mut completed));
    assert_eq!(allocations, 0);
    assert_eq!(completed, 0);
    assert_eq!(owner.pending_samples(), pcm);
    assert_eq!(owner.pending_report(), Some(pending));
    assert_eq!(owner.mixer().frame_cursor(), physical);
    assert_eq!(owner.converter_owner().source_position(), position);
    assert_eq!(owner.admitted_target_time(), admitted);
    assert_eq!(owner.mixer().counters(), counters);
    assert_eq!(t.retirement_request().unwrap().revision, receipt);
    assert!(
        owner
            .converter_owner()
            .source_playback_history()
            .unwrap()
            .retained_start_frame()
            <= pending.source_start_position.frame
    );
    owner.admit(6).unwrap();
    t.publish_submitted(owner.admitted_target_time().unwrap());
    hook(&mut owner, &t, &mut completed);
    assert_eq!(completed, receipt);
}

#[test]
fn subnanosecond_target_pin_is_deferred_exactly_instead_of_rounded_to_pending_start() {
    let (_, mut owner, _) = rig(3, 7, 2, 3);
    let t = telemetry(&owner);
    let basis = owner.target_frame_basis();
    generate(&mut owner, &t, false, 2);
    let pending = owner.render_held_pending(2).unwrap();
    t.publish_generated(Some(pending), &owner);
    let start = pending.target_start_time;
    let scale = 10_000_000_000;
    let pin = TargetTime::new(
        start.seconds(),
        start.numerator() * scale + 1,
        start.denominator() * scale,
    )
    .unwrap();
    let nanos = |x: TargetTime| {
        u128::from(x.seconds()) * 1_000_000_000
            + u128::from(x.numerator()) * 1_000_000_000 / u128::from(x.denominator())
    };
    assert_eq!(nanos(start), nanos(pin));
    let receipt = t
        .request_history_retirement(basis, identity(&owner), 0, pin)
        .unwrap()
        .unwrap();
    let mut completed = 0;
    hook(&mut owner, &t, &mut completed);
    assert_eq!(completed, 0);
    assert_eq!(owner.phase_history().unwrap().retained_start_time(), start);
    assert_eq!(owner.pending_report(), Some(pending));
    assert_eq!(owner.pending_samples(), [0.0; 2]);
    owner.admit(2).unwrap();
    t.publish_submitted(owner.admitted_target_time().unwrap());
    hook(&mut owner, &t, &mut completed);
    assert_eq!(completed, receipt);
    assert_eq!(t.retirement_request().unwrap().target_pin, pin);
}

#[test]
fn actual_converter_lookahead_retains_original_source_until_cached_sampling_passes_it() {
    let (mut producer, mut owner, _) = rig(3, 7, 1, 8);
    let t = telemetry(&owner);
    let basis = owner.target_frame_basis();
    generate(&mut owner, &t, false, 1);
    producer.request_pause(true);
    let mut paused = None;
    for _ in 0..16 {
        let report = generate(&mut owner, &t, false, 1);
        if report.source.is_some_and(|r| r.paused && r.frames != 0) {
            paused = Some(report);
            break;
        }
    }
    let paused = paused.expect("actual source pause adopted");
    let h = owner.converter_owner().source_playback_history().unwrap();
    let pulled = h.pulled_end_frame();
    let next = owner.converter_owner().source_position();
    assert!(next.frame < h.span(1).unwrap().physical_start());
    let receipt = t
        .request_history_retirement(basis, identity(&owner), pulled, paused.target_end_time)
        .unwrap()
        .unwrap();
    let mut completed = 0;
    hook(&mut owner, &t, &mut completed);
    assert_eq!(completed, 0);
    assert_eq!(
        owner
            .converter_owner()
            .source_playback_history()
            .unwrap()
            .len(),
        2
    );
    let cached = owner.render_pending(1).unwrap();
    assert_eq!(cached.source.unwrap().frames, 0);
    assert_eq!(cached.source_start_position, paused.source_position);
    assert_eq!(
        owner
            .converter_owner()
            .source_playback_history()
            .unwrap()
            .lookup_at_interpolated_time(
                owner.phase_history().unwrap().span(0).unwrap(),
                exact(cached.target_start_time),
                owner.phase_history().unwrap().end_time()
            )
            .unwrap()
            .state(),
        SourcePlaybackState::Running
    );
    owner.admit(1).unwrap();
    t.publish_generated(Some(cached), &owner);
    t.publish_submitted(owner.admitted_target_time().unwrap());
    for _ in 0..16 {
        hook(&mut owner, &t, &mut completed);
        if completed == receipt {
            break;
        }
        generate(&mut owner, &t, false, 1);
    }
    assert_eq!(completed, receipt);
    assert_eq!(
        owner
            .converter_owner()
            .source_playback_history()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn request_ranges_identity_and_basis_refuse_before_owner_or_existing_request_changes() {
    let (_, mut owner, _) = rig(3, 7, 2, 3);
    let t = telemetry(&owner);
    let basis = owner.target_frame_basis();
    let id = identity(&owner);
    let report = generate(&mut owner, &t, false, 2);
    let receipt = t
        .request_history_retirement(basis, id, 0, report.target_end_time)
        .unwrap()
        .unwrap();
    let bad_basis = TargetFrameBasis::new(basis.origin(), basis.start_time(), 8).unwrap();
    let future = report.target_end_time.checked_add_frames(1, 7).unwrap();
    for (b, i, source, target) in [
        (bad_basis, id, 0, report.target_end_time),
        (basis, (4, 0, 0), 0, report.target_end_time),
        (basis, (3, 1, 0), 0, report.target_end_time),
        (basis, (3, 0, 1), 0, report.target_end_time),
        (basis, id, u64::MAX, report.target_end_time),
        (basis, id, 0, future),
        (basis, id, 0, report.target_start_time),
    ] {
        let (result, allocations) =
            crate::audio::count_heap_calls(|| t.request_history_retirement(b, i, source, target));
        assert_eq!(result, Err(AudioError::InvalidFormat));
        assert_eq!(allocations, 0);
        assert_eq!(t.retirement_request().unwrap().revision, receipt);
        assert_eq!(owner.phase_history().unwrap().len(), 1);
        assert_eq!(owner.phase_history().unwrap().retirement_revision(), 0);
        assert_eq!(owner.admitted_target_time(), Some(report.target_end_time));
        assert_eq!(
            owner.mixer().frame_cursor(),
            report.source.unwrap().start_frame + report.source.unwrap().frames as u64
        );
    }
}

#[test]
fn cold_both_histories_accept_only_actual_creation_and_unsupported_modes_refuse() {
    let (_, mut owner, _) = rig(3, 7, 2, 3);
    let t = telemetry(&owner);
    let basis = owner.target_frame_basis();
    let id = identity(&owner);
    let receipt = t
        .request_history_retirement(basis, id, 0, basis.start_time())
        .unwrap()
        .unwrap();
    let mut completed = 0;
    let (_, allocations) = crate::audio::count_heap_calls(|| hook(&mut owner, &t, &mut completed));
    assert_eq!(allocations, 0);
    assert_eq!(completed, receipt);
    assert_eq!(owner.phase_history().unwrap().len(), 0);
    assert_eq!(
        owner
            .converter_owner()
            .source_playback_history()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        t.request_history_retirement(basis, id, 1, basis.start_time()),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(
        t.request_history_retirement(
            basis,
            id,
            0,
            basis.start_time().checked_add_frames(1, 7).unwrap()
        ),
        Err(AudioError::InvalidFormat)
    );
    let legacy = ConvertedTelemetry::new();
    assert_eq!(
        legacy.request_history_retirement(basis, id, 0, basis.start_time()),
        Err(AudioError::InvalidCapacity)
    );
    let (_, mut missing_source, _) = rig_boundaries(2, 3, 7, None, None);
    missing_source.enable_phase_history(3).unwrap();
    let phase_only = telemetry(&missing_source);
    assert_eq!(
        phase_only.request_history_retirement(basis, id, 0, basis.start_time()),
        Err(AudioError::InvalidCapacity)
    );
}

#[test]
fn actual_retirement_publishes_one_original_compound_generation_inside_outer_update() {
    let (_, mut owner, _) = rig(3, 7, 2, 3);
    let t = telemetry(&owner);
    let basis = owner.target_frame_basis();
    generate(&mut owner, &t, false, 2);
    generate(&mut owner, &t, true, 2);
    let resumed = generate(&mut owner, &t, false, 2);
    let before = t.phase_snapshot_at(resumed.target_start_time).unwrap();
    let shared = Shared::with_converted_history(&t);
    shared.status.store(1, Ordering::SeqCst);
    shared.submitted.store(6, Ordering::SeqCst);
    let receipt = t
        .request_history_retirement(basis, identity(&owner), 0, resumed.target_start_time)
        .unwrap()
        .unwrap();
    let outer_before = shared.timing.sequence();
    let guard = shared.timing.update();
    let mut completed = 0;
    hook(&mut owner, &t, &mut completed);
    assert_eq!(completed, receipt);
    assert_eq!(shared.timing.sequence(), outer_before + 1);
    assert!(shared_converted_playback_with(
        &shared,
        &t,
        basis,
        ClockDomainId(8),
        &exact(resumed.target_start_time),
        3,
        0,
        0,
        |_| {}
    )
    .is_none());
    let inner = t.phase_snapshot_at(resumed.target_start_time).unwrap();
    assert_eq!(inner.revision, before.revision + 2);
    assert_eq!(inner.report, before.report);
    assert_eq!(inner.boundaries, before.boundaries);
    assert_eq!(inner.generated_end, before.generated_end);
    assert_eq!(inner.submitted_end, before.submitted_end);
    assert_eq!(inner.span.to_words(), before.span.to_words());
    drop(guard);
    let after = shared_converted_playback_with(
        &shared,
        &t,
        basis,
        ClockDomainId(8),
        &exact(resumed.target_start_time),
        3,
        0,
        0,
        |_| {},
    )
    .unwrap();
    assert_eq!(after.revision, outer_before + 2);
    assert_eq!(after.phase.phase.revision, inner.revision);
    assert_eq!(after.submitted_target_frames, 6);
    assert!(after.timing.is_none());
    assert!(t
        .phase_snapshot_at(TargetTime::from_frames(0, 7).unwrap())
        .is_none());
    assert_eq!(
        t.request_history_retirement(
            basis,
            identity(&owner),
            0,
            TargetTime::from_frames(0, 7).unwrap()
        ),
        Err(AudioError::InvalidFormat)
    );
    assert_eq!(t.retirement_request().unwrap().revision, receipt);
}
