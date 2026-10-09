//! Host-neutral member construction over the existing chart, rules and voice allocator.
use crate::{
    input_sounds::InputSoundPlan,
    local_players::{validate_source_routes, PlayerId, ResolvedInputPlan},
    local_runtime::{MemberConfig, VoiceAllocator},
    mine_plan::prepare_judge_with_timing,
    mine_sounds::MineSoundPlan,
    PreparedBms,
};
use beatkernel::{
    audio::{AudioCommand, VoiceId},
    input::{BindingMap, DeviceSelector},
    judge::JudgeProfile,
    runtime::{hazard_sound::HazardSoundTimeline, input_sound::InputSoundTimeline},
};
use beatkernel_bms::BmsInputMode;
use std::collections::BTreeSet;

/// All members are returned together; the caller retains the original PCM bank.
pub struct PreparedLocalMembers {
    pub configs: Vec<MemberConfig>,
    pub reserved: Vec<VoiceId>,
}

/// Build independent actual judges and sounds in immutable source-plan order.
/// Binding coverage is by configured game control, not an inferred input evaluator.
pub fn prepare_local_members(
    prepared: &PreparedBms,
    plan: &ResolvedInputPlan,
    bindings: Vec<BindingMap>,
    profile: JudgeProfile,
    input_mode: BmsInputMode,
) -> Result<PreparedLocalMembers, String> {
    prepare_local_members_with_timing(prepared, plan, bindings, profile, input_mode, None)
}

/// Builds independent members with the same explicit immutable stage windows.
/// Legacy callers retain their original builtin rules and snapshot bytes.
pub fn prepare_local_members_with_timing(
    prepared: &PreparedBms,
    plan: &ResolvedInputPlan,
    bindings: Vec<BindingMap>,
    profile: JudgeProfile,
    input_mode: BmsInputMode,
    timing: Option<&beatkernel_bms::BmsTimingProfiles>,
) -> Result<PreparedLocalMembers, String> {
    if bindings.len() != plan.members().len() {
        return Err("local binding map count differs from source plan".into());
    }
    let required_lanes: BTreeSet<_> = prepared
        .source
        .notes
        .iter()
        .map(|note| note.lane)
        .chain(prepared.source.invisible.iter().map(|event| event.lane))
        .chain(prepared.source.mines.iter().map(|event| event.lane))
        .collect();
    for (&(player, source), bindings) in plan.members().iter().zip(&bindings) {
        if let Some(source) = source {
            if bindings
                .bindings()
                .iter()
                .any(|binding| binding.device != DeviceSelector::Exact(source))
            {
                return Err("assigned player's bindings must select its exact device".into());
            }
        }
        for lane in &required_lanes {
            if !bindings
                .bindings()
                .iter()
                .any(|binding| binding.game_control == lane.control())
            {
                return Err(format!(
                    "player {} lacks binding for BMS channel{:02X}",
                    player.0,
                    lane.channel()
                ));
            }
        }
    }
    if prepared.sounds.iter().any(|sound| !sound.gain.is_finite()) {
        return Err("cohort sound has nonfinite gain".into());
    }
    let mut reserved = Vec::new();
    reserved
        .try_reserve_exact(prepared.bgm_commands.len())
        .map_err(|_| "cohort reserved voice allocation failed")?;
    for command in &prepared.bgm_commands {
        match command {
            AudioCommand::Play { voice, .. } => reserved.push(*voice),
            _ => return Err("prepared BGM command is not Play".into()),
        }
    }
    let first = reserved
        .iter()
        .map(|voice| voice.0)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or("cohort voice namespace overflow")?;
    let mut allocator = VoiceAllocator::new(first);
    let mut configs = Vec::new();
    configs
        .try_reserve_exact(plan.members().len())
        .map_err(|_| "local member allocation failed")?;
    let pristine = prepare_judge_with_timing(
        &prepared.source,
        prepared.compiled.chart.clone(),
        profile,
        input_mode,
        beatkernel_bms::ParseOptions::default().max_objects,
        timing,
    )?;
    let checkpoint = if plan.members().len() > 1 {
        Some(pristine.snapshot().map_err(|error| error.to_string())?)
    } else {
        None
    };
    let mut first_judge = Some(pristine);
    for (&(player, device), bindings) in plan.members().iter().zip(bindings) {
        let judge = match first_judge.take() {
            Some(judge) => judge,
            None => beatkernel::judge::JudgeEngine::from_snapshot(
                checkpoint.as_ref().expect("multiple member checkpoint"),
            )
            .map_err(|error| error.to_string())?,
        };
        let mut sounds = Vec::new();
        sounds
            .try_reserve_exact(prepared.sounds.len())
            .map_err(|_| "local sound binding allocation failed")?;
        sounds.extend_from_slice(&prepared.sounds);
        allocator.remap(&mut sounds)?;
        configs.push(MemberConfig {
            player,
            device,
            bindings,
            judge,
            sounds,
        });
    }
    Ok(PreparedLocalMembers { configs, reserved })
}

/// Prepares disjoint member fallback voices over the same original PCM bank.
/// Empty invisible sources preserve the unconfigured legacy path. The actual
/// member sound IDs, not their pre-remap source IDs, determine the new range.
pub fn prepare_local_input_sounds(
    prepared: &PreparedBms,
    members: &[MemberConfig],
    reserved: &[VoiceId],
) -> Result<Vec<(PlayerId, InputSoundTimeline)>, String> {
    if prepared.source.invisible.is_empty() {
        return Ok(Vec::new());
    }
    validate_source_routes(members.iter().map(|member| (member.player, member.device)))?;
    let capacity = beatkernel_bms::ParseOptions::default().max_objects;
    let plan = InputSoundPlan::prepare(&prepared.source, &[], &prepared.bgm_commands, capacity)?;
    for &sample in plan.samples() {
        if prepared.bank.get(sample).is_none() {
            return Err("local input sound sample is missing from shared PCM bank".into());
        }
    }
    if members
        .iter()
        .flat_map(|member| &member.sounds)
        .any(|sound| !sound.gain.is_finite())
    {
        return Err("local input sound preparation found nonfinite gameplay gain".into());
    }
    let occupied = members
        .iter()
        .flat_map(|member| &member.sounds)
        .map(|sound| sound.voice.0)
        .chain(reserved.iter().map(|voice| voice.0))
        .chain(
            prepared
                .bgm_commands
                .iter()
                .filter_map(|command| match command {
                    AudioCommand::Play { voice, .. } => Some(voice.0),
                    _ => None,
                }),
        )
        .max()
        .unwrap_or(0);
    let first = occupied
        .checked_add(1)
        .ok_or("local input sound voice namespace exhausted")?;
    let mut allocator = VoiceAllocator::new(first);
    let template = plan.timeline();
    let mut timelines = Vec::new();
    timelines
        .try_reserve_exact(members.len())
        .map_err(|_| "local input sound allocation failed")?;
    for member in members {
        let mut markers = Vec::new();
        markers
            .try_reserve_exact(template.markers().len())
            .map_err(|_| "local input sound allocation failed")?;
        markers.extend_from_slice(template.markers());
        allocator.remap_input_sounds(&mut markers)?;
        let timeline =
            InputSoundTimeline::new(markers, capacity).map_err(|error| error.to_string())?;
        timelines.push((member.player, timeline));
    }
    Ok(timelines)
}

/// Prepares optional WAV00 voices after every actual gameplay, reserved and
/// press-sound voice. Source and member order remain unchanged.
pub fn prepare_local_mine_sounds(
    prepared: &PreparedBms,
    members: &[MemberConfig],
    reserved: &[VoiceId],
    input_sounds: &[(PlayerId, InputSoundTimeline)],
) -> Result<Vec<(PlayerId, HazardSoundTimeline)>, String> {
    if prepared.source.mines.is_empty() {
        return Ok(Vec::new());
    }
    let capacity = beatkernel_bms::ParseOptions::default().max_objects;
    let plan = MineSoundPlan::prepare(
        &prepared.source,
        &[],
        &prepared.bgm_commands,
        None,
        capacity,
    )?;
    if plan.bindings().is_empty() {
        return Ok(Vec::new());
    }
    validate_source_routes(members.iter().map(|member| (member.player, member.device)))?;
    if (!input_sounds.is_empty() || !prepared.source.invisible.is_empty())
        && (input_sounds.len() != members.len()
            || members
                .iter()
                .zip(input_sounds)
                .any(|(member, (player, _))| member.player != *player))
    {
        return Err("local mine sound press timelines differ from source-plan order".into());
    }
    for &sample in plan.samples() {
        if prepared.bank.get(sample).is_none() {
            return Err("local mine sound sample is missing from shared PCM bank".into());
        }
    }
    if members
        .iter()
        .flat_map(|member| &member.sounds)
        .any(|sound| !sound.gain.is_finite())
    {
        return Err("local mine sound preparation found nonfinite gameplay gain".into());
    }
    let occupied = members
        .iter()
        .flat_map(|member| &member.sounds)
        .map(|sound| sound.voice.0)
        .chain(reserved.iter().map(|voice| voice.0))
        .chain(
            input_sounds
                .iter()
                .flat_map(|(_, timeline)| timeline.markers())
                .map(|marker| marker.voice.0),
        )
        .chain(
            prepared
                .bgm_commands
                .iter()
                .filter_map(|command| match command {
                    AudioCommand::Play { voice, .. } => Some(voice.0),
                    _ => None,
                }),
        )
        .max()
        .unwrap_or(0);
    let first = occupied
        .checked_add(1)
        .ok_or("local mine sound voice namespace exhausted")?;
    let mut allocator = VoiceAllocator::new(first);
    let mut timelines = Vec::new();
    timelines
        .try_reserve_exact(members.len())
        .map_err(|_| "local mine sound allocation failed")?;
    for member in members {
        let mut bindings = Vec::new();
        bindings
            .try_reserve_exact(plan.bindings().len())
            .map_err(|_| "local mine sound allocation failed")?;
        bindings.extend_from_slice(plan.bindings());
        allocator.remap_hazard_sounds(&mut bindings)?;
        let timeline =
            HazardSoundTimeline::new(bindings, capacity).map_err(|error| error.to_string())?;
        timelines.push((member.player, timeline));
    }
    Ok(timelines)
}
