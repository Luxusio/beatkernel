//! Host-neutral member construction over the existing chart, rules and voice allocator.
use crate::{
    PreparedBms,
    local_players::ResolvedInputPlan,
    local_runtime::{MemberConfig, VoiceAllocator},
};
use beatkernel::{
    audio::{AudioCommand, VoiceId},
    input::{BindingMap, DeviceSelector},
    judge::{JudgeEngine, JudgeProfile},
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
    if bindings.len() != plan.members().len() {
        return Err("local binding map count differs from source plan".into());
    }
    let required_lanes: BTreeSet<_> = prepared.source.notes.iter().map(|note| note.lane).collect();
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
    for (&(player, device), bindings) in plan.members().iter().zip(bindings) {
        let constructor = if input_mode == BmsInputMode::ButtonOrContact
            && !prepared.source.invisible.is_empty()
        {
            JudgeEngine::new_with_contacts
        } else {
            JudgeEngine::new
        };
        let judge = constructor(
            prepared.compiled.chart.clone(),
            prepared.source.rules_with_input_mode(input_mode),
            profile.clone(),
        )
        .map_err(|error| error.to_string())?;
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
