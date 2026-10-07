//! Bounded committed display state. This owner never judges, advances time or proves completion.
use crate::{
    competition::ScoreSummary,
    competition_presentation::{CompetitionSnapshot, NetworkStatus},
    gauge::{BmsGauge, GaugeSnapshot, MAX_GAUGE_UNITS},
    image_assets::{ImageAssetLimits, ImageAssets, ImageAssetsTransfer},
    local_players::PlayerId,
    note_progress::{NoteProgress, NoteProgressPageUpdate},
    player_chart::{PlayerChart, PlayerChartTransfer},
    room_presentation::RoomPresentation,
    timing::TimingRecord,
};
use beatkernel::{
    chart::ObjectId,
    judge::{JudgeEvent, JudgeGrade, JudgeOutcome, JudgeStage, MissReason},
    time::{Duration, Timestamp},
};
use std::sync::Arc;

pub const RENDER_PROTOCOL_VERSION: u32 = 1;
pub const MAX_RENDER_PLAYERS: usize = 64;
pub const MAX_RENDER_VISIBLE: usize = 4;
pub const MAX_RENDER_RECENT: usize = 128;
pub const RENDER_PAGE_BYTES: usize = 3 * 4 + 128 * 8;
// Header128; roster64*u32; member header256 + timing128 + gauge32;
// event: object64, stage32+custom32, outcome32+grade/reason32, delta64, at64 =40;
// comparison <=8*(label256+header64)+network64; room metadata <=64*(host32+64*u32)
// plus four rows*(labels384+scalars64), heading256 and diagnostic4096.
pub const MAX_RENDER_SCALAR_BYTES: usize = 256 + 128 + 32 + MAX_RENDER_RECENT * 40 + 8 * 320 + 64;
pub const MAX_RENDER_ROOM_BYTES: usize = 128 + 64 * (32 + 64 * 4) + 4 * (384 + 64) + 256 + 4096;
pub const MAX_RENDER_FRAME_BYTES: usize = 128
    + MAX_RENDER_PLAYERS * 4
    + MAX_RENDER_VISIBLE
        * (MAX_RENDER_SCALAR_BYTES
            + beatkernel::chart::MAX_SOURCE_ITEMS.div_ceil(4096) * RENDER_PAGE_BYTES)
    + MAX_RENDER_ROOM_BYTES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderScore {
    pub hits: u64,
    pub misses: u64,
    pub combo: u64,
    pub max_combo: u64,
    pub timing: TimingRecord,
}
impl RenderScore {
    pub fn from_summary(score: &ScoreSummary) -> Self {
        Self {
            hits: score.hits,
            misses: score.misses,
            combo: score.combo,
            max_combo: score.max_combo,
            timing: score.timing.record(),
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.combo > self.max_combo
            || self.max_combo > self.hits
            || self.hits.checked_add(self.misses).is_none()
            || self.timing.count > self.hits
        {
            return Err("invalid visual score counters".into());
        }
        self.timing.validate().map_err(|error| error.to_string())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderGauge {
    pub snapshot: GaugeSnapshot,
    pub clear_units: u64,
}
impl RenderGauge {
    pub fn from_gauge(gauge: &BmsGauge) -> Self {
        Self {
            snapshot: *gauge.snapshot(),
            clear_units: gauge.profile().clear_units(),
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.snapshot.level_units > MAX_GAUGE_UNITS || self.clear_units > MAX_GAUGE_UNITS {
            return Err("invalid visual gauge units".into());
        }
        Ok(())
    }
}
/// Numeric visual event with no physical input metadata or replay input transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderJudgeEvent {
    pub object: u64,
    pub stage: u32,
    pub custom_stage: u32,
    pub outcome: u32,
    pub grade_or_reason: u32,
    pub delta_ns: i64,
    pub at_ns: i64,
}
impl RenderJudgeEvent {
    pub fn from_event(event: &JudgeEvent) -> Self {
        let (stage, custom_stage) = match event.stage {
            JudgeStage::Instant => (0, 0),
            JudgeStage::HoldHead => (1, 0),
            JudgeStage::HoldTail => (2, 0),
            JudgeStage::Custom(id) => (3, id),
        };
        let (outcome, grade_or_reason, delta_ns) = match event.outcome {
            JudgeOutcome::Hit { grade, delta } => (0, grade.0, delta.as_nanos()),
            JudgeOutcome::Miss { reason } => (
                1,
                match reason {
                    MissReason::HeadTimeout => 0,
                    MissReason::TailTimeout => 1,
                    MissReason::EarlyRelease => 2,
                    MissReason::RejectedInput => 3,
                },
                0,
            ),
        };
        Self {
            object: event.object.0,
            stage,
            custom_stage,
            outcome,
            grade_or_reason,
            delta_ns,
            at_ns: event.at.as_nanos(),
        }
    }
    /// Convert a validated visual event for existing paint routines; provenance stays absent.
    pub fn event(&self) -> JudgeEvent {
        JudgeEvent {
            object: ObjectId(self.object),
            stage: match self.stage {
                0 => JudgeStage::Instant,
                1 => JudgeStage::HoldHead,
                2 => JudgeStage::HoldTail,
                _ => JudgeStage::Custom(self.custom_stage),
            },
            outcome: if self.outcome == 0 {
                JudgeOutcome::Hit {
                    grade: JudgeGrade(self.grade_or_reason),
                    delta: Duration::from_nanos(self.delta_ns),
                }
            } else {
                JudgeOutcome::Miss {
                    reason: match self.grade_or_reason {
                        0 => MissReason::HeadTimeout,
                        1 => MissReason::TailTimeout,
                        2 => MissReason::EarlyRelease,
                        _ => MissReason::RejectedInput,
                    },
                }
            },
            at: Timestamp::from_nanos(self.at_ns),
            input: None,
        }
    }
    fn validate(&self, chart: &PlayerChart) -> Result<(), String> {
        if self.stage > 3
            || (self.stage != 3 && self.custom_stage != 0)
            || self.outcome > 1
            || (self.outcome == 1 && (self.grade_or_reason > 3 || self.delta_ns != 0))
        {
            return Err("invalid visual judge event tag".into());
        }
        let note = chart
            .note_by_object(ObjectId(self.object))
            .ok_or("visual judge event has unknown object")?;
        if (self.stage == 0 && note.end.is_some())
            || ((self.stage == 1 || self.stage == 2) && note.end.is_none())
        {
            return Err("visual judge event shape differs from chart".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderMemberScalars {
    pub song_ns: i64,
    pub pressed: u32,
    pub recent: Vec<RenderJudgeEvent>,
    pub score: Option<RenderScore>,
    pub gauge: Option<RenderGauge>,
    pub competition: Option<CompetitionSnapshot>,
    pub saved_comparison_height: u32,
    pub peer_admitted: bool,
    pub saved_failed: bool,
    pub peer_failed: bool,
}
impl RenderMemberScalars {
    fn validate(&self, chart: &PlayerChart) -> Result<(), String> {
        let lane_mask = if chart.lanes.len() >= 32 {
            u32::MAX
        } else {
            (1u32 << chart.lanes.len()) - 1
        };
        if self.recent.len() > MAX_RENDER_RECENT
            || self.pressed & !lane_mask != 0
            || self.saved_comparison_height > 8 * 14
            || self.saved_comparison_height % 14 != 0
            || (self.peer_failed && !self.peer_admitted)
        {
            return Err("invalid visual player scalars".into());
        }
        if let Some(score) = &self.score {
            score.validate()?;
        }
        if let Some(gauge) = &self.gauge {
            gauge.validate()?;
        }
        for event in &self.recent {
            event.validate(chart)?;
        }
        if let Some(snapshot) = &self.competition {
            validate_comparison(snapshot)?;
        }
        Ok(())
    }
}
pub(crate) fn validate_comparison(snapshot: &CompetitionSnapshot) -> Result<(), String> {
    if snapshot.ghosts.len() > 8 {
        return Err("visual comparison ghost capacity exceeded".into());
    }
    for ghost in &snapshot.ghosts {
        if ghost.label.is_empty()
            || ghost.label.len() > 256
            || ghost.label.chars().count() > 64
            || ghost.label.chars().any(char::is_control)
            || ghost.combo > ghost.max_combo
            || ghost.max_combo > ghost.hits
            || ghost.hits.checked_add(ghost.misses).is_none()
        {
            return Err("invalid visual comparison".into());
        }
    }
    if let Some(network) = &snapshot.network {
        if let Some(progress) = network.progress {
            if network.status == NetworkStatus::Waiting {
                return Err("waiting peer has progress".into());
            }
            crate::multiplayer_protocol::validate_progress(None, progress)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderProgressPage {
    pub index: u32,
    pub valid_count: u32,
    pub completed_count: u32,
    pub packed_states: Vec<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderMemberUpdate {
    pub player: PlayerId,
    pub scalars: RenderMemberScalars,
    pub pages: Vec<RenderProgressPage>,
    pub last_miss_ns: Option<i64>,
}
impl RenderMemberUpdate {
    pub fn from_progress(
        player: PlayerId,
        scalars: RenderMemberScalars,
        progress: &NoteProgress,
        acknowledged: &NoteProgress,
    ) -> Result<Self, String> {
        let pages = progress
            .changed_pages_since(acknowledged)?
            .map(|page| RenderProgressPage {
                index: page.index() as u32,
                valid_count: page.valid_count() as u32,
                completed_count: page.completed_count() as u32,
                packed_states: page.packed_states().to_vec(),
            })
            .collect();
        Ok(Self {
            player,
            scalars,
            pages,
            last_miss_ns: progress.last_miss().map(Timestamp::as_nanos),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderFrame {
    pub generation: u64,
    pub content: u64,
    pub sequence: u64,
    pub page: u32,
    pub lookahead_ns: i64,
    pub members: Vec<RenderMemberUpdate>,
    pub room: Option<RoomPresentation>,
    pub room_disabled: bool,
}
impl RenderFrame {
    /// Packed fixed-width protocol budget, independent of Rust struct layout.
    pub fn encoded_bytes(&self) -> Result<usize, String> {
        if self.members.len() > MAX_RENDER_VISIBLE {
            return Err("visual frame member capacity exceeded".into());
        }
        let mut bytes = 128usize;
        for member in &self.members {
            let scalars = &member.scalars;
            if scalars.recent.len() > MAX_RENDER_RECENT
                || member.pages.len() > beatkernel::chart::MAX_SOURCE_ITEMS.div_ceil(4096)
            {
                return Err("visual frame event or page capacity exceeded".into());
            }
            bytes = bytes
                .checked_add(256 + 128 + 32 + scalars.recent.len() * 40)
                .ok_or("visual frame byte overflow")?;
            if let Some(snapshot) = &scalars.competition {
                validate_comparison(snapshot)?;
                bytes = bytes
                    .checked_add(
                        64 + snapshot
                            .ghosts
                            .iter()
                            .map(|ghost| 64 + ghost.label.len())
                            .sum::<usize>(),
                    )
                    .ok_or("visual frame byte overflow")?;
            }
            for page in &member.pages {
                if page.packed_states.len() != 128 {
                    return Err("visual page byte extent differs from 128 words".into());
                }
                bytes = bytes
                    .checked_add(RENDER_PAGE_BYTES)
                    .ok_or("visual frame byte overflow")?;
            }
        }
        if let Some(room) = &self.room {
            room.validate()?;
            bytes = bytes
                .checked_add(
                    128 + room.heading.len()
                        + room.error.as_ref().map_or(0, String::len)
                        + room
                            .lobby
                            .members
                            .iter()
                            .map(|member| 32 + member.players.len() * 4)
                            .sum::<usize>()
                        + room
                            .rows
                            .iter()
                            .map(|row| {
                                64 + row.label.len()
                                    + row.counters.iter().map(String::len).sum::<usize>()
                            })
                            .sum::<usize>(),
                )
                .ok_or("visual frame byte overflow")?;
        }
        if bytes > MAX_RENDER_FRAME_BYTES {
            return Err("visual frame byte capacity exceeded".into());
        }
        Ok(bytes)
    }
}
#[derive(Clone)]
pub struct RenderMember {
    pub scalars: Option<RenderMemberScalars>,
    pub progress: NoteProgress,
}
pub struct BrowserRenderState {
    generation: u64,
    content: u64,
    sequence: u64,
    chart: Arc<PlayerChart>,
    images: Arc<ImageAssets>,
    roster: Vec<PlayerId>,
    members: Vec<RenderMember>,
    page: u32,
    lookahead_ns: i64,
    room: Option<RoomPresentation>,
    room_disabled: bool,
}
impl BrowserRenderState {
    pub fn new(
        generation: u64,
        content: u64,
        chart: Arc<PlayerChart>,
        images: Arc<ImageAssets>,
        roster: Vec<PlayerId>,
    ) -> Result<Self, String> {
        if generation == 0
            || content == 0
            || !(1..=MAX_RENDER_PLAYERS).contains(&roster.len())
            || roster
                .iter()
                .enumerate()
                .any(|(index, player)| player.0 == 0 || roster[..index].contains(player))
        {
            return Err("invalid visual registration identity or roster".into());
        }
        let mut members = Vec::new();
        members
            .try_reserve_exact(roster.len())
            .map_err(|error| error.to_string())?;
        for _ in &roster {
            members.push(RenderMember {
                scalars: None,
                progress: NoteProgress::new(chart.clone())?,
            });
        }
        Ok(Self {
            generation,
            content,
            sequence: 0,
            chart,
            images,
            roster,
            members,
            page: 0,
            lookahead_ns: 1,
            room: None,
            room_disabled: false,
        })
    }
    /// Convenience for the unchanged browser preparation defaults. Explicitly
    /// configured banks must use import_visual_with_limits at registration.
    pub fn import_visual(
        generation: u64,
        content: u64,
        chart: PlayerChartTransfer,
        images: ImageAssetsTransfer,
        roster: Vec<PlayerId>,
    ) -> Result<Self, String> {
        Self::import_visual_with_limits(
            generation,
            content,
            chart,
            images,
            roster,
            ImageAssetLimits::default(),
        )
    }
    pub fn import_visual_with_limits(
        generation: u64,
        content: u64,
        chart: PlayerChartTransfer,
        images: ImageAssetsTransfer,
        roster: Vec<PlayerId>,
        limits: ImageAssetLimits,
    ) -> Result<Self, String> {
        limits.validate()?;
        visual_registration_bytes(&chart, &images, &roster)?;
        let chart = Arc::new(PlayerChart::import_visual(chart).map_err(|error| error.to_string())?);
        let images = Arc::new(ImageAssets::import_visual(images, limits)?);
        Self::new(generation, content, chart, images, roster)
    }
    /// Worker transport registration additionally supplies its explicit diagnostic
    /// admission budget. Existing gameplay diagnostics are never truncated.
    pub fn import_visual_with_budget(
        generation: u64,
        content: u64,
        chart: PlayerChartTransfer,
        images: ImageAssetsTransfer,
        roster: Vec<PlayerId>,
        limits: ImageAssetLimits,
        max_diagnostic_bytes: usize,
    ) -> Result<Self, String> {
        limits.validate()?;
        let diagnostics = images
            .unavailable
            .iter()
            .try_fold(0usize, |sum, (_, reason)| {
                let bytes = match reason {
                    crate::image_assets::ImageUnavailable::InvalidData(reason) => reason.len(),
                    _ => 0,
                };
                sum.checked_add(bytes)
            })
            .filter(|bytes| *bytes <= max_diagnostic_bytes)
            .ok_or("visual image diagnostics exceed admitted byte budget")?;
        visual_registration_bytes(&chart, &images, &roster)?
            .checked_sub(diagnostics)
            .ok_or("visual registration byte count invalid")?;
        Self::import_visual_with_limits(generation, content, chart, images, roster, limits)
    }
    pub fn apply_frame(&mut self, frame: &RenderFrame) -> Result<(), String> {
        if frame.generation != self.generation
            || frame.content != self.content
            || frame.sequence <= self.sequence
            || frame.lookahead_ns <= 0
            || frame.page as usize >= self.roster.len().div_ceil(MAX_RENDER_VISIBLE)
            || (frame.room.is_some() && frame.room_disabled)
        {
            return Err("visual frame identity, sequence, page or lookahead is invalid".into());
        }
        frame.encoded_bytes()?;
        let start = frame.page as usize * MAX_RENDER_VISIBLE;
        let visible = &self.roster[start..(start + MAX_RENDER_VISIBLE).min(self.roster.len())];
        if frame.members.len() != visible.len() {
            return Err("visual frame omitted visible members".into());
        }
        let mut staged = Vec::new();
        staged
            .try_reserve_exact(visible.len())
            .map_err(|error| error.to_string())?;
        for (slot, update) in frame.members.iter().enumerate() {
            if update.player != visible[slot] {
                return Err("visual frame foreign, duplicate or unordered member".into());
            }
            update.scalars.validate(&self.chart)?;
            let index = start + slot;
            if let Some(previous) = &self.members[index].scalars {
                if previous.saved_comparison_height != update.scalars.saved_comparison_height
                    || previous.peer_admitted != update.scalars.peer_admitted
                {
                    return Err(
                        "visual comparison reservation changed after member admission".into(),
                    );
                }
            }
            let mut candidate = self.members[index].clone();
            let pages: Vec<_> = update
                .pages
                .iter()
                .map(|page| NoteProgressPageUpdate {
                    index: page.index as usize,
                    valid_count: page.valid_count as usize,
                    completed_count: page.completed_count as usize,
                    packed_states: &page.packed_states,
                })
                .collect();
            candidate
                .progress
                .apply_page_updates(&pages, update.last_miss_ns.map(Timestamp::from_nanos))?;
            candidate.scalars = Some(update.scalars.clone());
            staged.push((index, candidate));
        }
        // Publication occurs only after the complete final member/page/scalar validates.
        for (index, candidate) in staged {
            self.members[index] = candidate;
        }
        self.sequence = frame.sequence;
        self.page = frame.page;
        self.lookahead_ns = frame.lookahead_ns;
        self.room = frame.room.clone();
        self.room_disabled = frame.room_disabled;
        Ok(())
    }
    pub fn member(&self, player: PlayerId) -> Option<&RenderMember> {
        self.roster
            .iter()
            .position(|id| *id == player)
            .map(|index| &self.members[index])
    }
    pub(crate) fn members(&self) -> &[RenderMember] {
        &self.members
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn page(&self) -> u32 {
        self.page
    }
    pub fn room(&self) -> Option<&RoomPresentation> {
        self.room.as_ref()
    }
    pub fn room_disabled(&self) -> bool {
        self.room_disabled
    }
    pub fn roster(&self) -> &[PlayerId] {
        &self.roster
    }
    pub fn chart(&self) -> &Arc<PlayerChart> {
        &self.chart
    }
    pub fn images(&self) -> &Arc<ImageAssets> {
        &self.images
    }
    pub fn lookahead_ns(&self) -> i64 {
        self.lookahead_ns
    }
    /// Sender current+ACK baseline for every stable player, receiver retained
    /// state for every player, four pending sender and four staged receiver
    /// members.1024 packed bytes+64 cover page/Arc/directory allocation metadata;
    /// scalar copies include hidden retained members. Two encoded buffers remain
    /// separate. Cold chart/images and GPU/surface storage are separate.
    pub fn peak_progress_bytes(note_count: usize, roster_count: usize) -> Result<usize, String> {
        if note_count > beatkernel::chart::MAX_SOURCE_ITEMS
            || !(1..=MAX_RENDER_PLAYERS).contains(&roster_count)
        {
            return Err("visual peak budget extent exceeds source or roster limits".into());
        }
        let pages = note_count.div_ceil(4096);
        let snapshots = roster_count * 3 + MAX_RENDER_VISIBLE * 2;
        Ok(
            snapshots * (pages * (1024 + 64) + MAX_RENDER_SCALAR_BYTES + 64)
                + MAX_RENDER_FRAME_BYTES * 2,
        )
    }
}

/// Fixed numeric layout for the complete cold registration. UTF-8 chart metadata
/// retains the browser preparation's8MiB text limit; source item count stays1M.
pub const MAX_RENDER_CHART_TEXT_BYTES: usize = 8 * 1024 * 1024;
pub const RENDER_NOTE_RECORD_BYTES: usize = 32;
pub const RENDER_MINE_RECORD_BYTES: usize = 24;
pub const RENDER_BGA_RECORD_BYTES: usize = 24;
pub const RENDER_OPACITY_RECORD_BYTES: usize = 24;
pub const MAX_RENDER_IMAGE_METADATA_BYTES: usize =
    crate::image_assets::MAX_IMAGE_REFERENCES * (3 * 16 + 4 + 12 + 8 + 8 + 8);
/// Maximum fixed registration storage, excluding exact diagnostic UTF-8 bytes.
/// The Worker caller explicitly admits those additional bytes with_budget.
pub const MAX_RENDER_REGISTRATION_BYTES: usize = 128
    + MAX_RENDER_PLAYERS * 4
    + MAX_RENDER_CHART_TEXT_BYTES
    + 18 * 4
    + beatkernel::chart::MAX_SOURCE_ITEMS * RENDER_NOTE_RECORD_BYTES
    + crate::image_assets::MAX_IMAGE_BANK_BYTES as usize
    + MAX_RENDER_IMAGE_METADATA_BYTES;
pub const MAX_RENDER_GPU_NOTE_BYTES: usize = MAX_RENDER_VISIBLE
    * (crate::player_chart::MAX_VISIBLE_NOTES * 3 + crate::player_chart::MAX_VISIBLE_MINES)
    * 32;

pub fn visual_registration_bytes(
    chart: &PlayerChartTransfer,
    images: &ImageAssetsTransfer,
    roster: &[PlayerId],
) -> Result<usize, String> {
    if !(1..=MAX_RENDER_PLAYERS).contains(&roster.len())
        || chart.lanes.len() > 18
        || chart
            .title
            .len()
            .checked_add(chart.artist.len())
            .is_none_or(|bytes| bytes > MAX_RENDER_CHART_TEXT_BYTES)
        || [
            chart.notes.len(),
            chart.mines.len(),
            chart.bga.len(),
            chart.opacity.len(),
        ]
        .into_iter()
        .try_fold(0usize, |sum, count| sum.checked_add(count))
        .is_none_or(|count| count > beatkernel::chart::MAX_SOURCE_ITEMS)
    {
        return Err("visual registration exceeds original browser source limits".into());
    }
    let references = crate::image_assets::MAX_IMAGE_REFERENCES;
    if images.resources.len() > references * 3
        || images.sources.len() > references
        || images.source_ids.len() > references
        || images.images.len() > references
        || images.layers.len() > references
        || images.unavailable.len() > references
    {
        return Err("visual registration image table capacity exceeded".into());
    }
    let diagnostics = images
        .unavailable
        .iter()
        .try_fold(0usize, |sum, (_, reason)| {
            let bytes = match reason {
                crate::image_assets::ImageUnavailable::InvalidData(reason) => reason.len(),
                _ => 0,
            };
            sum.checked_add(bytes)
        })
        .ok_or("visual image diagnostic byte count overflow")?;
    let rgba = images
        .resources
        .iter()
        .try_fold(0usize, |sum, image| {
            sum.checked_add(usize::try_from(image.byte_len()).ok()?)
        })
        .filter(|bytes| *bytes <= crate::image_assets::MAX_IMAGE_BANK_BYTES as usize)
        .ok_or("visual registration RGBA byte capacity exceeded")?;
    let fixed_bytes = 128
        + roster.len() * 4
        + chart.title.len()
        + chart.artist.len()
        + chart.lanes.len() * 4
        + chart.notes.len() * RENDER_NOTE_RECORD_BYTES
        + chart.mines.len() * RENDER_MINE_RECORD_BYTES
        + chart.bga.len() * RENDER_BGA_RECORD_BYTES
        + chart.opacity.len() * RENDER_OPACITY_RECORD_BYTES
        + rgba
        + images.resources.len() * 16
        + images.sources.len() * 4
        + images.source_ids.len() * 12
        + images.images.len() * 8
        + images.layers.len() * 8
        + images.unavailable.len() * 8;
    fixed_bytes
        .checked_add(diagnostics)
        .ok_or_else(|| "visual registration byte count overflow".into())
}
/// Conservative peak includes six encoded representations plus the positive
/// decoded record expansion for three owned Rust charts. Images count exact
/// RGBA bytes separately;1024/reference covers the three resource tables,
/// four BTreeMaps, vectors, Strings, Arc headers and allocation padding per bank.
/// Receiver note indexes need at most4*n*i64 tree entries+n*(ObjectId,usize).
/// Additional timeline staging assumes every remaining source item is BGA or
/// opacity data, at its actual Rust size. Surface/depth attachments remain
/// separately extent/device-bound by Renderer.
pub fn peak_visual_transport_bytes(
    cold_bytes: usize,
    note_count: usize,
    roster_count: usize,
) -> Result<u64, String> {
    let progress = BrowserRenderState::peak_progress_bytes(note_count, roster_count)? as u64;
    let source_items = beatkernel::chart::MAX_SOURCE_ITEMS as u64;
    let expansion = [
        (
            std::mem::size_of::<crate::player_chart::PlayerNote>(),
            RENDER_NOTE_RECORD_BYTES,
        ),
        (
            std::mem::size_of::<crate::player_chart::PlayerMine>(),
            RENDER_MINE_RECORD_BYTES,
        ),
        (
            std::mem::size_of::<beatkernel_bms::ScheduledBga>(),
            RENDER_BGA_RECORD_BYTES,
        ),
        (
            std::mem::size_of::<beatkernel_bms::ScheduledBgaOpacity>(),
            RENDER_OPACITY_RECORD_BYTES,
        ),
    ]
    .into_iter()
    .map(|(decoded, encoded)| decoded.saturating_sub(encoded))
    .max()
    .unwrap_or(0) as u64;
    let note_index = note_count as u64
        * (4 * std::mem::size_of::<i64>() + std::mem::size_of::<(ObjectId, usize)>()) as u64;
    let timeline_record = std::mem::size_of::<beatkernel_bms::ScheduledBga>()
        .max(std::mem::size_of::<beatkernel_bms::ScheduledBgaOpacity>())
        as u64;
    let timeline_staging = (source_items - note_count as u64) * timeline_record;
    let cpu_metadata = 3 * source_items * expansion
        + 3 * (note_index + timeline_staging)
        + 3 * crate::image_assets::MAX_IMAGE_REFERENCES as u64 * 1024;
    (cold_bytes as u64)
        .checked_mul(6)
        .and_then(|bytes| {
            bytes.checked_add(
                cpu_metadata
                    + progress
                    + 2 * MAX_RENDER_GPU_NOTE_BYTES as u64
                    + 2 * crate::image_assets::MAX_IMAGE_BANK_BYTES,
            )
        })
        .ok_or_else(|| "visual peak byte count overflow".into())
}
