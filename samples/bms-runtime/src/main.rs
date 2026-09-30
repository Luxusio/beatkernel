//! Offline BMS composition: synthetic perfect input, real WAV assets, packed PCM.
use beatkernel::{audio::*, input::*, judge::*, runtime::*, time::*, transport::*};
use beatkernel_bms::{parse, ParseOptions};
use beatkernel_platform::audio::{encode_pcm, DeviceFormat, SampleEncoding};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Write},
    path::{Component, Path},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const DOMAIN: ClockDomainId = ClockDomainId(1);
const BLOCK: usize = 4096;
struct SameClock;
impl ClockMapper for SameClock {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn point(timestamp: Timestamp) -> ClockPoint {
    ClockPoint {
        domain: DOMAIN,
        timestamp,
    }
}
fn bounded_read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take((limit as u64) + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err("input file exceeds sample limit".into());
    }
    Ok(bytes)
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 1 && args[0] == "--help" {
        println!("beatkernel-bms-runtime CHART.bms NEW_OUTPUT.f32le SECONDS RATE\nOffline synthetic perfect input; bounded WAV assets, raw PCM output; no native playback.");
        return Ok(());
    }
    if args.len() != 4 {
        return Err("usage: beatkernel-bms-runtime CHART NEW_OUTPUT SECONDS RATE".into());
    }
    let seconds: u64 = args[2].parse()?;
    let rate: u32 = args[3].parse()?;
    if seconds == 0 || rate == 0 {
        return Err("duration/rate must be positive".into());
    }
    let frames = seconds
        .checked_mul(u64::from(rate))
        .ok_or("render extent overflow")?;
    let chart_path = std::fs::canonicalize(&args[0])?;
    let root = chart_path.parent().ok_or("chart has no parent")?;
    let chart = parse(
        std::str::from_utf8(&bounded_read(&chart_path, 8 * 1024 * 1024)?)?,
        ParseOptions::default(),
    )?;
    let compiled = chart.compile()?;
    let count = chart
        .notes
        .len()
        .checked_add(compiled.bgm.len())
        .ok_or("command count overflow")?;
    if count > AudioLimits::MAX_VOICES {
        return Err("offline sample supports at most 4096 scheduled sounds".into());
    }
    let pcm_limits = PcmLimits::new(64 * 1024 * 1024, 256 * 1024 * 1024, 1295)?;
    let referenced: BTreeSet<_> = chart
        .notes
        .iter()
        .map(|note| note.sample)
        .chain(compiled.bgm.iter().map(|bgm| bgm.sample))
        .collect();
    let mut bank: Option<SampleBank> = None;
    for sample in referenced {
        let name = chart
            .samples
            .get(&u16::try_from(sample.0)?)
            .ok_or("missing WAV reference")?;
        if name.as_bytes().get(1) == Some(&b':') {
            return Err("absolute asset path rejected".into());
        }
        let portable_name = name.replace('\\', "/");
        let relative = Path::new(&portable_name);
        if relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
        {
            return Err("absolute/parent asset path rejected".into());
        }
        let path = std::fs::canonicalize(root.join(relative))?;
        if !path.starts_with(root) {
            return Err("asset symlink escapes chart directory".into());
        }
        let pcm = PcmSample::from_wav(&bounded_read(&path, 64 * 1024 * 1024)?, pcm_limits)?;
        if bank.is_none() {
            bank = Some(SampleBank::new(
                AudioFormat::new(rate, pcm.format().channels())?,
                pcm_limits,
            )?);
        }
        bank.as_mut()
            .ok_or("missing sample bank")?
            .insert(sample, pcm)?;
    }
    let bank = match bank {
        Some(bank) => bank,
        None => SampleBank::new(AudioFormat::new(rate, 2)?, pcm_limits)?,
    };
    let format = bank.format();
    let capacity = count.max(1);
    let limits = AudioLimits::new(capacity, capacity, capacity, BLOCK, capacity)?;
    let (mut producer, consumer) = command_queue(capacity)?;
    for (index, bgm) in compiled.bgm.iter().enumerate() {
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId((chart.notes.len() + index) as u64),
                sample: bgm.sample,
                at: bgm.at,
                gain: 1.0,
            })
            .map_err(|e| format!("BGM queue: {e:?}"))?;
    }
    let lanes: BTreeSet<_> = chart.notes.iter().map(|note| note.lane.channel()).collect();
    let physical: BTreeMap<_, _> = lanes
        .into_iter()
        .enumerate()
        .map(|(index, lane)| (lane, PhysicalControlId::keyboard(4 + index as u16)))
        .collect();
    let bindings = BindingMap::from_bindings(
        chart
            .notes
            .iter()
            .map(|note| (note.lane.channel(), note.lane.control()))
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .map(|(lane, game_control)| Binding {
                device: DeviceSelector::Exact(DeviceId(1)),
                physical: physical[&lane],
                game_control,
            }),
    )?;
    let mut sounds = Vec::new();
    let mut events = Vec::new();
    for (index, note) in chart.notes.iter().enumerate() {
        let object = compiled
            .chart
            .objects()
            .iter()
            .find(|object| object.id == note.object)
            .ok_or("missing compiled object")?;
        sounds.push(SoundBinding {
            object: note.object,
            stage: if object.time.end.is_some() {
                JudgeStage::HoldHead
            } else {
                JudgeStage::Instant
            },
            sample: note.sample,
            voice: VoiceId(index as u64),
            gain: 1.0,
        });
        events.push((
            object.time.start,
            physical[&note.lane.channel()],
            ButtonState::Down,
        ));
        events.push((
            object.time.end.unwrap_or(object.time.start),
            physical[&note.lane.channel()],
            ButtonState::Up,
        ));
    }
    events.sort_by_key(|event| event.0);
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::from_nanos(1_000_000),
            late: Duration::from_nanos(1_000_000),
        }],
        Duration::ZERO,
    )?;
    let judge = JudgeEngine::new(compiled.chart, chart.rules(), profile)?;
    let mut runtime = Runtime::new(
        DOMAIN,
        DOMAIN,
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings,
        judge,
        producer,
        sounds,
        256,
    )?;
    let mut hits = 0;
    for (sequence, (at, control, state)) in events.into_iter().enumerate() {
        let input = PhysicalInputEvent::Button(ButtonEvent {
            meta: EventMeta::new(DeviceId(1), point(at), sequence as u64),
            control,
            state,
        });
        let report = runtime.process_input(input, &SameClock, point(at))?;
        if let Some(error) = report.judge_error {
            return Err(error.into());
        }
        if !report.audio_failures.is_empty() {
            return Err("keysound queue admission failed".into());
        }
        hits += report
            .judge_events
            .iter()
            .filter(|event| matches!(event.outcome, JudgeOutcome::Hit { .. }))
            .count();
    }
    let mut mixer = Mixer::new(
        MixerConfig::new(format, DOMAIN, Timestamp::ZERO, limits),
        bank,
        consumer,
    )?;
    let device = DeviceFormat::new(rate, format.channels(), SampleEncoding::Float32, None)?;
    let mut pcm = vec![0.0; BLOCK * usize::from(format.channels())];
    let mut bytes = vec![0; pcm.len() * 4];
    let mut output = File::create_new(&args[1])?;
    let mut remaining = frames;
    while remaining != 0 {
        let block = remaining.min(BLOCK as u64) as usize;
        let samples = block * usize::from(format.channels());
        mixer.render(&mut pcm[..samples])?;
        encode_pcm(device, &pcm[..samples], &mut bytes[..samples * 4])?;
        output.write_all(&bytes[..samples * 4])?;
        remaining -= block as u64;
    }
    output.flush()?;
    println!("synthetic perfect input: {hits} hits; {frames} frames, {} channels at {rate}Hz; raw f32le output; no native playback", format.channels());
    Ok(())
}
