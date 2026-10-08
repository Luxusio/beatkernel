//! Bounded browser menu intent and drafts. Rendering and platform effects are external.
use crate::screen_lifecycle::{ScreenInstanceId, ScreenNavigator, ScreenPhase, ScreenRoute};

pub const MAX_MENU_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_MENU_FIELDS: usize = 8192;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuToken {
    pub generation: u64,
    pub screen: ScreenInstanceId,
    pub revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuSnapshot {
    pub token: MenuToken,
    pub route: ScreenRoute,
    pub fields: Vec<String>,
    pub selected: usize,
    pub pending: bool,
    pub error: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuEffect {
    None,
    Apply,
    Load,
    Save,
    Start,
    Select(usize),
    Bridge(u64),
}

pub struct BrowserMenu {
    generation: u64,
    revision: u64,
    navigator: ScreenNavigator,
    drafts: Vec<(ScreenInstanceId, Vec<String>, usize)>,
    pending: bool,
    error: Option<String>,
    last_action: u64,
}
impl BrowserMenu {
    pub fn touch(&mut self, token: MenuToken) -> Result<(), String> {
        self.admit(token)?;
        self.revision += 1;
        Ok(())
    }
    pub fn token(&self) -> MenuToken {
        MenuToken {
            generation: self.generation,
            screen: self.navigator.active_id().unwrap_or(ScreenInstanceId(0)),
            revision: self.revision,
        }
    }
    pub fn route(&self) -> ScreenRoute {
        self.navigator.route()
    }
    pub fn selected(&self) -> usize {
        self.drafts
            .iter()
            .find(|draft| Some(draft.0) == self.navigator.active_id())
            .map_or(0, |draft| draft.2)
    }
    pub fn fields(&self) -> &[String] {
        self.drafts
            .iter()
            .find(|draft| Some(draft.0) == self.navigator.active_id())
            .map_or(&[], |draft| draft.1.as_slice())
    }
    pub fn new(generation: u64) -> Result<Self, String> {
        if generation == 0 {
            return Err("menu generation is zero".into());
        }
        Ok(Self {
            generation,
            revision: 1,
            navigator: ScreenNavigator::default(),
            drafts: vec![(ScreenInstanceId(1), Vec::new(), 0)],
            pending: false,
            error: None,
            last_action: 0,
        })
    }
    pub fn snapshot(&self) -> MenuSnapshot {
        let screen = self.navigator.active_id().unwrap_or(ScreenInstanceId(0));
        let draft = self.drafts.iter().find(|draft| draft.0 == screen);
        MenuSnapshot {
            token: MenuToken {
                generation: self.generation,
                screen,
                revision: self.revision,
            },
            route: self.navigator.route(),
            fields: draft.map_or_else(Vec::new, |draft| draft.1.clone()),
            selected: draft.map_or(0, |draft| draft.2),
            pending: self.pending,
            error: self.error.clone(),
        }
    }
    fn admit(&self, token: MenuToken) -> Result<(), String> {
        if token.generation != self.generation
            || token.revision != self.revision
            || !self.navigator.accepts(token.screen)
        {
            return Err("stale or inactive menu action".into());
        }
        if self.revision == u64::MAX {
            return Err("menu revision exhausted".into());
        }
        Ok(())
    }
    pub fn navigate(&mut self, token: MenuToken, route: ScreenRoute) -> Result<(), String> {
        self.admit(token)?;
        if matches!(
            route,
            ScreenRoute::Play { .. }
                | ScreenRoute::Results { .. }
                | ScreenRoute::LiveAudio
                | ScreenRoute::Closing
        ) {
            return Err("menu navigation has no gameplay completion authority".into());
        }
        // Clone only the bounded four-entry navigator. Refusal preserves drafts.
        let mut next = self.navigator.clone();
        if next.navigate(route, self.pending, true)?.is_none() {
            return Ok(());
        }
        let screen = next.active_id();
        self.drafts.retain(|draft| next.retains(draft.0));
        if let Some(id) = screen {
            if !self.drafts.iter().any(|draft| draft.0 == id) {
                let values = if route == ScreenRoute::Practice && self.fields().len() == 13 {
                    vec![self.fields()[11].clone(), self.fields()[12].clone()]
                } else {
                    Vec::new()
                };
                self.drafts.push((id, values, 0));
            }
        }
        self.navigator = next;
        self.error = None;
        self.revision += 1;
        Ok(())
    }
    pub fn back(&mut self, token: MenuToken) -> Result<(), String> {
        self.admit(token)?;
        let route = self
            .navigator
            .back_target()
            .ok_or("menu has no Back target")?;
        self.navigate(token, route)
    }
    /// Admits a destination and its complete value model in one transaction.
    pub fn navigate_with_fields(
        &mut self,
        token: MenuToken,
        route: ScreenRoute,
        values: Vec<String>,
    ) -> Result<(), String> {
        self.admit(token)?;
        validate_fields(&values)?;
        match route {
            ScreenRoute::Selection => {}
            ScreenRoute::Settings if values.len() <= crate::settings::MAX_FIELDS => {}
            ScreenRoute::Practice if values.len() == 2 => {}
            ScreenRoute::Display if values.len() == 4 => {}
            ScreenRoute::Records if values.len() <= 256 => {}
            ScreenRoute::Players | ScreenRoute::Devices { .. } => {
                decode_local_fields(&values, route == ScreenRoute::Players)?;
            }
            _ => return Err("invalid or unsupported destination menu fields".into()),
        }
        let mut next = self.navigator.clone();
        let transition = next.navigate(route, self.pending, true)?;
        let screen = next
            .active_id()
            .ok_or("destination menu instance unavailable")?;
        let existing = self.drafts.iter().find(|draft| draft.0 == screen);
        if transition.is_none() && existing.is_some_and(|draft| draft.1 == values) {
            return Ok(());
        }
        if existing.is_none() {
            self.drafts
                .try_reserve(1)
                .map_err(|_| "menu destination allocation failed")?;
        }
        // Everything fallible is staged. Move the admitted values into the destination.
        self.drafts.retain(|draft| next.retains(draft.0));
        if let Some(draft) = self.drafts.iter_mut().find(|draft| draft.0 == screen) {
            draft.2 = draft.2.min(values.len().saturating_sub(1));
            draft.1 = values;
        } else {
            self.drafts.push((screen, values, 0));
        }
        self.navigator = next;
        if transition.is_some() {
            self.error = None;
        }
        self.revision += 1;
        Ok(())
    }
    pub fn set_fields(&mut self, token: MenuToken, values: Vec<String>) -> Result<(), String> {
        self.admit(token)?;
        validate_fields(&values)?;
        if self.pending {
            return Err("menu draft waits for pending operation".into());
        }
        let draft = self
            .drafts
            .iter_mut()
            .find(|draft| draft.0 == token.screen)
            .ok_or("menu draft unavailable")?;
        if draft.1 != values {
            draft.1 = values;
            draft.2 = draft.2.min(draft.1.len().saturating_sub(1));
            self.revision += 1;
        }
        Ok(())
    }
    pub fn select(&mut self, token: MenuToken, index: usize) -> Result<(), String> {
        self.admit(token)?;
        if self.pending {
            return Err("menu selection waits for pending operation".into());
        }
        let draft = self
            .drafts
            .iter_mut()
            .find(|draft| draft.0 == token.screen)
            .ok_or("menu draft unavailable")?;
        if index >= draft.1.len() {
            return Err("menu selection exceeds fields".into());
        }
        if draft.2 != index {
            draft.2 = index;
            self.revision += 1;
        }
        Ok(())
    }
    pub fn edit(&mut self, token: MenuToken, index: usize, value: String) -> Result<(), String> {
        self.admit(token)?;
        if self.pending {
            return Err("menu edit waits for pending operation".into());
        }
        validate_fields(std::slice::from_ref(&value))?;
        let draft = self
            .drafts
            .iter_mut()
            .find(|draft| draft.0 == token.screen)
            .ok_or("menu draft unavailable")?;
        let old = draft.1.get(index).ok_or("menu field unavailable")?;
        let total = 48 + draft.1.iter().map(|value| value.len() + 4).sum::<usize>() - old.len()
            + value.len();
        if total > MAX_MENU_BYTES {
            return Err("menu edit exceeds aggregate byte limit".into());
        }
        if old != &value {
            draft.1[index] = value;
            self.revision += 1;
        }
        Ok(())
    }
    pub fn accept_action(
        &mut self,
        token: MenuToken,
        action_id: u64,
        control: u64,
    ) -> Result<MenuEffect, String> {
        self.admit(token)?;
        if action_id == 0 || action_id <= self.last_action || self.pending {
            return Err("stale or pending menu action".into());
        }
        use ScreenRoute::*;
        let route = self.navigator.route();
        let destination = match (route, control) {
            (Selection, 5) => Some(Settings),
            (Settings, 74) => Some(Practice),
            (Settings, 19) => Some(Records),
            (Settings, 18) => Some(Display),
            (Settings, 17) => Some(Players),
            (Settings, 16) => Some(Devices { players: false }),
            (Players, 34) => Some(Devices { players: true }),
            _ => None,
        };
        let effect = if let Some(destination) = destination {
            self.navigate(token, destination)?;
            MenuEffect::None
        } else if matches!(
            (route, control),
            (Settings, 11)
                | (Practice, 72)
                | (Display, 41)
                | (Records, 55)
                | (Players, 31)
                | (Devices { .. }, 21)
                | (Results { .. }, 4)
        ) {
            self.back(token)?;
            MenuEffect::None
        } else {
            match (route, control) {
                (Selection, 1) => MenuEffect::Start,
                (Selection, value) if value >= 100 && value - 100 < self.fields().len() as u64 => {
                    let index = (value - 100) as usize;
                    self.select(token, index)?;
                    MenuEffect::Select(index)
                }
                (Practice, 71) => {
                    let fields = self.fields();
                    if fields.len() != 2 {
                        return Err("practice requires a start/end draft".into());
                    }
                    let start = crate::practice::PracticeStart::parse(&fields[0])?;
                    let end = if fields[1].is_empty() {
                        None
                    } else {
                        Some(crate::practice::PracticeStart::parse(&fields[1])?)
                    };
                    if end.is_some_and(|end| end.nanoseconds() <= start.nanoseconds()) {
                        return Err("practice end must follow start".into());
                    }
                    let mut next = self.navigator.clone();
                    next.back(false, true)?;
                    let parent = next.active_id().ok_or("practice parent unavailable")?;
                    let draft = self
                        .drafts
                        .iter_mut()
                        .find(|draft| draft.0 == parent)
                        .ok_or("practice settings draft unavailable")?;
                    if draft.1.len() != 13 {
                        return Err("browser settings draft requires thirteen fields".into());
                    }
                    draft.1[11] = seconds_text(start.nanoseconds());
                    draft.1[12] =
                        end.map_or_else(String::new, |end| seconds_text(end.nanoseconds()));
                    self.drafts.retain(|draft| next.retains(draft.0));
                    self.navigator = next;
                    self.revision += 1;
                    MenuEffect::None
                }
                (Practice, 73 | 76) => {
                    let mut fields = self.fields().to_vec();
                    if fields.len() != 2 {
                        return Err("practice requires a start/end draft".into());
                    }
                    if control == 73 {
                        fields[0] = "0".into();
                        fields[1].clear();
                    } else {
                        fields[1].clear();
                    }
                    let reset_focus = control == 73 && self.selected() != 0;
                    self.set_fields(token, fields)?;
                    if reset_focus {
                        self.drafts
                            .iter_mut()
                            .find(|draft| draft.0 == token.screen)
                            .unwrap()
                            .2 = 0;
                        if self.revision == token.revision {
                            self.revision += 1;
                        }
                    }
                    MenuEffect::None
                }
                (Settings, 10) | (Display, 40) => MenuEffect::Apply,
                (Settings, 13) => MenuEffect::Load,
                (Settings, 14) => MenuEffect::Save,
                (Records, 50..=68) | (Players, 30..=37) | (Devices { .. }, 20..=24) => {
                    MenuEffect::Bridge(control)
                }
                _ => return Err("unsupported menu control".into()),
            }
        };
        self.last_action = action_id;
        Ok(effect)
    }
    pub fn suspend(&mut self) {
        self.navigator.suspend();
    }
    pub fn resume(&mut self) {
        self.navigator.resume();
    }
    pub fn dispose(&mut self) {
        let _ = self.navigator.navigate(ScreenRoute::Closing, false, true);
        self.drafts.clear();
        self.error = None;
        self.pending = false;
    }
    pub fn phase(&self) -> ScreenPhase {
        self.navigator.phase()
    }
}
fn seconds_text(ns: i64) -> String {
    let whole = ns / 1_000_000_000;
    let fraction = ns % 1_000_000_000;
    if fraction == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{fraction:09}")
            .trim_end_matches('0')
            .to_owned()
    }
}
fn validate_fields(values: &[String]) -> Result<(), String> {
    if values.len() > MAX_MENU_FIELDS {
        return Err("menu field count exceeds limit".into());
    }
    let mut bytes = 0usize;
    for value in values {
        if value.len() > 4096 || value.chars().any(char::is_control) {
            return Err("menu field exceeds text limit".into());
        }
        bytes = bytes
            .checked_add(value.len() + 4)
            .ok_or("menu byte overflow")?;
        if bytes > MAX_MENU_BYTES - 48 {
            return Err("menu exceeds aggregate byte limit".into());
        }
    }
    Ok(())
}

pub fn route_code(route: ScreenRoute) -> Result<u32, String> {
    use ScreenRoute::*;
    Ok(match route {
        Selection => 1,
        Settings => 2,
        Practice => 3,
        Records => 4,
        Players => 5,
        Devices { players: false } => 6,
        Display => 7,
        Results { replay: false } => 8,
        Devices { players: true } => 9,
        Results { replay: true } => 10,
        _ => return Err("route is not a browser menu".into()),
    })
}
pub fn route_from_code(code: u32) -> Result<ScreenRoute, String> {
    use ScreenRoute::*;
    Ok(match code {
        1 => Selection,
        2 => Settings,
        3 => Practice,
        4 => Records,
        5 => Players,
        6 => Devices { players: false },
        7 => Display,
        8 => Results { replay: false },
        9 => Devices { players: true },
        10 => Results { replay: true },
        _ => return Err("invalid browser menu route".into()),
    })
}

fn validate_snapshot(model: &MenuSnapshot) -> Result<usize, String> {
    validate_fields(&model.fields)?;
    if model.token.generation == 0
        || model.token.screen.0 == 0
        || model.token.revision == 0
        || (model.fields.is_empty() && model.selected != 0)
        || (!model.fields.is_empty() && model.selected >= model.fields.len())
    {
        return Err("invalid menu snapshot identity/selection".into());
    }
    let error = model.error.as_deref().unwrap_or("");
    validate_fields(&[error.to_owned()])?;
    let total = 48
        + model
            .fields
            .iter()
            .map(|value| 4 + value.len())
            .sum::<usize>()
        + error.len();
    if total > MAX_MENU_BYTES {
        return Err("menu snapshot exceeds byte limit".into());
    }
    route_code(model.route)?;
    Ok(total)
}
pub fn encode_snapshot(model: &MenuSnapshot) -> Result<Vec<u8>, String> {
    let total = validate_snapshot(model)?;
    let error = model.error.as_deref().unwrap_or("");
    let mut bytes = Vec::with_capacity(total);
    bytes.extend_from_slice(b"BKMN");
    for value in [
        model.token.generation,
        model.token.screen.0,
        model.token.revision,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [
        route_code(model.route)?,
        model.selected as u32,
        model.fields.len() as u32,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[u8::from(model.pending), 0, 0, 0]);
    for value in model
        .fields
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(error))
    {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }
    Ok(bytes)
}
pub fn decode_snapshot(bytes: &[u8]) -> Result<MenuSnapshot, String> {
    if !(48..=MAX_MENU_BYTES).contains(&bytes.len())
        || &bytes[..4] != b"BKMN"
        || bytes[40] > 1
        || bytes[41..44] != [0, 0, 0]
    {
        return Err("invalid menu snapshot envelope".into());
    }
    let u64_at = |at| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    let u32_at = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let count = u32_at(36) as usize;
    if count > MAX_MENU_FIELDS {
        return Err("menu field count exceeds limit".into());
    }
    let mut at = 44;
    let mut values = Vec::with_capacity(count + 1);
    for _ in 0..=count {
        let length = bytes
            .get(at..at + 4)
            .ok_or("truncated menu string length")?;
        let length = u32::from_le_bytes(length.try_into().unwrap()) as usize;
        at += 4;
        if length > 4096 {
            return Err("menu field exceeds text limit".into());
        }
        let value = std::str::from_utf8(bytes.get(at..at + length).ok_or("truncated menu string")?)
            .map_err(|_| "invalid menu UTF-8")?;
        values.push(value.to_owned());
        at += length;
    }
    if at != bytes.len() {
        return Err("trailing menu snapshot bytes".into());
    }
    let error = values.pop().filter(|value| !value.is_empty());
    let model = MenuSnapshot {
        token: MenuToken {
            generation: u64_at(4),
            screen: ScreenInstanceId(u64_at(12)),
            revision: u64_at(20),
        },
        route: route_from_code(u32_at(28))?,
        selected: u32_at(32) as usize,
        fields: values,
        pending: bytes[40] != 0,
        error,
    };
    // The same admitted typed boundary is used by sender and local renderer.
    validate_snapshot(&model)?;
    Ok(model)
}

/// Accepted-prefix display packet; the optional archive is actual associated stored data.
pub(crate) fn encode_record_preview(
    token: MenuToken,
    value: &crate::record_model::FrozenRecordPreview,
    archive: Option<&crate::result_archive::ResultArchive>,
) -> Result<Vec<u8>, String> {
    value.validate()?;
    if token.generation == 0 || token.screen.0 == 0 || token.revision == 0 {
        return Err("invalid record preview token".into());
    }
    let archived = if let Some((player, result)) = value.historical {
        let projected = archive
            .ok_or("associated stored preview requires its genuine archive")?
            .for_player(player)
            .map_err(|error| error.to_string())?;
        let entry = &projected.entries()[0];
        if entry.result != result
            || entry.score.as_ref() != value.historical_score.as_deref()
            || entry.bms_score().map_err(|error| error.to_string())? != value.historical_bms_score
            || projected.comparisons().map(|rows| &rows[0].1)
                != value.historical_comparison.as_deref()
        {
            return Err("stored preview differs from its associated archive".into());
        }
        crate::result_archive::encode_archive(&projected).map_err(|error| error.to_string())?
    } else {
        Vec::new()
    };
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"BKRP");
    for n in [token.generation, token.screen.0, token.revision] {
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    record_string(
        &mut bytes,
        value
            .path
            .to_str()
            .ok_or("record preview path is not UTF-8")?,
    )?;
    bytes.extend_from_slice(&(value.records as u64).to_le_bytes());
    bytes.extend_from_slice(&value.start.as_nanos().to_le_bytes());
    for time in [value.end, value.recorded_until] {
        record_time(&mut bytes, time.map(beatkernel::time::Timestamp::as_nanos));
    }
    let score = &value.score;
    for n in [score.hits, score.misses, score.combo, score.max_combo] {
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    bytes.extend_from_slice(&(score.grades.len() as u32).to_le_bytes());
    for (grade, count) in &score.grades {
        bytes.extend_from_slice(&grade.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
    }
    let timing = score.timing;
    for n in [timing.count, timing.early, timing.late, timing.exact] {
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    bytes.extend_from_slice(&timing.sum.to_le_bytes());
    bytes.extend_from_slice(&timing.absolute_sum.to_le_bytes());
    for time in [timing.last, timing.min, timing.max] {
        record_time(&mut bytes, time);
    }
    bytes.push(u8::from(value.bms_score.is_some()));
    if let Some(classes) = value.bms_score {
        for n in [
            classes.pgreat,
            classes.great,
            classes.good,
            classes.bad,
            classes.poor,
            classes.ex_score,
        ] {
            bytes.extend_from_slice(&n.to_le_bytes());
        }
    }
    record_string(&mut bytes, value.archive_error.as_deref().unwrap_or(""))?;
    bytes.extend_from_slice(&(archived.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&archived);
    if bytes.len() > MAX_MENU_BYTES {
        return Err("record preview exceeds byte limit".into());
    }
    Ok(bytes)
}
fn record_string(bytes: &mut Vec<u8>, value: &str) -> Result<(), String> {
    if value.len() > 4096 {
        return Err("record preview string exceeds limit".into());
    }
    bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}
fn record_time(bytes: &mut Vec<u8>, value: Option<i64>) {
    bytes.push(u8::from(value.is_some()));
    if let Some(value) = value {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}
struct RecordReader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> RecordReader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(length)
            .ok_or("record preview length overflow")?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or("truncated record preview")?;
        self.at = end;
        Ok(value)
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
    fn time(&mut self) -> Result<Option<i64>, String> {
        match self.take(1)?[0] {
            0 => Ok(None),
            1 => self.i64().map(Some),
            _ => Err("invalid record preview option".into()),
        }
    }
    fn string(&mut self) -> Result<String, String> {
        let length = self.u32()? as usize;
        if length > 4096 {
            return Err("record preview string exceeds limit".into());
        }
        Ok(std::str::from_utf8(self.take(length)?)
            .map_err(|_| "invalid record preview UTF-8")?
            .to_owned())
    }
}
pub(crate) fn decode_record_preview(
    bytes: &[u8],
) -> Result<(MenuToken, crate::record_model::FrozenRecordPreview), String> {
    use crate::{
        record_model::FrozenRecordPreview, result_archive::ArchivedScore, timing::TimingRecord,
    };
    use beatkernel::time::Timestamp;
    if bytes.len() > MAX_MENU_BYTES {
        return Err("record preview exceeds byte limit".into());
    }
    let mut reader = RecordReader { bytes, at: 0 };
    if reader.take(4)? != b"BKRP" {
        return Err("invalid record preview magic".into());
    }
    let token = MenuToken {
        generation: reader.u64()?,
        screen: ScreenInstanceId(reader.u64()?),
        revision: reader.u64()?,
    };
    if token.generation == 0 || token.screen.0 == 0 || token.revision == 0 {
        return Err("invalid record preview token".into());
    }
    let path = reader.string()?.into();
    let records = usize::try_from(reader.u64()?).map_err(|_| "record count exceeds usize")?;
    let start = Timestamp::from_nanos(reader.i64()?);
    let end = reader.time()?.map(Timestamp::from_nanos);
    let recorded_until = reader.time()?.map(Timestamp::from_nanos);
    let (hits, misses, combo, max_combo) =
        (reader.u64()?, reader.u64()?, reader.u64()?, reader.u64()?);
    let count = reader.u32()? as usize;
    if count > crate::result_archive::MAX_SCORE_GRADES
        || count
            .checked_mul(12)
            .is_none_or(|length| length > bytes.len() - reader.at)
    {
        return Err("record grade count exceeds admitted extent".into());
    }
    let mut grades = Vec::with_capacity(count);
    for _ in 0..count {
        grades.push((reader.u32()?, reader.u64()?));
    }
    let timing = TimingRecord {
        count: reader.u64()?,
        early: reader.u64()?,
        late: reader.u64()?,
        exact: reader.u64()?,
        sum: i128::from_le_bytes(reader.take(16)?.try_into().unwrap()),
        absolute_sum: u128::from_le_bytes(reader.take(16)?.try_into().unwrap()),
        last: reader.time()?,
        min: reader.time()?,
        max: reader.time()?,
    };
    let bms_score = match reader.take(1)?[0] {
        0 => None,
        1 => Some(crate::judgment_policy::BmsScoreSummary {
            pgreat: reader.u64()?,
            great: reader.u64()?,
            good: reader.u64()?,
            bad: reader.u64()?,
            poor: reader.u64()?,
            ex_score: reader.u64()?,
        }),
        _ => return Err("invalid record class option".into()),
    };
    let archive_error = Some(reader.string()?).filter(|value| !value.is_empty());
    let archive_length = reader.u32()? as usize;
    let archive = if archive_length == 0 {
        None
    } else {
        Some(
            crate::result_archive::decode_archive(reader.take(archive_length)?)
                .map_err(|error| error.to_string())?,
        )
    };
    if reader.at != bytes.len() {
        return Err("trailing record preview bytes".into());
    }
    let (historical, historical_score, historical_comparison, historical_bms_score) =
        if let Some(archive) = archive {
            if archive.entries().len() != 1 {
                return Err("record preview requires exactly one associated stored player".into());
            }
            let entry = &archive.entries()[0];
            (
                Some((entry.player, entry.result)),
                entry.score.clone().map(std::sync::Arc::new),
                archive
                    .comparisons()
                    .map(|rows| std::sync::Arc::new(rows[0].1.clone())),
                entry.bms_score().map_err(|error| error.to_string())?,
            )
        } else {
            (None, None, None, None)
        };
    let value = FrozenRecordPreview {
        bms_score,
        historical_bms_score,
        path,
        records,
        recorded_until,
        start,
        end,
        historical,
        historical_comparison,
        historical_score,
        archive_error,
        score: ArchivedScore {
            hits,
            misses,
            combo,
            max_combo,
            grades,
            timing,
        },
    };
    value.validate()?;
    Ok((token, value))
}

/// Renderer-local shared screen scopes. Only the bounded value model crosses Workers.
pub(crate) struct BrowserMenuPresentation {
    pub(crate) model: MenuSnapshot,
    views: Vec<MenuView>,
    hits: Vec<(
        crate::ui::interaction::ControlId,
        crate::ui::interaction::Bounds,
    )>,
    record_preview: Option<crate::record_model::FrozenRecordPreview>,
    record_details: bool,
    record_page: usize,
    opponents: usize,
    selected_opponents: [usize; 2],
    motions: Vec<MenuMotion>,
    motion_time: std::time::Duration,
    motion_suspended: bool,
    motion_disposed: bool,
    pending_hits: Vec<PresentedHit>,
    presented_hits: Vec<PresentedHit>,
    presented_pose: Option<crate::scene::UiPresentedPose>,
    presented_token: Option<MenuToken>,
    presented_extent: [u32; 2],
}
#[derive(Clone, Copy)]
struct PresentedHit {
    control: crate::ui::interaction::ControlId,
    bounds: crate::ui::interaction::Bounds,
    key: Option<crate::scene::UiComponentKey>,
}
struct MenuMotion {
    screen: ScreenInstanceId,
    scheduler: crate::ui::motion::MotionScheduler,
    nodes: Vec<(crate::ui::layout::NodeId, crate::scene::UiTransform)>,
    suspended: bool,
}
enum MenuView {
    Selection(crate::ui::selection::SelectionView, Vec<String>),
    Settings(crate::ui::settings::SettingsView),
    Practice(crate::ui::practice::PracticeView),
    Records(crate::ui::records::RecordsView),
    Players(crate::ui::players::PlayersView),
    Devices(crate::ui::devices::DevicesView),
    Display(crate::ui::display::DisplayView),
}
impl MenuView {
    fn id(&self) -> ScreenInstanceId {
        match self {
            Self::Selection(view, _) => view.id(),
            Self::Settings(view) => view.id(),
            Self::Practice(view) => view.id(),
            Self::Records(view) => view.id(),
            Self::Players(view) => view.id(),
            Self::Devices(view) => view.id(),
            Self::Display(view) => view.id(),
        }
    }
    fn kind(&self) -> crate::screen_lifecycle::ScreenKind {
        use crate::screen_lifecycle::ScreenKind;
        match self {
            Self::Selection(..) => ScreenKind::Selection,
            Self::Settings(..) => ScreenKind::Settings,
            Self::Practice(..) => ScreenKind::Practice,
            Self::Records(..) => ScreenKind::Records,
            Self::Players(..) => ScreenKind::Players,
            Self::Devices(..) => ScreenKind::Devices,
            Self::Display(..) => ScreenKind::Display,
        }
    }
}
impl BrowserMenuPresentation {
    pub(crate) fn new(model: MenuSnapshot) -> Result<Self, String> {
        let mut presentation = Self {
            model: model.clone(),
            views: Vec::new(),
            hits: Vec::new(),
            record_preview: None,
            record_details: false,
            record_page: 0,
            opponents: 0,
            selected_opponents: [0; 2],
            motions: Vec::new(),
            motion_time: std::time::Duration::ZERO,
            motion_suspended: false,
            motion_disposed: false,
            pending_hits: Vec::new(),
            presented_hits: Vec::new(),
            presented_pose: None,
            presented_token: None,
            presented_extent: [0; 2],
        };
        presentation.apply(model)?;
        Ok(presentation)
    }
    pub(crate) fn apply(&mut self, model: MenuSnapshot) -> Result<(), String> {
        validate_snapshot(&model)?;
        if model.token.generation != self.model.token.generation
            || model.token.revision < self.model.token.revision
        {
            return Err("stale renderer menu model".into());
        }
        if matches!(
            model.route,
            ScreenRoute::Players | ScreenRoute::Devices { .. }
        ) {
            decode_local_fields(&model.fields, model.route == ScreenRoute::Players)?;
        }
        // Create destination before pruning parent scopes; no navigation side effects.
        if !self
            .views
            .iter()
            .any(|view| view.id() == model.token.screen)
        {
            let id = model.token.screen;
            let view = match model.route {
                ScreenRoute::Selection => MenuView::Selection(crate::ui::selection::SelectionView::new(id,
                    model.fields.iter().map(|title| crate::ui::selection::SelectionItem { title: title.clone(), artist: String::new() }).collect::<Vec<_>>().into(),
                    Vec::<String>::new().into(), 960, 720)?, model.fields.clone()),
                ScreenRoute::Settings => MenuView::Settings(crate::ui::settings::SettingsView::new(id, 960, 720)?),
                ScreenRoute::Practice => MenuView::Practice(crate::ui::practice::PracticeView::new(id, 960, 720)?),
                ScreenRoute::Records => MenuView::Records(crate::ui::records::RecordsView::new(id, 960, 720)?),
                ScreenRoute::Players => MenuView::Players(crate::ui::players::PlayersView::new(id, 960, 720)?),
                ScreenRoute::Devices { .. } => MenuView::Devices(crate::ui::devices::DevicesView::new(id, 960, 720)?),
                ScreenRoute::Display => MenuView::Display(crate::ui::display::DisplayView::new(id, 960, 720)?),
                _ => return Err("menu body requires shared menu route; Results uses genuine frozen visual owner".into()),
            };
            self.views.push(view);
        }
        self.views.retain(|view| model.route.contains(view.kind()));
        for owner in &mut self.motions {
            if !self.views.iter().any(|view| view.id() == owner.screen) {
                owner.scheduler.dispose();
            } else if owner.screen != model.token.screen && !owner.suspended {
                owner.scheduler.suspend(self.motion_time)?;
                owner.suspended = true;
            }
        }
        self.motions.retain(|owner| !owner.scheduler.disposed());
        if model.route != ScreenRoute::Records
            || self.record_preview.as_ref().is_some_and(|preview| {
                model.fields.get(model.selected).map(String::as_str) != preview.path.to_str()
            })
        {
            self.record_preview = None;
            self.record_details = false;
            self.record_page = 0;
        }
        self.model = model;
        Ok(())
    }
    pub(crate) fn set_record_preview(
        &mut self,
        token: MenuToken,
        value: crate::record_model::FrozenRecordPreview,
    ) -> Result<(), String> {
        value.validate()?;
        if token != self.model.token
            || self.model.route != ScreenRoute::Records
            || self
                .model
                .fields
                .get(self.model.selected)
                .map(String::as_str)
                != value.path.to_str()
        {
            return Err("foreign or unselected record preview".into());
        }
        let keep_details = self.record_details
            && value.historical.is_some()
            && self
                .record_preview
                .as_ref()
                .is_some_and(|old| old.path == value.path)
            && self.record_page
                < crate::historical_record_presentation::historical_page_count(
                    value.historical_score.as_deref(),
                    value.historical_comparison.as_deref(),
                );
        self.record_preview = Some(value);
        if !keep_details {
            self.record_details = false;
            self.record_page = 0;
        }
        Ok(())
    }
    pub(crate) fn set_record_details(&mut self, details: bool, page: u32) -> Result<(), String> {
        let preview = self
            .record_preview
            .as_ref()
            .ok_or("record preview unavailable")?;
        let pages = crate::historical_record_presentation::historical_page_count(
            preview.historical_score.as_deref(),
            preview.historical_comparison.as_deref(),
        );
        if self.model.route != ScreenRoute::Records
            || details && preview.historical.is_none()
            || (!details && page != 0)
            || page as usize >= pages
        {
            return Err("invalid stored record details page".into());
        }
        self.record_details = details;
        self.record_page = page as usize;
        Ok(())
    }
    pub(crate) fn record_page(&self) -> u32 {
        self.record_page as u32
    }
    pub(crate) fn record_details(&self) -> bool {
        self.record_details
    }
    pub(crate) fn set_opponents(&mut self, count: u32, own: u32, other: u32) -> Result<(), String> {
        if count > 8
            || own
                .checked_add(other)
                .is_none_or(|selected| selected > count)
        {
            return Err("invalid browser opponent projection".into());
        }
        self.opponents = count as usize;
        self.selected_opponents = [own as usize, other as usize];
        Ok(())
    }
    pub(crate) fn compose(&mut self, scene: &mut crate::scene::Scene) -> Result<(), String> {
        use crate::ui::text_input::LineEditor;
        let model = &self.model;
        let value = |index| model.fields.get(index).map_or("", String::as_str);
        let editor = |index| LineEditor::new(value(index), 4096);
        let error = model.error.as_deref();
        let view = self
            .views
            .iter_mut()
            .find(|view| view.id() == model.token.screen)
            .ok_or("menu view unavailable")?;
        let animated: Vec<_> = self
            .motions
            .iter()
            .find(|m| m.screen == model.token.screen)
            .map(|m| m.nodes.iter().map(|(node, _)| *node).collect())
            .unwrap_or_default();
        if animated.is_empty() {
            // Static destinations do not keep suspended parents' GPU slots.
            // Their sampled poses and tracks live in the screen owner instead.
            scene.retain_component_keys(model.token.screen, &[]);
            scene.clear();
        }
        self.hits.clear();
        match view {
            MenuView::Selection(view, items) => {
                if items != &model.fields {
                    return Err("menu catalog replacement requires a new screen instance".into());
                }
                view.update(crate::ui::selection::SelectionFrame {
                    selected: model.selected,
                    hovered: None,
                    armed: None,
                    error: model.error.clone(),
                    backend_pending: model.pending,
                });
                if animated.is_empty() {
                    view.compose(scene, &mut self.hits)?;
                } else {
                    view.compose_components(scene, &mut self.hits, model.token.screen, &animated)?;
                }
            }
            MenuView::Settings(view) => {
                const LABELS: [&str; 14] = [
                    "EARLY WINDOW (MS)",
                    "LATE WINDOW (MS)",
                    "INPUT OFFSET (MS)",
                    "OUTPUT LATENCY",
                    "CUSTOM LATENCY (MS)",
                    "OUTPUT RATE",
                    "COMMAND QUEUE",
                    "VOICE CAPACITY",
                    "PENDING CAPACITY",
                    "MAXIMUM FRAMES",
                    "COMMANDS PER RENDER",
                    "SECTION START (SECONDS)",
                    "SECTION END (SECONDS)",
                    "KEYBOARD BINDINGS",
                ];
                let fields: Vec<_> = model.fields.iter().enumerate().map(|(index, value)| crate::settings::SettingsField {
                    flag: "", label: LABELS.get(index).copied().unwrap_or("BROWSER SETTING"),
                    hint: "Browser capabilities apply. Native drivers and callback buffer size are unavailable.", value: value.clone() }).collect();
                let selected = model.selected.min(fields.len().saturating_sub(1));
                let editor = editor(selected)?;
                let profile = LineEditor::new("browser-settings", 4096)?;
                view.update(crate::ui::settings::SettingsFrame {
                    fields: &fields,
                    selected,
                    editor: &editor,
                    profile: &profile,
                    profile_focused: false,
                    message: None,
                    error,
                    pending: model.pending,
                    hovered: None,
                    armed: None,
                })?;
                view.compose(scene, &mut self.hits)?;
            }
            MenuView::Practice(view) => {
                view.update(crate::ui::practice::PracticeFrame {
                    editor: editor(0)?,
                    end_editor: editor(1)?,
                    end_focused: model.selected == 1,
                    error: model.error.clone(),
                    hovered: None,
                    armed: None,
                });
                view.compose(scene, &mut self.hits)?;
            }
            MenuView::Display(view) => {
                let editors = [editor(0)?, editor(1)?, editor(2)?, editor(3)?];
                view.update(crate::ui::display::DisplayFrame {
                    editors: &editors,
                    selected: model.selected.min(3),
                    error,
                    pending: model.pending,
                    hovered: None,
                    armed: None,
                })?;
                if animated.is_empty() {
                    view.compose(scene, &mut self.hits)?;
                } else {
                    view.compose_components(scene, &mut self.hits, model.token.screen, &animated)?;
                }
            }
            MenuView::Records(view) => {
                let directory = LineEditor::new("Browser saved records", 4096)?;
                let catalog = crate::record_model::RecordCatalog {
                    entries: model.fields.iter().map(std::path::PathBuf::from).collect(),
                    truncated: false,
                };
                view.update_visual(crate::ui::records::VisualRecordsFrame {
                    directory: &directory,
                    directory_focused: false,
                    catalog: Some(&catalog),
                    selected: (!catalog.entries.is_empty()).then_some(model.selected),
                    first: model.selected / 10 * 10,
                    preview: self.record_preview.as_ref(),
                    pending: model.pending,
                    details: self.record_details,
                    grade_page: self.record_page,
                    opponents: self.opponents,
                    selected_opponents: self.selected_opponents,
                    message: None,
                    error,
                    hovered: None,
                    armed: None,
                })?;
                view.compose(scene, &mut self.hits)?;
            }
            MenuView::Players(view) => {
                // Exact stable player ID/source pairs, followed by the source descriptor table.
                let (players, sources, can_assign, _) = decode_local_fields(&model.fields, true)?;
                let projection = crate::local_setup::BrowserLocalProjection {
                    players: &players,
                    sources: &sources,
                    can_assign,
                };
                view.update_browser(crate::ui::players::BrowserPlayersFrame {
                    model: &projection,
                    selected: model.selected.min(players.len() - 1),
                    first: model.selected / 10 * 10,
                    pending: model.pending,
                    error,
                    message: None,
                    hovered: None,
                    armed: None,
                })?;
                view.compose(scene, &mut self.hits)?;
            }
            MenuView::Devices(view) => {
                let (_, sources, can_assign, can_refresh) =
                    decode_local_fields(&model.fields, false)?;
                view.update_browser(crate::ui::devices::BrowserDevicesFrame {
                    sources: &sources,
                    can_assign,
                    can_refresh,
                    player: None,
                    selected: (model.selected < sources.len()).then_some(model.selected),
                    first: if sources.is_empty() {
                        0
                    } else {
                        model.selected / 10 * 10
                    },
                    pending: model.pending,
                    error,
                    hovered: None,
                    armed: None,
                })?;
                view.compose(scene, &mut self.hits)?;
            }
        }
        self.pending_hits.clear();
        self.pending_hits
            .try_reserve(self.hits.len())
            .map_err(|_| "menu hit staging allocation failed")?;
        self.presented_hits
            .try_reserve(self.hits.len().saturating_sub(self.presented_hits.len()))
            .map_err(|_| "menu presented hit allocation failed")?;
        for &(control, bounds) in &self.hits {
            let node = match view {
                MenuView::Selection(view, _) => view.node_for_control(control)?,
                MenuView::Display(view) => view.node_for_control(control)?,
                _ => None,
            };
            self.pending_hits.push(PresentedHit {
                control,
                bounds,
                key: node.map(|node| crate::scene::UiComponentKey {
                    screen: model.token.screen,
                    node,
                }),
            });
        }
        if let Some(owner) = self
            .motions
            .iter_mut()
            .find(|m| m.screen == model.token.screen)
        {
            owner.scheduler.rebind(scene)?;
            let updates: Vec<_> = owner
                .nodes
                .iter()
                .map(|(node, pose)| {
                    scene
                        .component_live(crate::scene::UiComponentKey {
                            screen: owner.screen,
                            node: *node,
                        })
                        .map(|id| (id, *pose))
                        .ok_or("menu motion component unavailable")
                })
                .collect::<Result<_, _>>()?;
            scene.set_component_transforms(&updates)?;
        }
        Ok(())
    }
    pub(crate) fn control_node(
        &self,
        control: crate::ui::interaction::ControlId,
    ) -> Result<crate::ui::layout::NodeId, String> {
        let view = self
            .views
            .iter()
            .find(|v| v.id() == self.model.token.screen)
            .ok_or("menu view unavailable")?;
        let node = match view {
            MenuView::Selection(view, _) => view.node_for_control(control)?,
            MenuView::Display(view) => view.node_for_control(control)?,
            _ => return Err("menu route does not support explicit component motion".into()),
        };
        node.ok_or_else(|| "control is not displayed".into())
    }
    fn check_motion_time(&self, now: std::time::Duration) -> Result<(), String> {
        if self.motion_disposed || now < self.motion_time {
            return Err("disposed menu motion or regressed presentation time".into());
        }
        for owner in &self.motions {
            owner.scheduler.validate_time(now)?;
        }
        Ok(())
    }
    pub(crate) fn validate_motion_time(&self, now: std::time::Duration) -> Result<(), String> {
        self.check_motion_time(now)
    }
    pub(crate) fn validate_motion_request(
        &self,
        token: MenuToken,
        now: std::time::Duration,
    ) -> Result<(), String> {
        self.check_motion_time(now)?;
        if token != self.model.token || self.model.pending || self.motion_suspended {
            return Err("stale, pending or suspended menu motion request".into());
        }
        Ok(())
    }
    pub(crate) fn request_motion(
        &mut self,
        token: MenuToken,
        control: crate::ui::interaction::ControlId,
        motion: crate::ui::motion::ComponentMotion,
        now: std::time::Duration,
        scene: &mut crate::scene::Scene,
    ) -> Result<(), String> {
        self.check_motion_time(now)?;
        if token != self.model.token || self.model.pending || self.motion_suspended {
            return Err("stale, pending or suspended menu motion request".into());
        }
        let node = self.control_node(control)?;
        if !self.hits.iter().any(|(id, _)| *id == control) {
            return Err("control is not displayed".into());
        }
        let index = if let Some(index) = self.motions.iter().position(|m| m.screen == token.screen)
        {
            index
        } else {
            self.motions
                .try_reserve(1)
                .map_err(|_| "menu motion owner allocation failed")?;
            self.motions.push(MenuMotion {
                screen: token.screen,
                scheduler: crate::ui::motion::MotionScheduler::new(
                    token.screen,
                    crate::scene::MAX_UI_COMPONENTS,
                )?,
                nodes: Vec::new(),
                suspended: false,
            });
            self.motions.len() - 1
        };
        if self.motions[index].suspended {
            return Err("menu motion owner suspended; draw before requesting".into());
        }
        let added = !self.motions[index]
            .nodes
            .iter()
            .any(|(old, _)| *old == node);
        if added {
            if self.motions[index].nodes.len() == crate::scene::MAX_UI_COMPONENTS {
                return Err("menu motion component capacity exhausted".into());
            }
            self.motions[index]
                .nodes
                .try_reserve(1)
                .map_err(|_| "menu motion component allocation failed")?;
            self.motions[index]
                .nodes
                .push((node, crate::scene::UiTransform::default()));
            if let Err(error) = self.compose(scene) {
                self.motions[index].nodes.pop();
                return Err(error);
            }
        }
        let key = crate::scene::UiComponentKey {
            screen: token.screen,
            node,
        };
        let id = scene
            .component_live(key)
            .ok_or("menu motion component unavailable")?;
        let mut scheduler = self.motions[index].scheduler.clone();
        scheduler.rebind(scene)?;
        scheduler.schedule(key, id, motion, now)?;
        self.motions[index].scheduler = scheduler;
        self.motion_time = now;
        self.tick_motion(now, [960, 720], scene)?;
        Ok(())
    }
    pub(crate) fn tick_motion(
        &mut self,
        now: std::time::Duration,
        extent: [u32; 2],
        scene: &mut crate::scene::Scene,
    ) -> Result<bool, String> {
        self.check_motion_time(now)?;
        let mut changed = false;
        if let Some(owner) = self
            .motions
            .iter_mut()
            .find(|m| m.screen == self.model.token.screen)
        {
            if extent.contains(&0) || self.motion_suspended {
                owner.scheduler.suspend(self.motion_time)?;
                owner.suspended = true;
            } else {
                // Completed tracks still own their final pose. Preflight these
                // bindings too, before any live track changes the scene.
                for (node, _) in &owner.nodes {
                    let key = crate::scene::UiComponentKey {
                        screen: owner.screen,
                        node: *node,
                    };
                    if scene
                        .component_live(key)
                        .and_then(|id| scene.component_transform(id))
                        .is_none()
                    {
                        return Err("menu motion pose unavailable".into());
                    }
                }
                let mut scheduler = owner.scheduler.clone();
                scheduler.rebind(scene)?;
                if owner.suspended {
                    scheduler.resume(now)?;
                }
                changed = scheduler.tick(owner.screen, now, scene)?;
                owner.scheduler = scheduler;
                owner.suspended = false;
                for (node, pose) in &mut owner.nodes {
                    *pose = scene
                        .component_live(crate::scene::UiComponentKey {
                            screen: owner.screen,
                            node: *node,
                        })
                        .and_then(|id| scene.component_transform(id))
                        .ok_or("menu motion pose unavailable")?;
                }
            }
        }
        self.motion_time = now;
        Ok(changed)
    }
    pub(crate) fn freeze_extent(&mut self) -> Result<(), String> {
        if self.motion_disposed {
            return Ok(());
        }
        if let Some(owner) = self
            .motions
            .iter_mut()
            .find(|m| m.screen == self.model.token.screen)
        {
            owner.scheduler.suspend(self.motion_time)?;
            owner.suspended = true;
        }
        Ok(())
    }
    pub(crate) fn publish_presented(
        &mut self,
        scene: &crate::scene::Scene,
        extent: [u32; 2],
    ) -> Result<(), String> {
        if extent.contains(&0) {
            return Err("zero extent cannot publish menu input pose".into());
        }
        scene.status()?;
        self.presented_hits.clear();
        self.presented_hits.extend_from_slice(&self.pending_hits);
        self.presented_pose = Some(scene.presented_pose());
        self.presented_token = Some(self.model.token);
        self.presented_extent = extent;
        Ok(())
    }
    pub(crate) fn motion_active(&self) -> bool {
        !self.motion_disposed
            && !self.motion_suspended
            && self.motions.iter().any(|m| {
                m.screen == self.model.token.screen
                    && !m.suspended
                    && m.scheduler.active_count() > 0
            })
    }
    pub(crate) fn motion_time(&self) -> std::time::Duration {
        self.motion_time
    }
    pub(crate) fn suspend_motion(&mut self, now: std::time::Duration) -> Result<(), String> {
        self.check_motion_time(now)?;
        for owner in &mut self.motions {
            owner.scheduler.suspend(now)?;
            owner.suspended = true;
        }
        self.motion_suspended = true;
        self.motion_time = now;
        Ok(())
    }
    pub(crate) fn resume_motion(&mut self, now: std::time::Duration) -> Result<(), String> {
        self.check_motion_time(now)?;
        // Actual active owner resumes on the next draw, after fresh binding preflight.
        self.motion_suspended = false;
        self.motion_time = now;
        Ok(())
    }
    pub(crate) fn dispose_motion(&mut self) {
        for owner in &mut self.motions {
            owner.scheduler.dispose();
        }
        self.motions.clear();
        self.motion_disposed = true;
        self.presented_pose = None;
        self.presented_hits.clear();
    }
    pub(crate) fn hit(&self, point: (f64, f64), extent: [u32; 2]) -> u64 {
        if self.model.pending
            || self.motion_suspended
            || extent.contains(&0)
            || self.presented_token != Some(self.model.token)
        {
            return 0;
        }
        let Some(pose) = &self.presented_pose else {
            return 0;
        };
        crate::ui::interaction::logical_point(
            point,
            (self.presented_extent[0], self.presented_extent[1]),
            (960, 720),
        )
        .and_then(|point| {
            self.presented_hits.iter().rev().find_map(|hit| {
                pose.project(hit.key, point)
                    .filter(|point| hit.bounds.contains(*point))
                    .map(|_| hit.control.0)
            })
        })
        .unwrap_or(0)
    }
}
fn decode_local_fields(
    fields: &[String],
    players: bool,
) -> Result<
    (
        Vec<crate::local_setup::BrowserPlayerMember<'_>>,
        Vec<crate::local_setup::BrowserInputSource<'_>>,
        bool,
        bool,
    ),
    String,
> {
    use crate::local_setup::{BrowserInputKind, BrowserInputSource, BrowserPlayerMember};
    let value = |index| {
        fields
            .get(index)
            .map(String::as_str)
            .ok_or("truncated browser input menu metadata")
    };
    let boolean = |index| match value(index)? {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err("invalid browser capability"),
    };
    let can_assign = boolean(0)?;
    let can_refresh = boolean(1)?;
    let count: usize = value(2)?
        .parse()
        .map_err(|_| "invalid browser member count")?;
    if count > 64 || (players && count == 0) {
        return Err("invalid browser member count".into());
    }
    let mut members = Vec::with_capacity(count);
    let mut at = 3;
    for _ in 0..count {
        let id = value(at)?
            .parse::<u32>()
            .map_err(|_| "invalid browser member ID")?;
        let source = value(at + 1)?;
        members.push(BrowserPlayerMember {
            id: crate::local_players::PlayerId(id),
            source: (!source.is_empty()).then_some(source),
        });
        at += 2;
    }
    let count: usize = value(at)?
        .parse()
        .map_err(|_| "invalid browser source count")?;
    at += 1;
    if count > crate::device_catalog::MAX_DEVICES {
        return Err("browser source count exceeds limit".into());
    }
    let mut sources = Vec::with_capacity(count);
    for _ in 0..count {
        let kind = match value(at + 1)? {
            "keyboard" => BrowserInputKind::Keyboard,
            "touch" => BrowserInputKind::Touch,
            "hid" => BrowserInputKind::Hid,
            "gamepad" => BrowserInputKind::Gamepad,
            "pointer" => BrowserInputKind::Pointer,
            _ => return Err("invalid browser source kind".into()),
        };
        sources.push(BrowserInputSource {
            id: value(at)?,
            kind,
            label: value(at + 2)?,
            detail: value(at + 3)?,
            selectable: boolean(at + 4)?,
        });
        at += 5;
    }
    if at != fields.len() {
        return Err("trailing browser input metadata".into());
    }
    crate::local_setup::validate_browser_sources(&sources)?;
    if players {
        crate::local_setup::BrowserLocalProjection {
            players: &members,
            sources: &sources,
            can_assign,
        }
        .validate()?;
    }
    Ok((members, sources, can_assign, can_refresh))
}
