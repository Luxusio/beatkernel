//! Browser transport owner. Pending COW snapshots are never detached by JS transfer.
use crate::{
    browser_render_state::{RenderFrame, RenderMemberScalars, RenderMemberUpdate},
    local_players::PlayerId,
    note_progress::NoteProgress,
    player_chart::PlayerChart,
    room_presentation::RoomPresentation,
};
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct RenderProducer {
    identity: Option<(u64, u64)>,
    roster: Vec<PlayerId>,
    baseline: Vec<NoteProgress>,
    pending: Option<(u64, Vec<(PlayerId, NoteProgress)>)>,
    last_sequence: u64,
}
impl RenderProducer {
    pub(crate) fn register(
        &mut self,
        generation: u64,
        content: u64,
        chart: &Arc<PlayerChart>,
        roster: &[PlayerId],
    ) -> Result<(), String> {
        if generation == 0
            || content == 0
            || !(1..=64).contains(&roster.len())
            || roster
                .iter()
                .enumerate()
                .any(|(index, player)| player.0 == 0 || roster[..index].contains(player))
        {
            return Err("invalid visual producer registration".into());
        }
        if self.identity == Some((generation, content)) {
            return Err("visual producer identity already registered".into());
        }
        let baseline = roster
            .iter()
            .map(|_| NoteProgress::new(chart.clone()))
            .collect::<Result<Vec<_>, _>>()?;
        self.identity = Some((generation, content));
        self.roster = roster.to_vec();
        self.baseline = baseline;
        self.pending = None;
        self.last_sequence = 0;
        Ok(())
    }
    pub(crate) fn frame(
        &mut self,
        sequence: u64,
        page: u32,
        members: &[(PlayerId, RenderMemberScalars, &NoteProgress)],
        room: Option<RoomPresentation>,
        room_disabled: bool,
    ) -> Result<RenderFrame, String> {
        let (generation, content) = self
            .identity
            .ok_or("register visuals before exporting state")?;
        if self.pending.is_some()
            || sequence <= self.last_sequence
            || page as usize >= self.roster.len().div_ceil(4)
        {
            return Err("visual producer has pending state or invalid sequence/page".into());
        }
        let start = page as usize * 4;
        let visible = &self.roster[start..(start + 4).min(self.roster.len())];
        if members.len() != visible.len()
            || members
                .iter()
                .zip(visible)
                .any(|(member, player)| member.0 != *player)
        {
            return Err("visual producer requires the complete visible page".into());
        }
        let updates = members
            .iter()
            .enumerate()
            .map(|(slot, (player, scalars, progress))| {
                RenderMemberUpdate::from_progress(
                    *player,
                    scalars.clone(),
                    progress,
                    &self.baseline[start + slot],
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let frame = RenderFrame {
            generation,
            content,
            sequence,
            page,
            lookahead_ns: super::LOOKAHEAD_NS,
            members: updates,
            room,
            room_disabled,
        };
        frame.encoded_bytes()?;
        // Save exactly the exported prefix, including last_miss, even while the
        // authority changes immediately after this synchronous export returns.
        self.pending = Some((
            sequence,
            members
                .iter()
                .map(|(player, _, progress)| (*player, (*progress).clone()))
                .collect(),
        ));
        self.last_sequence = sequence;
        Ok(frame)
    }
    pub(crate) fn acknowledge(&mut self, generation: u64, content: u64, sequence: u64) -> bool {
        if self.identity != Some((generation, content))
            || self
                .pending
                .as_ref()
                .is_none_or(|(pending, _)| *pending != sequence)
        {
            return false;
        }
        let (_, snapshots) = self.pending.take().expect("matching pending visual frame");
        for (player, progress) in snapshots {
            let index = self
                .roster
                .iter()
                .position(|id| *id == player)
                .expect("registered pending player");
            self.baseline[index] = progress;
        }
        true
    }
    pub(crate) fn cancel_unencoded(&mut self, sequence: u64) {
        if self
            .pending
            .as_ref()
            .is_some_and(|(pending, _)| *pending == sequence)
        {
            self.pending = None;
        }
    }
}

#[path = "render_wire.rs"]
pub(crate) mod wire;

pub(crate) fn limits(packet: u32, diagnostics: u32) -> wire::WireLimits {
    wire::WireLimits {
        max_packet_bytes: packet as usize,
        image: crate::image_assets::ImageAssetLimits {
            max_decoded_bytes: crate::image_assets::MAX_IMAGE_BANK_BYTES,
            ..crate::image_assets::ImageAssetLimits::default()
        },
        max_diagnostic_bytes: diagnostics as usize,
    }
}
pub(crate) fn header(kind: u16, generation: u64, content: u64, sequence: u64) -> wire::WireHeader {
    wire::WireHeader {
        kind,
        generation,
        content,
        sequence,
        payload_len: 0,
    }
}
pub(crate) fn registration(
    chart: &PlayerChart,
    images: &crate::image_assets::ImageAssets,
    roster: &[PlayerId],
    generation: u64,
    content: u64,
    packet: u32,
    diagnostics: u32,
) -> Result<Vec<u8>, String> {
    wire::encode_packet(
        header(wire::REGISTRATION, generation, content, 0),
        &wire::WirePacket::Registration(wire::VisualRegistration {
            chart: chart.export_visual(),
            images: images.export_visual(),
            roster: roster.to_vec(),
        }),
        limits(packet, diagnostics),
    )
}
impl RenderProducer {
    pub(crate) fn encode_frame(
        &mut self,
        frame: RenderFrame,
        packet: u32,
        diagnostics: u32,
    ) -> Result<Vec<u8>, String> {
        let sequence = frame.sequence;
        let result = wire::encode_packet(
            header(wire::FRAME, frame.generation, frame.content, sequence),
            &wire::WirePacket::Frame(frame),
            limits(packet, diagnostics),
        );
        if result.is_err() {
            self.cancel_unencoded(sequence);
        }
        result
    }
}

pub(crate) enum VisualPresentation {
    Preview {
        chart: Arc<PlayerChart>,
        images: Arc<crate::image_assets::ImageAssets>,
        song: beatkernel::time::Timestamp,
        lookahead: i64,
    },
    Play {
        state: crate::browser_render_state::BrowserRenderState,
        local: bool,
    },
    History(crate::historical_record_presentation::HistoricalRecordPresentation),
    Results {
        view: crate::ui::results::FrozenResultsView,
        page: usize,
        comparisons: bool,
        room: Option<(crate::room_results_builder::FrozenRoomResults, usize)>,
    },
    Room {
        model: crate::room_results_builder::FrozenRoomResults,
        page: usize,
    },
}
