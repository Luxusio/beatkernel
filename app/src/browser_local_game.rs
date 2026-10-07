//! Worker-owned local gameplay: one shared output, independent actual members.
use std::{collections::VecDeque, sync::Arc};

use crate::{
    browser::BrowserPrepared,
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
    browser_game::{BrowserSample, OUTPUT, LOGICAL, encode_batch, encode_saved_opponents},
    browser_hid_input::BrowserHidSetup,
    browser_input::{
        BrowserInputQueue, LocalPhysicalInputSetup, LocalTouchPageRouting, TouchInputSetup, decode_input,
        project_touch_on_surface, project_touch_position_on_surface,
    },
    competition::OpponentKind,
    image_assets::ImageAssets,
    local_players::PlayerId,
    local_runtime::{InputResult, PlayerReport},
    note_progress::NoteProgress,
    player_chart::PlayerChart,
    pressed_keys::PressedKeys,
    room_opponent_hud::{RoomHudStatus, RoomOpponentHud},
    saved_opponent_hud::SavedOpponentHud,
    saved_opponents::SavedOpponents,
    step_gameplay::{
        StepGameplayConfig, StepGameplayError, StepLocalGameplay, StepLocalGameplayError,
    },
    worklet_audio::{OutputEvidence, decode_output, decode_section_output},
};
use beatkernel::{
    audio::{PcmSample, SampleId},
    input::{
        Binding, ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId,
        PhysicalInputEvent, Position2, codec::CodecLimits,
    },
    judge::JudgeEvent,
    replay::codec::ReplayCodecLimits,
    time::{ClockDomainId, ClockPair, ClockPoint, Duration, ExtrapolationPolicy, Timestamp},
};
use beatkernel_bms::BmsInputMode;
use wasm_bindgen::prelude::*;

const HOST: ClockDomainId = ClockDomainId(0x57494e);
fn point(domain: ClockDomainId, ns: i64) -> ClockPoint {
    ClockPoint {
        domain,
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn error(value: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&value.to_string()).into()
}

pub(crate) struct BrowserLocalMember {
    pub(crate) player: PlayerId,
    pub(crate) progress: NoteProgress,
    pub(crate) recent: Vec<JudgeEvent>,
    pub(crate) pressed: u32,
    pub(crate) saved_hud: SavedOpponentHud,
    pub(crate) peer_admitted: bool,
    opponents: Option<SavedOpponents>,
    opponent_error: Option<String>,
    source: Option<DeviceId>,
    pressed_owners: PressedKeys,
    touch_configured: bool,
}

impl BrowserLocalMember {
    /// Admission is setup-only. Failed presentation keeps this reserved space
    /// so a configured touch field never moves during gameplay.
    pub(crate) fn comparison_height(&self) -> i64 {
        self.saved_comparison_height() + if self.peer_admitted { 28 } else { 0 }
    }

    pub(crate) fn saved_comparison_height(&self) -> i64 {
        self.opponents
            .as_ref()
            .map_or(0, |opponents| opponents.count() as i64 * 14)
    }
}

/// Owns one original prepared bank and the actual shared local runtime. Input
/// source IDs come from the caller's admitted plan, never from member indices.
#[wasm_bindgen]
pub struct BrowserLocalGame {
    pub(crate) game: StepLocalGameplay,
    pub(crate) render_producer: crate::browser::render::RenderProducer,
    render_packet_budget: u32,
    render_diagnostic_budget: u32,
    pub(crate) chart: Arc<PlayerChart>,
    pub(crate) images: Arc<ImageAssets>,
    pub(crate) members: Vec<BrowserLocalMember>,
    pub(crate) room_hud: Option<RoomOpponentHud>,
    room_lobby: Option<Arc<crate::room_presentation::RoomLobby>>,
    room_render_status: crate::room_presentation::RoomStatus,
    pub(crate) room_hud_disabled: bool,
    input_limits: CodecLimits,
    input_bindings: Vec<Binding>,
    hid_setup: Option<BrowserHidSetup>,
    hid_events: Vec<PhysicalInputEvent>,
    samples: VecDeque<(SampleId, PcmSample)>,
    output_start: Option<u64>,
    output_context: Option<u64>,
    output_evidence: Option<(OutputEvidence, Option<ClockPoint>)>,
    input_queue: Option<BrowserInputQueue>,
    touch_page_routing: LocalTouchPageRouting,
    chart_seed: u64,
    opponent_source: Option<beatkernel_bms::BmsChart>,
    opponent_count: usize,
    opponent_bytes: usize,
}

#[wasm_bindgen]
impl BrowserLocalGame {
    /// Whole original completion archive; never promotes an exported replay prefix.
    pub fn completed_archive(&self) -> Result<Option<Vec<u8>>, JsValue> {
        let mut comparisons = Vec::new();
        comparisons
            .try_reserve_exact(self.members.len())
            .map_err(error)?;
        comparisons.extend(
            self.members
                .iter()
                .map(|member| (member.player, member.saved_hud.snapshot())),
        );
        self.game
            .completed_archive_with_comparisons(&comparisons)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    pub fn completed_results(
        &self,
    ) -> Result<Option<crate::browser_completed_results::BrowserCompletedResults>, JsValue> {
        let comparisons: Vec<_> = self
            .members
            .iter()
            .map(|member| (member.player, member.saved_hud.snapshot()))
            .collect();
        let mut presentation =
            crate::completed_results_presentation::CompletedResultsPresentation::default();
        let captured = presentation.capture_local(&self.game, &comparisons);
        if presentation.results().is_none() {
            captured.map_err(|error| JsValue::from_str(&error))?;
        }
        Ok(
            crate::browser_completed_results::BrowserCompletedResults::from_presentation(
                presentation,
            ),
        )
    }
    /// Consume live preparation once. Plan rows have four words; binding rows
    /// have a stable player ID followed by the seven physical identity words.
    pub fn new_physical(
        prepared: BrowserPrepared,
        host_origin_ns: i64,
        preroll_ns: i64,
        early_ns: i64,
        late_ns: i64,
        offset_ns: i64,
        plan_words: Vec<u32>,
        binding_words: Vec<u32>,
        end_ns: Option<i64>,
        contact: bool,
        max_encoded_input: u32,
        max_payload_input: u32,
    ) -> Result<Self, JsValue> {
        if prepared.replay.is_some() || host_origin_ns < 0 {
            return Err(error(
                "live resources and a nonnegative browser host origin are required",
            ));
        }
        let input = LocalPhysicalInputSetup::new(
            &plan_words,
            &binding_words,
            &prepared.chart.lanes,
            max_encoded_input,
            max_payload_input,
        )
        .map_err(error)?;
        let mut input_bindings = Vec::new();
        input_bindings
            .try_reserve_exact(binding_words.len() / 8)
            .map_err(|_| error("local browser binding snapshot allocation failed"))?;
        for bindings in &input.bindings {
            input_bindings.extend_from_slice(bindings.bindings());
        }
        let chart = Arc::new(prepared.chart);
        let touch_page_routing = LocalTouchPageRouting::new(&input.plan, &chart.lanes)
            .map_err(error)?;
        let mut members = Vec::new();
        members
            .try_reserve_exact(input.plan.members().len())
            .map_err(|_| error("local browser member allocation failed"))?;
        for &(player, source) in input.plan.members() {
            let mut recent = Vec::new();
            recent
                .try_reserve_exact(128)
                .map_err(|_| error("local recent judgment allocation failed"))?;
            members.push(BrowserLocalMember {
                player,
                source,
                progress: NoteProgress::new(chart.clone()).map_err(error)?,
                recent,
                pressed: 0,
                saved_hud: SavedOpponentHud::default(),
                peer_admitted: false,
                opponents: None,
                opponent_error: None,
                pressed_owners: PressedKeys::default(),
                touch_configured: false,
            });
        }
        let config = StepGameplayConfig {
            host_origin: point(HOST, host_origin_ns),
            output_origin: point(OUTPUT, 0),
            preroll: Duration::from_nanos(preroll_ns),
            early_ns,
            late_ns,
            offset_ns,
            command_capacity: 4096,
            bgm_pending: 3072,
            bgm_lookahead: Duration::from_nanos(500_000_000),
            telemetry_capacity: 256,
        };
        let mode = if contact {
            BmsInputMode::ButtonOrContact
        } else {
            BmsInputMode::ButtonOnly
        };
        let opponent_source = prepared.prepared.source.clone();
        let authority = AudioAuthority::new(
            AudioAuthorityConfig {
                // Permission covers the maximum existing latency hint plus startup;
                // this never assigns an observation accuracy or forward prediction.
                input_extrapolation: ExtrapolationPolicy::Bounded {
                    before: Duration::from_nanos(70_000_000_000),
                    after: Duration::ZERO,
                },
                ..AudioAuthorityConfig::default()
            },
            AudioAuthorityEpoch {
                id: 1,
                stream_origin: point(OUTPUT, 0),
                logical_origin: point(LOGICAL, 0),
                host_domain: HOST,
            },
        )
        .map_err(error)?;
        let (game, bank) = StepLocalGameplay::new_audio_section(
            prepared.prepared,
            config,
            input.plan,
            input.bindings,
            prepared.start,
            end_ns.map(Timestamp::from_nanos),
            mode,
            authority,
        )
        .map_err(error)?;
        Ok(Self {
            game,
            chart,
            render_producer: crate::browser::render::RenderProducer::default(),
            render_packet_budget: 0,
            render_diagnostic_budget: 0,
            images: prepared.images,
            members,
            room_hud: None,
            room_lobby: None,
            room_render_status: crate::room_presentation::RoomStatus::Waiting,
            room_hud_disabled: false,
            input_limits: input.limits,
            input_bindings,
            hid_setup: None,
            hid_events: Vec::new(),
            samples: bank.into_samples().collect(),
            output_start: None,
            output_context: None,
            output_evidence: None,
            input_queue: None,
            touch_page_routing,
            chart_seed: prepared.chart_seed,
            opponent_source: Some(opponent_source),
            opponent_count: 0,
            opponent_bytes: 0,
        })
    }

    #[wasm_bindgen(getter)]
    pub fn players(&self) -> Vec<u32> {
        self.game.players().iter().map(|player| player.0).collect()
    }
    pub fn hits(&self, player: u32) -> Result<u64, JsValue> {
        Ok(self.score(player)?.hits)
    }
    pub fn misses(&self, player: u32) -> Result<u64, JsValue> {
        Ok(self.score(player)?.misses)
    }
    pub fn combo(&self, player: u32) -> Result<u64, JsValue> {
        Ok(self.score(player)?.combo)
    }
    pub fn max_combo(&self, player: u32) -> Result<u64, JsValue> {
        Ok(self.score(player)?.max_combo)
    }
    pub fn member_song_ns(&self, player: u32) -> Result<i64, JsValue> {
        self.game
            .member_song_time(PlayerId(player))
            .map(Timestamp::as_nanos)
            .ok_or_else(|| error("unknown local player"))
    }
    /// Exact ordered member progress for a bounded control-side network snapshot.
    /// PlayerId precedes five low/high word pairs; no host clock is sampled.
    pub fn progress_words(&self) -> Result<Vec<u32>, JsValue> {
        let rows = self.game.group_progress().map_err(error)?;
        crate::multiplayer_group::encode_words(&rows).map_err(error)
    }
    pub fn pressed(&self, player: u32) -> Result<u32, JsValue> {
        self.members
            .iter()
            .find(|member| member.player == PlayerId(player))
            .map(|member| member.pressed)
            .ok_or_else(|| error("unknown local player"))
    }
    #[wasm_bindgen(getter)]
    pub fn song_ns(&self) -> i64 {
        self.game.song_time().as_nanos()
    }
    #[wasm_bindgen(getter)]
    pub fn end_ns(&self) -> Option<i64> {
        self.game.end_ns()
    }
    #[wasm_bindgen(getter)]
    pub fn playback_end_frame(&self) -> Option<u64> {
        self.game.playback_end_frame()
    }
    #[wasm_bindgen(getter)]
    pub fn failed(&self) -> bool {
        self.game.failed()
    }
    pub fn input_setup_available(&self) -> bool {
        self.game.input_setup_available()
    }
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }
    pub fn next_sample(&mut self) -> Option<BrowserSample> {
        self.samples
            .pop_front()
            .map(|(id, sample)| BrowserSample::from_pcm(id, sample))
    }

    pub fn configure_capture(
        &mut self,
        player: u32,
        max_bytes: u32,
        max_records: u32,
    ) -> Result<(), JsValue> {
        let limits = ReplayCodecLimits::new(
            max_bytes as usize,
            max_records as usize,
            4096,
            CodecLimits::new(65_536, 32_768).map_err(error)?,
        )
        .map_err(error)?;
        self.game
            .configure_capture(PlayerId(player), limits, self.chart_seed)
            .map_err(error)
    }
    pub fn take_replay(&mut self, player: u32) -> Result<Option<Vec<u8>>, JsValue> {
        self.game.take_replay(PlayerId(player)).map_err(error)
    }
    pub fn competition_identity(&self, player: u32) -> Result<Vec<u8>, JsValue> {
        let limits = crate::competition_live::replay_limits().map_err(error)?;
        self.game
            .competition_identity(PlayerId(player), limits, self.chart_seed)
            .map_err(error)
    }
    /// Adds one genuine recorded prefix for one member before activation.
    /// The eight-record and 64 MiB quotas belong to the entire local owner.
    pub fn add_saved_opponent(
        &mut self,
        player: u32,
        encoded: Vec<u8>,
        own: bool,
        label: String,
    ) -> Result<usize, JsValue> {
        let index = self
            .members
            .iter()
            .position(|member| member.player == PlayerId(player))
            .ok_or_else(|| error("unknown local player"))?;
        if self.members[index].touch_configured {
            return Err(error(
                "saved opponents must be admitted before touch configuration",
            ));
        }
        let source = self
            .opponent_source
            .as_ref()
            .ok_or_else(|| error("saved opponents must be admitted before activation"))?;
        if self.opponent_count >= 8 {
            return Err(error("local saved opponent capacity reached"));
        }
        let charged = self
            .opponent_bytes
            .checked_add(encoded.len())
            .filter(|bytes| *bytes <= 64 * 1024 * 1024)
            .ok_or_else(|| error("local saved opponent encoded-byte quota exceeded"))?;
        if self.members[index].saved_hud.failed() {
            return Err(error("saved opponent presentation is disabled"));
        }
        let limits = crate::competition_live::replay_limits().map_err(error)?;
        let header = self
            .game
            .competition_header(PlayerId(player), limits, self.chart_seed)
            .map_err(error)?;
        let kind = if own {
            OpponentKind::Own
        } else {
            OpponentKind::Other
        };
        let member = &mut self.members[index];
        let admitted = if let Some(opponents) = &mut member.opponents {
            opponents
                .add(source, &encoded, kind, &label)
                .map_err(error)?
        } else {
            let mut opponents =
                SavedOpponents::new(header, limits, 8, 64 * 1024 * 1024).map_err(error)?;
            let admitted = opponents
                .add(source, &encoded, kind, &label)
                .map_err(error)?;
            member.opponents = Some(opponents);
            admitted
        };
        self.opponent_count += 1;
        self.opponent_bytes = charged;
        Ok(admitted)
    }

    /// Refresh each member at its own committed song frontier. A comparison
    /// failure only disables that member's retained display, never gameplay.
    pub fn saved_opponents(&mut self) -> Result<JsValue, JsValue> {
        let array = js_sys::Array::new();
        for member in &mut self.members {
            let result = (|| -> Result<JsValue, String> {
                if member.saved_hud.failed() {
                    return Err(member
                        .opponent_error
                        .clone()
                        .unwrap_or_else(|| "saved opponent presentation is disabled".into()));
                }
                let Some(opponents) = &mut member.opponents else {
                    return Ok(js_sys::Array::new().into());
                };
                let song = self
                    .game
                    .member_song_time(member.player)
                    .ok_or("local member has no song frontier")?;
                opponents
                    .advance_to(song)
                    .map_err(|failure| failure.to_string())?;
                member.saved_hud.update(opponents)?;
                encode_saved_opponents(opponents)
                    .map_err(|_| "saved opponent snapshot encoding failed".to_string())
            })();
            let (opponents, failure) = match result {
                Ok(opponents) => (opponents, JsValue::NULL),
                Err(failure) => {
                    member.saved_hud.mark_failed();
                    let retained: String = failure.chars().take(2048).collect();
                    member.opponent_error = Some(retained.clone());
                    (JsValue::NULL, JsValue::from_str(&retained))
                }
            };
            let row = js_sys::Object::new();
            for (name, value) in [
                ("player", JsValue::from_f64(f64::from(member.player.0))),
                ("opponents", opponents),
                ("error", failure),
            ] {
                js_sys::Reflect::set(&row, &JsValue::from_str(name), &value)?;
            }
            array.push(&row);
        }
        Ok(array.into())
    }

    pub fn disable_saved_opponent_hud(&mut self, player: u32) -> Result<(), JsValue> {
        let member = self
            .members
            .iter_mut()
            .find(|member| member.player == PlayerId(player))
            .ok_or_else(|| error("unknown local player"))?;
        member.saved_hud.mark_failed();
        member
            .opponent_error
            .get_or_insert_with(|| "saved opponent presentation is disabled".into());
        Ok(())
    }

    /// Reserve one peer display before the member's touch geometry is fixed.
    /// Admission does not create a network connection or advance gameplay.
    pub fn configure_peer_hud(&mut self, player: u32) -> Result<(), JsValue> {
        let member = self
            .members
            .iter_mut()
            .find(|member| member.player == PlayerId(player))
            .ok_or_else(|| error("unknown local player"))?;
        if !self.game.input_setup_available() || self.opponent_source.is_none() {
            return Err(error(
                "peer display must be admitted before activation or gameplay",
            ));
        }
        if member.touch_configured {
            return Err(error(
                "peer display must be admitted before touch configuration",
            ));
        }
        if member.peer_admitted || member.saved_hud.peer_failed() {
            return Err(error("peer display is already admitted or disabled"));
        }
        member.saved_hud.update_peer(0, &[]).map_err(error)?;
        member.peer_admitted = true;
        Ok(())
    }

    /// Update only the named member using the common peer-prefix validation.
    pub fn update_peer_hud(
        &mut self,
        player: u32,
        status: u32,
        words: Vec<u32>,
    ) -> Result<(), JsValue> {
        let member = self
            .members
            .iter_mut()
            .find(|member| member.player == PlayerId(player))
            .ok_or_else(|| error("unknown local player"))?;
        if !member.peer_admitted {
            return Err(error("local player's peer display has not been admitted"));
        }
        member.saved_hud.update_peer(status, &words).map_err(error)
    }

    /// Hide only this member's peer prefix while retaining its reserved space.
    pub fn disable_peer_hud(&mut self, player: u32) -> Result<(), JsValue> {
        let member = self
            .members
            .iter_mut()
            .find(|member| member.player == PlayerId(player))
            .ok_or_else(|| error("unknown local player"))?;
        if !member.peer_admitted {
            return Err(error("local player's peer display has not been admitted"));
        }
        member.saved_hud.mark_peer_failed();
        Ok(())
    }

    /// Freeze actual Prepared host/player metadata without changing any field
    /// or touch geometry. Rows are [host low, host high, count, players...].
    pub fn configure_room_hud(&mut self, own: u64, words: Vec<u32>) -> Result<(), JsValue> {
        use crate::multiplayer_rooms::ParticipantId;
        if self.room_hud.is_some()
            || self.room_hud_disabled
            || !self.game.input_setup_available()
            || words.is_empty()
            || words.len() > 4288
        {
            return Err(error(
                "room HUD requires pristine bounded Prepared membership",
            ));
        }
        let members = crate::room_results_builder::decode_room_roster(&words).map_err(error)?;
        let own_member = members
            .iter()
            .find(|member| member.id == ParticipantId(own))
            .ok_or_else(|| error("room HUD is missing this participant"))?;
        if own_member.players.as_slice() != self.game.players() {
            return Err(error("room HUD changed the actual local roster"));
        }
        let hud = RoomOpponentHud::new(ParticipantId(own), &members).map_err(error)?;
        let lobby = crate::room_presentation::RoomLobby::new(Some(ParticipantId(own)), 1,
            Some(crate::multiplayer_group_rooms::GroupRoomPhase::Prepared), None, members).map_err(error)?;
        self.room_lobby = Some(Arc::new(lobby));
        self.room_hud = Some(hud);
        Ok(())
    }

    pub fn update_room_hud(
        &mut self,
        participant: u64,
        sequence: u64,
        final_prefix: bool,
        words: Vec<u32>,
    ) -> Result<(), JsValue> {
        let members = crate::multiplayer_group::decode_words(&words).map_err(error)?;
        let prefix = crate::multiplayer_group::GroupPrefix {
            sequence,
            final_prefix,
            members,
        };
        self.room_hud
            .as_mut()
            .ok_or_else(|| error("room HUD is not configured"))?
            .update(
                crate::multiplayer_rooms::ParticipantId(participant),
                &prefix,
            )
            .map_err(error)
    }

    pub fn set_room_hud_status(&mut self, status: u32) -> Result<(), JsValue> {
        let status = match status {
            0 => RoomHudStatus::Waiting,
            1 => RoomHudStatus::Connected,
            2 => RoomHudStatus::Disconnected,
            _ => return Err(error("invalid room HUD status")),
        };
        self.room_hud
            .as_mut()
            .ok_or_else(|| error("room HUD is not configured"))?
            .set_status(status)
            .map_err(error)?;
        self.room_render_status = match status {
            RoomHudStatus::Waiting => crate::room_presentation::RoomStatus::Waiting,
            RoomHudStatus::Connected => crate::room_presentation::RoomStatus::Connected,
            RoomHudStatus::Disconnected => crate::room_presentation::RoomStatus::Disconnected,
        };
        Ok(())
    }
    pub fn set_room_hud_page(&mut self, page: u32) -> Result<(), JsValue> {
        self.room_hud
            .as_mut()
            .ok_or_else(|| error("room HUD is not configured"))?
            .set_page(page as usize)
            .map_err(error)
    }
    pub fn room_hud_pages(&self) -> u32 {
        self.room_hud
            .as_ref()
            .map_or(0, |hud| hud.page_count() as u32)
    }
    pub fn disable_room_hud(&mut self) {
        self.room_hud_disabled = true;
        if let Some(hud) = &mut self.room_hud {
            hud.mark_failed();
        }
    }

    pub fn configure_touch_regions(
        &mut self,
        player: u32,
        words: Vec<u32>,
        bounds: Vec<f32>,
        max_contacts: u32,
    ) -> Result<(), JsValue> {
        let index = self
            .members
            .iter()
            .position(|member| member.player == PlayerId(player))
            .ok_or_else(|| error("unknown local player"))?;
        let member = &self.members[index];
        if let Some(source) = member.source {
            if words.chunks_exact(7).any(|row| {
                row[1] != 1 || (u64::from(row[2]) | (u64::from(row[3]) << 32)) != source.0
            }) {
                return Err(error(
                    "assigned player's touch regions must select its exact source",
                ));
            }
        }
        let setup = TouchInputSetup::new(&words, &bounds, &self.chart.lanes, max_contacts)
            .map_err(error)?;
        let mut regions = Vec::new();
        regions
            .try_reserve_exact(setup.router.regions().len())
            .map_err(|_| error("local touch region snapshot allocation failed"))?;
        regions.extend_from_slice(setup.router.regions());
        self.game
            .configure_touch_router(PlayerId(player), setup.router)
            .map_err(error)?;
        self.touch_page_routing.configure(
            PlayerId(player), regions, self.members[index].comparison_height(),
        ).map_err(error)?;
        self.members[index].touch_configured = true;
        Ok(())
    }

    /// Move only the configured router's bounds for a visible page. Hidden
    /// members admit new contacts as unbound while retaining every held owner.
    pub fn set_touch_page(&mut self, player: u32, page: u32) -> Result<bool, JsValue> {
        self.touch_page_routing.set_page(&mut self.game, PlayerId(player), page)
            .map_err(error)
    }

    /// Prepared lane regions for this member's actual visible field. Coordinates
    /// remain global scene coordinates, separately from original pointer payloads.
    pub fn touch_bounds(&self, player: u32, page: u32) -> Result<Vec<f32>, JsValue> {
        let count = self.members.len();
        let page_size = crate::ui::organisms::LOCAL_PLAYERS_PER_PAGE;
        let page = page as usize;
        if page >= count.div_ceil(page_size) {
            return Err(error("invalid local touch page"));
        }
        let index = self
            .members
            .iter()
            .position(|member| member.player == PlayerId(player))
            .ok_or_else(|| error("unknown local touch player"))?;
        let first = page * page_size;
        let visible = (count - first).min(page_size);
        if index < first || index >= first + visible {
            return Err(error("touch player is not on the visible local page"));
        }
        crate::playfield_layout::local_touch_bounds_with_comparison_space(
            &self.chart.lanes,
            visible,
            index - first,
            self.members[index].comparison_height(),
        )
        .map_err(error)
    }

    #[wasm_bindgen(getter)]
    pub fn touch_width(&self) -> u32 {
        crate::playfield_layout::LOGICAL_EXTENT[0]
    }
    #[wasm_bindgen(getter)]
    pub fn touch_height(&self) -> u32 {
        crate::playfield_layout::LOGICAL_EXTENT[1]
    }
    pub fn configure_hid_devices(
        &mut self,
        device_words: Vec<u32>,
        field_words: Vec<u32>,
        axis_params: Vec<f32>,
    ) -> Result<(), JsValue> {
        if !self.game.input_setup_available() || self.hid_setup.is_some() {
            return Err(error(
                "HID configuration requires a pristine, unconfigured gameplay owner",
            ));
        }
        let setup = BrowserHidSetup::new(
            &device_words,
            &field_words,
            &axis_params,
            &self.input_bindings,
        )
        .map_err(error)?;
        let mut events = Vec::new();
        events
            .try_reserve_exact(256)
            .map_err(|_| error("HID event scratch allocation failed"))?;
        self.hid_setup = Some(setup);
        self.hid_events = events;
        Ok(())
    }
    pub fn activate(&mut self, host_ns: i64) -> Result<(), JsValue> {
        if host_ns < 0 || self.input_queue.is_some() {
            return Err(error("activation requires a fresh nonnegative HOST origin"));
        }
        let queue = if self.members.len() == 1 && self.members[0].source.is_none() {
            BrowserInputQueue::solo(point(HOST, host_ns))
        } else {
            let sources = self
                .members
                .iter()
                .map(|member| member.source.ok_or("configured local member has no source"))
                .collect::<Result<Vec<_>, _>>()
                .map_err(error)?;
            BrowserInputQueue::local(point(HOST, host_ns), sources)
        }
        .map_err(error)?;
        self.game.activate_audio().map_err(error)?;
        self.input_queue = Some(queue);
        self.opponent_source = None;
        Ok(())
    }
    pub fn input_blob(&mut self, _bytes: Vec<u8>, _audio_ns: i64) -> Result<(), JsValue> {
        Err(error(
            "live playback requires queued input and audio service",
        ))
    }
    pub fn input_blob_at(
        &mut self,
        _bytes: Vec<u8>,
        _x: f32,
        _y: f32,
        _audio_ns: i64,
    ) -> Result<(), JsValue> {
        Err(error(
            "live playback requires queued input and audio service",
        ))
    }
    /// Read-only scalar preflight shares the actual input projection below.
    pub fn preflight_touch_surface(
        &self,
        x: f32,
        y: f32,
        css_width: f64,
        css_height: f64,
        surface_width: u32,
        surface_height: u32,
    ) -> Result<(), JsValue> {
        project_touch_position_on_surface(
            Position2 { x, y },
            [css_width, css_height],
            [surface_width, surface_height],
            crate::playfield_layout::LOGICAL_EXTENT,
        )
        .map(|_| ())
        .map_err(error)
    }
    pub fn input_blob_on_surface(
        &mut self,
        _bytes: Vec<u8>,
        _css_width: f64,
        _css_height: f64,
        _surface_width: u32,
        _surface_height: u32,
        _audio_ns: i64,
    ) -> Result<(), JsValue> {
        Err(error(
            "live playback requires queued input and audio service",
        ))
    }
    /// Decode an entire genuine report before dispatch. Every typed event keeps
    /// the same original source/time/sequence, including its committed prefix on
    /// failure. Empty fanout submits the original raw report, never a fake key.
    pub fn input_hid_blob(&mut self, _bytes: Vec<u8>, _audio_ns: i64) -> Result<(), JsValue> {
        Err(error(
            "live playback requires queued input and audio service",
        ))
    }
    pub fn advance(&mut self, _host_ns: i64, _audio_ns: i64) -> Result<(), JsValue> {
        Err(error(
            "live playback requires queued input and audio service",
        ))
    }
    pub fn observe_presentation(&mut self, output_ns: i64, host_ns: i64) -> Result<(), JsValue> {
        if output_ns < 0 || host_ns < 0 {
            self.game.fail();
            return Err(error("browser presentation points must be nonnegative"));
        }
        self.game
            .observe_audio_output(
                1,
                ClockPair {
                    source: point(OUTPUT, output_ns),
                    target: point(HOST, host_ns),
                },
            )
            .map(|_| ())
            .map_err(error)
    }
    pub fn feed_audio(&mut self, rendered_frames: u64, budget: u32) -> Result<(), JsValue> {
        self.game
            .feed_audio(rendered_frames, budget as usize)
            .map(|_| ())
            .map_err(error)
    }
    /// Original Worklet evidence is validated before it can admit more BGM.
    /// One shared completion barrier still waits for every actual member.
    pub fn observe_output(
        &mut self,
        _words: Vec<u32>,
        _presented_ns: Option<i64>,
    ) -> Result<bool, JsValue> {
        Err(error(
            "live playback requires queued input and audio service",
        ))
    }
    /// Admit actual output and scheduling credit before servicing the acquired prefix.
    pub fn admit_output(
        &mut self,
        words: Vec<u32>,
        presented_ns: Option<i64>,
    ) -> Result<(), JsValue> {
        if self.game.failed() {
            return Err(error(StepGameplayError::Failed));
        }
        let decoded = match self.game.playback_end_frame() {
            Some(end) => decode_section_output(&words, Some(end)),
            None => decode_output(&words),
        };
        let evidence = match decoded {
            Ok(evidence) => evidence,
            Err(reason) => {
                self.game.fail();
                return Err(error(reason));
            }
        };
        if self
            .output_start
            .is_some_and(|start| start != evidence.start)
            || self
                .output_context
                .is_some_and(|previous| evidence.context.is_none_or(|current| current < previous))
        {
            self.game.fail();
            return Err(error(
                "browser output start changed or context cursor regressed",
            ));
        }
        let presented = presented_ns.map(|ns| point(OUTPUT, ns));
        self.game
            .admit_output_evidence(evidence.report, presented)
            .map_err(error)?;
        if let Some(report) = evidence.report {
            self.game
                .feed_audio(report.counters.rendered_frames, 256)
                .map_err(error)?;
        }
        self.output_start = Some(evidence.start);
        self.output_context = evidence.context;
        self.output_evidence = Some((evidence, presented));
        Ok(())
    }
    pub fn evaluate_completion(&mut self) -> Result<bool, JsValue> {
        let Some(queue) = &self.input_queue else {
            return Err(error("input queue is not activated"));
        };
        let authority = self
            .game
            .audio_authority()
            .expect("browser owns audio authority");
        if queue.pending() != 0
            || authority.closed_host_prefix().is_none()
            || authority.closed_host_prefix() != authority.acquired_prefix()
        {
            return Ok(false);
        }
        let Some((evidence, presented)) = &self.output_evidence else {
            return Ok(false);
        };
        self.game
            .observe_completion(evidence.report, *presented)
            .map_err(error)
    }
    pub fn queue_hid_blob(&mut self, bytes: Vec<u8>, received_host_ns: i64) -> Result<(), JsValue> {
        let result = (|| {
            let input = decode_input(&bytes, self.input_limits, HOST).map_err(error)?;
            let PhysicalInputEvent::RawHidReport(report) = input else {
                return Err(error("HID profile input requires a genuine raw HID report"));
            };
            if self.game.failed() {
                return Err(error(StepGameplayError::Failed));
            }
            if self.hid_setup.is_none() {
                return Err(error("HID profile input requires configured device owners"));
            }
            let mut events = std::mem::take(&mut self.hid_events);
            events.clear();
            let result = (|| {
                self.hid_setup
                    .as_mut()
                    .expect("HID setup checked")
                    .decode_report(&report, &mut events)
                    .map_err(error)?;
                if events.is_empty() {
                    if let Err(failure) = self.queue_physical(
                        PhysicalInputEvent::RawHidReport(report),
                        received_host_ns,
                        None,
                    ) {
                        self.game.fail();
                        return Err(failure);
                    }
                } else {
                    for input in events.drain(..) {
                        if let Err(failure) = self.queue_physical(input, received_host_ns, None) {
                            self.game.fail();
                            return Err(failure);
                        }
                    }
                }
                Ok(())
            })();
            events.clear();
            self.hid_events = events;
            result
        })();
        self.queue_result(result)
    }
    pub fn queue_input(
        &mut self,
        host_ns: i64,
        key: u16,
        down: bool,
        sequence: u64,
        received_host_ns: i64,
    ) -> Result<(), JsValue> {
        let result = (|| {
            if !self
                .input_bindings
                .iter()
                .any(|binding| binding.physical == PhysicalControlId::keyboard(key))
            {
                return Err(error("browser input key is not bound"));
            }
            self.queue_physical(
                PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(1), point(HOST, host_ns), sequence),
                    control: PhysicalControlId::keyboard(key),
                    state: if down {
                        ButtonState::Down
                    } else {
                        ButtonState::Up
                    },
                }),
                received_host_ns,
                None,
            )
        })();
        self.queue_result(result)
    }
    pub fn queue_input_blob(
        &mut self,
        bytes: Vec<u8>,
        received_host_ns: i64,
    ) -> Result<(), JsValue> {
        let result = (|| {
            let input = decode_input(&bytes, self.input_limits, HOST).map_err(error)?;
            self.queue_physical(input, received_host_ns, None)
        })();
        self.queue_result(result)
    }
    pub fn queue_input_blob_at(
        &mut self,
        bytes: Vec<u8>,
        x: f32,
        y: f32,
        received_host_ns: i64,
    ) -> Result<(), JsValue> {
        let result = (|| {
            let input = decode_input(&bytes, self.input_limits, HOST).map_err(error)?;
            if !matches!(&input, PhysicalInputEvent::Touch(_)) || !x.is_finite() || !y.is_finite() {
                return Err(error(
                    "projected input requires a genuine touch and finite position",
                ));
            }
            self.queue_physical(input, received_host_ns, Some(Position2 { x, y }))
        })();
        self.queue_result(result)
    }
    pub fn queue_input_blob_on_surface(
        &mut self,
        bytes: Vec<u8>,
        css_width: f64,
        css_height: f64,
        surface_width: u32,
        surface_height: u32,
        received_host_ns: i64,
    ) -> Result<(), JsValue> {
        let result = (|| {
            let input = decode_input(&bytes, self.input_limits, HOST).map_err(error)?;
            let position = project_touch_on_surface(
                &input,
                [css_width, css_height],
                [surface_width, surface_height],
                crate::playfield_layout::LOGICAL_EXTENT,
            )
            .map_err(error)?;
            self.queue_physical(input, received_host_ns, Some(position))
        })();
        self.queue_result(result)
    }
    /// Admit the original acquisition surface and page on the same ordered input.
    pub fn queue_input_blob_on_surface_on_page(
        &mut self,
        bytes: Vec<u8>,
        css_width: f64,
        css_height: f64,
        surface_width: u32,
        surface_height: u32,
        page: u32,
        received_host_ns: i64,
    ) -> Result<(), JsValue> {
        let result = (|| {
            let input = decode_input(&bytes, self.input_limits, HOST).map_err(error)?;
            // Page and event checks precede even dynamic source registration.
            self.touch_page_routing.validate_page(page).map_err(error)?;
            let position = project_touch_on_surface(
                &input,
                [css_width, css_height],
                [surface_width, surface_height],
                crate::playfield_layout::LOGICAL_EXTENT,
            ).map_err(error)?;
            if self.game.failed() || received_host_ns < 0 {
                return Err(error("input queue requires usable nonnegative HOST acquisition"));
            }
            let queue = self.input_queue.as_mut()
                .ok_or_else(|| error("input queue is not activated"))?;
            queue.register_source(input.meta().source).map_err(error)?;
            queue.admit_on_page(
                input, point(HOST, received_host_ns), Some(position), page, &self.touch_page_routing,
            ).map_err(error)
        })();
        self.queue_result(result)
    }

    pub fn close_input_prefix(&mut self, host_ns: i64) -> Result<(), JsValue> {
        if self.input_queue.is_none() || host_ns < 0 {
            return Err(error("input prefix requires activated HOST input"));
        }
        self.game
            .record_audio_prefix(point(HOST, host_ns))
            .map_err(error)
    }
    pub fn pending_inputs(&self) -> u32 {
        self.input_queue
            .as_ref()
            .map_or(0, |queue| queue.pending() as u32)
    }
    pub fn service_audio(&mut self, now_host_ns: i64, audio_ns: i64) -> Result<u32, JsValue> {
        if now_host_ns < 0 || audio_ns < 0 {
            return Err(error(
                "audio service requires nonnegative HOST and raw scheduling points",
            ));
        }
        let now = point(HOST, now_host_ns);
        let audio_at = point(OUTPUT, audio_ns);
        let mut count = 0;
        loop {
            let result = self
                .input_queue
                .as_mut()
                .ok_or_else(|| error("input queue is not activated"))?
                .process_next_local_on_page(&mut self.game, &mut self.touch_page_routing, now, audio_at);
            match result {
                Ok(Some(report)) => {
                    self.accept_input(Ok(report))?;
                    count += 1;
                }
                Ok(None) => break,
                Err(failure) => {
                    return self.accept_input(Err(failure)).map(|_| count);
                }
            }
        }
        let result = self
            .input_queue
            .as_mut()
            .expect("activated queue was checked")
            .advance_local(&mut self.game, now, audio_at);
        match result {
            Ok(Some(reports)) => self.accept_reports(Ok(reports))?,
            Ok(None) => {}
            Err(failure) => return self.accept_reports(Err(failure)).map(|_| count),
        }
        Ok(count)
    }
    pub fn commands(&mut self, max: u32) -> Result<JsValue, JsValue> {
        let Some(batch) = self.game.take_commands(max as usize).map_err(error)? else {
            return Ok(JsValue::NULL);
        };
        let result = encode_batch(batch);
        if result.is_err() {
            self.game.fail();
        }
        result
    }
    pub fn acknowledge(
        &mut self,
        sequence: u64,
        admitted: u32,
        success: bool,
    ) -> Result<(), JsValue> {
        self.game
            .acknowledge(sequence, admitted as usize, success)
            .map_err(error)
    }
    pub fn stop(&mut self) {
        self.input_queue = None;
        self.output_evidence = None;
        self.game.fail();
        self.opponent_source = None;
        self.samples.clear();
        self.hid_setup = None;
        self.hid_events.clear();
        for member in &mut self.members {
            member.pressed_owners.clear();
            member.pressed = 0;
        }
    }
}

impl BrowserLocalGame {
    fn queue_physical(
        &mut self,
        input: PhysicalInputEvent,
        received_host_ns: i64,
        position: Option<Position2>,
    ) -> Result<(), JsValue> {
        if self.game.failed() || received_host_ns < 0 {
            return Err(error(
                "input queue requires usable nonnegative HOST acquisition",
            ));
        }
        let queue = self
            .input_queue
            .as_mut()
            .ok_or_else(|| error("input queue is not activated"))?;
        queue.register_source(input.meta().source).map_err(error)?;
        queue
            .admit(input, point(HOST, received_host_ns), position)
            .map_err(error)
    }
    fn queue_result(&mut self, result: Result<(), JsValue>) -> Result<(), JsValue> {
        if result.is_err() {
            self.game.fail();
        }
        result
    }

    fn score(&self, player: u32) -> Result<&crate::competition::ScoreSummary, JsValue> {
        self.game
            .score(PlayerId(player))
            .ok_or_else(|| error("unknown local player"))
    }

    fn accept_input(
        &mut self,
        result: Result<InputResult, StepLocalGameplayError>,
    ) -> Result<(), JsValue> {
        match result {
            Ok(InputResult::Ignored { .. }) => Ok(()),
            Ok(InputResult::Processed(reports)) => self.accept_reports(Ok(reports)),
            Err(failure) => self.accept_reports(Err(failure)),
        }
    }
    fn accept_reports(
        &mut self,
        result: Result<Vec<PlayerReport>, StepLocalGameplayError>,
    ) -> Result<(), JsValue> {
        match result {
            Ok(reports) => self.observe_reports(&reports).map_err(error),
            Err(failure) => {
                if let StepLocalGameplayError::Operation { reports, .. } = &failure {
                    // Observe every already committed member, retaining the
                    // original operation failure even if feedback also fails.
                    let _ = self.observe_reports(reports);
                }
                Err(error(failure))
            }
        }
    }
    fn observe_reports(&mut self, reports: &[PlayerReport]) -> Result<(), String> {
        let mut failure = None;
        for PlayerReport { player, report } in reports {
            let member = self
                .members
                .iter_mut()
                .find(|member| member.player == *player)
                .expect("actual local reports reference prepared members");
            member.progress.apply(&report.judge_events);
            for event in &report.judge_events {
                if member.recent.len() == 128 {
                    member.recent.remove(0);
                }
                member.recent.push(*event);
            }
            if report.song_end_reached
                || self
                    .game
                    .gauge(*player)
                    .expect("actual local reports reference prepared members")
                    .snapshot()
                    .failure
                    .is_some()
            {
                member.pressed_owners.clear();
                member.pressed = 0;
            } else if let Err(reason) = member.pressed_owners.apply(&report.bound_inputs) {
                if failure.is_none() {
                    failure = Some(reason);
                }
            } else {
                member.pressed = member.pressed_owners.mask();
            }
        }
        match failure {
            Some(reason) => {
                self.game.fail();
                Err(reason)
            }
            None => Ok(()),
        }
    }
}

#[cfg(test)]
#[path = "gauge_pressed_browser_local_fixtures.rs"]
mod gauge_pressed_fixtures;

#[wasm_bindgen]
impl BrowserLocalGame {
    pub fn visual_registration(&mut self, generation: u64, content: u64, max_packet_bytes: u32, max_diagnostic_bytes: u32) -> Result<Vec<u8>, JsValue> {
        let roster = self.members.iter().map(|member| member.player).collect::<Vec<_>>();
        let bytes = crate::browser::render::registration(&self.chart, &self.images, &roster, generation, content, max_packet_bytes, max_diagnostic_bytes).map_err(error)?;
        self.render_producer.register(generation, content, &self.chart, &roster).map_err(error)?;
        self.render_packet_budget = max_packet_bytes;
        self.render_diagnostic_budget = max_diagnostic_bytes;
        Ok(bytes)
    }
    pub fn acknowledge_visual(&mut self, generation: u64, content: u64, sequence: u64) -> bool {
        self.render_producer.acknowledge(generation, content, sequence)
    }
}
#[wasm_bindgen]
impl BrowserLocalGame {
    pub fn visual_frame(&mut self, sequence: u64, page: u32) -> Result<Vec<u8>, JsValue> {
        use crate::browser_render_state::{RenderMemberScalars, RenderScore, RenderGauge, RenderJudgeEvent};
        if page as usize >= self.members.len().div_ceil(4) { return Err(error("invalid local visual page")); }
        let start = page as usize * 4;
        let visible = self.members.get(start..(start + 4).min(self.members.len())).ok_or_else(|| error("invalid local visual page"))?;
        let members = visible.iter().map(|member| {
            let scalars = RenderMemberScalars {
                song_ns: self.game.member_song_time(member.player).ok_or("local member song frontier unavailable")?.as_nanos(),
                pressed: member.pressed,
                recent: member.recent.iter().map(RenderJudgeEvent::from_event).collect(),
                score: Some(RenderScore::from_summary(self.game.score(member.player).ok_or("local member score unavailable")?)),
                gauge: self.game.gauge(member.player).map(RenderGauge::from_gauge),
                competition: member.saved_hud.snapshot().cloned(),
                saved_comparison_height: member.saved_comparison_height() as u32,
                peer_admitted: member.peer_admitted,
                saved_failed: member.saved_hud.failed(), peer_failed: member.saved_hud.peer_failed(),
            };
            Ok((member.player, scalars, &member.progress))
        }).collect::<Result<Vec<_>, String>>().map_err(error)?;
        let room = match (&self.room_lobby, &self.room_hud) {
            (Some(lobby), Some(hud)) => Some(crate::room_presentation::RoomPresentation::new(lobby.clone(), self.room_render_status, Some(hud), None).map_err(error)?),
            _ => None,
        };
        let frame = self.render_producer.frame(sequence, page, &members, room, self.room_hud_disabled).map_err(error)?;
        self.render_producer.encode_frame(frame, self.render_packet_budget, self.render_diagnostic_budget).map_err(error)
    }
}
