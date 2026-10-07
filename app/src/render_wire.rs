//! Pure, allocation-preflighted little-endian visual transport. No gameplay authority.
use crate::{
    browser_render_state::*,
    competition::OpponentKind,
    competition_presentation::{
        CompetitionSnapshot, GhostSnapshot, NetworkSnapshot, NetworkStatus,
    },
    gauge::{GaugeFailure, GaugeSnapshot},
    historical_record_presentation::FrozenHistoricalRecord,
    image_assets::{ImageAssetLimits, ImageAssets, ImageAssetsTransfer, ImageUnavailable},
    judgment_policy::BmsScoreSummary,
    local_players::PlayerId,
    multiplayer_group_rooms::{GroupRoomMember, GroupRoomPhase},
    multiplayer_protocol::Progress,
    multiplayer_rooms::ParticipantId,
    play_result::{PlayResultOutcome, PlayResultScope},
    player_chart::{PlayerChart, PlayerChartTransfer, PlayerMine, PlayerNote},
    result_archive::{ArchivedResult, ArchivedScore},
    room_presentation::{RoomLobby, RoomPresentation, RoomScoreRow, RoomStatus},
    room_results_builder::FrozenRoomResults,
    texture::RgbaImage,
    timing::TimingRecord,
    ui::results::{FrozenResultRow, FrozenResultsModel, FrozenScoreDetails},
};
use beatkernel::{chart::ObjectId, time::Timestamp};
use beatkernel_bms::{
    BgaChannel, ImageId, MineDamage, PoorBgaMode, ScheduledBga, ScheduledBgaOpacity,
};
use std::sync::Arc;

pub const HEADER_BYTES: usize = 40;
pub const MAGIC: [u8; 4] = *b"BKRV";
pub const VERSION: u16 = 1;
pub const REGISTRATION: u16 = 1;
pub const FRAME: u16 = 2;
pub const PREVIEW: u16 = 3;
pub const HISTORY: u16 = 4;
pub const RESULTS: u16 = 5;
pub const ROOM: u16 = 6;
// Actual records use optional tags rather than native struct padding. These
// bounds cover the existing capacities, including all 4096 archived grades.
pub const MAX_FRAME_PACKET_BYTES: usize = HEADER_BYTES + MAX_RENDER_FRAME_BYTES;
pub const MAX_HISTORY_PACKET_BYTES: usize = HEADER_BYTES + 512 + 4096 * 12 + 8 * 320;
pub const MAX_RESULTS_PACKET_BYTES: usize = HEADER_BYTES + 64 * (512 + 4096 * 12 + 8 * 320);
pub const MAX_ROOM_PACKET_BYTES: usize =
    HEADER_BYTES + 64 * (32 + 64 * 4) + 1008 * (128 + 256 + 4096 + 4 * (384 + 64));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WireHeader {
    pub kind: u16,
    pub generation: u64,
    pub content: u64,
    pub sequence: u64,
    pub payload_len: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct WireLimits {
    pub max_packet_bytes: usize,
    pub image: ImageAssetLimits,
    pub max_diagnostic_bytes: usize,
}
#[derive(Clone)]
pub struct VisualRegistration {
    pub chart: PlayerChartTransfer,
    pub images: ImageAssetsTransfer,
    pub roster: Vec<PlayerId>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewState {
    pub song_ns: i64,
    pub lookahead_ns: i64,
}
#[derive(Clone)]
pub enum WirePacket {
    Registration(VisualRegistration),
    Frame(RenderFrame),
    Preview(PreviewState),
    History(FrozenHistoricalRecord),
    Results(FrozenResultsModel),
    Room(FrozenRoomResults),
}
impl WirePacket {
    pub fn kind(&self) -> u16 {
        match self {
            Self::Registration(_) => REGISTRATION,
            Self::Frame(_) => FRAME,
            Self::Preview(_) => PREVIEW,
            Self::History(_) => HISTORY,
            Self::Results(_) => RESULTS,
            Self::Room(_) => ROOM,
        }
    }
}
fn kind_limit(kind: u16, limits: WireLimits) -> Result<usize, String> {
    limits.image.validate()?;
    let bound = match kind {
        REGISTRATION => MAX_RENDER_REGISTRATION_BYTES
            .checked_sub(crate::image_assets::MAX_IMAGE_BANK_BYTES as usize)
            .and_then(|v| v.checked_add(usize::try_from(limits.image.max_decoded_bytes).ok()?))
            .and_then(|v| v.checked_add(limits.max_diagnostic_bytes))
            .and_then(|v| v.checked_add(HEADER_BYTES))
            .ok_or("registration budget overflow")?,
        FRAME => MAX_FRAME_PACKET_BYTES,
        PREVIEW => HEADER_BYTES + 16,
        HISTORY => MAX_HISTORY_PACKET_BYTES,
        RESULTS => MAX_RESULTS_PACKET_BYTES,
        ROOM => MAX_ROOM_PACKET_BYTES,
        _ => return Err("unknown visual packet kind".into()),
    };
    Ok(bound.min(limits.max_packet_bytes))
}
/// Call against a stack copy of exactly the header before copying a JS buffer.
pub fn preflight_header(
    bytes: &[u8],
    packet_len: usize,
    limits: WireLimits,
) -> Result<WireHeader, String> {
    if bytes.len() != HEADER_BYTES
        || bytes[..4] != MAGIC
        || u16::from_le_bytes(bytes[4..6].try_into().unwrap()) != VERSION
    {
        return Err("visual envelope magic, version or header extent invalid".into());
    }
    let h = WireHeader {
        kind: u16::from_le_bytes(bytes[6..8].try_into().unwrap()),
        generation: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
        content: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        sequence: u64::from_le_bytes(bytes[24..32].try_into().unwrap()),
        payload_len: u64::from_le_bytes(bytes[32..40].try_into().unwrap()),
    };
    if h.generation == 0
        || h.content == 0
        || packet_len > kind_limit(h.kind, limits)?
        || usize::try_from(h.payload_len)
            .ok()
            .and_then(|n| n.checked_add(HEADER_BYTES))
            != Some(packet_len)
    {
        return Err("visual envelope identity or admitted byte extent invalid".into());
    }
    Ok(h)
}
// A single parser runs twice: first all tags, counts, extents and UTF8 are
// scanned without heap allocation; only a complete scan permits materialization.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    scan: bool,
    limits: WireLimits,
    diagnostics: usize,
    source_items: usize,
    rgba: u64,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], scan: bool, limits: WireLimits) -> Self {
        Self {
            bytes,
            at: 0,
            scan,
            limits,
            diagnostics: 0,
            source_items: 0,
            rgba: 0,
        }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(n)
            .filter(|end| *end <= self.bytes.len())
            .ok_or("truncated visual packet")?;
        let value = &self.bytes[self.at..end];
        self.at = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn i64(&mut self) -> Result<i64, String> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn i128(&mut self) -> Result<i128, String> {
        Ok(i128::from_le_bytes(self.take(16)?.try_into().unwrap()))
    }
    fn u128(&mut self) -> Result<u128, String> {
        Ok(u128::from_le_bytes(self.take(16)?.try_into().unwrap()))
    }
    fn tag(&mut self, max: u8) -> Result<u8, String> {
        let t = self.u8()?;
        if t > max {
            Err("unknown visual tag".into())
        } else {
            Ok(t)
        }
    }
    fn bool(&mut self) -> Result<bool, String> {
        Ok(self.tag(1)? != 0)
    }
    fn optional<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<Option<T>, String> {
        if self.bool()? {
            Ok(Some(f(self)?))
        } else {
            Ok(None)
        }
    }
    fn text(&mut self, max: usize, diagnostic: bool) -> Result<String, String> {
        let n = usize::try_from(self.u64()?).map_err(|_| "visual text width overflow")?;
        if n > max {
            return Err("visual text budget exceeded".into());
        }
        if diagnostic {
            self.diagnostics = self
                .diagnostics
                .checked_add(n)
                .filter(|n| *n <= self.limits.max_diagnostic_bytes)
                .ok_or("visual diagnostic allowance exceeded")?;
        }
        let text = std::str::from_utf8(self.take(n)?).map_err(|_| "invalid visual UTF8")?;
        if self.scan {
            Ok(String::new())
        } else {
            let mut out = String::new();
            out.try_reserve_exact(n).map_err(|e| e.to_string())?;
            out.push_str(text);
            Ok(out)
        }
    }
    fn count(&mut self, max: usize, min_bytes: usize) -> Result<usize, String> {
        let n = self.u32()? as usize;
        if n > max
            || n.checked_mul(min_bytes)
                .is_none_or(|bytes| bytes > self.bytes.len() - self.at)
        {
            return Err("visual list count or byte extent invalid".into());
        }
        Ok(n)
    }
    fn list<T>(
        &mut self,
        max: usize,
        min: usize,
        mut f: impl FnMut(&mut Self) -> Result<T, String>,
    ) -> Result<Vec<T>, String> {
        let n = self.count(max, min)?;
        let mut out = Vec::new();
        if !self.scan {
            out.try_reserve_exact(n).map_err(|e| e.to_string())?;
        }
        for _ in 0..n {
            let value = f(self)?;
            if !self.scan {
                out.push(value);
            }
        }
        Ok(out)
    }
    fn source_list<T>(
        &mut self,
        min: usize,
        f: impl FnMut(&mut Self) -> Result<T, String>,
    ) -> Result<Vec<T>, String> {
        let n = self.u32()? as usize;
        self.at -= 4;
        self.source_items = self
            .source_items
            .checked_add(n)
            .filter(|n| *n <= beatkernel::chart::MAX_SOURCE_ITEMS)
            .ok_or("visual aggregate source item cap exceeded")?;
        self.list(beatkernel::chart::MAX_SOURCE_ITEMS, min, f)
    }
}
// Writer also measures before allocation: no packet can grow past its admission.
struct Writer {
    bytes: Vec<u8>,
    length: usize,
    measure: bool,
    cap: usize,
}
impl Writer {
    fn put(&mut self, b: &[u8]) -> Result<(), String> {
        self.length = self
            .length
            .checked_add(b.len())
            .filter(|n| *n <= self.cap)
            .ok_or("visual encoded byte budget exceeded")?;
        if !self.measure {
            self.bytes.extend_from_slice(b);
        }
        Ok(())
    }
    fn u8(&mut self, v: u8) -> Result<(), String> {
        self.put(&[v])
    }
    fn u16(&mut self, v: u16) -> Result<(), String> {
        self.put(&v.to_le_bytes())
    }
    fn u32(&mut self, v: u32) -> Result<(), String> {
        self.put(&v.to_le_bytes())
    }
    fn u64(&mut self, v: u64) -> Result<(), String> {
        self.put(&v.to_le_bytes())
    }
    fn i64(&mut self, v: i64) -> Result<(), String> {
        self.put(&v.to_le_bytes())
    }
    fn bool(&mut self, v: bool) -> Result<(), String> {
        self.u8(u8::from(v))
    }
    fn optional<T>(
        &mut self,
        v: &Option<T>,
        f: impl FnOnce(&mut Self, &T) -> Result<(), String>,
    ) -> Result<(), String> {
        self.bool(v.is_some())?;
        if let Some(v) = v {
            f(self, v)?;
        }
        Ok(())
    }
    fn text(&mut self, v: &str) -> Result<(), String> {
        self.u64(v.len() as u64)?;
        self.put(v.as_bytes())
    }
    fn list<T>(
        &mut self,
        v: &[T],
        mut f: impl FnMut(&mut Self, &T) -> Result<(), String>,
    ) -> Result<(), String> {
        self.u32(u32::try_from(v.len()).map_err(|_| "visual list width overflow")?)?;
        for item in v {
            f(self, item)?;
        }
        Ok(())
    }
}
fn channel(t: u8) -> BgaChannel {
    match t {
        0 => BgaChannel::Base,
        1 => BgaChannel::Poor,
        2 => BgaChannel::Layer,
        _ => BgaChannel::Layer2,
    }
}
fn channel_tag(t: BgaChannel) -> u8 {
    match t {
        BgaChannel::Base => 0,
        BgaChannel::Poor => 1,
        BgaChannel::Layer => 2,
        BgaChannel::Layer2 => 3,
    }
}
fn read_chart(r: &mut Reader<'_>) -> Result<PlayerChartTransfer, String> {
    let text_start = r.at;
    let title = r.text(MAX_RENDER_CHART_TEXT_BYTES, false)?;
    let artist = r.text(MAX_RENDER_CHART_TEXT_BYTES, false)?;
    if r.at - text_start - 16 > MAX_RENDER_CHART_TEXT_BYTES {
        return Err("chart metadata aggregate cap exceeded".into());
    }
    let lanes = r.list(18, 1, Reader::u8)?;
    let duration_ns = r.i64()?;
    let poor_bga_mode = match r.tag(2)? {
        0 => PoorBgaMode::Replace,
        1 => PoorBgaMode::Overlay,
        _ => PoorBgaMode::Off,
    };
    let initial_poor = r.optional(|r| Ok(ImageId(r.u16()?)))?;
    let notes = r.source_list(21, |r| {
        Ok(PlayerNote {
            object: ObjectId(r.u64()?),
            lane_index: r.u32()? as usize,
            start: Timestamp::from_nanos(r.i64()?),
            end: r.optional(|r| Ok(Timestamp::from_nanos(r.i64()?)))?,
        })
    })?;
    let mines = r.source_list(22, |r| {
        Ok(PlayerMine {
            ordinal: r.u64()?,
            lane_index: r.u32()? as usize,
            at: Timestamp::from_nanos(r.i64()?),
            damage: MineDamage::from_raw(r.u16()?)
                .map_err(|e| format!("invalid wire mine: {e:?}"))?,
        })
    })?;
    let bga = r.source_list(19, |r| {
        Ok(ScheduledBga {
            at: Timestamp::from_nanos(r.i64()?),
            channel: channel(r.tag(3)?),
            image: ImageId(r.u16()?),
            ordinal: r.u64()?,
        })
    })?;
    let opacity = r.source_list(18, |r| {
        Ok(ScheduledBgaOpacity {
            at: Timestamp::from_nanos(r.i64()?),
            channel: channel(r.tag(3)?),
            alpha: r.u8()?,
            ordinal: r.u64()?,
        })
    })?;
    Ok(PlayerChartTransfer {
        title,
        artist,
        lanes,
        notes,
        duration_ns,
        poor_bga_mode,
        mines,
        bga,
        initial_poor,
        opacity,
    })
}
fn write_chart(w: &mut Writer, c: &PlayerChartTransfer) -> Result<(), String> {
    w.text(&c.title)?;
    w.text(&c.artist)?;
    w.list(&c.lanes, |w, v| w.u8(*v))?;
    w.i64(c.duration_ns)?;
    w.u8(match c.poor_bga_mode {
        PoorBgaMode::Replace => 0,
        PoorBgaMode::Overlay => 1,
        PoorBgaMode::Off => 2,
    })?;
    w.optional(&c.initial_poor, |w, v| w.u16(v.0))?;
    w.list(&c.notes, |w, n| {
        w.u64(n.object.0)?;
        w.u32(u32::try_from(n.lane_index).map_err(|_| "lane width overflow")?)?;
        w.i64(n.start.as_nanos())?;
        w.optional(&n.end, |w, v| w.i64(v.as_nanos()))
    })?;
    w.list(&c.mines, |w, n| {
        w.u64(n.ordinal)?;
        w.u32(u32::try_from(n.lane_index).map_err(|_| "lane width overflow")?)?;
        w.i64(n.at.as_nanos())?;
        w.u16(n.damage.raw())
    })?;
    w.list(&c.bga, |w, n| {
        w.i64(n.at.as_nanos())?;
        w.u8(channel_tag(n.channel))?;
        w.u16(n.image.0)?;
        w.u64(n.ordinal)
    })?;
    w.list(&c.opacity, |w, n| {
        w.i64(n.at.as_nanos())?;
        w.u8(channel_tag(n.channel))?;
        w.u8(n.alpha)?;
        w.u64(n.ordinal)
    })
}
fn read_images(r: &mut Reader<'_>) -> Result<ImageAssetsTransfer, String> {
    let count = r.count(r.limits.image.max_images * 3, 16)?;
    let mut resources = Vec::new();
    if !r.scan {
        resources
            .try_reserve_exact(count)
            .map_err(|e| e.to_string())?;
    }
    for _ in 0..count {
        let width = r.u32()?;
        let height = r.u32()?;
        let n = r.u64()?;
        if width == 0
            || height == 0
            || width > r.limits.image.decode.max_width
            || height > r.limits.image.decode.max_height
            || u64::from(width)
                .checked_mul(u64::from(height))
                .and_then(|n| n.checked_mul(4))
                != Some(n)
            || n > r.limits.image.decode.max_decoded_bytes
        {
            return Err("wire image extent invalid".into());
        }
        r.rgba = r
            .rgba
            .checked_add(n)
            .filter(|n| *n <= r.limits.image.max_decoded_bytes)
            .ok_or("wire image bank budget exceeded")?;
        let pixels = r.take(usize::try_from(n).map_err(|_| "image byte width overflow")?)?;
        if !r.scan {
            let mut owned = Vec::new();
            owned
                .try_reserve_exact(pixels.len())
                .map_err(|e| e.to_string())?;
            owned.extend_from_slice(pixels);
            resources.push(Arc::new(RgbaImage::new(width, height, owned)?));
        }
    }
    let max = r.limits.image.max_images;
    let sources = r.list(max, 4, |r| Ok(r.u32()? as usize))?;
    let source_ids = r.list(max, 3, |r| {
        Ok((ImageId(r.u16()?), r.optional(|r| Ok(r.u32()? as usize))?))
    })?;
    let images = r.list(max, 6, |r| Ok((ImageId(r.u16()?), r.u32()? as usize)))?;
    let layers = r.list(max, 6, |r| Ok((ImageId(r.u16()?), r.u32()? as usize)))?;
    let unavailable = r.list(max, 3, |r| {
        let id = ImageId(r.u16()?);
        let reason = match r.tag(3)? {
            0 => ImageUnavailable::Undefined,
            1 => ImageUnavailable::Missing,
            2 => ImageUnavailable::Unsupported,
            _ => ImageUnavailable::InvalidData(r.text(r.limits.max_diagnostic_bytes, true)?),
        };
        Ok((id, reason))
    })?;
    Ok(ImageAssetsTransfer {
        resources,
        sources,
        source_ids,
        images,
        layers,
        unavailable,
    })
}
fn write_images(w: &mut Writer, i: &ImageAssetsTransfer) -> Result<(), String> {
    w.list(&i.resources, |w, i| {
        w.u32(i.width())?;
        w.u32(i.height())?;
        w.u64(i.byte_len())?;
        w.put(i.pixels())
    })?;
    w.list(&i.sources, |w, v| {
        w.u32(u32::try_from(*v).map_err(|_| "source index width overflow")?)
    })?;
    w.list(&i.source_ids, |w, (id, v)| {
        w.u16(id.0)?;
        w.optional(v, |w, v| {
            w.u32(u32::try_from(*v).map_err(|_| "alias width overflow")?)
        })
    })?;
    for aliases in [&i.images, &i.layers] {
        w.list(aliases, |w, (id, v)| {
            w.u16(id.0)?;
            w.u32(u32::try_from(*v).map_err(|_| "alias width overflow")?)
        })?;
    }
    w.list(&i.unavailable, |w, (id, v)| {
        w.u16(id.0)?;
        match v {
            ImageUnavailable::Undefined => w.u8(0),
            ImageUnavailable::Missing => w.u8(1),
            ImageUnavailable::Unsupported => w.u8(2),
            ImageUnavailable::InvalidData(v) => {
                w.u8(3)?;
                w.text(v)
            }
        }
    })
}
fn read_timing(r: &mut Reader<'_>) -> Result<TimingRecord, String> {
    Ok(TimingRecord {
        count: r.u64()?,
        early: r.u64()?,
        late: r.u64()?,
        exact: r.u64()?,
        sum: r.i128()?,
        absolute_sum: r.u128()?,
        last: r.optional(Reader::i64)?,
        min: r.optional(Reader::i64)?,
        max: r.optional(Reader::i64)?,
    })
}
fn write_timing(w: &mut Writer, t: &TimingRecord) -> Result<(), String> {
    for n in [t.count, t.early, t.late, t.exact] {
        w.u64(n)?;
    }
    w.put(&t.sum.to_le_bytes())?;
    w.put(&t.absolute_sum.to_le_bytes())?;
    for n in [&t.last, &t.min, &t.max] {
        w.optional(n, |w, n| w.i64(*n))?;
    }
    Ok(())
}
fn read_gauge(r: &mut Reader<'_>) -> Result<GaugeSnapshot, String> {
    Ok(GaugeSnapshot {
        level_units: r.u64()?,
        failure: match r.tag(2)? {
            0 => None,
            1 => Some(GaugeFailure::InstantDeath),
            _ => Some(GaugeFailure::Depleted),
        },
    })
}
fn write_gauge(w: &mut Writer, g: &GaugeSnapshot) -> Result<(), String> {
    w.u64(g.level_units)?;
    w.u8(match g.failure {
        None => 0,
        Some(GaugeFailure::InstantDeath) => 1,
        Some(GaugeFailure::Depleted) => 2,
    })
}
fn read_progress(r: &mut Reader<'_>) -> Result<Progress, String> {
    let p = Progress {
        song_ns: r.i64()?,
        hits: r.u64()?,
        misses: r.u64()?,
        combo: r.u64()?,
        max_combo: r.u64()?,
    };
    crate::multiplayer_protocol::validate_progress(None, p).map_err(|e| e.to_string())?;
    Ok(p)
}
fn write_progress(w: &mut Writer, p: &Progress) -> Result<(), String> {
    w.i64(p.song_ns)?;
    for n in [p.hits, p.misses, p.combo, p.max_combo] {
        w.u64(n)?;
    }
    Ok(())
}
fn read_comparison(r: &mut Reader<'_>) -> Result<CompetitionSnapshot, String> {
    let ghosts = r.list(8, 42, |r| {
        Ok(GhostSnapshot {
            kind: if r.bool()? {
                OpponentKind::Other
            } else {
                OpponentKind::Own
            },
            label: r.text(256, false)?,
            hits: r.u64()?,
            misses: r.u64()?,
            combo: r.u64()?,
            max_combo: r.u64()?,
            recorded_until: r.optional(|r| Ok(Timestamp::from_nanos(r.i64()?)))?,
        })
    })?;
    let network = r.optional(|r| {
        Ok(NetworkSnapshot {
            status: match r.tag(3)? {
                0 => NetworkStatus::Waiting,
                1 => NetworkStatus::Connected,
                2 => NetworkStatus::Disconnected,
                _ => NetworkStatus::Stopped,
            },
            progress: r.optional(read_progress)?,
        })
    })?;
    Ok(CompetitionSnapshot { ghosts, network })
}
fn write_comparison(w: &mut Writer, c: &CompetitionSnapshot) -> Result<(), String> {
    w.list(&c.ghosts, |w, g| {
        w.bool(g.kind == OpponentKind::Other)?;
        w.text(&g.label)?;
        for n in [g.hits, g.misses, g.combo, g.max_combo] {
            w.u64(n)?;
        }
        w.optional(&g.recorded_until, |w, n| w.i64(n.as_nanos()))
    })?;
    w.optional(&c.network, |w, n| {
        w.u8(match n.status {
            NetworkStatus::Waiting => 0,
            NetworkStatus::Connected => 1,
            NetworkStatus::Disconnected => 2,
            NetworkStatus::Stopped => 3,
        })?;
        w.optional(&n.progress, write_progress)
    })
}
fn read_score(r: &mut Reader<'_>) -> Result<ArchivedScore, String> {
    Ok(ArchivedScore {
        hits: r.u64()?,
        misses: r.u64()?,
        combo: r.u64()?,
        max_combo: r.u64()?,
        timing: read_timing(r)?,
        grades: r.list(4096, 12, |r| Ok((r.u32()?, r.u64()?)))?,
    })
}
fn write_score(w: &mut Writer, s: &ArchivedScore) -> Result<(), String> {
    for n in [s.hits, s.misses, s.combo, s.max_combo] {
        w.u64(n)?;
    }
    write_timing(w, &s.timing)?;
    w.list(&s.grades, |w, (g, n)| {
        w.u32(*g)?;
        w.u64(*n)
    })
}
fn read_result(r: &mut Reader<'_>) -> Result<ArchivedResult, String> {
    let scope = if r.bool()? {
        PlayResultScope::PracticeSection {
            start: Timestamp::from_nanos(r.i64()?),
            end: r.optional(|r| Ok(Timestamp::from_nanos(r.i64()?)))?,
        }
    } else {
        PlayResultScope::FullSong
    };
    let outcome = match r.tag(3)? {
        0 => PlayResultOutcome::Cleared,
        1 => PlayResultOutcome::BelowClearThreshold,
        2 => PlayResultOutcome::Failed(GaugeFailure::InstantDeath),
        _ => PlayResultOutcome::Failed(GaugeFailure::Depleted),
    };
    Ok(ArchivedResult {
        scope,
        outcome,
        gauge: read_gauge(r)?,
    })
}
fn write_result(w: &mut Writer, v: &ArchivedResult) -> Result<(), String> {
    match v.scope {
        PlayResultScope::FullSong => w.bool(false)?,
        PlayResultScope::PracticeSection { start, end } => {
            w.bool(true)?;
            w.i64(start.as_nanos())?;
            w.optional(&end, |w, n| w.i64(n.as_nanos()))?;
        }
    }
    w.u8(match v.outcome {
        PlayResultOutcome::Cleared => 0,
        PlayResultOutcome::BelowClearThreshold => 1,
        PlayResultOutcome::Failed(GaugeFailure::InstantDeath) => 2,
        PlayResultOutcome::Failed(GaugeFailure::Depleted) => 3,
    })?;
    write_gauge(w, &v.gauge)
}
fn read_lobby(r: &mut Reader<'_>) -> Result<Option<Arc<RoomLobby>>, String> {
    let participant = r.optional(|r| Ok(ParticipantId(r.u64()?)))?;
    let revision = r.u64()?;
    let phase = r.optional(|r| {
        Ok(match r.tag(2)? {
            0 => GroupRoomPhase::Collecting,
            1 => GroupRoomPhase::Frozen,
            _ => GroupRoomPhase::Prepared,
        })
    })?;
    let deadline_ns = r.optional(Reader::i64)?;
    let members = r.list(64, 13, |r| {
        Ok(GroupRoomMember {
            id: ParticipantId(r.u64()?),
            prepared: r.bool()?,
            players: r.list(64, 4, |r| Ok(PlayerId(r.u32()?)))?,
        })
    })?;
    if r.scan {
        Ok(None)
    } else {
        Ok(Some(Arc::new(RoomLobby::new(
            participant,
            revision,
            phase,
            deadline_ns,
            members,
        )?)))
    }
}
fn write_lobby(w: &mut Writer, l: &RoomLobby) -> Result<(), String> {
    w.optional(&l.participant, |w, n| w.u64(n.0))?;
    w.u64(l.revision)?;
    w.optional(&l.phase, |w, p| {
        w.u8(match p {
            GroupRoomPhase::Collecting => 0,
            GroupRoomPhase::Frozen => 1,
            GroupRoomPhase::Prepared => 2,
        })
    })?;
    w.optional(&l.deadline_ns, |w, n| w.i64(*n))?;
    w.list(&l.members, |w, m| {
        w.u64(m.id.0)?;
        w.bool(m.prepared)?;
        w.list(&m.players, |w, p| w.u32(p.0))
    })
}
fn read_room_page(
    r: &mut Reader<'_>,
    lobby: Option<Arc<RoomLobby>>,
) -> Result<Option<RoomPresentation>, String> {
    let status = match r.tag(3)? {
        0 => RoomStatus::Waiting,
        1 => RoomStatus::Connected,
        2 => RoomStatus::Disconnected,
        _ => RoomStatus::Closed,
    };
    let page = r.u32()? as usize;
    let pages = r.u32()? as usize;
    let failed = r.bool()?;
    let heading = r.text(256, false)?;
    let error = r.optional(|r| r.text(4096, true))?;
    let rows = r.list(4, 38, |r| {
        Ok(RoomScoreRow {
            participant: ParticipantId(r.u64()?),
            player: PlayerId(r.u32()?),
            progress: r.optional(read_progress)?,
            final_prefix: r.bool()?,
            label: r.text(128, false)?,
            counters: [r.text(128, false)?, r.text(128, false)?],
        })
    })?;
    if r.scan {
        Ok(None)
    } else {
        Ok(Some(RoomPresentation {
            lobby: lobby.ok_or("missing validated lobby")?,
            status,
            page,
            pages,
            heading,
            rows,
            error,
            failed,
        }))
    }
}
fn write_room_page(w: &mut Writer, r: &RoomPresentation) -> Result<(), String> {
    w.u8(match r.status {
        RoomStatus::Waiting => 0,
        RoomStatus::Connected => 1,
        RoomStatus::Disconnected => 2,
        RoomStatus::Closed => 3,
    })?;
    w.u32(u32::try_from(r.page).map_err(|_| "room page width overflow")?)?;
    w.u32(u32::try_from(r.pages).map_err(|_| "room page count overflow")?)?;
    w.bool(r.failed)?;
    w.text(&r.heading)?;
    w.optional(&r.error, |w, t| w.text(t))?;
    w.list(&r.rows, |w, r| {
        w.u64(r.participant.0)?;
        w.u32(r.player.0)?;
        w.optional(&r.progress, write_progress)?;
        w.bool(r.final_prefix)?;
        w.text(&r.label)?;
        w.text(&r.counters[0])?;
        w.text(&r.counters[1])
    })
}
fn validate_event(e: &RenderJudgeEvent) -> Result<(), String> {
    if e.stage > 3
        || (e.stage != 3 && e.custom_stage != 0)
        || e.outcome > 1
        || (e.outcome == 1 && (e.grade_or_reason > 3 || e.delta_ns != 0))
    {
        return Err("invalid visual event tag".into());
    }
    Ok(())
}
fn read_frame(r: &mut Reader<'_>, h: WireHeader) -> Result<RenderFrame, String> {
    let page = r.u32()?;
    let lookahead_ns = r.i64()?;
    let room_disabled = r.bool()?;
    let members = r.list(MAX_RENDER_VISIBLE, 32, |r| {
        let player = PlayerId(r.u32()?);
        let song_ns = r.i64()?;
        let pressed = r.u32()?;
        let saved_comparison_height = r.u32()?;
        let peer_admitted = r.bool()?;
        let saved_failed = r.bool()?;
        let peer_failed = r.bool()?;
        let score = r.optional(|r| {
            Ok(RenderScore {
                hits: r.u64()?,
                misses: r.u64()?,
                combo: r.u64()?,
                max_combo: r.u64()?,
                timing: read_timing(r)?,
            })
        })?;
        let gauge = r.optional(|r| {
            Ok(RenderGauge {
                snapshot: read_gauge(r)?,
                clear_units: r.u64()?,
            })
        })?;
        let competition = r.optional(read_comparison)?;
        let recent = r.list(MAX_RENDER_RECENT, 40, |r| {
            let event = RenderJudgeEvent {
                object: r.u64()?,
                stage: r.u32()?,
                custom_stage: r.u32()?,
                outcome: r.u32()?,
                grade_or_reason: r.u32()?,
                delta_ns: r.i64()?,
                at_ns: r.i64()?,
            };
            validate_event(&event)?;
            Ok(event)
        })?;
        let last_miss_ns = r.optional(Reader::i64)?;
        let mut previous_page = None;
        let pages = r.list(
            beatkernel::chart::MAX_SOURCE_ITEMS.div_ceil(4096),
            1036,
            |r| {
                let index = r.u32()?;
                let valid_count = r.u32()?;
                let completed_count = r.u32()?;
                if index as usize >= beatkernel::chart::MAX_SOURCE_ITEMS.div_ceil(4096)
                    || valid_count == 0
                    || valid_count > 4096
                    || completed_count > valid_count
                    || previous_page.is_some_and(|p| p >= index)
                {
                    return Err("invalid visual page extent or order".into());
                }
                previous_page = Some(index);
                let mut packed_states = Vec::new();
                if !r.scan {
                    packed_states
                        .try_reserve_exact(128)
                        .map_err(|e| e.to_string())?;
                }
                let mut completed = 0u32;
                for word_index in 0..128 {
                    let word = r.u64()?;
                    for bit in 0..32 {
                        let slot = word_index * 32 + bit;
                        let state = (word >> (bit * 2)) & 3;
                        if state == 3 || (slot >= valid_count as usize && state != 0) {
                            return Err("invalid visual page state or padding".into());
                        }
                        if slot < valid_count as usize && state == 2 {
                            completed += 1;
                        }
                    }
                    if !r.scan {
                        packed_states.push(word);
                    }
                }
                if completed != completed_count {
                    return Err("visual completed page count mismatch".into());
                }
                Ok(RenderProgressPage {
                    index,
                    valid_count,
                    completed_count,
                    packed_states,
                })
            },
        )?;
        Ok(RenderMemberUpdate {
            player,
            scalars: RenderMemberScalars {
                song_ns,
                pressed,
                recent,
                score,
                gauge,
                competition,
                saved_comparison_height,
                peer_admitted,
                saved_failed,
                peer_failed,
            },
            pages,
            last_miss_ns,
        })
    })?;
    let room = r
        .optional(|r| {
            let lobby = read_lobby(r)?;
            read_room_page(r, lobby)
        })?
        .flatten();
    Ok(RenderFrame {
        generation: h.generation,
        content: h.content,
        sequence: h.sequence,
        page,
        lookahead_ns,
        members,
        room,
        room_disabled,
    })
}
fn write_frame(w: &mut Writer, f: &RenderFrame) -> Result<(), String> {
    w.u32(f.page)?;
    w.i64(f.lookahead_ns)?;
    w.bool(f.room_disabled)?;
    w.list(&f.members, |w, m| {
        let s = &m.scalars;
        w.u32(m.player.0)?;
        w.i64(s.song_ns)?;
        w.u32(s.pressed)?;
        w.u32(s.saved_comparison_height)?;
        w.bool(s.peer_admitted)?;
        w.bool(s.saved_failed)?;
        w.bool(s.peer_failed)?;
        w.optional(&s.score, |w, s| {
            for n in [s.hits, s.misses, s.combo, s.max_combo] {
                w.u64(n)?;
            }
            write_timing(w, &s.timing)
        })?;
        w.optional(&s.gauge, |w, g| {
            write_gauge(w, &g.snapshot)?;
            w.u64(g.clear_units)
        })?;
        w.optional(&s.competition, write_comparison)?;
        w.list(&s.recent, |w, e| {
            w.u64(e.object)?;
            for n in [e.stage, e.custom_stage, e.outcome, e.grade_or_reason] {
                w.u32(n)?;
            }
            w.i64(e.delta_ns)?;
            w.i64(e.at_ns)
        })?;
        w.optional(&m.last_miss_ns, |w, n| w.i64(*n))?;
        w.list(&m.pages, |w, p| {
            w.u32(p.index)?;
            w.u32(p.valid_count)?;
            w.u32(p.completed_count)?;
            for n in &p.packed_states {
                w.u64(*n)?;
            }
            Ok(())
        })
    })?;
    w.optional(&f.room, |w, r| {
        write_lobby(w, &r.lobby)?;
        write_room_page(w, r)
    })
}
fn read_body(r: &mut Reader<'_>, h: WireHeader) -> Result<Option<WirePacket>, String> {
    let packet = match h.kind {
        REGISTRATION => {
            let chart = read_chart(r)?;
            let images = read_images(r)?;
            let roster = r.list(MAX_RENDER_PLAYERS, 4, |r| Ok(PlayerId(r.u32()?)))?;
            WirePacket::Registration(VisualRegistration {
                chart,
                images,
                roster,
            })
        }
        FRAME => WirePacket::Frame(read_frame(r, h)?),
        PREVIEW => WirePacket::Preview(PreviewState {
            song_ns: r.i64()?,
            lookahead_ns: r.i64()?,
        }),
        HISTORY => {
            let value = (PlayerId(r.u32()?), read_result(r)?);
            let score = r.optional(read_score)?;
            let bms_score = r.optional(|r| {
                Ok(BmsScoreSummary {
                    pgreat: r.u64()?,
                    great: r.u64()?,
                    good: r.u64()?,
                    bad: r.u64()?,
                    poor: r.u64()?,
                    ex_score: r.u64()?,
                })
            })?;
            let comparison = r.optional(|r| r.optional(read_comparison))?;
            let grade_page = r.u32()? as usize;
            WirePacket::History(FrozenHistoricalRecord {
                value,
                score,
                bms_score,
                comparison,
                grade_page,
            })
        }
        RESULTS => {
            let roster = r.list(MAX_RENDER_PLAYERS, 4, |r| Ok(PlayerId(r.u32()?)))?;
            let rows = r.list(MAX_RENDER_PLAYERS, 15, |r| {
                Ok(FrozenResultRow {
                    player: PlayerId(r.u32()?),
                    result: read_result(r)?,
                })
            })?;
            let details = r.list(MAX_RENDER_PLAYERS, 76, |r| {
                Ok(FrozenScoreDetails {
                    player: PlayerId(r.u32()?),
                    score: read_score(r)?,
                    competition: r.optional(read_comparison)?,
                })
            })?;
            WirePacket::Results(FrozenResultsModel {
                roster,
                rows,
                details,
            })
        }
        ROOM => {
            let lobby = read_lobby(r)?;
            let initial_page = r.u32()? as usize;
            let count = r.count(1008, 23)?;
            if count == 0 || initial_page >= count {
                return Err("frozen room page selection invalid".into());
            }
            let mut pages = Vec::new();
            if !r.scan {
                pages.try_reserve_exact(count).map_err(|e| e.to_string())?;
            }
            for _ in 0..count {
                if let Some(page) = read_room_page(r, lobby.clone())? {
                    pages.push(page);
                }
            }
            WirePacket::Room(FrozenRoomResults {
                pages,
                initial_page,
            })
        }
        _ => return Err("unknown visual packet kind".into()),
    };
    if r.scan { Ok(None) } else { Ok(Some(packet)) }
}
fn write_body(w: &mut Writer, p: &WirePacket) -> Result<(), String> {
    match p {
        WirePacket::Registration(r) => {
            write_chart(w, &r.chart)?;
            write_images(w, &r.images)?;
            w.list(&r.roster, |w, p| w.u32(p.0))
        }
        WirePacket::Frame(f) => write_frame(w, f),
        WirePacket::Preview(p) => {
            w.i64(p.song_ns)?;
            w.i64(p.lookahead_ns)
        }
        WirePacket::History(h) => {
            w.u32(h.value.0.0)?;
            write_result(w, &h.value.1)?;
            w.optional(&h.score, write_score)?;
            w.optional(&h.bms_score, |w, s| {
                for n in [s.pgreat, s.great, s.good, s.bad, s.poor, s.ex_score] {
                    w.u64(n)?;
                }
                Ok(())
            })?;
            w.optional(&h.comparison, |w, c| w.optional(c, write_comparison))?;
            w.u32(u32::try_from(h.grade_page).map_err(|_| "history page width overflow")?)
        }
        WirePacket::Results(r) => {
            w.list(&r.roster, |w, p| w.u32(p.0))?;
            w.list(&r.rows, |w, r| {
                w.u32(r.player.0)?;
                write_result(w, &r.result)
            })?;
            w.list(&r.details, |w, d| {
                w.u32(d.player.0)?;
                write_score(w, &d.score)?;
                w.optional(&d.competition, write_comparison)
            })
        }
        WirePacket::Room(r) => {
            write_lobby(w, &r.pages[0].lobby)?;
            w.u32(u32::try_from(r.initial_page).map_err(|_| "room initial page width overflow")?)?;
            w.list(&r.pages, write_room_page)
        }
    }
}
fn validate_packet(h: WireHeader, p: &WirePacket, limits: WireLimits) -> Result<(), String> {
    if h.kind != p.kind() || h.generation == 0 || h.content == 0 {
        return Err("visual packet/header context mismatch".into());
    }
    // Charge each transported diagnostic, including repeated frozen-page errors.
    // History, Results and comparisons contain display labels, not diagnostics.
    let diagnostic_bytes = match p {
        WirePacket::Registration(r) => {
            r.images
                .unavailable
                .iter()
                .try_fold(0usize, |bytes, (_, reason)| {
                    bytes.checked_add(match reason {
                        ImageUnavailable::InvalidData(text) => text.len(),
                        _ => 0,
                    })
                })
        }
        WirePacket::Frame(frame) => Some(
            frame
                .room
                .as_ref()
                .and_then(|room| room.error.as_ref())
                .map_or(0, String::len),
        ),
        WirePacket::Room(room) => room.pages.iter().try_fold(0usize, |bytes, page| {
            bytes.checked_add(page.error.as_ref().map_or(0, String::len))
        }),
        _ => Some(0),
    };
    diagnostic_bytes
        .filter(|bytes| *bytes <= limits.max_diagnostic_bytes)
        .ok_or("visual diagnostic allowance exceeded")?;
    match p {
        WirePacket::Registration(r) => {
            if r.roster.len() > MAX_RENDER_PLAYERS
                || r.roster
                    .iter()
                    .enumerate()
                    .any(|(i, p)| p.0 == 0 || r.roster[..i].contains(p))
            {
                return Err("invalid actual visual roster".into());
            }
            // A preview has no player; the chart/image caps remain identical.
            if r.chart
                .title
                .len()
                .checked_add(r.chart.artist.len())
                .is_none_or(|n| n > MAX_RENDER_CHART_TEXT_BYTES)
            {
                return Err("chart metadata aggregate cap exceeded".into());
            }
            if !r.roster.is_empty() {
                visual_registration_bytes(&r.chart, &r.images, &r.roster)?;
            }
            PlayerChart::import_visual(r.chart.clone()).map_err(|e| e.to_string())?;
            ImageAssets::import_visual(r.images.clone(), limits.image)?;
        }
        WirePacket::Frame(f) => {
            if (f.generation, f.content, f.sequence) != (h.generation, h.content, h.sequence)
                || f.lookahead_ns <= 0
                || (f.room.is_some() && f.room_disabled)
            {
                return Err("visual frame/header context mismatch".into());
            }
            f.encoded_bytes()?;
            if let Some(room) = &f.room {
                room.validate()?;
            }
            for (i, m) in f.members.iter().enumerate() {
                if m.player.0 == 0 || f.members[..i].iter().any(|p| p.player == m.player) {
                    return Err("duplicate visual frame player".into());
                }
                if let Some(s) = &m.scalars.score {
                    s.validate()?;
                }
                if let Some(g) = &m.scalars.gauge {
                    g.validate()?;
                }
                if m.scalars.saved_comparison_height > 112
                    || m.scalars.saved_comparison_height % 14 != 0
                    || (m.scalars.peer_failed && !m.scalars.peer_admitted)
                {
                    return Err("invalid frame scalar reservation".into());
                }
                for e in &m.scalars.recent {
                    validate_event(e)?;
                }
                for (i, p) in m.pages.iter().enumerate() {
                    if p.index as usize >= beatkernel::chart::MAX_SOURCE_ITEMS.div_ceil(4096)
                        || p.valid_count == 0
                        || p.valid_count > 4096
                        || p.completed_count > p.valid_count
                        || (i > 0 && m.pages[i - 1].index >= p.index)
                    {
                        return Err("invalid visual page extent or order".into());
                    }
                    let mut completed = 0u32;
                    for slot in 0..4096usize {
                        let state = (p.packed_states[slot / 32] >> ((slot % 32) * 2)) & 3;
                        if (slot >= p.valid_count as usize && state != 0) || state == 3 {
                            return Err("invalid visual page state or padding".into());
                        }
                        if slot < p.valid_count as usize && state == 2 {
                            completed += 1;
                        }
                    }
                    if completed != p.completed_count {
                        return Err("visual completed page count mismatch".into());
                    }
                }
            }
        }
        WirePacket::Preview(p) => {
            if p.song_ns < 0
                || p.lookahead_ns <= 0
                || p.song_ns.checked_add(p.lookahead_ns).is_none()
            {
                return Err("invalid preview extent".into());
            }
        }
        WirePacket::History(h) => h.validate()?,
        WirePacket::Results(r) => r.validate()?,
        WirePacket::Room(r) => r.validate()?,
    }
    Ok(())
}
pub fn encode_packet(
    mut header: WireHeader,
    packet: &WirePacket,
    limits: WireLimits,
) -> Result<Vec<u8>, String> {
    let cap = kind_limit(header.kind, limits)?
        .checked_sub(HEADER_BYTES)
        .ok_or("visual packet budget below envelope")?;
    validate_packet(header, packet, limits)?;
    let mut measure = Writer {
        bytes: Vec::new(),
        length: 0,
        measure: true,
        cap,
    };
    write_body(&mut measure, packet)?;
    header.payload_len = measure.length as u64;
    let mut w = Writer {
        bytes: Vec::new(),
        length: 0,
        measure: false,
        cap: cap + HEADER_BYTES,
    };
    w.bytes
        .try_reserve_exact(HEADER_BYTES + measure.length)
        .map_err(|e| e.to_string())?;
    w.put(&MAGIC)?;
    w.u16(VERSION)?;
    w.u16(header.kind)?;
    w.u64(header.generation)?;
    w.u64(header.content)?;
    w.u64(header.sequence)?;
    w.u64(header.payload_len)?;
    write_body(&mut w, packet)?;
    Ok(w.bytes)
}
pub fn decode_packet(bytes: &[u8], limits: WireLimits) -> Result<(WireHeader, WirePacket), String> {
    let header = preflight_header(
        bytes
            .get(..HEADER_BYTES)
            .ok_or("truncated visual envelope")?,
        bytes.len(),
        limits,
    )?;
    let body = &bytes[HEADER_BYTES..];
    let mut scan = Reader::new(body, true, limits);
    let _ = read_body(&mut scan, header)?;
    if scan.at != body.len() {
        return Err("trailing visual packet bytes".into());
    }
    let mut reader = Reader::new(body, false, limits);
    let packet = read_body(&mut reader, header)?.ok_or("missing decoded visual packet")?;
    validate_packet(header, &packet, limits)?;
    Ok((header, packet))
}
