//! Shared public setup only. Each owning suite supplies its own timing/PCM oracle.
#![allow(dead_code)]
use beatkernel::{
    audio::*,
    input::*,
    judge::{JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow},
    replay::codec::{ReplayCodecLimits, ReplayFile},
    runtime::{Runtime, SoundBinding},
    time::*,
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsInputMode, ParseOptions, parse};
use beatkernel_bms_runtime::{
    PreparedBms, gauge::GaugeProfile, mine_plan::prepare_judge, replay_capture::LiveReplayCapture,
};
use std::collections::BTreeMap;
pub const DOMAIN: ClockDomainId = ClockDomainId(31);
pub const MAIN: &str = "#BPM 60\n#LNTYPE 1\n#WAV01 tap.wav\n#WAV02 hold.wav\n#WAV03 layer.wav\n#00011:00010000\n#00052:00000202\n#00001:00030000\n";
pub fn point(n: i64) -> ClockPoint {
    ClockPoint {
        domain: DOMAIN,
        timestamp: Timestamp::from_nanos(n),
    }
}
pub fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(
        16 << 20,
        20_000,
        4096,
        CodecLimits::new(4096, 1024).unwrap(),
    )
    .unwrap()
}
pub fn pcm_limits() -> PcmLimits {
    PcmLimits::new(4096, 16384, 8).unwrap()
}
pub fn prepared(text: &str, rate: u32, assets: &[(u64, &[f32])]) -> PreparedBms {
    let source = parse(text, ParseOptions::default()).unwrap();
    let compiled = source.compile().unwrap();
    let format = AudioFormat::new(rate, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits()).unwrap();
    for &(id, pcm) in assets {
        bank.insert(
            SampleId(id),
            PcmSample::new(format, pcm.to_vec(), pcm_limits()).unwrap(),
        )
        .unwrap();
    }
    let holds: BTreeMap<_, _> = compiled
        .chart
        .objects()
        .iter()
        .map(|o| (o.id, o.time.end.is_some()))
        .collect();
    let sounds = source
        .notes
        .iter()
        .map(|n| SoundBinding {
            object: n.object,
            stage: if holds[&n.object] {
                JudgeStage::HoldHead
            } else {
                JudgeStage::Instant
            },
            sample: n.sample,
            voice: VoiceId(n.object.0),
            gain: 1.0,
        })
        .collect();
    let first = compiled
        .chart
        .objects()
        .iter()
        .map(|o| o.id.0)
        .max()
        .unwrap_or(0)
        + 1;
    let bgm_commands = compiled
        .bgm
        .iter()
        .enumerate()
        .map(|(i, c)| AudioCommand::Play {
            voice: VoiceId(first + i as u64),
            sample: c.sample,
            at: c.at,
            gain: 1.0,
        })
        .collect();
    PreparedBms {
        source,
        compiled,
        bank,
        sounds,
        bgm_commands,
    }
}
pub fn main_prepared() -> PreparedBms {
    prepared(
        MAIN,
        8,
        &[
            (1, &[0.5, 0.25, -0.5, -0.25]),
            (2, &[0.75, 0.375, -0.75, -0.375]),
            (3, &[0.125, 0.0625, -0.125, -0.0625]),
        ],
    )
}
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
pub fn captured(p: &PreparedBms, offset: i64, finish: Option<i64>) -> ReplayFile {
    captured_at(p, offset, Timestamp::ZERO, None, finish)
}
pub fn captured_at(
    p: &PreparedBms,
    offset: i64,
    start: Timestamp,
    end: Option<Timestamp>,
    finish: Option<i64>,
) -> ReplayFile {
    let controls: BTreeMap<_, _> = p
        .source
        .notes
        .iter()
        .map(|n| (n.object, n.lane.control()))
        .collect();
    let mut events = Vec::new();
    for o in p.compiled.chart.objects() {
        let at = o.time.start.as_nanos().checked_sub(offset).unwrap();
        let control = controls[&o.id].0;
        events.push((at, control, ButtonState::Down));
        events.push((
            o.time
                .end
                .map_or(at, |t| t.as_nanos().checked_sub(offset).unwrap()),
            control,
            ButtonState::Up,
        ));
    }
    events.sort_by_key(|e| e.0);
    capture_events(
        p,
        offset,
        start,
        end,
        &GaugeProfile::default(),
        &events,
        finish,
    )
}
pub fn capture_events(
    p: &PreparedBms,
    offset: i64,
    start: Timestamp,
    end: Option<Timestamp>,
    gauge: &GaugeProfile,
    events: &[(i64, u32, ButtonState)],
    finish: Option<i64>,
) -> ReplayFile {
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(2),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::from_nanos(offset),
    )
    .unwrap();
    let judge = prepare_judge(
        &p.source,
        p.compiled.chart.clone(),
        profile,
        BmsInputMode::ButtonOnly,
        ParseOptions::default().max_objects,
    )
    .unwrap();
    let sound_identity =
        beatkernel_bms_runtime::input_sounds::InputSoundIdentity::from_source(&p.source).unwrap();
    let mut capture = LiveReplayCapture::new_with_gauge(
        &judge,
        DOMAIN,
        limits(),
        start,
        0,
        end,
        BmsInputMode::ButtonOnly,
        sound_identity,
        gauge,
    )
    .unwrap();
    let controls: BTreeMap<_, _> = events.iter().map(|e| (e.1, e.1)).collect();
    let bindings = BindingMap::from_bindings(controls.keys().map(|&c| Binding {
        device: DeviceSelector::Exact(DeviceId(1)),
        physical: PhysicalControlId::keyboard(c as u16),
        game_control: GameControlId(c),
    }))
    .unwrap();
    let (producer, _consumer) = command_queue(1).unwrap();
    let anchor = events
        .first()
        .map_or(start.as_nanos(), |e| e.0.min(start.as_nanos()));
    let mut runtime = Runtime::new(
        DOMAIN,
        DOMAIN,
        Transport::new(
            Timestamp::from_nanos(anchor),
            Timestamp::from_nanos(anchor),
            Rate::NORMAL,
        ),
        bindings,
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    for (sequence, &(at, control, state)) in events.iter().enumerate() {
        let report = runtime
            .process_input(
                PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(1), point(at), sequence as u64),
                    control: PhysicalControlId::keyboard(control as u16),
                    state,
                }),
                &Identity,
                point(at),
            )
            .unwrap();
        assert!(report.judge_error.is_none());
        capture.record_report(&report).unwrap();
    }
    if let Some(at) = finish {
        capture
            .record_report(&runtime.advance_to(point(at), &Identity, point(at)).unwrap())
            .unwrap();
    }
    capture.into_file()
}
