//! ALSA-only output conversion on the native play thread, before retirement.
use crate::{
    gameplay::output::domain::control::{OutputCapability, OutputRequest},
    settings::{NativeSettings, SettingsHost},
    gameplay::output::adapters::alsa::{AlsaReplacementBackend, AlsaReplacementOutput},
    gameplay::output::application::requests::GameplayOutputUi,
    gameplay::output::adapters::player::PlayerOutputUi,
    gameplay::output::application::owner::GameplayOutputOwner,
    gameplay_presentation::GameplayOutputContext,
};
use beatkernel::{audio::AudioLimits, time::ClockPoint};
use beatkernel_platform::{
    linux::{AlsaRequest, AlsaAppliedConfig},
    audio::presentation::discipline::PresentationDiscipline,
};
pub fn request_for_args(current: &AlsaRequest, args: &[String]) -> Result<AlsaRequest, String> {
    let draft = NativeSettings::output_only(args, SettingsHost::Linux)?;
    let mut request = current.clone();
    for field in draft.fields() {
        if field.value.is_empty() {
            continue;
        }
        match field.flag {
            "--alsa" => request.device = field.value.clone(),
            "--period-frames" => request.period_frames = parse_frames(&field.value)?,
            "--buffer-frames" => request.buffer_frames = parse_frames(&field.value)?,
            _ => return Err("unsupported ALSA output field".into()),
        }
    }
    if request.period_frames == 0
        || request.period_frames >= request.buffer_frames
        || request.period_frames as usize > AudioLimits::MAX_RENDER_FRAMES
    {
        return Err("processing period must be positive, within the render limit and smaller than the buffer".into());
    }
    Ok(request)
}
fn parse_frames(value: &str) -> Result<u32, String> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("frame count must be unsigned decimal".into());
    }
    value.parse().map_err(|_| "frame count exceeds u32".into())
}
pub fn capability(applied: &AlsaAppliedConfig) -> Result<OutputCapability, String> {
    let adjusted = applied.buffer_frames != applied.requested.buffer_frames
        || applied.period_frames != applied.requested.period_frames;
    if applied.period_frames == 0
        || applied.period_frames >= applied.buffer_frames
        || applied.period_frames as usize > AudioLimits::MAX_RENDER_FRAMES
        || applied.format != applied.requested.format
        || adjusted != applied.sizing_adjusted
        || (adjusted && !applied.requested.allow_size_rounding)
    {
        return Err("applied ALSA output metadata is inconsistent".into());
    }
    let cap = OutputCapability {
        host: SettingsHost::Linux,
        current_args: vec![
            "--alsa".into(),
            applied.requested.device.clone(),
            "--buffer-frames".into(),
            applied.buffer_frames.to_string(),
            "--period-frames".into(),
            applied.period_frames.to_string(),
        ],
    };
    cap.validate()?;
    Ok(cap)
}
pub struct NativeAlsaOutputUi {
    bridge: GameplayOutputUi<PlayerOutputUi>,
}
impl NativeAlsaOutputUi {
    pub fn new(
        owner: &GameplayOutputOwner<AlsaReplacementBackend>,
        enabled: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut bridge = GameplayOutputUi::new(PlayerOutputUi);
        let cap = if enabled {
            owner
                .current()
                .map(|out| capability(out.stream().configuration()))
                .transpose()?
        } else {
            None
        };
        bridge.advertise(cap)?;
        Ok(Self { bridge })
    }
    pub fn pending(&self) -> bool {
        self.bridge.pending()
    }
    pub fn service(
        &mut self,
        owner: &mut GameplayOutputOwner<AlsaReplacementBackend>,
        context: GameplayOutputContext<'_, PresentationDiscipline>,
        now: ClockPoint,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.bridge.service(
            owner,
            context,
            now,
            &mut |req: &OutputRequest, out: &AlsaReplacementOutput| {
                let applied = out.stream().configuration();
                let mut current = applied.requested.clone();
                current.buffer_frames = applied.buffer_frames;
                current.period_frames = applied.period_frames;
                request_for_args(&current, &req.args)
            },
            &mut |out: &AlsaReplacementOutput| capability(out.stream().configuration()),
        )
    }
}
