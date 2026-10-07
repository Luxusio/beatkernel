//! Native main-thread presentation of snapshots from the actual gameplay owner.
use crate::desktop_clipboard::ClipboardWorker;
#[cfg(test)]
#[path = "desktop_catalog_fixtures.rs"]
mod catalog_fixtures;
#[cfg(test)]
#[path = "desktop_clipboard_fixtures.rs"]
mod clipboard_fixtures;
#[cfg(test)]
#[path = "desktop_completed_results_fixtures.rs"]
mod completed_results_fixtures;
#[cfg(test)]
#[path = "font_fixture.rs"]
mod font_fixture;
#[cfg(test)]
#[path = "desktop_ime_area_fixtures.rs"]
mod ime_area_fixtures;
#[cfg(test)]
#[path = "desktop_ime_fields_fixtures.rs"]
mod ime_fields_fixtures;
#[cfg(test)]
#[path = "desktop_renderer_startup_fixtures.rs"]
mod renderer_startup_fixtures;
#[cfg(test)]
#[path = "desktop_results_detail_fixtures.rs"]
mod results_detail_fixtures;
#[cfg(test)]
#[path = "desktop_room_fixtures.rs"]
mod room_fixtures;
#[cfg(test)]
#[path = "desktop_room_results_fixtures.rs"]
mod room_results_fixtures;
#[cfg(test)]
#[path = "desktop_startup_fixtures.rs"]
mod startup_fixtures;
#[cfg(test)]
#[path = "native_viewport_fixtures.rs"]
mod viewport_fixtures;
#[cfg(test)]
use beatkernel_bms_runtime::bga::BgaState;
use beatkernel_bms_runtime::ui::{
    atoms::{rect, text},
    catalog_search::CatalogSearch,
    clipboard::{ClipboardAction, ClipboardEdit},
    devices::{DevicesFrame, DevicesView},
    display::{BUTTONS as DISPLAY_BUTTONS, DisplayFrame, DisplayView},
    interaction::{Bounds, ControlId, Gesture, WheelSteps, logical_point},
    molecules, organisms,
    players::{PlayersFrame, PlayersView},
    practice::{PracticeFrame, PracticeView},
    records::{RecordsFrame, RecordsView},
    results::{ResultsView, ResultDetails},
    selection::{ROW_HEIGHT, SelectionFrame, SelectionItem, SelectionView, VISIBLE_ROWS},
    settings::{BUTTONS as SETTINGS_BUTTONS, SettingsFrame, SettingsView},
    text_input::LineEditor,
};
use beatkernel_bms_runtime::{
    bga_render::{BgaFrame, BgaTextureCache},
    competition::OpponentKind,
    device_catalog::{DeviceCatalog, DeviceRequest},
    font_atlas::{FontAtlas, MAX_FONT_CHAIN},
    font_text::{FontText, MAX_TEXT_GLYPHS},
    local_players::PlayerId,
    local_setup::LocalSetup,
    native_catalog::{CatalogControl, NativeCatalog},
    panel_scope::{PanelScope, TaskPermit},
    player, player_chart,
    room_presentation::RoomUiAction,
    practice::PracticeStart,
    practice_loop::PracticeLoop,
    presentation_settings::PresentationSettings,
    record_catalog::{RecordCatalog, RecordPreview},
    screen_lifecycle::{ScreenInstanceId, ScreenKind, ScreenNavigator, ScreenPhase, ScreenRoute},
    session_launch::SessionLaunch,
    settings::{NativeSettings, SettingsHost},
    settings_profile::PlayerProfile,
};
use beatkernel_bms_runtime::{
    graphics::{self, BackendChoice, Presentation, Renderer},
    scene::Scene,
};
use std::{
    error::Error,
    io::Read,
    path::PathBuf,
    sync::Arc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, KeyCode, ModifiersState, PhysicalKey},
    window::{Window, WindowId},
};

const WIDTH: usize = 960;
const HEIGHT: usize = 720;
fn catalog_scroll_lines(delta: MouseScrollDelta, physical: (u32, u32)) -> f64 {
    match delta {
        MouseScrollDelta::LineDelta(_, y) => f64::from(y),
        MouseScrollDelta::PixelDelta(position) => {
            let Ok(viewport) = beatkernel_bms_runtime::viewport::Viewport::new(
                [physical.0, physical.1],
                [WIDTH as u32, HEIGHT as u32],
            ) else {
                return f64::NAN;
            };
            position.y * (HEIGHT as f64 / f64::from(viewport.rect()[3])) / ROW_HEIGHT as f64
        }
    }
}
const PAUSE_BOUNDS: Bounds = Bounds {
    x: 740,
    y: 680,
    width: 190,
    height: 34,
};
const LOOP_END_BOUNDS: Bounds = Bounds {
    x: 550,
    y: 642,
    width: 180,
    height: 30,
};
const LOOP_TOGGLE_BOUNDS: Bounds = Bounds {
    x: 740,
    y: 642,
    width: 190,
    height: 30,
};
type Native = fn(&[String]) -> Result<(), Box<dyn Error>>;
type QueryDevices = fn(DeviceRequest) -> Result<DeviceCatalog, Box<dyn Error>>;

#[derive(Clone)]
struct Options {
    native: Vec<String>,
    library: Option<PathBuf>,
    chart: Option<PathBuf>,
    lookahead: i64,
    fps: usize,
    backend: BackendChoice,
    presentation: Presentation,
    profile: Option<PathBuf>,
    title_font: Option<PathBuf>,
    // Ordered after the title font; consulted only for glyphs it lacks.
    fallback_fonts: Vec<PathBuf>,
    display_overrides: Vec<String>,
}
impl Options {
    fn parse(args: &[String]) -> Result<Self, Box<dyn Error>> {
        let mut options = Self {
            native: Vec::new(),
            library: None,
            chart: None,
            lookahead: 2_000_000_000,
            fps: 120,
            backend: BackendChoice::Auto,
            presentation: Presentation::Fifo,
            profile: None,
            title_font: None,
            fallback_fonts: Vec::new(),
            display_overrides: Vec::new(),
        };
        let mut index = 0;
        while index < args.len() {
            let flag = args[index].as_str();
            let value = args
                .get(index + 1)
                .ok_or("desktop/native option requires a value")?;
            if matches!(
                flag,
                "--library"
                    | "--profile"
                    | "--title-font"
                    | "--fallback-font"
                    | "--ui-lookahead-ms"
                    | "--ui-fps"
                    | "--gpu-backend"
                    | "--present"
            ) {
                match flag {
                    "--title-font" => {
                        if value.is_empty() {
                            return Err("--title-font path cannot be empty".into());
                        }
                        if options.title_font.replace(PathBuf::from(value)).is_some() {
                            return Err("duplicate --title-font".into());
                        }
                    }
                    "--fallback-font" => {
                        if value.is_empty() {
                            return Err("--fallback-font path cannot be empty".into());
                        }
                        if options.fallback_fonts.len() + 1 >= MAX_FONT_CHAIN {
                            return Err("at most seven --fallback-font paths".into());
                        }
                        options.fallback_fonts.push(PathBuf::from(value));
                    }
                    "--profile" => {
                        if value.is_empty() {
                            return Err("--profile path cannot be empty".into());
                        }
                        if options.profile.replace(PathBuf::from(value)).is_some() {
                            return Err("duplicate --profile".into());
                        }
                    }
                    "--library" => {
                        if value.is_empty() {
                            return Err("--library path cannot be empty".into());
                        }
                        if options.library.replace(PathBuf::from(value)).is_some() {
                            return Err("duplicate --library".into());
                        }
                    }
                    _ => options
                        .display_overrides
                        .extend([flag.to_owned(), value.clone()]),
                }
                index += 2;
            } else {
                if flag == "--chart" {
                    if value.is_empty() {
                        return Err("--chart path cannot be empty".into());
                    }
                    if options.chart.replace(PathBuf::from(value)).is_some() {
                        return Err("duplicate --chart".into());
                    }
                }
                options.native.extend_from_slice(&args[index..index + 2]);
                index += 2;
            }
        }
        if options.library.is_some() == options.chart.is_some() {
            return Err("choose exactly one of --library DIR or --chart PATH".into());
        }
        if !options.fallback_fonts.is_empty() && options.title_font.is_none() {
            return Err("--fallback-font requires --title-font".into());
        }
        let display =
            PresentationSettings::default().apply_overrides(&options.display_overrides)?;
        options.set_display(display);
        Ok(options)
    }
    fn display(&self) -> PresentationSettings {
        PresentationSettings {
            backend: self.backend,
            presentation: self.presentation,
            fps: self.fps as u16,
            lookahead_ms: (self.lookahead / 1_000_000) as u32,
        }
    }
    fn set_display(&mut self, display: PresentationSettings) {
        self.backend = display.backend;
        self.presentation = display.presentation;
        self.fps = usize::from(display.fps);
        self.lookahead = i64::from(display.lookahead_ms) * 1_000_000;
    }
}

struct Entry {
    path: PathBuf,
    title: String,
    artist: String,
}
fn direct_entry(path: PathBuf) -> Entry {
    let title = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned();
    Entry {
        path,
        title,
        artist: String::new(),
    }
}
#[cfg(test)]
fn prepare_title_font(bytes: Vec<u8>, items: &[SelectionItem]) -> Result<Arc<FontAtlas>, String> {
    prepare_title_font_with(bytes, Vec::new(), items, || Ok(()))
}
fn prepare_title_font_with(
    bytes: Vec<u8>,
    fallbacks: Vec<Vec<u8>>,
    items: &[SelectionItem],
    mut checkpoint: impl FnMut() -> Result<(), String>,
) -> Result<Arc<FontAtlas>, String> {
    checkpoint()?;
    let mut atlas = FontAtlas::with_fallbacks(bytes, fallbacks, 14.0, 1024, 1024, 4096)?;
    for item in items {
        checkpoint()?;
        for value in [&item.title, &item.artist] {
            for character in value.chars().take(MAX_TEXT_GLYPHS) {
                atlas.prepare(character)?;
            }
        }
    }
    checkpoint()?;
    Ok(Arc::new(atlas))
}

struct PreparedCatalog {
    entries: Vec<Entry>,
    items: Arc<[SelectionItem]>,
    search: CatalogSearch,
    diagnostics: Arc<[String]>,
    font: Option<Arc<FontAtlas>>,
}
fn prepare_catalog(
    library: player_chart::ChartLibrary,
    font_path: Option<PathBuf>,
    fallbacks: Vec<PathBuf>,
    control: &CatalogControl,
) -> Result<PreparedCatalog, String> {
    control.checkpoint()?;
    let entries: Vec<_> = library
        .entries
        .into_iter()
        .map(|entry| Entry {
            path: entry.path,
            title: entry.title,
            artist: entry.artist,
        })
        .collect();
    prepare_catalog_parts(entries, library.diagnostics, font_path, fallbacks, control)
}
fn prepare_direct_catalog(
    path: PathBuf,
    font_path: PathBuf,
    fallbacks: Vec<PathBuf>,
    control: &CatalogControl,
) -> Result<PreparedCatalog, String> {
    control.checkpoint()?;
    prepare_catalog_parts(
        vec![direct_entry(path)],
        Vec::new(),
        Some(font_path),
        fallbacks,
        control,
    )
}
/// Reads one caller-provided font with the atlas byte limit plus one rejection byte.
fn read_font(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|error| error.to_string())?
        .take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}
fn prepare_catalog_parts(
    entries: Vec<Entry>,
    diagnostics: Vec<String>,
    font_path: Option<PathBuf>,
    fallback_paths: Vec<PathBuf>,
    control: &CatalogControl,
) -> Result<PreparedCatalog, String> {
    control.checkpoint()?;
    let items: Arc<[SelectionItem]> = entries
        .iter()
        .map(|entry| SelectionItem {
            title: entry.title.clone(),
            artist: entry.artist.clone(),
        })
        .collect::<Vec<_>>()
        .into();
    control.checkpoint()?;
    let search = CatalogSearch::new(&items)?;
    control.checkpoint()?;
    let font = if let Some(path) = font_path {
        let bytes = read_font(&path)?;
        let mut fallbacks = Vec::with_capacity(fallback_paths.len());
        for path in &fallback_paths {
            control.checkpoint()?;
            fallbacks.push(read_font(path)?);
        }
        control.checkpoint()?;
        Some(prepare_title_font_with(bytes, fallbacks, &items, || {
            control.checkpoint()
        })?)
    } else {
        None
    };
    control.checkpoint()?;
    Ok(PreparedCatalog {
        entries,
        items,
        search,
        diagnostics: diagnostics.into(),
        font,
    })
}
fn spawn_catalog(options: &Options) -> Result<Option<NativeCatalog<PreparedCatalog>>, String> {
    if let Some(root) = &options.library {
        let font_path = options.title_font.clone();
        let fallbacks = options.fallback_fonts.clone();
        NativeCatalog::spawn(root.clone(), move |library, control| {
            prepare_catalog(library, font_path, fallbacks, control)
        })
        .map(Some)
    } else if let Some(font_path) = &options.title_font {
        let path = options
            .chart
            .clone()
            .ok_or("direct title font requires a chart")?;
        let font_path = font_path.clone();
        let fallbacks = options.fallback_fonts.clone();
        NativeCatalog::spawn_prepared(move |control| {
            prepare_direct_catalog(path, font_path, fallbacks, control)
        })
        .map(Some)
    } else {
        Ok(None)
    }
}
fn prepare_startup_options(
    mut options: Options,
    control: &CatalogControl,
) -> Result<Options, String> {
    control.checkpoint()?;
    if let Some(path) = &options.profile {
        let profile =
            beatkernel_bms_runtime::settings_profile::load_player_profile(path, settings_host())
                .map_err(|error| error.to_string())?;
        control.checkpoint()?;
        let native = beatkernel_bms_runtime::settings::overlay_native_args(
            &profile.native.native_args(),
            &without_chart(&options.native),
            settings_host(),
        )?;
        let display = profile
            .presentation
            .apply_overrides(&options.display_overrides)?;
        options.native = native;
        options.set_display(display);
    }
    control.checkpoint()?;
    Ok(options)
}
fn spawn_startup_profile(options: &Options) -> Result<Option<NativeCatalog<Options>>, String> {
    if options.profile.is_none() {
        return Ok(None);
    }
    let options = options.clone();
    NativeCatalog::spawn_prepared(move |control| prepare_startup_options(options, control))
        .map(Some)
}
struct Game {
    viewer: player::PlayerViewer,
    worker: Option<JoinHandle<Result<(), String>>>,
    snapshot: Option<player::PlayerSnapshot>,
    completed_results: Option<ResultsView>,
    completed_results_error: Option<String>,
    cancelling: bool,
    joined: bool,
    local_page: usize,
    local_comparisons: bool,
    replay: bool,
    launch: SessionLaunch,
    prepared_retry: Option<SessionLaunch>,
    practice_bookmark: Option<PracticeStart>,
    practice_loop: Option<PracticeLoop>,
    loop_enabled: bool,
}

impl Game {
    fn presentation_page_count(&self) -> usize {
        if self.joined && !self.replay {
            if let Some(results) = &self.completed_results {
                return results.page_count_for(self.local_comparisons);
            }
        }
        self.snapshot.as_ref().map_or(0, |snapshot| {
            snapshot
                .players
                .len()
                .div_ceil(organisms::LOCAL_PLAYERS_PER_PAGE)
        })
    }
    fn comparisons_available(&self) -> bool {
        if self.joined && !self.replay {
            return self
                .completed_results
                .as_ref()
                .is_some_and(ResultsView::has_comparisons);
        }
        self.snapshot
            .as_ref()
            .is_some_and(|snapshot| local_comparisons_available(&snapshot.players))
    }

    fn room_action_allowed(&self, action: RoomUiAction) -> bool {
        if self.replay || self.prepared_retry.is_some() {
            return false;
        }
        if self.joined {
            return matches!(action, RoomUiAction::Page(page) if self.snapshot.as_ref()
                .and_then(|snapshot| snapshot.room_results.as_ref())
                .is_some_and(|archive| !archive.failed() && page < archive.page_count()));
        }
        if self.cancelling || self.viewer.room_pending() {
            return false;
        }
        self.snapshot.as_ref().is_some_and(|snapshot| {
            !snapshot.cancelled
                && matches!(
                    snapshot.status,
                    player::PlayerStatus::Loading | player::PlayerStatus::Playing
                )
                && snapshot
                    .room
                    .as_ref()
                    .is_some_and(|room| room.allows(action))
        })
    }
    fn request_room(&self, action: RoomUiAction) -> Result<u64, String> {
        if self.joined || !self.room_action_allowed(action) {
            return Err("room control is unavailable".into());
        }
        self.viewer
            .request_room(action)
            .map_err(|error| error.to_string())
    }
    /// Results paging owns no command identity and never touches live controls.
    fn select_room_result_page(&mut self, page: usize) -> Result<(), String> {
        if !self.joined || !self.room_action_allowed(RoomUiAction::Page(page)) {
            return Err("room Results page is unavailable".into());
        }
        let snapshot = self.snapshot.as_mut().ok_or("room Results are missing")?;
        let archive = snapshot
            .room_results
            .as_ref()
            .ok_or("room Results are missing")?;
        let projected = Arc::new(archive.project(page)?);
        snapshot.room = Some(projected);
        Ok(())
    }
    fn pause_target(&self) -> Option<bool> {
        if self.joined
            || self.cancelling
            || self.prepared_retry.is_some()
            || self.viewer.output_pending()
        {
            return None;
        }
        let snapshot = self.snapshot.as_ref()?;
        if snapshot.cancelled
            || snapshot.status != player::PlayerStatus::Playing
            || snapshot.room.is_some()
        {
            return None;
        }
        match snapshot.pause {
            player::PauseState::Running => Some(true),
            player::PauseState::Paused => Some(false),
            _ => None,
        }
    }
    fn accept_snapshot(&mut self, mut snapshot: player::PlayerSnapshot) {
        if !self.replay {
            if self.completed_results.is_some() {
                if let Some(current) = &self.snapshot {
                    snapshot.completed_results = current.completed_results.clone();
                    snapshot.players = current.players.clone();
                }
            } else if let Some(results) = &snapshot.completed_results {
                let roster: Vec<_> = snapshot
                    .players
                    .iter()
                    .map(|member| member.player)
                    .collect();
                let details: Vec<_> = snapshot
                    .players
                    .iter()
                    .map(|member| ResultDetails {
                        player: member.player,
                        score: &member.score,
                        competition: member.competition.as_ref(),
                    })
                    .collect();
                match ResultsView::new_with_details(results, &roster, &details) {
                    Ok(view) => {
                        self.completed_results = Some(view);
                        self.completed_results_error = None;
                    }
                    Err(error) => self.completed_results_error = Some(error),
                }
            }
        }
        // The archive belongs to this joined Game. A trailing final snapshot
        // cannot reset local Results selection or install another owner.
        if self.joined {
            if let Some(current) = self
                .snapshot
                .as_ref()
                .filter(|current| current.room_results.is_some())
            {
                snapshot.room_results = current.room_results.clone();
                snapshot.room = current.room.clone();
            }
        }
        let count = snapshot.players.len();
        if count > 0 && !(self.joined && !self.replay && self.completed_results.is_some()) {
            self.local_page = self.local_page.min(
                count
                    .div_ceil(organisms::LOCAL_PLAYERS_PER_PAGE)
                    .saturating_sub(1),
            );
        }
        self.snapshot = Some(snapshot);
        if self.joined && !self.replay && self.completed_results.is_some() {
            self.local_page = self
                .local_page
                .min(self.presentation_page_count().saturating_sub(1));
        }
    }
    fn retry_available(&self) -> bool {
        self.prepared_retry.is_none() && (self.joined || !self.cancelling)
    }
    fn practice_position(&self) -> Option<PracticeStart> {
        if self.replay || self.joined || self.cancelling || self.prepared_retry.is_some() {
            return None;
        }
        let snapshot = self.snapshot.as_ref()?;
        if snapshot.status != player::PlayerStatus::Playing
            || snapshot.cancelled
            || snapshot.room.is_some()
        {
            return None;
        }
        let nanos = snapshot.song_time?.as_nanos();
        if nanos < 0 {
            return None;
        }
        PracticeStart::from_nanoseconds(nanos).ok()
    }
    fn mark_practice(&mut self) -> Result<(), String> {
        let position = self
            .practice_position()
            .ok_or("practice mark requires a live native song position")?;
        self.practice_bookmark = Some(position);
        self.practice_loop = None;
        self.loop_enabled = false;
        Ok(())
    }
    fn loop_controls_available(&self) -> bool {
        self.practice_position().is_some()
            && !self.network_launch()
            && self.snapshot.as_ref().is_some_and(|snapshot| {
                matches!(
                    snapshot.pause,
                    player::PauseState::Running
                        | player::PauseState::Paused
                        | player::PauseState::Unavailable
                )
            })
    }
    fn network_launch(&self) -> bool {
        self.launch.args().chunks_exact(2).any(|pair| {
            matches!(
                pair[0].as_str(),
                "--mp-host" | "--mp-join" | "--mp-webtransport" | "--mp-room"
            )
        })
    }
    fn mark_loop_end(&mut self) -> Result<(), String> {
        if !self.loop_controls_available() {
            return Err("loop end requires a stable live nonnetwork position".into());
        }
        let start = self
            .practice_bookmark
            .ok_or("mark loop start with F7 first")?;
        let end = self.practice_position().expect("loop position admission");
        let region = PracticeLoop::new(start, end)?;
        self.practice_loop = Some(region);
        self.loop_enabled = false;
        Ok(())
    }
    fn toggle_loop(&mut self) -> Result<(), String> {
        if !self.loop_controls_available() || self.practice_loop.is_none() {
            return Err("loop requires a stable live nonnetwork start/end region".into());
        }
        self.loop_enabled = !self.loop_enabled;
        Ok(())
    }
    fn loop_due(&self) -> bool {
        self.loop_enabled
            && !self.network_launch()
            && self.joined
            && self.worker.is_none()
            && !self.replay
            && !self.cancelling
            && self.prepared_retry.is_none()
            && !self.viewer.pause_requested()
            && self.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.status == player::PlayerStatus::Finished
                    && !snapshot.cancelled
                    && self.practice_loop.is_some_and(|region| {
                        snapshot.completed_end
                            == Some(beatkernel::time::Timestamp::from_nanos(
                                region.end().nanoseconds(),
                            ))
                    })
            })
    }
    fn practice_restart_available(&self) -> bool {
        !self.replay
            && self
                .snapshot
                .as_ref()
                .is_none_or(|snapshot| snapshot.room.is_none())
            && self.practice_bookmark.is_some()
            && self.retry_available()
    }
    fn cancel(&mut self) {
        self.prepared_retry = None;
        self.loop_enabled = false;
        if !self.joined {
            self.viewer.cancel();
            self.cancelling = true;
        }
    }
    /// Called only after the finished native worker has joined and its final
    /// snapshot has been drained. Cleanup failure discards automatic retry.
    fn owner_finished(&mut self, succeeded: bool) -> Option<SessionLaunch> {
        self.joined = true;
        let prepared = self.prepared_retry.take();
        if !succeeded {
            self.loop_enabled = false;
        }
        if succeeded { prepared } else { None }
    }
}
fn spawn_game(
    native: Native,
    launch: SessionLaunch,
    local_page: usize,
    local_comparisons: bool,
    replay: bool,
) -> Result<Game, String> {
    let args = launch.args().to_vec();
    let (publisher, viewer) = player::channel();
    let worker = thread::Builder::new()
        .name("bms-game".into())
        .spawn(move || {
            player::with_publisher(publisher, || {
                native(&args).map_err(|error| error.to_string())
            })
        })
        .map_err(|error| error.to_string())?;
    Ok(Game {
        viewer,
        worker: Some(worker),
        snapshot: None,
        completed_results: None,
        completed_results_error: None,
        cancelling: false,
        joined: false,
        local_page,
        local_comparisons,
        replay,
        launch,
        prepared_retry: None,
        practice_bookmark: None,
        practice_loop: None,
        loop_enabled: false,
    })
}

impl Drop for Game {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.viewer.cancel();
            let _ = worker.join();
        }
    }
}

pub(super) fn run(
    args: &[String],
    native: Native,
    validate: Native,
    query_devices: QueryDevices,
    replay: Native,
    validate_replay: Native,
) -> Result<(), Box<dyn Error>> {
    if args.len() == 1 && args[0] == "--help" {
        println!(
            "player (--library DIR | --chart PATH) [--profile PATH] [--title-font PATH [--fallback-font PATH]...] [--ui-lookahead-ms 100..10000] [--ui-fps 30..240] [--gpu-backend auto|vulkan|dx12|metal|gl] [--present fifo|immediate|mailbox] NATIVE_OPTIONS\nSolo devices are automatic. Advanced native overrides and key bindings use flag-value pairs.\nF2: settings or audio output in supported paused play; F3 in selection: search; F4 in settings: records; F6 in settings: practice; W in records list: watch; Up/Down: select; Enter: play/return; PageUp/PageDown: local player pages; C: toggle local comparisons; F5: retry pinned start and disable loop; F7: mark live position/loop start; F8: restart mark after cleanup; F9: pause/resume when native owner supports it; F10: mark loop end; F11: toggle native finite loop (live nonnetwork only, joins before restart; reopening may leave a gap); Escape or focus loss: cancel; close: cancel and drain.\nUI keys do not provide gameplay input. Use the native play command's help for platform options."
        );
        return Ok(());
    }
    let options = Options::parse(args)?;
    let startup = spawn_startup_profile(&options)?;
    let selection_items: Arc<[SelectionItem]> = Arc::from([]);
    let catalog_search = CatalogSearch::new(&selection_items)?;
    let search_editor = LineEditor::new("", 256)?;
    let event_loop = EventLoop::new()?;
    let active_backend = options.backend;
    let mut app = Desktop {
        options,
        active_backend,
        display: None,
        display_view: None,
        players_view: None,
        devices_view: None,
        practice: None,
        records: None,
        records_view: None,
        native,
        validate,
        query_devices,
        replay,
        validate_replay,
        picker: None,
        local_setup: None,
        accepted_local: None,
        live_audio: None,
        live_audio_view: None,
        settings: None,
        settings_view: None,
        profile_io: None,
        startup,
        catalog_message: None,
        catalog_progress: player_chart::ScanProgress::default(),
        catalog: None,
        entries: Vec::new(),
        selected: 0,
        selection_items,
        catalog_search,
        search_editor,
        search_focused: false,
        catalog_wheel: WheelSteps::default(),
        ime: ImeDraft::default(),
        modifiers: ModifiersState::empty(),
        clipboard: None,
        pending_clipboard: None,
        title_font: None,
        font_text: None,
        input_font: None,
        input_font_error: None,
        selection_diagnostics: Arc::from([]),
        selection_view: None,
        painted_reactive: None,
        window: None,
        renderer: None,
        renderer_startup: None,
        bga_cache: BgaTextureCache::default(),
        instance: None,
        scene: Scene::new(WIDTH as u32, HEIGHT as u32),
        game: None,
        navigator: ScreenNavigator::default(),
        active: false,
        occluded: false,
        failure: None,
        fatal: None,
        next_frame: Instant::now(),
        pointer: None,
        gesture: Gesture::default(),
        hits: Vec::with_capacity(32),
    };
    if app.startup.is_none() {
        app.begin_selection()?;
    }
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.fatal {
        return Err(error.into());
    }
    Ok(())
}

const SETTINGS_ROWS: usize = 10;
struct LiveAudioDraft {
    values: NativeSettings,
    loaded_args: Vec<String>,
    selected: usize,
    editor: LineEditor,
    request: Option<u64>,
    message: Option<String>,
}
impl LiveAudioDraft {
    fn is_unchanged(&self) -> bool {
        self.values.native_args() == self.loaded_args
            && self
                .values
                .fields()
                .get(self.selected)
                .is_some_and(|field| self.editor.value() == field.value)
    }
    fn refresh_applied(
        &mut self,
        capability: &beatkernel_bms_runtime::live_output_control::OutputCapability,
    ) -> Result<(), String> {
        let values = capability.settings()?;
        let selected = self.selected.min(values.fields().len().saturating_sub(1));
        let field = values
            .fields()
            .get(selected)
            .ok_or("output fields unavailable")?;
        let editor = LineEditor::new(&field.value, 4096)?;
        let loaded_args = values.native_args();
        self.values = values;
        self.loaded_args = loaded_args;
        self.selected = selected;
        self.editor = editor;
        Ok(())
    }
    fn select(&mut self, index: usize) -> Result<(), String> {
        let field = self
            .values
            .fields()
            .get(index)
            .ok_or("output field unavailable")?;
        self.editor = LineEditor::new(&field.value, 4096)?;
        self.selected = index;
        Ok(())
    }
    fn edit(&mut self, key: Option<KeyCode>, value: Option<&str>) {
        let before = self.editor.clone();
        let result = edit_line(&mut self.editor, key, value)
            .and_then(|_| self.values.set_value(self.selected, self.editor.value()));
        if let Err(error) = result {
            self.editor = before;
            self.message = Some(error);
        } else {
            self.message = None;
        }
    }
}
struct SettingsDraft {
    values: NativeSettings,
    presentation: PresentationSettings,
    cached_local: Option<LocalSetup>,
    selected: usize,
    editor: LineEditor,
    profile: LineEditor,
    profile_focused: bool,
    message: Option<String>,
    error: Option<String>,
}
impl SettingsDraft {
    fn refresh_selected(&mut self) -> Result<(), String> {
        self.select(self.selected.min(self.values.fields().len() - 1))
    }
    fn select(&mut self, index: usize) -> Result<(), String> {
        let field = self
            .values
            .fields()
            .get(index)
            .ok_or("settings row unavailable")?;
        self.editor = LineEditor::new(&field.value, 4096)?;
        self.selected = index;
        self.profile_focused = false;
        Ok(())
    }
    fn edit(&mut self, key: Option<KeyCode>, value: Option<&str>) {
        self.message = None;
        if self.profile_focused {
            self.error = edit_line(&mut self.profile, key, value).err();
            self.message = None;
            return;
        }
        let before = self.editor.clone();
        let result = edit_line(&mut self.editor, key, value)
            .and_then(|()| self.values.set_value(self.selected, self.editor.value()));
        if let Err(error) = result {
            // Preserve the last accepted bounded draft if an edit exceeds a model limit.
            self.editor = before;
            self.error = Some(error);
        } else {
            self.error = None;
        }
    }
}
fn edit_line(
    editor: &mut LineEditor,
    key: Option<KeyCode>,
    value: Option<&str>,
) -> Result<(), String> {
    match key {
        Some(KeyCode::ArrowLeft) => editor.left(),
        Some(KeyCode::ArrowRight) => editor.right(),
        Some(KeyCode::Home) => editor.home(),
        Some(KeyCode::End) => editor.end(),
        Some(KeyCode::Delete) => editor.delete(),
        Some(KeyCode::Backspace) => editor.backspace(),
        _ => return value.map_or(Ok(()), |value| editor.insert(value)),
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionCommand {
    All,
    Left,
    Right,
    Home,
    End,
}
fn selection_command(
    physical: PhysicalKey,
    logical: &Key,
    modifiers: ModifiersState,
    macos: bool,
) -> Option<SelectionCommand> {
    let command = if macos {
        ModifiersState::SUPER
    } else {
        ModifiersState::CONTROL
    };
    if modifiers == command
        && matches!(logical, Key::Character(value) if value.eq_ignore_ascii_case("a"))
    {
        return Some(SelectionCommand::All);
    }
    if modifiers != ModifiersState::SHIFT {
        return None;
    }
    match physical {
        PhysicalKey::Code(KeyCode::ArrowLeft) => Some(SelectionCommand::Left),
        PhysicalKey::Code(KeyCode::ArrowRight) => Some(SelectionCommand::Right),
        PhysicalKey::Code(KeyCode::Home) => Some(SelectionCommand::Home),
        PhysicalKey::Code(KeyCode::End) => Some(SelectionCommand::End),
        _ => None,
    }
}
fn clipboard_command(
    logical: &Key,
    modifiers: ModifiersState,
    macos: bool,
) -> Option<ClipboardAction> {
    let command = if macos {
        ModifiersState::SUPER
    } else {
        ModifiersState::CONTROL
    };
    if modifiers != command {
        return None;
    }
    match logical {
        Key::Character(value) if value.eq_ignore_ascii_case("c") => Some(ClipboardAction::Copy),
        Key::Character(value) if value.eq_ignore_ascii_case("x") => Some(ClipboardAction::Cut),
        Key::Character(value) if value.eq_ignore_ascii_case("v") => Some(ClipboardAction::Paste),
        _ => None,
    }
}
const DISPLAY_FLAGS: [&str; 4] = [
    "--gpu-backend",
    "--present",
    "--ui-fps",
    "--ui-lookahead-ms",
];
struct DisplayDraft {
    editors: [LineEditor; 4],
    selected: usize,
    error: Option<String>,
}
impl DisplayDraft {
    fn new(value: PresentationSettings) -> Result<Self, String> {
        Ok(Self {
            editors: [
                LineEditor::new(value.backend.as_str(), 32)?,
                LineEditor::new(value.presentation.as_str(), 32)?,
                LineEditor::new(&value.fps.to_string(), 32)?,
                LineEditor::new(&value.lookahead_ms.to_string(), 32)?,
            ],
            selected: 0,
            error: None,
        })
    }
    fn value(&self) -> Result<PresentationSettings, String> {
        let args: Vec<String> = DISPLAY_FLAGS
            .iter()
            .zip(&self.editors)
            .flat_map(|(flag, editor)| [(*flag).to_owned(), editor.value().to_owned()])
            .collect();
        PresentationSettings::default().apply_overrides(&args)
    }
    fn edit(&mut self, key: Option<KeyCode>, value: Option<&str>) {
        self.error = edit_line(&mut self.editors[self.selected], key, value).err();
    }
}

struct PracticeDraft {
    editor: LineEditor,
    end_editor: LineEditor,
    end_focused: bool,
    error: Option<String>,
    view: PracticeView,
}
impl PracticeDraft {
    fn edit(&mut self, key: Option<KeyCode>, value: Option<&str>) {
        let editor = if self.end_focused {
            &mut self.end_editor
        } else {
            &mut self.editor
        };
        self.error = edit_line(editor, key, value).err();
    }
    fn clear_end(&mut self) {
        self.end_editor = LineEditor::new("", 64).expect("empty bounded practice end");
        self.error = None;
    }
    fn reset(&mut self) {
        match LineEditor::new("0:00", 64) {
            Ok(editor) => {
                self.editor = editor;
                self.clear_end();
                self.end_focused = false;
            }
            Err(error) => self.error = Some(error),
        }
    }
}

struct RecordsDraft {
    details: bool,
    grade_page: usize,
    chart: PathBuf,
    directory: LineEditor,
    directory_focused: bool,
    catalog: Option<RecordCatalog>,
    selected: Option<usize>,
    first: usize,
    preview: Option<RecordPreview>,
    error: Option<String>,
    message: Option<String>,
}
impl RecordsDraft {
    fn new(chart: PathBuf) -> Result<Self, String> {
        let parent = chart
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."));
        let directory = LineEditor::new(
            parent.to_str().ok_or("record directory must be UTF-8")?,
            4096,
        )?;
        Ok(Self {
            details: false,
            grade_page: 0,
            chart,
            directory,
            directory_focused: true,
            catalog: None,
            selected: None,
            first: 0,
            preview: None,
            error: None,
            message: None,
        })
    }
    fn selected_path(&self) -> Option<&PathBuf> {
        self.catalog.as_ref()?.entries.get(self.selected?)
    }
    fn valid_preview(&self) -> Option<&RecordPreview> {
        self.preview
            .as_ref()
            .filter(|preview| self.selected_path() == Some(&preview.path))
    }
    fn select(&mut self, index: usize) {
        if self.details {
            return;
        }
        if self
            .catalog
            .as_ref()
            .is_some_and(|catalog| index < catalog.entries.len())
        {
            if self.selected != Some(index) {
                self.details = false;
                self.grade_page = 0;
                self.preview = None;
                self.message = None;
            }
            self.selected = Some(index);
            self.directory_focused = false;
            self.first = index / SETTINGS_ROWS * SETTINGS_ROWS;
        }
    }
    fn page(&mut self, forward: bool) {
        if self.details {
            return;
        }
        let count = self
            .catalog
            .as_ref()
            .map_or(0, |catalog| catalog.entries.len());
        if count == 0 {
            return;
        }
        let first = if forward {
            (self.first + SETTINGS_ROWS).min((count - 1) / SETTINGS_ROWS * SETTINGS_ROWS)
        } else {
            self.first.saturating_sub(SETTINGS_ROWS)
        };
        self.select(first);
    }
    fn edit(&mut self, key: Option<KeyCode>, value: Option<&str>) {
        if self.details || !self.directory_focused {
            return;
        }
        let before = self.directory.value().to_owned();
        self.error = edit_line(&mut self.directory, key, value).err();
        if self.directory.value() != before {
            self.details = false;
            self.grade_page = 0;
            self.catalog = None;
            self.preview = None;
            self.selected = None;
            self.first = 0;
            self.message = None;
        }
    }
}

struct LocalDraft {
    model: LocalSetup,
    selected: usize,
    first: usize,
}

struct DevicePicker {
    catalog: DeviceCatalog,
    player: Option<PlayerId>,
    first: usize,
    selected: Option<usize>,
}
enum ProfileResult {
    Devices(DeviceCatalog, Option<PlayerId>),
    Loaded(PlayerProfile),
    Records(RecordCatalog),
    Record(RecordPreview),
    Saved,
}
struct ProfileOperation {
    owner: ScreenInstanceId,
    permit: TaskPermit,
    worker: Option<JoinHandle<Result<ProfileResult, String>>>,
}
impl Drop for ProfileOperation {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Check cancellation around a bounded metadata operation. An in-progress
/// filesystem/native metadata call still drains; its late result is discarded.
fn scoped_metadata(
    permit: TaskPermit,
    operation: impl FnOnce() -> Result<ProfileResult, String>,
) -> Result<ProfileResult, String> {
    if permit.is_cancelled() {
        return Err("panel task cancelled".into());
    }
    let result = operation();
    if permit.is_cancelled() {
        Err("panel task cancelled".into())
    } else {
        result
    }
}

fn release_panel<T>(panel: &mut Option<PanelScope<T>>, navigator: &ScreenNavigator) {
    synchronize_panel(panel, navigator);
    if panel
        .as_ref()
        .is_some_and(|panel| !navigator.retains(panel.id()))
    {
        *panel = None;
    }
}

fn synchronize_panel<T>(panel: &mut Option<PanelScope<T>>, navigator: &ScreenNavigator) {
    if let Some(panel) = panel {
        panel.synchronize(navigator);
    }
}

fn settings_host() -> SettingsHost {
    if cfg!(target_os = "windows") {
        SettingsHost::Windows
    } else if cfg!(target_os = "macos") {
        SettingsHost::Macos
    } else {
        SettingsHost::Linux
    }
}
fn without_chart(args: &[String]) -> Vec<String> {
    args.chunks_exact(2)
        .filter(|pair| pair[0] != "--chart")
        .flat_map(|pair| pair.iter().cloned())
        .collect()
}
fn with_chart(args: &[String], path: &str) -> Vec<String> {
    let mut args = without_chart(args);
    args.extend(["--chart".into(), path.into()]);
    args
}

/// Preserve the selected record across retry; watching cannot create captures.
fn record_launch(
    values: &NativeSettings,
    chart: &std::path::Path,
    record: &std::path::Path,
) -> Result<SessionLaunch, String> {
    let mut args: Vec<_> = values
        .native_args()
        .chunks_exact(2)
        .filter(|pair| pair[0] != "--record-replay")
        .flat_map(|pair| pair.iter().cloned())
        .collect();
    args.extend([
        "--chart".into(),
        chart.to_str().ok_or("chart path must be UTF-8")?.into(),
        "--replay".into(),
        record.to_str().ok_or("record path must be UTF-8")?.into(),
    ]);
    SessionLaunch::new(args)
}

type ImeField = TextField;
type ImeTarget = TextTarget;
#[derive(Default)]
struct ImeDraft {
    target: Option<ImeTarget>,
    enabled: bool,
    composing: bool,
    preview: Option<LineEditor>,
    cursor_area: Option<(WindowId, ImeTarget, [u32; 4])>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextField {
    Search,
    Setting(usize),
    LiveOutput(usize),
    Profile,
    Display(usize),
    PracticeStart,
    PracticeEnd,
    RecordDirectory,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TextTarget {
    screen: ScreenInstanceId,
    field: TextField,
}
struct PendingClipboard {
    target: TextTarget,
    edit: ClipboardEdit,
}
struct PreparedRenderer {
    instance: wgpu::Instance,
    renderer: Renderer,
}
struct RendererStartup {
    job: NativeCatalog<PreparedRenderer>,
    retired: bool,
}
struct Desktop {
    options: Options,
    active_backend: BackendChoice,
    display: Option<PanelScope<DisplayDraft>>,
    display_view: Option<DisplayView>,
    players_view: Option<PlayersView>,
    devices_view: Option<DevicesView>,
    practice: Option<PanelScope<PracticeDraft>>,
    records: Option<PanelScope<RecordsDraft>>,
    records_view: Option<RecordsView>,
    native: Native,
    validate: Native,
    query_devices: QueryDevices,
    replay: Native,
    validate_replay: Native,
    picker: Option<PanelScope<DevicePicker>>,
    local_setup: Option<PanelScope<LocalDraft>>,
    accepted_local: Option<LocalSetup>,
    live_audio: Option<PanelScope<LiveAudioDraft>>,
    live_audio_view: Option<beatkernel_bms_runtime::ui::live_audio::LiveAudioView>,
    settings: Option<PanelScope<SettingsDraft>>,
    settings_view: Option<SettingsView>,
    profile_io: Option<ProfileOperation>,
    startup: Option<NativeCatalog<Options>>,
    catalog: Option<NativeCatalog<PreparedCatalog>>,
    catalog_progress: player_chart::ScanProgress,
    catalog_message: Option<String>,
    entries: Vec<Entry>,
    selected: usize,
    selection_items: Arc<[SelectionItem]>,
    catalog_search: CatalogSearch,
    search_editor: LineEditor,
    search_focused: bool,
    catalog_wheel: WheelSteps,
    ime: ImeDraft,
    modifiers: ModifiersState,
    clipboard: Option<ClipboardWorker>,
    pending_clipboard: Option<PendingClipboard>,
    title_font: Option<Arc<FontAtlas>>,
    font_text: Option<FontText>,
    input_font: Option<FontText>,
    input_font_error: Option<String>,
    selection_diagnostics: Arc<[String]>,
    selection_view: Option<SelectionView>,
    painted_reactive: Option<ScreenInstanceId>,
    // On unexpected Desktop drop, join the transferred surface owner before
    // releasing this UI-owned window reference.
    renderer_startup: Option<RendererStartup>,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    bga_cache: BgaTextureCache,
    instance: Option<wgpu::Instance>,
    scene: Scene,
    game: Option<Game>,
    navigator: ScreenNavigator,
    active: bool,
    occluded: bool,
    failure: Option<String>,
    fatal: Option<String>,
    next_frame: Instant,
    pointer: Option<(f64, f64)>,
    gesture: Gesture,
    hits: Vec<(ControlId, Bounds)>,
}
impl Desktop {
    fn startup_busy(&self) -> bool {
        self.startup.as_ref().is_some_and(|startup| {
            !startup.is_finished()
                || self.closing()
                || (!self.is_suspended()
                    && !self.occluded
                    && self.navigator.route() == ScreenRoute::Selection)
        })
    }
    fn collect_startup(&mut self) {
        if self.startup.is_none()
            || (!self.closing()
                && (self.is_suspended()
                    || self.occluded
                    || self.navigator.route() != ScreenRoute::Selection))
        {
            return;
        }
        let Some(result) = self.startup.as_mut().and_then(NativeCatalog::poll) else {
            return;
        };
        self.startup = None;
        if self.closing() {
            return;
        }
        let result = result.and_then(|options| {
            self.active_backend = options.backend;
            self.options = options;
            self.initialize_renderer()?;
            self.begin_selection()
        });
        if let Err(error) = result {
            self.fail(error);
            return;
        }
        self.next_frame = Instant::now();
        if let Some(window) = &self.window {
            if !self.renderer_pending() {
                window.set_title("BeatKernel BMS player");
                window.request_redraw();
            }
        }
    }
    fn begin_selection(&mut self) -> Result<(), String> {
        if self.startup.is_some() || self.closing() {
            return Err("selection waits for startup profile preparation".into());
        }
        let (catalog, message) = match spawn_catalog(&self.options) {
            Ok(Some(catalog)) => (
                Some(catalog),
                Some(if self.options.library.is_some() {
                    "Loading library catalog…".into()
                } else {
                    "Preparing chart title font…".into()
                }),
            ),
            Ok(None) => (None, None),
            Err(error) => (
                None,
                Some(format!("Selection preparation unavailable: {error}")),
            ),
        };
        let entries = if self.options.library.is_some() || self.options.title_font.is_some() {
            Vec::new()
        } else {
            vec![direct_entry(
                self.options
                    .chart
                    .clone()
                    .ok_or("missing chart selection")?,
            )]
        };
        let items: Arc<[SelectionItem]> = entries
            .iter()
            .map(|entry| SelectionItem {
                title: entry.title.clone(),
                artist: entry.artist.clone(),
            })
            .collect::<Vec<_>>()
            .into();
        let search = CatalogSearch::new(&items)?;
        self.catalog = catalog;
        self.catalog_message = message;
        self.catalog_progress = player_chart::ScanProgress::default();
        self.entries = entries;
        self.selection_items = items;
        self.catalog_search = search;
        self.selection_diagnostics = Arc::from([]);
        self.selection_view = None;
        self.invalidate_hits();
        Ok(())
    }
    fn catalog_busy(&self) -> bool {
        // A worker can finish just after collect_catalog's poll. Keep waking
        // active Selection until it has actually consumed that joined result.
        self.catalog.as_ref().is_some_and(|catalog| {
            !catalog.is_finished()
                || self.closing()
                || (!self.is_suspended()
                    && !self.renderer_pending()
                    && self.navigator.route() == ScreenRoute::Selection)
        })
    }
    fn collect_catalog(&mut self) {
        let Some(catalog) = &mut self.catalog else {
            return;
        };
        let progress = catalog.progress();
        let changed = progress != self.catalog_progress;
        if changed {
            self.catalog_progress = progress;
            self.catalog_message = Some(if self.options.library.is_some() {
                format!(
                    "{} - {} charts, {} entries, {} bytes",
                    if progress.stage == player_chart::ScanStage::Complete {
                        "Preparing search and title font"
                    } else {
                        "Loading library catalog"
                    },
                    progress.charts,
                    progress.entries,
                    progress.bytes
                )
            } else {
                "Preparing chart title font…".into()
            });
        }
        // A hidden/suspended Selection does not receive another panel's draft
        // or reactive bindings. The finished thread retains its owned result.
        if !self.closing()
            && (self.is_suspended()
                || self.renderer_pending()
                || self.navigator.route() != ScreenRoute::Selection)
        {
            return;
        }
        let result = self.catalog.as_mut().and_then(NativeCatalog::poll);
        let completed = result.is_some();
        if let Some(result) = result {
            self.catalog = None;
            if self.closing() {
                return;
            }
            match result.and_then(|catalog| self.install_catalog(catalog)) {
                Ok(()) => self.catalog_message = None,
                Err(error) => self.catalog_message = Some(format!("Catalog unavailable: {error}")),
            }
        }
        if (changed || completed) && !self.closing() && !self.is_suspended() && !self.occluded {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
    fn install_catalog(&mut self, catalog: PreparedCatalog) -> Result<(), String> {
        if self.closing() || self.is_suspended() || self.navigator.route() != ScreenRoute::Selection
        {
            return Err("catalog Selection is not active".into());
        }
        let search_editor = LineEditor::new("", 256)?;
        // Preflight GPU ownership before publishing any catalog component.
        let font = if let (Some(renderer), Some(atlas)) = (&mut self.renderer, &catalog.font) {
            let texture = renderer.upload_texture(atlas.image())?;
            match FontText::new(Arc::clone(atlas), texture) {
                Ok(font) => Some(font),
                Err(error) => {
                    let _ = renderer.remove_texture(texture);
                    return Err(error);
                }
            }
        } else {
            None
        };
        if let (Some(renderer), Some(previous)) = (&mut self.renderer, &self.font_text) {
            if let Err(error) = renderer.remove_texture(previous.texture_id()) {
                if let Some(font) = &font {
                    let _ = renderer.remove_texture(font.texture_id());
                }
                return Err(error);
            }
        }
        self.entries = catalog.entries;
        self.selection_items = catalog.items;
        self.catalog_search = catalog.search;
        self.selection_diagnostics = catalog.diagnostics;
        self.title_font = catalog.font;
        self.selected = 0;
        self.search_editor = search_editor;
        self.search_focused = false;
        self.catalog_wheel.reset();
        self.gesture.cancel();
        self.bind_title_font(font);
        Ok(())
    }
    fn bind_title_font(&mut self, font: Option<FontText>) {
        self.font_text = font;
        self.selection_view = None;
        self.invalidate_hits();
    }
    fn invalidate_hits(&mut self) {
        self.sync_ime();
        self.sync_input_font();
        self.hits.clear();
        // A retained scene must restore hit regions even if its signals are equal.
        self.painted_reactive = None;
        self.ime.cursor_area = None;
    }
    /// Prepare changed field glyphs at UI state boundaries, never in retained paint.
    fn sync_input_font(&mut self) {
        if self.startup.is_some() || self.renderer_pending() {
            return;
        }
        self.input_font_error = None;
        if let Err(error) = self.prepare_input_font() {
            self.input_font = None;
            self.input_font_error = Some(format!("FIELD FONT: {error} - USING BITMAP"));
        }
    }
    fn prepare_input_font(&mut self) -> Result<(), String> {
        let Some(current) = &self.title_font else {
            self.input_font = None;
            return Ok(());
        };
        let mut values = [""; 11];
        let count = match self.navigator.route() {
            ScreenRoute::Selection => {
                values[0] = self
                    .ime_editor(ImeField::Search, &self.search_editor)
                    .value();
                1
            }
            ScreenRoute::LiveAudio => {
                if let Some(draft) = &self.live_audio {
                    let count = draft.values.fields().len().min(values.len());
                    for (index, field) in draft.values.fields().iter().take(count).enumerate() {
                        values[index] = if index == draft.selected {
                            self.ime_editor(ImeField::LiveOutput(index), &draft.editor)
                                .value()
                        } else {
                            &field.value
                        };
                    }
                    count
                } else {
                    0
                }
            }
            ScreenRoute::Settings => {
                if let Some(draft) = &self.settings {
                    let first = draft.selected / SETTINGS_ROWS * SETTINGS_ROWS;
                    for (slot, (index, field)) in draft
                        .values
                        .fields()
                        .iter()
                        .enumerate()
                        .skip(first)
                        .take(SETTINGS_ROWS)
                        .enumerate()
                    {
                        values[slot] = if index == draft.selected {
                            self.ime_editor(ImeField::Setting(index), &draft.editor)
                                .value()
                        } else {
                            field.value.as_str()
                        };
                    }
                    values[10] = self.ime_editor(ImeField::Profile, &draft.profile).value();
                }
                11
            }
            ScreenRoute::Display => {
                if let Some(draft) = &self.display {
                    for (slot, editor) in draft.editors.iter().enumerate() {
                        values[slot] = self.ime_editor(ImeField::Display(slot), editor).value();
                    }
                }
                4
            }
            ScreenRoute::Practice => {
                if let Some(draft) = &self.practice {
                    values[0] = self
                        .ime_editor(ImeField::PracticeStart, &draft.editor)
                        .value();
                    values[1] = self
                        .ime_editor(ImeField::PracticeEnd, &draft.end_editor)
                        .value();
                }
                2
            }
            ScreenRoute::Records => {
                if let Some(draft) = &self.records {
                    values[0] = self
                        .ime_editor(ImeField::RecordDirectory, &draft.directory)
                        .value();
                }
                1
            }
            _ => return Ok(()),
        };
        let next = FontAtlas::extend_texts(current, &values[..count])?;
        let changed = !Arc::ptr_eq(current, &next);
        if !changed && self.renderer.is_some() && self.font_text.is_some() {
            self.input_font = self.font_text.clone();
            return Ok(());
        }
        let binding = if let Some(renderer) = &mut self.renderer {
            let texture = if let Some(font) = &self.font_text {
                if changed {
                    renderer.update_texture(font.texture_id(), next.image())?;
                }
                font.texture_id()
            } else {
                renderer.upload_texture(next.image())?
            };
            Some(FontText::new(Arc::clone(&next), texture)?)
        } else {
            None
        };
        self.title_font = Some(next);
        self.font_text = binding.clone();
        self.input_font = binding;
        Ok(())
    }
    fn closing(&self) -> bool {
        self.navigator.phase() == ScreenPhase::Exiting
    }
    fn is_suspended(&self) -> bool {
        self.navigator.phase() == ScreenPhase::Suspended
    }
    /// Panels observe the Navigator's lifecycle without owning another stack.
    fn synchronize_panel_lifecycles(&mut self) {
        synchronize_panel(&mut self.picker, &self.navigator);
        synchronize_panel(&mut self.records, &self.navigator);
        synchronize_panel(&mut self.display, &self.navigator);
        synchronize_panel(&mut self.practice, &self.navigator);
        synchronize_panel(&mut self.local_setup, &self.navigator);
        synchronize_panel(&mut self.settings, &self.navigator);
    }
    fn ui_ready(&self) -> bool {
        self.active
            && self.startup.is_none()
            && !self.renderer_pending()
            && self.navigator.phase() == ScreenPhase::Active
            && !self.occluded
            && self.profile_io.is_none()
    }
    /// Prepare an atomic route change before data preparation or thread spawn.
    fn live_output_available(&self) -> bool {
        self.game.as_ref().is_some_and(|game| {
            !game.replay
                && !game.joined
                && !game.cancelling
                && !game.network_launch()
                && game.snapshot.as_ref().is_some_and(|snapshot| {
                    !snapshot.cancelled
                        && snapshot.pause == player::PauseState::Paused
                        && snapshot.status == player::PlayerStatus::Playing
                })
                && game.viewer.pause_requested()
                && game.viewer.output_supported()
        })
    }
    fn open_live_audio(&mut self) {
        if !self.ui_ready()
            || self.navigator.route() != (ScreenRoute::Play { replay: false })
            || !self.live_output_available()
        {
            return;
        }
        let prepared = (|| -> Result<_, String> {
            let next = self.prepare_route(ScreenRoute::LiveAudio)?;
            let capability = self
                .game
                .as_ref()
                .ok_or("play owner unavailable")?
                .viewer
                .output_capability()
                .map_err(|e| e.to_string())?
                .ok_or("live output controls unsupported")?;
            let values = capability.settings()?;
            let editor = LineEditor::new(
                &values
                    .fields()
                    .first()
                    .ok_or("output fields unavailable")?
                    .value,
                4096,
            )?;
            let view = beatkernel_bms_runtime::ui::live_audio::LiveAudioView::new(
                next.active_id()
                    .ok_or("output screen identity unavailable")?,
                WIDTH as u32,
                HEIGHT as u32,
            )?;
            Ok((
                next,
                LiveAudioDraft {
                    loaded_args: values.native_args(),
                    values,
                    selected: 0,
                    editor,
                    request: None,
                    message: None,
                },
                view,
            ))
        })();
        match prepared {
            Ok((next, draft, view)) => {
                self.commit_route(next);
                self.live_audio = Some(PanelScope::new(
                    self.navigator.active_id().expect("live output route"),
                    draft,
                ));
                self.live_audio_view = Some(view);
                self.invalidate_hits();
            }
            Err(error) => self.failure = Some(error),
        }
    }
    fn apply_live_audio(&mut self) {
        if !self.ui_ready() || self.navigator.route() != ScreenRoute::LiveAudio {
            return;
        }
        let result = (|| -> Result<u64, String> {
            let draft = self.live_audio.as_ref().ok_or("output draft unavailable")?;
            self.game
                .as_ref()
                .ok_or("play owner unavailable")?
                .viewer
                .request_output(draft.values.native_args())
                .map_err(|e| e.to_string())
        })();
        if let Some(draft) = &mut self.live_audio {
            match result {
                Ok(id) => {
                    draft.request = Some(id);
                    draft.message = Some("Applying audio output settings...".into());
                }
                Err(error) => draft.message = Some(error),
            }
        }
        self.invalidate_hits();
    }
    fn live_audio_key(&mut self, key: KeyCode, repeat: bool) {
        if !repeat && key == KeyCode::Escape {
            self.back();
            return;
        }
        if !repeat && key == KeyCode::Enter {
            self.apply_live_audio();
            return;
        }
        if self
            .game
            .as_ref()
            .is_some_and(|game| game.viewer.output_pending())
        {
            return;
        }
        if let Some(draft) = &mut self.live_audio {
            if matches!(
                key,
                KeyCode::Tab
                    | KeyCode::ArrowUp
                    | KeyCode::ArrowDown
                    | KeyCode::PageUp
                    | KeyCode::PageDown
            ) {
                let count = draft.values.fields().len();
                let next = if key == KeyCode::PageUp {
                    draft.selected.saturating_sub(4)
                } else if key == KeyCode::PageDown {
                    (draft.selected + 4).min(count - 1)
                } else if key == KeyCode::ArrowUp {
                    (draft.selected + count - 1) % count
                } else {
                    (draft.selected + 1) % count
                };
                draft.message = draft.select(next).err();
                self.gesture.cancel();
            } else {
                draft.edit(Some(key), None);
            }
        }
        self.invalidate_hits();
    }
    fn draw_live_audio(&mut self) -> Result<(), String> {
        let draft = self.live_audio.as_ref().ok_or("output draft unavailable")?;
        let view = self
            .live_audio_view
            .as_ref()
            .ok_or("output view unavailable")?;
        let id = draft.id();
        let pending = self
            .game
            .as_ref()
            .is_some_and(|game| game.viewer.output_pending());
        let hovered = self.hit();
        let armed = beatkernel_bms_runtime::ui::live_audio::BUTTONS
            .iter()
            .map(|(id, _, _)| *id)
            .find(|id| self.gesture.is_armed(*id));
        view.update(beatkernel_bms_runtime::ui::live_audio::LiveAudioFrame {
            fields: draft.values.fields(),
            selected: draft.selected,
            editor: self.ime_editor(ImeField::LiveOutput(draft.selected), &draft.editor),
            message: draft.message.as_deref(),
            pending,
            hovered,
            armed,
        })?;
        view.set_input_font(self.input_font.clone());
        if view.dirty() || self.painted_reactive != Some(id) {
            view.compose(&mut self.scene, &mut self.hits)?;
            self.painted_reactive = Some(id);
        }
        self.render_scene()
    }
    fn prepare_route(&self, to: ScreenRoute) -> Result<ScreenNavigator, String> {
        if (self.startup.is_some() || self.renderer_pending()) && to != ScreenRoute::Closing {
            return Err("navigation waits for startup preparation".into());
        }
        if to != ScreenRoute::Closing
            && !matches!(
                to,
                ScreenRoute::Play { .. } | ScreenRoute::Results { .. } | ScreenRoute::LiveAudio
            )
            && self.game.as_ref().is_some_and(|game| !game.joined)
        {
            return Err("navigation waits for native session cleanup".into());
        }
        if to == ScreenRoute::LiveAudio && !self.live_output_available() {
            return Err("live output controls require a supported acknowledged pause".into());
        }
        let mut next = self.navigator.clone();
        next.navigate(
            to,
            self.profile_io.is_some(),
            self.game.as_ref().is_some_and(|game| game.joined),
        )?;
        Ok(next)
    }
    /// Only this boundary changes the active route and releases screen scopes.
    /// Native owners are drained separately and survive application Closing.
    fn commit_route(&mut self, next: ScreenNavigator) {
        self.catalog_wheel.reset();
        self.navigator = next;
        self.set_search_focus(false);
        self.gesture.cancel();
        self.invalidate_hits();
        self.pointer = None;
        // Children leave before their retained parent state.
        release_panel(&mut self.picker, &self.navigator);
        if self
            .devices_view
            .as_ref()
            .is_some_and(|view| !self.navigator.retains(view.id()))
        {
            self.devices_view = None;
        }
        release_panel(&mut self.records, &self.navigator);
        if self
            .records_view
            .as_ref()
            .is_some_and(|view| !self.navigator.retains(view.id()))
        {
            self.records_view = None;
        }
        release_panel(&mut self.display, &self.navigator);
        if self
            .display_view
            .as_ref()
            .is_some_and(|view| !self.navigator.retains(view.id()))
        {
            self.display_view = None;
        }
        release_panel(&mut self.practice, &self.navigator);
        release_panel(&mut self.local_setup, &self.navigator);
        if self
            .players_view
            .as_ref()
            .is_some_and(|view| !self.navigator.retains(view.id()))
        {
            self.players_view = None;
        }
        release_panel(&mut self.live_audio, &self.navigator);
        if self
            .live_audio_view
            .as_ref()
            .is_some_and(|view| !self.navigator.retains(view.id()))
        {
            self.live_audio_view = None;
        }
        release_panel(&mut self.settings, &self.navigator);
        if self
            .settings_view
            .as_ref()
            .is_some_and(|view| !self.navigator.retains(view.id()))
        {
            self.settings_view = None;
        }
        if self
            .selection_view
            .as_ref()
            .is_some_and(|view| !self.navigator.retains(view.id()))
        {
            self.selection_view = None;
        }
        self.painted_reactive = None;
        if self.navigator.route() != ScreenRoute::Closing
            && !self.navigator.route().contains(ScreenKind::Play)
        {
            self.game = None;
        }
        if !self.navigator.route().contains(ScreenKind::Play) {
            self.release_backgrounds();
        }
    }
    fn navigate(&mut self, to: ScreenRoute) -> Result<(), String> {
        let next = self.prepare_route(to)?;
        self.commit_route(next);
        Ok(())
    }
    fn back(&mut self) {
        if self.navigator.route() == ScreenRoute::Records
            && self.records.as_ref().is_some_and(|records| records.details)
        {
            if !self.ui_ready() {
                return;
            }
            let records = self.records.as_mut().expect("records route");
            records.details = false;
            records.grade_page = 0;
            self.gesture.cancel();
            self.sync_ime();
            self.invalidate_hits();
            return;
        }
        if let Some(to) = self.navigator.back_target() {
            if let Err(error) = self.navigate(to) {
                self.failure = Some(error);
            }
        }
    }
    fn request_close(&mut self) {
        // Closing is always admitted, including pending tasks and native owners.
        if let Ok(next) = self.prepare_route(ScreenRoute::Closing) {
            self.commit_route(next);
        }
        self.cancel();
        if let Some(startup) = &self.startup {
            startup.cancel();
        }
        self.retire_renderer_startup();
        if let Some(catalog) = &self.catalog {
            catalog.cancel();
        }
        if let Some(clipboard) = &mut self.clipboard {
            clipboard.begin_close();
        }
    }
    fn metadata_scope(&self) -> Result<(ScreenInstanceId, TaskPermit), String> {
        let scope = match self.navigator.route() {
            ScreenRoute::Settings => self
                .settings
                .as_ref()
                .map(|scope| (scope.id(), scope.task_permit())),
            ScreenRoute::Records => self
                .records
                .as_ref()
                .map(|scope| (scope.id(), scope.task_permit())),
            ScreenRoute::Players => self
                .local_setup
                .as_ref()
                .map(|scope| (scope.id(), scope.task_permit())),
            ScreenRoute::Devices { .. } => self
                .picker
                .as_ref()
                .map(|scope| (scope.id(), scope.task_permit())),
            _ => None,
        }
        .ok_or("metadata panel unavailable")?;
        if !scope.1.is_active() || !self.navigator.accepts(scope.0) {
            return Err("metadata panel is not active".into());
        }
        Ok(scope)
    }
    fn open_settings(&mut self) {
        if !self.ui_ready() || self.navigator.route() != ScreenRoute::Selection {
            return;
        }
        let result = (|| {
            let next = self.prepare_route(ScreenRoute::Settings)?;
            let values =
                NativeSettings::from_args(&without_chart(&self.options.native), settings_host())?;
            let editor = LineEditor::new(
                &values.fields().first().ok_or("no settings fields")?.value,
                4096,
            )?;
            let draft = SettingsDraft {
                values,
                presentation: self.options.display(),
                cached_local: self.accepted_local.clone(),
                selected: 0,
                editor,
                profile: LineEditor::new(
                    self.options
                        .profile
                        .as_ref()
                        .map(|path| path.to_str().ok_or("profile path must be UTF-8"))
                        .transpose()?
                        .unwrap_or(""),
                    4096,
                )?,
                profile_focused: false,
                message: None,
                error: None,
            };
            Ok::<_, String>((next, draft))
        })();
        match result {
            Ok((next, draft)) => {
                self.commit_route(next);
                self.settings = Some(PanelScope::new(
                    self.navigator.active_id().expect("settings route"),
                    draft,
                ));
                self.failure = None;
            }
            Err(error) => self.failure = Some(error),
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn open_local(&mut self) {
        if !self.ui_ready() || self.navigator.route() != ScreenRoute::Settings {
            return;
        }
        let result = (|| {
            let next = self.prepare_route(ScreenRoute::Players)?;
            let draft = self.settings.as_ref().ok_or("settings unavailable")?;
            let parsed = LocalSetup::from_settings(&draft.values, settings_host())?;
            let model = draft
                .cached_local
                .as_ref()
                .filter(|old| {
                    old.players() == parsed.players()
                        || (old.players().len() == 1 && parsed.players().len() == 1)
                })
                .cloned()
                .unwrap_or(parsed);
            let local = LocalDraft {
                model,
                selected: 0,
                first: 0,
            };
            Ok::<_, String>((next, local))
        })();
        match result {
            Ok((next, local)) => {
                self.commit_route(next);
                self.local_setup = Some(PanelScope::new(
                    self.navigator.active_id().expect("players route"),
                    local,
                ));
                self.local_error(None);
            }
            Err(error) => self.local_error(Some(error)),
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn local_error(&mut self, error: Option<String>) {
        if let Some(draft) = &mut self.settings {
            draft.error = error;
            draft.message = None;
        }
    }
    fn resize_local(&mut self, increase: bool) {
        let result = (|| {
            let local = self.local_setup.as_mut().ok_or("local setup unavailable")?;
            let count = local.model.players().len();
            let next = if increase {
                count.checked_add(1).ok_or("player count overflow")?
            } else {
                count.saturating_sub(1).max(1)
            };
            local.model.resize(next)?;
            local.selected = local.selected.min(next - 1);
            local.first = local.selected / SETTINGS_ROWS * SETTINGS_ROWS;
            Ok::<(), String>(())
        })();
        self.local_error(result.err());
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn clear_local(&mut self) {
        let result = (|| {
            let local = self.local_setup.as_mut().ok_or("local setup unavailable")?;
            let player = local
                .model
                .players()
                .get(local.selected)
                .ok_or("player unavailable")?
                .id;
            local.model.clear(player)
        })();
        self.local_error(result.err());
    }
    fn local_page(&mut self, forward: bool) {
        if let Some(local) = &mut self.local_setup {
            let last =
                local.model.players().len().saturating_sub(1) / SETTINGS_ROWS * SETTINGS_ROWS;
            local.first = if forward {
                (local.first + SETTINGS_ROWS).min(last)
            } else {
                local.first.saturating_sub(SETTINGS_ROWS)
            };
            local.selected = local.first;
        }
    }
    fn local_key(&mut self, key: KeyCode, repeat: bool) {
        match key {
            KeyCode::Escape if !repeat => self.back(),
            KeyCode::Enter if !repeat => self.finish_local(),
            KeyCode::Equal | KeyCode::NumpadAdd if !repeat => self.resize_local(true),
            KeyCode::Minus | KeyCode::NumpadSubtract if !repeat => self.resize_local(false),
            KeyCode::Space if !repeat => self.device_request(true),
            KeyCode::Delete if !repeat => self.clear_local(),
            KeyCode::PageUp => self.local_page(false),
            KeyCode::PageDown => self.local_page(true),
            KeyCode::ArrowUp | KeyCode::ArrowDown => {
                if let Some(local) = &mut self.local_setup {
                    local.selected = if key == KeyCode::ArrowUp {
                        local.selected.saturating_sub(1)
                    } else {
                        (local.selected + 1).min(local.model.players().len() - 1)
                    };
                    local.first = local.selected / SETTINGS_ROWS * SETTINGS_ROWS;
                }
            }
            _ => {}
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn finish_local(&mut self) {
        let result = (|| {
            let next = self.prepare_route(ScreenRoute::Settings)?;
            let local = self.local_setup.as_ref().ok_or("local setup unavailable")?;
            let draft = self.settings.as_ref().ok_or("settings unavailable")?;
            let values = local.model.settings(&draft.values)?;
            self.validate_settings(&values)?;
            let editor = LineEditor::new(
                &values.fields().first().ok_or("settings empty")?.value,
                4096,
            )?;
            Ok::<_, String>((next, values, editor, local.model.clone()))
        })();
        match result {
            Ok((next, values, editor, model)) => {
                if let Some(draft) = &mut self.settings {
                    draft.values = values;
                    draft.cached_local = Some(model);
                    draft.selected = 0;
                    draft.editor = editor;
                    draft.profile_focused = false;
                    draft.error = None;
                    draft.message = Some("PLAYERS READY - APPLY TO USE".into());
                }
                self.commit_route(next);
            }
            Err(error) => self.local_error(Some(error)),
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn validate_settings(&self, values: &NativeSettings) -> Result<Vec<String>, String> {
        let args = without_chart(&values.native_args());
        let path = self
            .entries
            .get(self.selected)
            .map(|entry| entry.path.as_path())
            .unwrap_or_else(|| std::path::Path::new("settings-validation.bms"));
        let path = path.to_str().ok_or("native chart path must be UTF-8")?;
        (self.validate)(&with_chart(&args, path)).map_err(|error| error.to_string())?;
        Ok(args)
    }
    fn device_request(&mut self, keyboard: bool) {
        if !self.ui_ready()
            || !matches!(
                self.navigator.route(),
                ScreenRoute::Settings | ScreenRoute::Players | ScreenRoute::Devices { .. }
            )
        {
            return;
        }
        if keyboard
            && self
                .local_setup
                .as_ref()
                .is_some_and(|local| local.model.players().len() == 1)
        {
            return;
        }
        let result = (|| {
            let draft = self.settings.as_ref().ok_or("settings unavailable")?;
            let request = if let Some(picker) = &self.picker {
                picker.catalog.request()
            } else if keyboard {
                DeviceRequest::keyboard(&draft.values, settings_host())?
            } else {
                DeviceRequest::from_settings(&draft.values, settings_host())?
            };
            let player = self
                .picker
                .as_ref()
                .and_then(|picker| picker.player)
                .or_else(|| {
                    request
                        .is_keyboard()
                        .then(|| {
                            self.local_setup
                                .as_ref()
                                .and_then(|local| local.model.players().get(local.selected))
                                .map(|member| member.id)
                        })
                        .flatten()
                });
            let (owner, permit) = self.metadata_scope()?;
            let task = permit.clone();
            self.prepare_route(ScreenRoute::Devices {
                players: player.is_some(),
            })?;
            let query = self.query_devices;
            thread::Builder::new()
                .name("bms-devices".into())
                .spawn(move || {
                    scoped_metadata(task, || {
                        query(request)
                            .map(|catalog| ProfileResult::Devices(catalog, player))
                            .map_err(|error| error.to_string())
                    })
                })
                .map(|worker| ProfileOperation {
                    owner,
                    permit,
                    worker: Some(worker),
                })
                .map_err(|error| error.to_string())
        })();
        match result {
            Ok(operation) => {
                self.profile_io = Some(operation);
                if let Some(draft) = &mut self.settings {
                    draft.error = None;
                    draft.message = None;
                }
            }
            Err(error) => {
                if let Some(draft) = &mut self.settings {
                    draft.error = Some(error);
                }
            }
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn use_device(&mut self) {
        let result = (|| {
            let to = match self.navigator.route() {
                ScreenRoute::Devices { players: true } => ScreenRoute::Players,
                ScreenRoute::Devices { players: false } => ScreenRoute::Settings,
                _ => return Err("device picker is not active".into()),
            };
            let next = self.prepare_route(to)?;
            let picker = self.picker.as_ref().ok_or("device catalog unavailable")?;
            let index = picker.selected.ok_or("select a device first")?;
            if let Some(player) = picker.player {
                self.local_setup
                    .as_mut()
                    .ok_or("local setup unavailable")?
                    .model
                    .assign(player, &picker.catalog, index)?;
                if let Some(draft) = &mut self.settings {
                    draft.error = None;
                    draft.message = Some("KEYBOARD ASSIGNED - DONE TO KEEP".into());
                }
                return Ok(next);
            }
            let draft = self.settings.as_mut().ok_or("settings unavailable")?;
            // Prepare editor before changing the accepted draft.
            let editor = LineEditor::new(&picker.catalog.choices()[index].id, 4096)?;
            picker.catalog.apply(index, &mut draft.values)?;
            let flag = picker.catalog.request().field_flag();
            draft.selected = draft
                .values
                .fields()
                .iter()
                .position(|f| f.flag == flag)
                .expect("catalog validated field");
            draft.editor = editor;
            draft.profile_focused = false;
            draft.error = None;
            draft.message = Some("DEVICE SELECTED - APPLY TO USE".into());
            Ok::<_, String>(next)
        })();
        match result {
            Ok(next) => self.commit_route(next),
            Err(error) => {
                if let Some(draft) = &mut self.settings {
                    draft.error = Some(error);
                }
            }
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn picker_page(&mut self, forward: bool) {
        let picker = self.picker.as_mut().expect("picker page routing");
        let last = picker.catalog.choices().len().saturating_sub(1) / SETTINGS_ROWS * SETTINGS_ROWS;
        picker.first = if forward {
            (picker.first + SETTINGS_ROWS).min(last)
        } else {
            picker.first.saturating_sub(SETTINGS_ROWS)
        };
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn picker_key(&mut self, key: KeyCode, repeat: bool) {
        match key {
            KeyCode::Escape if !repeat => {
                self.back();
            }
            KeyCode::Enter if !repeat => self.use_device(),
            KeyCode::PageUp => self.picker_page(false),
            KeyCode::PageDown => self.picker_page(true),
            KeyCode::ArrowUp | KeyCode::ArrowDown => {
                let picker = self.picker.as_mut().expect("picker key routing");
                let count = picker.catalog.choices().len();
                let next = match (picker.selected, key) {
                    (None, KeyCode::ArrowUp) => (0..count)
                        .rev()
                        .find(|&i| picker.catalog.choices()[i].selectable),
                    (None, _) => (0..count).find(|&i| picker.catalog.choices()[i].selectable),
                    (Some(i), KeyCode::ArrowUp) => (0..i)
                        .rev()
                        .find(|&i| picker.catalog.choices()[i].selectable),
                    (Some(i), _) => {
                        (i + 1..count).find(|&i| picker.catalog.choices()[i].selectable)
                    }
                };
                if let Some(index) = next {
                    picker.selected = Some(index);
                    picker.first = index / SETTINGS_ROWS * SETTINGS_ROWS;
                }
                self.invalidate_hits();
            }
            _ => {}
        }
    }
    fn profile_request(&mut self, save: bool) {
        if !self.ui_ready() || self.navigator.route() != ScreenRoute::Settings {
            return;
        }
        if let Some(draft) = &mut self.settings {
            draft.message = None;
        }
        let result = (|| {
            let draft = self.settings.as_ref().ok_or("settings unavailable")?;
            if draft.profile.value().is_empty() {
                return Err("enter an explicit profile path".into());
            }
            if save {
                self.validate_settings(&draft.values)?;
                draft.presentation.validate()?;
            }
            let path = PathBuf::from(draft.profile.value());
            let values = PlayerProfile {
                native: draft.values.clone(),
                presentation: draft.presentation,
            };
            let host = settings_host();
            let (owner, permit) = self.metadata_scope()?;
            let task = permit.clone();
            thread::Builder::new()
                .name("bms-profile".into())
                .spawn(move || {
                    scoped_metadata(task, || {
                        if save {
                            beatkernel_bms_runtime::settings_profile::save_player_profile(
                                &path, &values, host,
                            )
                            .map(|()| ProfileResult::Saved)
                            .map_err(|error| error.to_string())
                        } else {
                            beatkernel_bms_runtime::settings_profile::load_player_profile(
                                &path, host,
                            )
                            .map(ProfileResult::Loaded)
                            .map_err(|error| error.to_string())
                        }
                    })
                })
                .map(|worker| ProfileOperation {
                    owner,
                    permit,
                    worker: Some(worker),
                })
                .map_err(|error| error.to_string())
        })();
        match result {
            Ok(operation) => {
                self.profile_io = Some(operation);
                if let Some(draft) = &mut self.settings {
                    draft.error = None;
                    draft.message = None;
                }
            }
            Err(error) => {
                if let Some(draft) = &mut self.settings {
                    draft.error = Some(error);
                }
            }
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn collect_profile(&mut self) {
        // Retain completed work while suspended; Closing still drains it.
        if self.is_suspended()
            || !self.profile_io.as_ref().is_some_and(|operation| {
                operation
                    .worker
                    .as_ref()
                    .is_some_and(|worker| worker.is_finished())
            })
        {
            return;
        }
        let mut operation = self.profile_io.take().expect("finished profile operation");
        let result = operation
            .worker
            .take()
            .expect("profile worker")
            .join()
            .unwrap_or_else(|_| Err("profile worker panicked".into()));
        if !operation.permit.is_active() || !self.navigator.accepts(operation.owner) {
            return;
        }
        // Completion must wake event-driven menus before polling switches to Wait.
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        if self.navigator.route() == ScreenRoute::Records {
            if let Some(records) = &mut self.records {
                records.details = false;
                records.grade_page = 0;
                match result {
                    Ok(ProfileResult::Records(catalog)) => {
                        records.selected = (!catalog.entries.is_empty()).then_some(0);
                        records.first = 0;
                        records.preview = None;
                        records.directory_focused = false;
                        records.catalog = Some(catalog);
                        records.error = None;
                    }
                    Ok(ProfileResult::Record(preview)) => {
                        if records.selected_path() == Some(&preview.path) {
                            records.error = preview
                                .archive_error
                                .as_ref()
                                .map(|error| format!("Historical archive unavailable: {error}"));
                            records.preview = Some(preview);
                        }
                    }
                    Err(error) => {
                        records.preview = None;
                        records.error = Some(error);
                    }
                    _ => {}
                }
            }
        } else {
            match result {
                Ok(ProfileResult::Devices(catalog, player)) => {
                    let result = self.prepare_route(ScreenRoute::Devices {
                        players: player.is_some(),
                    });
                    match result {
                        Ok(next) => {
                            self.commit_route(next);
                            let data = DevicePicker {
                                catalog,
                                player,
                                first: 0,
                                selected: None,
                            };
                            if let Some(picker) = &mut self.picker {
                                **picker = data;
                            } else {
                                self.picker = Some(PanelScope::new(
                                    self.navigator.active_id().expect("device route"),
                                    data,
                                ));
                            }
                            self.local_error(None);
                        }
                        Err(error) => self.local_error(Some(error)),
                    }
                }
                Ok(ProfileResult::Saved) => {
                    if let Some(draft) = &mut self.settings {
                        draft.error = None;
                        draft.message = Some("PROFILE SAVED - APPLY IS SEPARATE".into());
                    }
                }
                Ok(ProfileResult::Loaded(profile)) => {
                    if let Some(draft) = &mut self.settings {
                        let values = profile.native;
                        let editor = values
                            .fields()
                            .first()
                            .ok_or_else(|| "profile has no fields".to_owned())
                            .and_then(|field| LineEditor::new(&field.value, 4096));
                        match editor {
                            Ok(editor) => {
                                draft.values = values;
                                draft.presentation = profile.presentation;
                                draft.cached_local = None;
                                draft.selected = 0;
                                draft.editor = editor;
                                draft.profile_focused = false;
                                draft.error = None;
                                draft.message = Some("PROFILE LOADED - APPLY TO USE".into());
                            }
                            Err(error) => draft.error = Some(error),
                        }
                    }
                }
                Err(error) => self.local_error(Some(error)),
                _ => {}
            }
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn apply_settings(&mut self) {
        if !self.ui_ready() || self.navigator.route() != ScreenRoute::Settings {
            return;
        }
        let Some(draft) = &self.settings else {
            return;
        };
        let result = (|| {
            let next = self.prepare_route(ScreenRoute::Selection)?;
            let args = self.validate_settings(&draft.values)?;
            let presentation = draft.presentation;
            presentation.validate()?;
            graphics::instance_descriptor(presentation.backend)?;
            let cached_local = draft.cached_local.clone();
            let profile =
                (!draft.profile.value().is_empty()).then(|| PathBuf::from(draft.profile.value()));
            Ok::<_, String>((next, args, presentation, cached_local, profile))
        })();
        // All drafts and host/build validation are complete before GPU mutation.
        // No options are committed if the current surface rejects this mode.
        let result = result.and_then(|(next, args, presentation, cached_local, profile)| {
            if presentation.presentation != self.options.presentation {
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_presentation(presentation.presentation)?;
                }
            }
            Ok((next, args, presentation, cached_local, profile))
        });
        match result {
            Ok((next, args, presentation, cached_local, profile)) => {
                self.options.native = args;
                self.options.set_display(presentation);
                self.next_frame = Instant::now();
                self.accepted_local = cached_local;
                self.options.profile = profile;
                self.commit_route(next);
                self.failure = None;
            }
            Err(error) => {
                if let Some(draft) = &mut self.settings {
                    draft.error = Some(error);
                }
            }
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn records_admitted(&self) -> bool {
        !self.records.as_ref().is_some_and(|records| records.details)
            && self.ui_ready()
            && matches!(
                self.navigator.route(),
                ScreenRoute::Settings | ScreenRoute::Records
            )
            && self.settings.is_some()
    }
    fn open_records(&mut self) {
        if !self.records_admitted() || self.navigator.route() != ScreenRoute::Settings {
            return;
        }
        let result = self.prepare_route(ScreenRoute::Records).and_then(|next| {
            self.catalog_search
                .selected()
                .and_then(|index| self.entries.get(index))
                .ok_or_else(|| "select a matching chart before opening records".to_owned())
                .and_then(|entry| RecordsDraft::new(entry.path.clone()))
                .map(|records| (next, records))
        });
        match result {
            Ok((next, records)) => {
                self.commit_route(next);
                self.records = Some(PanelScope::new(
                    self.navigator.active_id().expect("records route"),
                    records,
                ));
            }
            Err(error) => {
                if let Some(draft) = &mut self.settings {
                    draft.error = Some(error);
                }
            }
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn toggle_record_details(&mut self) {
        if self.navigator.route() != ScreenRoute::Records {
            return;
        }
        if self.records.as_ref().is_some_and(|records| records.details) {
            self.back();
            return;
        }
        if !self.records_admitted() {
            return;
        }
        if let Some(records) = &mut self.records {
            if records
                .valid_preview()
                .is_none_or(|preview| preview.historical.is_none())
            {
                return;
            }
            records.details = true;
            records.grade_page = 0;
            records.directory_focused = false;
        }
        self.gesture.cancel();
        self.sync_ime();
        self.invalidate_hits();
    }
    fn set_record_grade_page(&mut self, requested: usize) {
        if !self.ui_ready() || self.navigator.route() != ScreenRoute::Records {
            return;
        }
        let Some(records) = &mut self.records else {
            return;
        };
        if !records.details {
            return;
        }
        let Some(preview) = records.valid_preview() else {
            return;
        };
        let pages = beatkernel_bms_runtime::historical_record_presentation::historical_page_count(
            preview.historical_score.as_deref(),
            preview.historical_comparison.as_deref(),
        );
        let page = requested.min(pages - 1);
        if records.grade_page != page {
            records.grade_page = page;
            self.gesture.cancel();
            self.invalidate_hits();
        }
    }
    fn change_record_grade_page(&mut self, forward: bool) {
        let page = self
            .records
            .as_ref()
            .map_or(0, |records| records.grade_page);
        self.set_record_grade_page(if forward {
            page.saturating_add(1)
        } else {
            page.saturating_sub(1)
        });
    }
    fn records_request(&mut self, preview: bool) {
        if !self.records_admitted() {
            return;
        }
        let result = (|| {
            let records = self.records.as_ref().ok_or("records unavailable")?;
            if records.directory.value().is_empty() {
                return Err("enter a record directory".into());
            }
            let directory = PathBuf::from(records.directory.value());
            let selected = if preview {
                Some(
                    records
                        .selected_path()
                        .ok_or("select a record first")?
                        .clone(),
                )
            } else {
                None
            };
            let chart = records.chart.clone();
            let settings = self
                .settings
                .as_ref()
                .ok_or("settings unavailable")?
                .values
                .clone();
            let (owner, permit) = self.metadata_scope()?;
            let task = permit.clone();
            thread::Builder::new()
                .name("bms-profile".into())
                .spawn(move || {
                    scoped_metadata(task, || match selected {
                        Some(path) => RecordPreview::inspect(&path, &chart, &settings)
                            .map(ProfileResult::Record),
                        None => RecordCatalog::scan(&directory).map(ProfileResult::Records),
                    })
                })
                .map(|worker| ProfileOperation {
                    owner,
                    permit,
                    worker: Some(worker),
                })
                .map_err(|error| error.to_string())
        })();
        match result {
            Ok(operation) => {
                self.profile_io = Some(operation);
                if let Some(records) = &mut self.records {
                    records.details = false;
                    records.grade_page = 0;
                    records.preview = None;
                    records.error = None;
                    records.message = None;
                    if !preview {
                        records.catalog = None;
                        records.selected = None;
                        records.first = 0;
                    }
                }
            }
            Err(error) => {
                if let Some(records) = &mut self.records {
                    records.error = Some(error);
                }
            }
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn watch_record(&mut self) {
        if !self.records_admitted() {
            return;
        }
        let result = (|| {
            let next = self.prepare_route(ScreenRoute::Play { replay: true })?;
            let records = self.records.as_ref().ok_or("records unavailable")?;
            let preview = records
                .valid_preview()
                .ok_or("preview the selected compatible record first")?;
            let draft = self.settings.as_ref().ok_or("settings unavailable")?;
            let launch = record_launch(&draft.values, &records.chart, &preview.path)?;
            (self.validate_replay)(launch.args()).map_err(|error| error.to_string())?;
            spawn_game(self.replay, launch, 0, false, true).map(|game| (next, game))
        })();
        match result {
            Ok((next, game)) => {
                if let Some(window) = &self.window {
                    let name = game
                        .launch
                        .args()
                        .chunks_exact(2)
                        .find(|p| p[0] == "--replay")
                        .and_then(|p| std::path::Path::new(&p[1]).file_name())
                        .map(|n| n.to_string_lossy())
                        .unwrap_or_default();
                    window.set_title(&window_title(&name, "REPLAY"));
                }
                self.commit_route(next);
                self.game = Some(game);
                self.failure = None;
                self.release_backgrounds();
            }
            Err(error) => {
                if let Some(records) = &mut self.records {
                    records.error = Some(error);
                }
            }
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn attach_record(&mut self, kind: OpponentKind) {
        if !self.records_admitted() {
            return;
        }
        let result = (|| {
            let records = self.records.as_ref().ok_or("records unavailable")?;
            let path = records
                .valid_preview()
                .ok_or("preview the selected compatible record first")?
                .path
                .to_str()
                .ok_or("record path must be UTF-8")?
                .to_owned();
            let draft = self.settings.as_mut().ok_or("settings unavailable")?;
            draft.values.add_opponent(kind, &path)?;
            draft.refresh_selected()?;
            Ok::<_, String>(())
        })();
        if let Some(records) = &mut self.records {
            match result {
                Ok(()) => {
                    records.error = None;
                    records.message = Some("RECORD ADDED TO DRAFT - APPLY IS SEPARATE".into());
                }
                Err(error) => records.error = Some(error),
            }
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn clear_records(&mut self) {
        if !self.records_admitted() {
            return;
        }
        if let Some(draft) = &mut self.settings {
            draft.values.clear_opponents();
            if let Err(error) = draft.refresh_selected() {
                draft.error = Some(error);
            }
        }
        if let Some(records) = &mut self.records {
            records.error = None;
            records.message = Some("GHOSTS CLEARED FROM DRAFT - APPLY IS SEPARATE".into());
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn remove_record(&mut self, kind: OpponentKind) {
        if !self.records_admitted() || self.navigator.route() != ScreenRoute::Records {
            return;
        }
        let result = (|| {
            let path = self
                .records
                .as_ref()
                .and_then(|records| records.selected_path())
                .and_then(|path| path.to_str())
                .ok_or("select a UTF-8 record path first")?
                .to_owned();
            let draft = self.settings.as_mut().ok_or("settings unavailable")?;
            if !draft.values.remove_opponent(kind, &path)? {
                return Err("selected record is not configured with this kind".into());
            }
            draft.refresh_selected()?;
            Ok::<_, String>(())
        })();
        if let Some(records) = &mut self.records {
            match result {
                Ok(()) => {
                    records.error = None;
                    records.message =
                        Some("ONE RECORD REMOVED FROM DRAFT - APPLY IS SEPARATE".into());
                }
                Err(error) => records.error = Some(error),
            }
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn records_key(&mut self, key: KeyCode, repeat: bool) {
        if self.records.as_ref().is_some_and(|records| records.details) {
            match key {
                KeyCode::Escape if !repeat => self.back(),
                KeyCode::ArrowLeft | KeyCode::ArrowUp | KeyCode::PageUp => {
                    self.change_record_grade_page(false)
                }
                KeyCode::ArrowRight | KeyCode::ArrowDown | KeyCode::PageDown => {
                    self.change_record_grade_page(true)
                }
                KeyCode::Home => self.set_record_grade_page(0),
                KeyCode::End => self.set_record_grade_page(usize::MAX),
                _ => {}
            }
            return;
        }
        match key {
            KeyCode::KeyD
                if !repeat && self.records.as_ref().is_some_and(|r| !r.directory_focused) =>
            {
                self.toggle_record_details()
            }
            KeyCode::KeyW
                if !repeat && self.records.as_ref().is_some_and(|r| !r.directory_focused) =>
            {
                self.watch_record()
            }
            KeyCode::Escape if !repeat => self.back(),
            KeyCode::Enter if !repeat => self.records_request(
                self.records
                    .as_ref()
                    .is_some_and(|records| !records.directory_focused),
            ),
            KeyCode::Tab => {
                if let Some(records) = &mut self.records {
                    records.directory_focused = !records.directory_focused;
                }
            }
            KeyCode::PageUp => {
                if let Some(records) = &mut self.records {
                    records.page(false);
                }
            }
            KeyCode::PageDown => {
                if let Some(records) = &mut self.records {
                    records.page(true);
                }
            }
            KeyCode::ArrowUp | KeyCode::ArrowDown => {
                if let Some(records) = &mut self.records {
                    if !records.directory_focused {
                        let count = records
                            .catalog
                            .as_ref()
                            .map_or(0, |catalog| catalog.entries.len());
                        if count > 0 {
                            let index = records.selected.unwrap_or(0);
                            records.select(if key == KeyCode::ArrowUp {
                                index.saturating_sub(1)
                            } else {
                                (index + 1).min(count - 1)
                            });
                        }
                    }
                }
            }
            KeyCode::ArrowLeft
            | KeyCode::ArrowRight
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Backspace
            | KeyCode::Delete => {
                if let Some(records) = &mut self.records {
                    records.edit(Some(key), None);
                }
            }
            _ => {}
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn open_practice(&mut self) {
        if !self.ui_ready() || self.navigator.route() != ScreenRoute::Settings {
            return;
        }
        let result = (|| {
            let next = self.prepare_route(ScreenRoute::Practice)?;
            let start = PracticeStart::from_settings(
                &self.settings.as_ref().ok_or("settings unavailable")?.values,
            )?;
            let end = PracticeStart::section_end(
                &self.settings.as_ref().ok_or("settings unavailable")?.values,
            )?;
            let practice = PracticeDraft {
                editor: LineEditor::new(&start.formatted(), 64)?,
                end_editor: LineEditor::new(
                    &end.map_or_else(String::new, PracticeStart::formatted),
                    64,
                )?,
                end_focused: false,
                error: None,
                view: PracticeView::new(
                    next.active_id().ok_or("practice instance unavailable")?,
                    WIDTH as u32,
                    HEIGHT as u32,
                )?,
            };
            Ok::<_, String>((next, practice))
        })();
        match result {
            Ok((next, practice)) => {
                self.commit_route(next);
                self.practice = Some(PanelScope::new(
                    self.navigator.active_id().expect("practice route"),
                    practice,
                ));
            }
            Err(error) => self.local_error(Some(error)),
        }
    }
    fn finish_practice(&mut self) {
        let result = (|| {
            let next = self.prepare_route(ScreenRoute::Settings)?;
            let practice = self.practice.as_ref().ok_or("practice unavailable")?;
            let start = PracticeStart::parse(practice.editor.value())?;
            let end = if practice.end_editor.value().is_empty() {
                None
            } else {
                Some(PracticeStart::parse(practice.end_editor.value())?)
            };
            let settings = self.settings.as_ref().ok_or("settings unavailable")?;
            let mut values = settings.values.clone();
            start.apply_section(end, &mut values)?;
            let editor = if values
                .fields()
                .get(settings.selected)
                .is_some_and(|field| matches!(field.flag, "--start-ns" | "--end-ns"))
            {
                LineEditor::new(&values.fields()[settings.selected].value, 4096)?
            } else {
                settings.editor.clone()
            };
            Ok::<_, String>((next, values, editor))
        })();
        match result {
            Ok((next, values, editor)) => {
                if let Some(settings) = &mut self.settings {
                    settings.values = values;
                    settings.editor = editor;
                    settings.error = None;
                    settings.message = Some("PRACTICE DRAFT UPDATED - APPLY IS SEPARATE".into());
                }
                self.commit_route(next);
            }
            Err(error) => {
                if let Some(practice) = &mut self.practice {
                    practice.error = Some(error);
                }
            }
        }
    }
    fn practice_key(&mut self, key: KeyCode, repeat: bool) {
        match key {
            KeyCode::Escape if !repeat => self.back(),
            KeyCode::Enter if !repeat => self.finish_practice(),
            KeyCode::Tab if !repeat => {
                if let Some(practice) = &mut self.practice {
                    practice.end_focused = !practice.end_focused;
                }
            }
            KeyCode::ArrowLeft
            | KeyCode::ArrowRight
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Backspace
            | KeyCode::Delete => {
                if let Some(practice) = &mut self.practice {
                    practice.edit(Some(key), None);
                }
            }
            _ => {}
        }
    }
    fn open_display(&mut self) {
        if !self.ui_ready() || self.navigator.route() != ScreenRoute::Settings {
            return;
        }
        let result = (|| {
            let next = self.prepare_route(ScreenRoute::Display)?;
            let draft = self.settings.as_ref().ok_or("settings unavailable")?;
            DisplayDraft::new(draft.presentation).map(|display| (next, display))
        })();
        match result {
            Ok((next, display)) => {
                self.commit_route(next);
                self.display = Some(PanelScope::new(
                    self.navigator.active_id().expect("display route"),
                    display,
                ));
            }
            Err(error) => self.local_error(Some(error)),
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn finish_display(&mut self) {
        let result = self.prepare_route(ScreenRoute::Settings).and_then(|next| {
            self.display
                .as_mut()
                .ok_or("display unavailable")?
                .value()
                .map(|value| (next, value))
        });
        match result {
            Ok((next, value)) => {
                if let Some(draft) = &mut self.settings {
                    draft.presentation = value;
                    draft.error = None;
                    draft.message = Some("DISPLAY DRAFT UPDATED - APPLY IS SEPARATE".into());
                }
                self.commit_route(next);
            }
            Err(error) => {
                if let Some(display) = &mut self.display {
                    display.error = Some(error);
                }
            }
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn display_key(&mut self, key: KeyCode, repeat: bool) {
        match key {
            KeyCode::Escape if !repeat => self.back(),
            KeyCode::Enter if !repeat => self.finish_display(),
            KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::Tab => {
                if let Some(display) = &mut self.display {
                    display.selected = match key {
                        KeyCode::ArrowUp => display.selected.saturating_sub(1),
                        KeyCode::Tab => (display.selected + 1) % 4,
                        _ => (display.selected + 1).min(3),
                    };
                }
            }
            KeyCode::ArrowLeft
            | KeyCode::ArrowRight
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Backspace
            | KeyCode::Delete => {
                if let Some(display) = &mut self.display {
                    display.edit(Some(key), None);
                }
            }
            _ => {}
        }
        self.invalidate_hits();
    }
    fn settings_key(&mut self, key: KeyCode, repeat: bool) {
        match key {
            KeyCode::Escape if !repeat => {
                self.back();
            }
            KeyCode::Enter if !repeat => self.apply_settings(),
            KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::Tab => {
                if let Some(draft) = &mut self.settings {
                    if draft.profile_focused {
                        let next = if key == KeyCode::Tab {
                            0
                        } else {
                            draft.selected
                        };
                        if let Err(error) = draft.select(next) {
                            draft.error = Some(error);
                        }
                        self.invalidate_hits();
                        return;
                    }
                    let length = draft.values.fields().len();
                    if key == KeyCode::Tab && draft.selected + 1 == length {
                        draft.profile_focused = true;
                        self.invalidate_hits();
                        return;
                    }
                    let next = if key == KeyCode::ArrowUp {
                        draft.selected.saturating_sub(1)
                    } else if key == KeyCode::Tab {
                        (draft.selected + 1) % length
                    } else {
                        (draft.selected + 1).min(length - 1)
                    };
                    if let Err(error) = draft.select(next) {
                        draft.error = Some(error);
                    }
                    self.invalidate_hits();
                }
            }
            KeyCode::ArrowLeft
            | KeyCode::ArrowRight
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Backspace
            | KeyCode::Delete => {
                if let Some(draft) = &mut self.settings {
                    draft.edit(Some(key), None);
                }
            }
            _ => {}
        }
    }
    fn point(&self) -> Option<(f64, f64)> {
        let window = self.window.as_ref()?;
        let size = window.inner_size();
        logical_point(
            self.pointer?,
            (size.width, size.height),
            (WIDTH as u32, HEIGHT as u32),
        )
        .and_then(|point| self.scene.project_ui_point(point))
    }
    fn hit(&self) -> Option<ControlId> {
        if self.startup.is_some()
            || self.renderer_pending()
            || !self.active
            || self.closing()
            || self.is_suspended()
            || self.occluded
        {
            return None;
        }
        let point = self.point()?;
        self.hits
            .iter()
            .rev()
            .find(|(_, bounds)| bounds.contains(point))
            .map(|(id, _)| *id)
    }
    fn activate(&mut self, id: ControlId) {
        if !self.ui_ready() {
            return;
        }
        if self.navigator.route() == ScreenRoute::LiveAudio {
            match id.0 {
                90 => self.apply_live_audio(),
                91 => self.back(),
                value if value == 94 || value == 95 || (1000..1008).contains(&value) => {
                    if !self
                        .game
                        .as_ref()
                        .is_some_and(|game| game.viewer.output_pending())
                    {
                        if let Some(draft) = &mut self.live_audio {
                            let index = match value {
                                94 => draft.selected.saturating_sub(4),
                                95 => (draft.selected + 4).min(draft.values.fields().len() - 1),
                                _ => (value - 1000) as usize,
                            };
                            draft.message = draft.select(index).err();
                        }
                        self.gesture.cancel();
                        self.invalidate_hits();
                    }
                }
                _ => {}
            }
            return;
        }
        if self.navigator.route() == ScreenRoute::Records {
            if id.0 == 66 {
                self.toggle_record_details();
                return;
            }
            if matches!(id.0, 67 | 68) {
                self.change_record_grade_page(id.0 == 68);
                return;
            }
            if !self.records_admitted() {
                return;
            }
            match id.0 {
                50 => self.records_request(false),
                51 => self.records_request(true),
                52 => self.attach_record(OpponentKind::Own),
                53 => self.attach_record(OpponentKind::Other),
                54 => self.clear_records(),
                55 => self.back(),
                59 => self.watch_record(),
                60 => self.remove_record(OpponentKind::Own),
                61 => self.remove_record(OpponentKind::Other),
                56 => self.records.as_mut().expect("records routing").page(false),
                57 => self.records.as_mut().expect("records routing").page(true),
                58 => {
                    self.records
                        .as_mut()
                        .expect("records routing")
                        .directory_focused = true
                }
                50000..=50255 => self
                    .records
                    .as_mut()
                    .expect("records routing")
                    .select((id.0 - 50000) as usize),
                _ => {}
            }
            self.gesture.cancel();
            self.invalidate_hits();
            return;
        }
        if self.navigator.route() == ScreenRoute::Practice {
            match id.0 {
                70 | 75 => {
                    if let Some(practice) = &mut self.practice {
                        practice.end_focused = id.0 == 75;
                    }
                }
                71 => self.finish_practice(),
                76 => {
                    if let Some(practice) = &mut self.practice {
                        practice.clear_end();
                    }
                }
                72 => self.back(),
                73 => {
                    if let Some(practice) = &mut self.practice {
                        practice.reset();
                    }
                }
                _ => {}
            }
            self.gesture.cancel();
            return;
        }
        if self.navigator.route() == ScreenRoute::Display {
            match id.0 {
                40 => self.finish_display(),
                41 => self.back(),
                40000..=40003 => {
                    self.display.as_mut().expect("display routing").selected =
                        (id.0 - 40000) as usize
                }
                _ => {}
            }
            self.gesture.cancel();
            self.invalidate_hits();
            return;
        }
        if matches!(self.navigator.route(), ScreenRoute::Devices { .. }) {
            match id.0 {
                20 => self.use_device(),
                21 => {
                    self.back();
                }
                22 => self.device_request(false),
                23 => self.picker_page(false),
                24 => self.picker_page(true),
                row if row >= 10000 => {
                    let picker = self.picker.as_mut().expect("picker routing");
                    let index = (row - 10000) as usize;
                    if picker
                        .catalog
                        .choices()
                        .get(index)
                        .is_some_and(|c| c.selectable)
                    {
                        picker.selected = Some(index);
                    }
                    self.gesture.cancel();
                    self.invalidate_hits();
                }
                _ => {}
            }
            return;
        }
        if self.navigator.route() == ScreenRoute::Players {
            match id.0 {
                30 => self.finish_local(),
                31 => self.back(),
                32 => self.resize_local(false),
                33 => self.resize_local(true),
                34 => self.device_request(true),
                35 => self.clear_local(),
                36 => self.local_page(false),
                37 => self.local_page(true),
                row if row >= 20000 => {
                    if let Some(local) = &mut self.local_setup {
                        let index = (row - 20000) as usize;
                        if index < local.model.players().len() {
                            local.selected = index;
                        }
                    }
                }
                _ => {}
            }
            self.gesture.cancel();
            self.invalidate_hits();
            return;
        }
        if self.navigator.route() == ScreenRoute::Settings {
            match id.0 {
                74 => self.open_practice(),
                19 => self.open_records(),
                18 => self.open_display(),
                17 => self.open_local(),
                16 => self.device_request(false),

                13 => self.profile_request(false),
                14 => self.profile_request(true),
                15 => {
                    if let Some(draft) = &mut self.settings {
                        draft.profile_focused = true;
                    }
                    self.gesture.cancel();
                    self.invalidate_hits();
                }
                10 => self.apply_settings(),
                11 => {
                    self.back();
                }
                12 => {
                    if let Some(draft) = &mut self.settings {
                        match draft
                            .values
                            .add_binding()
                            .and_then(|index| draft.select(index))
                        {
                            Ok(()) => {
                                draft.error = None;
                                draft.message = None;
                            }
                            Err(error) => draft.error = Some(error),
                        }
                    }
                    self.gesture.cancel();
                    self.invalidate_hits();
                }
                row if row >= 1000 => {
                    if let Some(draft) = &mut self.settings {
                        if let Err(error) = draft.select((row - 1000) as usize) {
                            draft.error = Some(error);
                        }
                    }
                    self.gesture.cancel();
                    self.invalidate_hits();
                }
                _ => {}
            }
            return;
        }
        match id.0 {
            90..=94
                if matches!(
                    self.navigator.route(),
                    ScreenRoute::Play { .. } | ScreenRoute::Results { .. }
                ) =>
            {
                if let Some(game) = &mut self.game {
                    let page = game
                        .snapshot
                        .as_ref()
                        .and_then(|snapshot| snapshot.room.as_ref())
                        .map_or(0, |room| room.page);
                    let action = match id.0 {
                        90 => RoomUiAction::Seal,
                        91 => RoomUiAction::Ready,
                        92 => RoomUiAction::Leave,
                        93 => RoomUiAction::Page(page.saturating_sub(1)),
                        _ => RoomUiAction::Page(page.saturating_add(1)),
                    };
                    let requested = if game.joined {
                        match action {
                            RoomUiAction::Page(page) => game.select_room_result_page(page),
                            _ => Err("joined room controls are closed".into()),
                        }
                    } else {
                        game.request_room(action).map(|_| ())
                    };
                    if let Err(error) = requested {
                        self.failure = Some(error);
                    }
                }
                self.gesture.cancel();
                self.invalidate_hits();
            }
            6 if matches!(
                self.navigator.route(),
                ScreenRoute::Play { .. } | ScreenRoute::Results { .. }
            ) =>
            {
                self.change_local_page(false)
            }
            7 if matches!(
                self.navigator.route(),
                ScreenRoute::Play { .. } | ScreenRoute::Results { .. }
            ) =>
            {
                self.change_local_page(true)
            }
            8 if matches!(
                self.navigator.route(),
                ScreenRoute::Play { .. } | ScreenRoute::Results { .. }
            ) =>
            {
                self.toggle_local_comparisons()
            }
            9 if matches!(
                self.navigator.route(),
                ScreenRoute::Play { .. } | ScreenRoute::Results { .. }
            ) =>
            {
                self.request_retry()
            }
            60 if self.navigator.route() == (ScreenRoute::Play { replay: false }) => {
                self.mark_practice()
            }
            62 if matches!(self.navigator.route(), ScreenRoute::Play { .. }) => self.toggle_pause(),
            63 if self.navigator.route() == (ScreenRoute::Play { replay: false }) => {
                self.edit_loop(false)
            }
            64 if self.navigator.route() == (ScreenRoute::Play { replay: false }) => {
                self.edit_loop(true)
            }
            61 if matches!(
                self.navigator.route(),
                ScreenRoute::Play { replay: false } | ScreenRoute::Results { replay: false }
            ) =>
            {
                self.request_restart(true)
            }
            80 if self.navigator.route() == ScreenRoute::Selection => self.set_search_focus(true),
            5 if self.navigator.route() == ScreenRoute::Selection => self.open_settings(),
            1 if self.navigator.route() == ScreenRoute::Selection
                && self.catalog_search.selected().is_some() =>
            {
                self.set_search_focus(false);
                if let Err(error) = self.start() {
                    self.failure = Some(error);
                }
            }
            2 if self.game.as_ref().is_some_and(|game| !game.joined) => self.cancel(),
            3 if matches!(self.navigator.route(), ScreenRoute::Results { .. }) => {
                self.key(KeyCode::Enter, false)
            }
            4 if self.navigator.route() == ScreenRoute::Selection => {
                self.request_close();
                self.cancel();
            }
            row if row >= 100 && self.navigator.route() == ScreenRoute::Selection => {
                if let Ok(index) = usize::try_from(row - 100) {
                    if self.catalog_search.select(index).is_ok() {
                        self.selected = index;
                        self.set_search_focus(false);
                    }
                }
            }
            _ => {}
        }
    }
    fn change_local_page(&mut self, forward: bool) {
        if let Some(game) = &mut self.game {
            let last = game.presentation_page_count().saturating_sub(1);
            game.local_page = if forward {
                game.local_page.saturating_add(1).min(last)
            } else {
                game.local_page.saturating_sub(1).min(last)
            };
            self.gesture.cancel();
            self.invalidate_hits();
        }
    }
    fn toggle_local_comparisons(&mut self) {
        if let Some(game) = &mut self.game {
            if game.comparisons_available() {
                game.local_comparisons = !game.local_comparisons;
                game.local_page = game
                    .local_page
                    .min(game.presentation_page_count().saturating_sub(1));
                self.gesture.cancel();
                self.invalidate_hits();
            }
        }
    }
    fn mark_practice(&mut self) {
        if !self.ui_ready() || self.navigator.route() != (ScreenRoute::Play { replay: false }) {
            return;
        }
        if let Some(game) = &mut self.game {
            self.failure = game.mark_practice().err();
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn toggle_pause(&mut self) {
        if !self.ui_ready() || !matches!(self.navigator.route(), ScreenRoute::Play { .. }) {
            return;
        }
        if let Some(game) = &self.game {
            if let Some(paused) = game.pause_target() {
                game.viewer.request_pause(paused);
            }
        }
    }
    fn edit_loop(&mut self, toggle: bool) {
        if !self.ui_ready() || self.navigator.route() != (ScreenRoute::Play { replay: false }) {
            return;
        }
        if let Some(game) = &mut self.game {
            self.failure = if toggle {
                game.toggle_loop()
            } else {
                game.mark_loop_end()
            }
            .err();
        }
        // A running owner's audio endpoint is immutable. Enabling repetition
        // preflights a fresh finite invocation before cancelling that owner.
        if toggle
            && self.failure.is_none()
            && self.game.as_ref().is_some_and(|game| game.loop_enabled)
        {
            self.request_restart(true);
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn request_retry(&mut self) {
        self.request_restart(false);
    }
    fn request_restart(&mut self, from_bookmark: bool) {
        if !self.ui_ready()
            || !matches!(
                self.navigator.route(),
                ScreenRoute::Play { .. } | ScreenRoute::Results { .. }
            )
        {
            return;
        }
        let Some(game) = &self.game else {
            return;
        };
        if !game.retry_available() || (from_bookmark && !game.practice_restart_available()) {
            return;
        }
        // Preflight the exact retained invocation before signalling cancellation.
        let validate = if game.replay {
            self.validate_replay
        } else {
            self.validate
        };
        let prepared = if from_bookmark && game.loop_enabled {
            game.launch
                .retry_loop(game.practice_loop.expect("enabled loop admission"))
        } else if from_bookmark {
            game.launch
                .retry_from(game.practice_bookmark.expect("bookmark admission"))
        } else {
            game.launch.retry()
        }
        .and_then(|launch| {
            validate(launch.args()).map_err(|error| error.to_string())?;
            Ok(launch)
        });
        match prepared {
            Ok(launch) => {
                if !from_bookmark {
                    if let Some(game) = &mut self.game {
                        game.loop_enabled = false;
                    }
                }
                if self.game.as_ref().is_some_and(|game| game.joined) {
                    self.replace_joined_game(launch);
                } else if let Some(game) = &mut self.game {
                    game.prepared_retry = Some(launch);
                    game.viewer.cancel();
                    game.cancelling = true;
                    self.failure = None;
                }
            }
            Err(error) => {
                if let Some(game) = &mut self.game {
                    game.loop_enabled = false;
                }
                self.failure = Some(format!("retry preflight: {error}"));
            }
        }
        self.gesture.cancel();
        self.invalidate_hits();
    }
    fn replace_joined_game(&mut self, launch: SessionLaunch) {
        let Some(old) = &self.game else {
            return;
        };
        if !old.joined || old.worker.is_some() {
            return;
        }
        let next = match self.prepare_route(ScreenRoute::Play { replay: old.replay }) {
            Ok(next) => next,
            Err(error) => {
                self.failure = Some(error);
                if let Some(game) = &mut self.game {
                    game.loop_enabled = false;
                }
                return;
            }
        };
        // Spawn is the only fallible step; retain joined results on failure.
        let native = if old.replay { self.replay } else { self.native };
        let bookmark = old.practice_bookmark;
        let region = old.practice_loop;
        let loop_enabled = old.loop_enabled;
        match spawn_game(
            native,
            launch,
            old.local_page,
            old.local_comparisons,
            old.replay,
        ) {
            Ok(mut game) => {
                game.practice_bookmark = bookmark;
                game.practice_loop = region;
                game.loop_enabled = loop_enabled;
                self.commit_route(next);
                self.game = Some(game);
                self.failure = None;
                self.release_backgrounds();
            }
            Err(error) => {
                if let Some(game) = &mut self.game {
                    game.loop_enabled = false;
                }
                self.failure = Some(format!("retry spawn: {error}"));
            }
        }
    }
    fn cancel(&mut self) {
        self.gesture.cancel();
        self.invalidate_hits();
        if let Some(game) = &mut self.game {
            game.cancel();
        }
    }
    fn fail(&mut self, error: impl ToString) {
        self.fatal = Some(error.to_string());
        self.request_close();
    }
    fn collect_game(&mut self) {
        if let Some(game) = &self.game {
            if let Ok(Some(reply)) = game.viewer.take_output_reply() {
                if self.navigator.route() == ScreenRoute::LiveAudio {
                    if let Some(draft) = &mut self.live_audio {
                        if self.navigator.accepts(draft.id()) && draft.request == Some(reply.id) {
                            draft.request = None;
                            match reply.result {
                                Ok(cap) => match draft.refresh_applied(&cap) {
                                    Ok(()) => {
                                        draft.message = Some(
                                            "Output settings applied. Playback remains paused."
                                                .into(),
                                        );
                                    }
                                    Err(error) => draft.message = Some(error),
                                },
                                Err(error) => draft.message = Some(error),
                            }
                        } else if self.navigator.accepts(draft.id())
                            && draft.request.is_none()
                            && draft.is_unchanged()
                        {
                            // Applied state belongs to the session; the old
                            // child's notice/identity does not belong here.
                            if let Ok(capability) = &reply.result {
                                if let Err(error) = draft.refresh_applied(capability) {
                                    self.failure = Some(error);
                                }
                            }
                        }
                    }
                }
            }
        }

        let renderer_pending = self.renderer_pending();
        if let Some(game) = &mut self.game {
            for _ in 0..beatkernel_bms_runtime::room_presentation::ROOM_UI_CAPACITY {
                match game.viewer.take_room_reply() {
                    Ok(Some(_)) => {} // PlayerViewer caches this exact correlated notice once.
                    Ok(None) | Err(_) => break,
                }
            }
            if let Some(snapshot) = game.viewer.take_latest() {
                if let (Some(window), Some(chart)) = (&self.window, &snapshot.chart) {
                    if !renderer_pending {
                        window.set_title(&window_title(&chart.title, &chart.artist));
                    }
                }
                game.accept_snapshot(snapshot);
            }
        }
        let mut retry = None;
        if let Some(game) = &mut self.game {
            if !game.joined
                && game
                    .worker
                    .as_ref()
                    .is_some_and(|worker| worker.is_finished())
            {
                let result = game
                    .worker
                    .take()
                    .expect("active game worker")
                    .join()
                    .unwrap_or_else(|_| Err("game worker panicked".into()));
                // The old owner can publish during cleanup. Drain once more only
                // after joining; fresh channels cannot replace this evidence.
                if let Some(snapshot) = game.viewer.take_latest() {
                    game.accept_snapshot(snapshot);
                }
                let succeeded = result.is_ok();
                if let Err(error) = result {
                    self.failure = Some(error);
                }
                retry = game.owner_finished(succeeded);
            }
        }
        // Only the joined worker's final native acknowledgement can repeat.
        self.repeat_practice_if_due();
        if self.navigator.phase() == ScreenPhase::Active && !self.renderer_pending() {
            if let Some(game) = &self.game {
                if game.joined
                    && matches!(
                        self.navigator.route(),
                        ScreenRoute::Play { .. } | ScreenRoute::LiveAudio
                    )
                {
                    if let Err(error) = self.navigate(ScreenRoute::Results {
                        replay: game.replay,
                    }) {
                        self.failure = Some(error);
                    }
                }
            }
        }
        if let Some(launch) = retry {
            if self.active && !self.closing() && !self.is_suspended() && !self.occluded {
                self.replace_joined_game(launch);
            }
        }
    }
    fn repeat_practice_if_due(&mut self) {
        if self.ui_ready()
            && matches!(
                self.navigator.route(),
                ScreenRoute::Play { replay: false } | ScreenRoute::Results { replay: false }
            )
            && self.game.as_ref().is_some_and(Game::loop_due)
        {
            self.request_restart(true);
        }
    }
    fn set_search_focus(&mut self, focused: bool) {
        if focused
            && (!self.ui_ready()
                || self.navigator.route() != ScreenRoute::Selection
                || self.catalog.is_some())
        {
            return;
        }
        if self.search_focused != focused {
            self.catalog_wheel.reset();
            self.search_focused = focused;
            self.sync_ime();
        }
    }
    fn ime_target(&self) -> Option<ImeTarget> {
        self.text_target()
    }
    /// Map the current painted field, rather than a second copy of its layout.
    fn ime_cursor_area(&self, physical: [u32; 2]) -> Option<[u32; 4]> {
        let target = self.ime_target()?;
        if self.ime.target != Some(target)
            || self.painted_reactive != Some(target.screen)
            || physical.contains(&0)
        {
            return None;
        }
        let control = ControlId(match target.field {
            ImeField::Search => 80,
            ImeField::LiveOutput(index) => 1000_u64.checked_add(u64::try_from(index).ok()?)?,
            ImeField::Setting(index) => 1000_u64.checked_add(u64::try_from(index).ok()?)?,
            ImeField::Profile => 15,
            ImeField::Display(index) => 40_000_u64.checked_add(u64::try_from(index).ok()?)?,
            ImeField::PracticeStart => 70,
            ImeField::PracticeEnd => 75,
            ImeField::RecordDirectory => 58,
        });
        let bounds = self.hits.iter().find(|(id, _)| *id == control)?.1;
        if bounds.x < 0 || bounds.y < 0 || bounds.width <= 0 || bounds.height <= 0 {
            return None;
        }
        let right = bounds.x.checked_add(bounds.width)?;
        let bottom = bounds.y.checked_add(bounds.height)?;
        let [x, y, width, height] = beatkernel_bms_runtime::viewport::Viewport::new(
            physical,
            [WIDTH as u32, HEIGHT as u32],
        )
        .ok()?
        .rect();
        // Outward rounding keeps a positive painted field reachable on tiny
        // surfaces. All arithmetic precedes clipping and remains checked.
        let axis = |begin: i64, end: i64, origin: u32, fitted: u32, logical: u32, limit: u32| {
            let logical = u64::from(logical);
            let first = u64::try_from(begin).ok()?.checked_mul(u64::from(fitted))? / logical;
            let last = u64::try_from(end)
                .ok()?
                .checked_mul(u64::from(fitted))?
                .checked_add(logical - 1)?
                / logical;
            let first = u64::from(origin).checked_add(first)?.min(u64::from(limit));
            let last = u64::from(origin).checked_add(last)?.min(u64::from(limit));
            if first > i32::MAX as u64 || last > i32::MAX as u64 || last <= first {
                return None;
            }
            Some([
                u32::try_from(first).ok()?,
                u32::try_from(last - first).ok()?,
            ])
        };
        let [left, width] = axis(bounds.x, right, x, width, WIDTH as u32, physical[0])?;
        let [top, height] = axis(bounds.y, bottom, y, height, HEIGHT as u32, physical[1])?;
        Some([left, top, width, height])
    }
    fn publish_ime_cursor_area(&mut self) {
        let Some(target) = self.ime_target() else {
            self.ime.cursor_area = None;
            return;
        };
        let candidate = self.window.as_ref().and_then(|window| {
            let size = window.inner_size();
            Some((
                window.id(),
                target,
                self.ime_cursor_area([size.width, size.height])?,
            ))
        });
        if candidate == self.ime.cursor_area {
            return;
        }
        self.ime.cursor_area = None;
        if let (Some(window), Some((_, _, [left, top, width, height]))) = (&self.window, candidate)
        {
            window.set_ime_cursor_area(
                PhysicalPosition::new(left as i32, top as i32),
                PhysicalSize::new(width, height),
            );
            self.ime.cursor_area = candidate;
        }
    }
    fn sync_ime(&mut self) {
        if !self.ui_ready() {
            self.modifiers = ModifiersState::empty();
        }
        let target = self.ime_target();
        if target == self.ime.target {
            if target.is_none()
                || self.painted_reactive != target.map(|target| target.screen)
                || self.window.is_none()
            {
                self.ime.cursor_area = None;
            }
            self.sync_clipboard();
            return;
        }
        self.ime = ImeDraft {
            target,
            ..ImeDraft::default()
        };
        self.painted_reactive = None;
        if let Some(window) = &self.window {
            window.set_ime_allowed(false);
            if target.is_some() {
                window.set_ime_allowed(true);
            }
        }
        self.sync_clipboard();
    }
    fn ime_owns_keyboard(&self) -> bool {
        self.ime.enabled
            && self.ime.composing
            && self.ime.target.is_some()
            && self.ime.target == self.ime_target()
    }
    fn ime_editor<'a>(&'a self, field: ImeField, editor: &'a LineEditor) -> &'a LineEditor {
        if self.ime.target.is_some_and(|target| target.field == field)
            && self.ime.target == self.ime_target()
        {
            if let Some(preview) = &self.ime.preview {
                return preview;
            }
        }
        editor
    }
    fn ime_error(&mut self, error: String) {
        self.clipboard_error(Some(error));
    }
    fn ime_event(&mut self, event: Ime) {
        self.sync_ime();
        match event {
            Ime::Enabled if self.ime.target.is_some() => self.ime.enabled = true,
            Ime::Disabled => {
                self.ime.enabled = false;
                self.ime.composing = false;
                self.ime.preview = None;
            }
            Ime::Preedit(text, cursor) if self.ime.enabled => {
                self.ime.composing = !text.is_empty();
                self.ime.preview = None;
                if !text.is_empty() {
                    let editor = self
                        .ime
                        .target
                        .and_then(|target| self.text_editor(target.field));
                    if let Some(editor) = editor {
                        match editor.preedit(&text, cursor) {
                            Ok(preview) => self.ime.preview = Some(preview),
                            Err(error) => self.ime_error(error),
                        }
                    }
                }
            }
            Ime::Commit(text) if self.ime.enabled => {
                self.ime.preview = None;
                self.ime.composing = false;
                if let Some(target) = self.ime.target {
                    let result = self
                        .text_editor(target.field)
                        .cloned()
                        .ok_or_else(|| "text field unavailable".to_string())
                        .and_then(|mut editor| {
                            editor.insert(&text)?;
                            self.commit_clipboard_editor(target, editor)
                        });
                    self.clipboard_error(result.err());
                }
            }
            _ => {}
        }
        self.invalidate_hits();
    }
    fn edit_search(&mut self, key: Option<KeyCode>, value: Option<&str>) {
        if !self.ui_ready()
            || self.navigator.route() != ScreenRoute::Selection
            || !self.search_focused
        {
            return;
        }
        let mut editor = self.search_editor.clone();
        self.catalog_wheel.reset();
        let result = edit_line(&mut editor, key, value)
            .and_then(|()| self.catalog_search.set_query(editor.value()));
        match result {
            Ok(()) => {
                self.search_editor = editor;
                if let Some(index) = self.catalog_search.selected() {
                    self.selected = index;
                }
                self.failure = None;
            }
            Err(error) => self.failure = Some(error),
        }
        self.invalidate_hits();
    }
    /// Menu navigation only: wheel events never enter native gameplay input.
    fn scroll_catalog(&mut self, lines: f64, over_catalog: bool) {
        if !self.ui_ready() || self.navigator.route() != ScreenRoute::Selection || !over_catalog {
            self.catalog_wheel.reset();
            return;
        }
        if lines == 0.0 {
            return; // Horizontal-only movement does not consume vertical remainder.
        }
        self.gesture.cancel();
        let steps = self.catalog_wheel.push(lines);
        if steps == 0 {
            return;
        }
        let previous = self.catalog_search.selected();
        self.catalog_search
            .step_by(steps < 0, steps.unsigned_abs() as usize);
        if let Some(index) = self.catalog_search.selected() {
            self.selected = index;
        }
        if self.catalog_search.selected() != previous {
            self.invalidate_hits();
        }
    }
    fn over_catalog(&self, point: Option<(f64, f64)>) -> bool {
        self.navigator.route() == ScreenRoute::Selection
            && self.selection_view.as_ref().is_some_and(|view| {
                Some(view.id()) == self.navigator.active_id()
                    && point.is_some_and(|point| view.contains_chart(point))
            })
    }
    fn modifiers_changed(&mut self, modifiers: ModifiersState) {
        self.modifiers = if self.ui_ready() {
            modifiers
        } else {
            ModifiersState::empty()
        };
    }
    fn text_target(&self) -> Option<TextTarget> {
        if !self.ui_ready() {
            return None;
        }
        let screen = self.navigator.active_id()?;
        let field = match self.navigator.route() {
            ScreenRoute::Selection if self.search_focused => TextField::Search,
            ScreenRoute::LiveAudio if !self.game.as_ref()?.viewer.output_pending() => {
                TextField::LiveOutput(self.live_audio.as_ref()?.selected)
            }
            ScreenRoute::Settings => {
                let draft = self.settings.as_ref()?;
                if draft.profile_focused {
                    TextField::Profile
                } else {
                    TextField::Setting(draft.selected)
                }
            }
            ScreenRoute::Display => TextField::Display(self.display.as_ref()?.selected),
            ScreenRoute::Practice => {
                if self.practice.as_ref()?.end_focused {
                    TextField::PracticeEnd
                } else {
                    TextField::PracticeStart
                }
            }
            ScreenRoute::Records if self.records.as_ref()?.directory_focused => {
                TextField::RecordDirectory
            }
            _ => return None,
        };
        Some(TextTarget { screen, field })
    }
    fn text_editor(&self, field: TextField) -> Option<&LineEditor> {
        match field {
            TextField::Search => Some(&self.search_editor),
            TextField::LiveOutput(index) => self
                .live_audio
                .as_ref()
                .filter(|draft| draft.selected == index)
                .map(|draft| &draft.editor),
            TextField::Setting(index) => self
                .settings
                .as_ref()
                .filter(|draft| draft.selected == index)
                .map(|draft| &draft.editor),
            TextField::Profile => self.settings.as_ref().map(|draft| &draft.profile),
            TextField::Display(index) => self.display.as_ref()?.editors.get(index),
            TextField::PracticeStart => self.practice.as_ref().map(|draft| &draft.editor),
            TextField::PracticeEnd => self.practice.as_ref().map(|draft| &draft.end_editor),
            TextField::RecordDirectory => self.records.as_ref().map(|draft| &draft.directory),
        }
    }
    fn text_editor_mut(&mut self, field: TextField) -> Option<&mut LineEditor> {
        match field {
            TextField::Search => Some(&mut self.search_editor),
            TextField::LiveOutput(index) => self
                .live_audio
                .as_mut()
                .filter(|draft| draft.selected == index)
                .map(|draft| &mut draft.editor),
            TextField::Setting(index) => self
                .settings
                .as_mut()
                .filter(|draft| draft.selected == index)
                .map(|draft| &mut draft.editor),
            TextField::Profile => self.settings.as_mut().map(|draft| &mut draft.profile),
            TextField::Display(index) => self.display.as_mut()?.editors.get_mut(index),
            TextField::PracticeStart => self.practice.as_mut().map(|draft| &mut draft.editor),
            TextField::PracticeEnd => self.practice.as_mut().map(|draft| &mut draft.end_editor),
            TextField::RecordDirectory => self.records.as_mut().map(|draft| &mut draft.directory),
        }
    }
    fn sync_clipboard(&mut self) {
        if self.pending_clipboard.as_ref().is_some_and(|pending| {
            self.ime_owns_keyboard()
                || self.text_target() != Some(pending.target)
                || self
                    .text_editor(pending.target.field)
                    .is_none_or(|editor| !pending.edit.matches(editor))
        }) {
            // The worker must still drain an accepted operation. Only publication
            // into the obsolete draft is cancelled; an OS write cannot be recalled.
            self.pending_clipboard = None;
        }
    }
    fn clipboard_busy(&self) -> bool {
        self.clipboard
            .as_ref()
            .is_some_and(ClipboardWorker::is_busy)
    }
    fn clipboard_error(&mut self, error: Option<String>) {
        match self.navigator.route() {
            ScreenRoute::Selection => self.failure = error,
            ScreenRoute::LiveAudio => {
                if let Some(draft) = &mut self.live_audio {
                    draft.message = error;
                }
            }
            ScreenRoute::Settings => {
                if let Some(draft) = &mut self.settings {
                    draft.error = error;
                    draft.message = None;
                }
            }
            ScreenRoute::Display => {
                if let Some(draft) = &mut self.display {
                    draft.error = error;
                }
            }
            ScreenRoute::Practice => {
                if let Some(draft) = &mut self.practice {
                    draft.error = error;
                }
            }
            ScreenRoute::Records => {
                if let Some(draft) = &mut self.records {
                    draft.error = error;
                    draft.message = None;
                }
            }
            _ => {}
        }
    }
    fn clipboard_settings(
        &self,
        field: TextField,
        editor: &LineEditor,
    ) -> Result<Option<NativeSettings>, String> {
        if let TextField::LiveOutput(index) = field {
            let mut values = self
                .live_audio
                .as_ref()
                .ok_or("output draft unavailable")?
                .values
                .clone();
            values.set_value(index, editor.value())?;
            Ok(Some(values))
        } else if let TextField::Setting(index) = field {
            let mut values = self
                .settings
                .as_ref()
                .ok_or("settings draft unavailable")?
                .values
                .clone();
            values.set_value(index, editor.value())?;
            Ok(Some(values))
        } else {
            Ok(None)
        }
    }
    fn begin_clipboard(&mut self, action: ClipboardAction) -> Result<(), String> {
        self.sync_clipboard();
        let Some(target) = self.text_target().filter(|_| !self.ime_owns_keyboard()) else {
            return Ok(());
        };
        let editor = self
            .text_editor(target.field)
            .ok_or("text field unavailable")?;
        let Some(edit) = ClipboardEdit::prepare(editor, action)? else {
            return Ok(());
        };
        if let Some(candidate) = edit.candidate() {
            // Cut validates before the external write, while retaining the draft.
            self.clipboard_settings(target.field, candidate)?;
        }
        if self.clipboard.is_none() {
            self.clipboard = Some(ClipboardWorker::native()?);
        }
        self.clipboard
            .as_mut()
            .expect("clipboard worker")
            .submit(edit.request())?;
        self.pending_clipboard = Some(PendingClipboard { target, edit });
        Ok(())
    }
    fn commit_clipboard_editor(
        &mut self,
        target: TextTarget,
        editor: LineEditor,
    ) -> Result<(), String> {
        // Revalidate aggregate settings: unrelated rows may have changed while
        // the OS was busy. Prepare the entire model before assigning either part.
        let settings = self.clipboard_settings(target.field, &editor)?;
        if target.field == TextField::Search {
            self.catalog_search.set_query(editor.value())?;
            self.catalog_wheel.reset();
            if let Some(index) = self.catalog_search.selected() {
                self.selected = index;
            }
            self.search_editor = editor;
        } else if let Some(values) = settings {
            if matches!(target.field, TextField::LiveOutput(_)) {
                let draft = self.live_audio.as_mut().ok_or("output draft unavailable")?;
                draft.values = values;
                draft.editor = editor;
            } else {
                let draft = self.settings.as_mut().ok_or("settings draft unavailable")?;
                draft.values = values;
                draft.editor = editor;
            }
        } else if target.field == TextField::RecordDirectory {
            let draft = self
                .records
                .as_mut()
                .ok_or("record directory unavailable")?;
            if draft.directory.value() != editor.value() {
                draft.catalog = None;
                draft.preview = None;
                draft.selected = None;
                draft.first = 0;
                draft.message = None;
            }
            draft.directory = editor;
        } else {
            *self
                .text_editor_mut(target.field)
                .ok_or("text field unavailable")? = editor;
        }
        Ok(())
    }
    fn finish_clipboard(&mut self, reply: Result<Option<String>, String>) {
        self.sync_clipboard();
        let Some(pending) = self.pending_clipboard.take() else {
            return;
        };
        let result = pending.edit.complete(reply).and_then(|editor| {
            if let Some(editor) = editor {
                self.commit_clipboard_editor(pending.target, editor)?;
            }
            Ok(())
        });
        self.clipboard_error(result.err());
        self.invalidate_hits();
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
    fn collect_clipboard(&mut self) {
        self.sync_clipboard();
        if let Some(reply) = self.clipboard.as_mut().and_then(ClipboardWorker::poll) {
            self.finish_clipboard(reply);
        }
    }
    fn clipboard_shortcut(&mut self, logical: &Key, repeat: bool) -> bool {
        let Some(action) = clipboard_command(logical, self.modifiers, cfg!(target_os = "macos"))
        else {
            return false;
        };
        if self.text_target().is_none() || self.ime_owns_keyboard() {
            return false;
        }
        if !repeat {
            if let Err(error) = self.begin_clipboard(action) {
                self.clipboard_error(Some(error));
            }
            self.gesture.cancel();
            self.invalidate_hits();
        }
        true
    }
    fn select_text(&mut self, physical: PhysicalKey, logical: &Key) -> bool {
        if !self.ui_ready() || self.ime_owns_keyboard() {
            return false;
        }
        let Some(command) =
            selection_command(physical, logical, self.modifiers, cfg!(target_os = "macos"))
        else {
            return false;
        };
        let Some(target) = self.text_target() else {
            return false;
        };
        let Some(editor) = self.text_editor_mut(target.field) else {
            return false;
        };
        match command {
            SelectionCommand::All => editor.select_all(),
            SelectionCommand::Left => editor.move_left(true),
            SelectionCommand::Right => editor.move_right(true),
            SelectionCommand::Home => editor.move_home(true),
            SelectionCommand::End => editor.move_end(true),
        }
        self.gesture.cancel();
        self.invalidate_hits();
        true
    }
    /// The actual pressed-key route, also callable without an OS window in fixtures.
    fn keyboard_input(
        &mut self,
        physical: PhysicalKey,
        logical: &Key,
        text: Option<&str>,
        repeat: bool,
    ) {
        self.sync_ime();
        if self.ime_owns_keyboard() {
            return;
        }
        if self.clipboard_shortcut(logical, repeat) {
            return;
        }
        if self.select_text(physical, logical) {
            return;
        }
        let editing = self.navigator.active_id();
        let navigation = matches!(
            physical,
            PhysicalKey::Code(
                KeyCode::Escape
                    | KeyCode::Enter
                    | KeyCode::Tab
                    | KeyCode::PageUp
                    | KeyCode::PageDown
                    | KeyCode::ArrowUp
                    | KeyCode::ArrowDown
                    | KeyCode::ArrowLeft
                    | KeyCode::ArrowRight
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::Backspace
                    | KeyCode::Delete
                    | KeyCode::F4
            )
        );
        if let PhysicalKey::Code(key) = physical {
            self.key(key, repeat);
        }
        if editing.is_some_and(|id| self.navigator.accepts(id)) && !navigation && self.ui_ready() {
            let value = text.or_else(|| match logical {
                Key::Character(value) => Some(value.as_str()),
                _ => None,
            });
            if let Some(value) = value {
                match self.navigator.route() {
                    ScreenRoute::Practice => {
                        if let Some(practice) = &mut self.practice {
                            practice.edit(None, Some(value));
                        }
                    }
                    ScreenRoute::Selection if self.search_focused => {
                        self.edit_search(None, Some(value))
                    }
                    ScreenRoute::Records => {
                        if let Some(records) = &mut self.records {
                            records.edit(None, Some(value));
                        }
                    }
                    ScreenRoute::Display => {
                        if let Some(display) = &mut self.display {
                            display.edit(None, Some(value));
                        }
                    }
                    ScreenRoute::LiveAudio => {
                        if !self
                            .game
                            .as_ref()
                            .is_some_and(|game| game.viewer.output_pending())
                        {
                            if let Some(draft) = &mut self.live_audio {
                                draft.edit(None, Some(value));
                            }
                        }
                    }
                    ScreenRoute::Settings => {
                        if let Some(draft) = &mut self.settings {
                            draft.edit(None, Some(value));
                        }
                    }
                    _ => {}
                }
            }
        }
        self.sync_clipboard();
        self.sync_input_font();
    }
    fn key(&mut self, key: KeyCode, repeat: bool) {
        if self.startup.is_some() || self.renderer_pending() {
            if key == KeyCode::Escape && !repeat {
                self.request_close();
            }
            return;
        }
        self.sync_ime();
        if self.ime_owns_keyboard() {
            return;
        }
        self.gesture.cancel();
        if !self.ui_ready() {
            return;
        }
        if self.navigator.route() == ScreenRoute::Selection {
            if key == KeyCode::F3 && !repeat {
                self.set_search_focus(true);
                self.invalidate_hits();
                return;
            }
            if self.search_focused {
                match key {
                    KeyCode::Escape if !repeat => {
                        self.search_editor =
                            LineEditor::new("", 256).expect("empty bounded search");
                        if let Err(error) = self.catalog_search.set_query("") {
                            self.failure = Some(error);
                        }
                        if let Some(index) = self.catalog_search.selected() {
                            self.selected = index;
                        }
                        self.set_search_focus(false);
                        self.invalidate_hits();
                        return;
                    }
                    KeyCode::Enter if !repeat => {
                        self.set_search_focus(false);
                        self.invalidate_hits();
                        return;
                    }
                    KeyCode::ArrowLeft
                    | KeyCode::ArrowRight
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::Backspace
                    | KeyCode::Delete => {
                        self.edit_search(Some(key), None);
                        return;
                    }
                    _ => {}
                }
            }
        }
        if self.navigator.route() == ScreenRoute::Records {
            self.records_key(key, repeat);
            return;
        }
        if self.navigator.route() == ScreenRoute::Practice {
            self.practice_key(key, repeat);
            return;
        }
        if self.navigator.route() == ScreenRoute::Display {
            self.display_key(key, repeat);
            return;
        }
        if matches!(self.navigator.route(), ScreenRoute::Devices { .. }) {
            self.picker_key(key, repeat);
            return;
        }
        if self.navigator.route() == ScreenRoute::Players {
            self.local_key(key, repeat);
            return;
        }
        if self.navigator.route() == ScreenRoute::LiveAudio {
            self.live_audio_key(key, repeat);
            return;
        }
        if !repeat
            && key == KeyCode::F2
            && self.navigator.route() == (ScreenRoute::Play { replay: false })
        {
            self.open_live_audio();
            return;
        }
        if self.navigator.route() == ScreenRoute::Settings {
            if key == KeyCode::F6 && !repeat {
                self.open_practice();
                return;
            }
            if key == KeyCode::F4 && !repeat {
                self.open_records();
                return;
            }
            self.settings_key(key, repeat);
            return;
        }
        if matches!(
            self.navigator.route(),
            ScreenRoute::Play { .. } | ScreenRoute::Results { .. }
        ) && !repeat
            && matches!(key, KeyCode::PageUp | KeyCode::PageDown)
        {
            self.change_local_page(key == KeyCode::PageDown);
            return;
        }
        if matches!(
            self.navigator.route(),
            ScreenRoute::Play { .. } | ScreenRoute::Results { .. }
        ) && !repeat
            && key == KeyCode::KeyC
        {
            self.toggle_local_comparisons();
            return;
        }
        if matches!(
            self.navigator.route(),
            ScreenRoute::Play { .. } | ScreenRoute::Results { .. }
        ) && !repeat
            && key == KeyCode::F5
        {
            self.request_retry();
            return;
        }
        if !repeat
            && key == KeyCode::F7
            && self.navigator.route() == (ScreenRoute::Play { replay: false })
        {
            self.mark_practice();
            return;
        }
        if !repeat
            && key == KeyCode::F9
            && matches!(self.navigator.route(), ScreenRoute::Play { .. })
        {
            self.toggle_pause();
            return;
        }
        if !repeat
            && matches!(key, KeyCode::F10 | KeyCode::F11)
            && self.navigator.route() == (ScreenRoute::Play { replay: false })
        {
            self.edit_loop(key == KeyCode::F11);
            return;
        }
        if !repeat
            && key == KeyCode::F8
            && matches!(
                self.navigator.route(),
                ScreenRoute::Play { replay: false } | ScreenRoute::Results { replay: false }
            )
        {
            self.request_restart(true);
            return;
        }
        if matches!(self.navigator.route(), ScreenRoute::Results { .. }) {
            if !repeat && matches!(key, KeyCode::Enter | KeyCode::Escape) {
                match self.navigate(ScreenRoute::Selection) {
                    Ok(()) => {
                        self.failure = None;
                        if let Some(window) = &self.window {
                            window.set_title("BeatKernel BMS player");
                        }
                    }
                    Err(error) => self.failure = Some(error),
                }
            }
        } else if matches!(self.navigator.route(), ScreenRoute::Play { .. }) {
            if key == KeyCode::Escape && !repeat {
                self.cancel();
            }
        } else {
            match key {
                KeyCode::F2 if !repeat => self.open_settings(),
                KeyCode::Escape if !repeat => self.request_close(),
                KeyCode::ArrowUp | KeyCode::ArrowDown => {
                    self.catalog_wheel.reset();
                    self.catalog_search.step(key == KeyCode::ArrowDown);
                    if let Some(index) = self.catalog_search.selected() {
                        self.selected = index;
                    }
                    self.invalidate_hits();
                }
                KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End => {
                    self.catalog_wheel.reset();
                    if matches!(key, KeyCode::Home | KeyCode::End) {
                        self.catalog_search.edge(key == KeyCode::End);
                    } else {
                        self.catalog_search
                            .step_by(key == KeyCode::PageDown, VISIBLE_ROWS);
                    }
                    if let Some(index) = self.catalog_search.selected() {
                        self.selected = index;
                    }
                    self.invalidate_hits();
                }
                KeyCode::Enter if !repeat && self.catalog_search.selected().is_some() => {
                    if let Err(error) = self.start() {
                        self.failure = Some(error);
                    }
                }
                _ => {}
            }
        }
    }
    fn start(&mut self) -> Result<(), String> {
        if self.catalog.is_some() {
            return Err(if self.options.library.is_some() {
                "wait for the library catalog to finish loading".into()
            } else {
                "wait for chart title font preparation to finish".into()
            });
        }
        if !self.ui_ready() || self.navigator.route() != ScreenRoute::Selection {
            return Err("chart selection is not active".into());
        }
        let next = self.prepare_route(ScreenRoute::Play { replay: false })?;
        let index = self
            .catalog_search
            .selected()
            .ok_or("no matching chart selected")?;
        let entry = self
            .entries
            .get(index)
            .ok_or("selected chart unavailable")?;
        let path = entry
            .path
            .to_str()
            .ok_or("native chart path must be UTF-8")?;
        let args = with_chart(&self.options.native, path);
        (self.validate)(&args).map_err(|error| error.to_string())?;
        let launch = SessionLaunch::new(args)?;
        let game = spawn_game(self.native, launch, 0, false, false)?;
        if let Some(window) = &self.window {
            window.set_title(&window_title(&entry.title, &entry.artist));
        }
        self.failure = None;
        self.commit_route(next);
        self.game = Some(game);
        Ok(())
    }
    fn reactive_waits_for_events(&self) -> bool {
        self.reactive_scene_idle()
            && !self.clipboard_busy()
            && !self.catalog_busy()
            && !self.startup_busy()
            && !self.renderer_startup_busy()
    }
    fn reactive_scene_idle(&self) -> bool {
        matches!(
            self.navigator.route(),
            ScreenRoute::Selection
                | ScreenRoute::Practice
                | ScreenRoute::Settings
                | ScreenRoute::Display
                | ScreenRoute::Records
                | ScreenRoute::Players
                | ScreenRoute::Devices { .. }
        ) && self.navigator.phase() == ScreenPhase::Active
            && !self.occluded
            && self.profile_io.is_none()
            && self
                .renderer
                .as_ref()
                .is_none_or(|renderer| !renderer.needs_redraw())
    }
    fn draw_selection(&mut self) -> Result<(), String> {
        if self.catalog.is_some() {
            let library = self.options.library.is_some();
            self.selection_view = None;
            self.painted_reactive = None;
            self.scene.clear();
            self.hits.clear();
            rect(&mut self.scene, 0, 0, 960, 720, 0x10151e);
            text(
                &mut self.scene,
                24,
                20,
                "BEATKERNEL BMS PLAYER",
                3,
                0xf0f4ff,
            );
            text(
                &mut self.scene,
                24,
                140,
                if library {
                    "LOADING LIBRARY"
                } else {
                    "PREPARING CHART"
                },
                2,
                0xf0f4ff,
            );
            text(
                &mut self.scene,
                24,
                180,
                self.catalog_message.as_deref().unwrap_or(if library {
                    "Preparing library catalog"
                } else {
                    "Preparing chart title font"
                }),
                1,
                0x9bb1cf,
            );
            text(
                &mut self.scene,
                24,
                204,
                "Chart selection is available when preparation finishes.",
                1,
                0x9bb1cf,
            );
            molecules::button(
                &mut self.scene,
                Bounds {
                    x: 550,
                    y: 65,
                    width: 180,
                    height: 34,
                },
                "START",
                false,
                false,
            );
            let point = self.point();
            for (id, y, label) in [(ControlId(5), 20, "SETTINGS"), (ControlId(4), 65, "EXIT")] {
                control(
                    &mut self.scene,
                    &mut self.hits,
                    &self.gesture,
                    point,
                    id,
                    Bounds {
                        x: 750,
                        y,
                        width: 180,
                        height: 34,
                    },
                    label,
                );
            }
            return self.render_scene();
        }
        let id = self
            .navigator
            .active_id()
            .ok_or("Selection instance unavailable")?;
        if self
            .selection_view
            .as_ref()
            .is_none_or(|view| view.id() != id)
        {
            self.selection_view = Some(SelectionView::new_with_font(
                id,
                Arc::clone(&self.selection_items),
                Arc::clone(&self.selection_diagnostics),
                WIDTH as u32,
                HEIGHT as u32,
                self.font_text.clone(),
            )?);
            self.painted_reactive = None;
        }
        let frame = SelectionFrame {
            selected: self.selected,
            hovered: self.hit(),
            armed: [ControlId(1), ControlId(5), ControlId(4)]
                .into_iter()
                .find(|&id| self.gesture.is_armed(id)),
            error: self
                .input_font_error
                .clone()
                .or_else(|| self.failure.clone())
                .or_else(|| self.catalog_message.clone()),
            backend_pending: self.active_backend != self.options.backend,
        };
        let view = self
            .selection_view
            .as_ref()
            .ok_or("Selection view unavailable")?;
        view.set_projection(self.catalog_search.indices(), self.catalog_search.cursor())?;
        let editor = self.ime_editor(ImeField::Search, &self.search_editor);
        view.set_search(editor, self.search_focused)?;
        view.update(frame);
        view.set_input_font(self.input_font.clone());
        if view.dirty() || self.painted_reactive != Some(id) {
            view.compose(&mut self.scene, &mut self.hits)?;
            self.painted_reactive = Some(id);
        }
        self.render_scene()
    }
    fn draw_settings_view(&mut self) -> Result<(), String> {
        let id = self
            .navigator
            .active_id()
            .ok_or("settings instance unavailable")?;
        if self
            .settings_view
            .as_ref()
            .is_none_or(|view| view.id() != id)
        {
            self.settings_view = Some(SettingsView::new(id, WIDTH as u32, HEIGHT as u32)?);
            self.painted_reactive = None;
        }
        let pending = self.profile_io.is_some();
        let point = self.point();
        let hovered = if pending {
            None
        } else {
            point.and_then(|point| {
                SETTINGS_BUTTONS
                    .iter()
                    .rev()
                    .find(|(_, bounds, _)| bounds.contains(point))
                    .map(|(id, _, _)| *id)
            })
        };
        let armed = if pending {
            None
        } else {
            SETTINGS_BUTTONS
                .iter()
                .map(|(id, _, _)| *id)
                .find(|&id| self.gesture.is_armed(id))
        };
        let settings = self.settings.as_ref().ok_or("settings data unavailable")?;
        let editor = self.ime_editor(ImeField::Setting(settings.selected), &settings.editor);
        let profile = self.ime_editor(ImeField::Profile, &settings.profile);
        let view = self
            .settings_view
            .as_ref()
            .ok_or("settings view unavailable")?;
        view.update(SettingsFrame {
            fields: settings.values.fields(),
            selected: settings.selected,
            editor,
            profile,
            profile_focused: settings.profile_focused,
            message: settings.message.as_deref(),
            error: self
                .input_font_error
                .as_deref()
                .or(settings.error.as_deref()),
            pending,
            hovered,
            armed,
        })?;
        view.set_input_font(self.input_font.clone());
        if view.dirty() || self.painted_reactive != Some(id) {
            view.compose(&mut self.scene, &mut self.hits)?;
            self.painted_reactive = Some(id);
        }
        self.render_scene()
    }
    fn draw_display_view(&mut self) -> Result<(), String> {
        let id = self
            .navigator
            .active_id()
            .ok_or("display instance unavailable")?;
        if self
            .display_view
            .as_ref()
            .is_none_or(|view| view.id() != id)
        {
            self.display_view = Some(DisplayView::new(id, WIDTH as u32, HEIGHT as u32)?);
            self.painted_reactive = None;
        }
        let pending = self.profile_io.is_some();
        let point = self.point();
        let hovered = if pending {
            None
        } else {
            point.and_then(|point| {
                DISPLAY_BUTTONS
                    .iter()
                    .rev()
                    .find(|(_, bounds, _)| bounds.contains(point))
                    .map(|(id, _, _)| *id)
            })
        };
        let armed = if pending {
            None
        } else {
            DISPLAY_BUTTONS
                .iter()
                .map(|(id, _, _)| *id)
                .find(|&id| self.gesture.is_armed(id))
        };
        let display = self.display.as_ref().ok_or("display data unavailable")?;
        let preview_editors: Option<[LineEditor; 4]> = self.ime.preview.as_ref().map(|_| {
            std::array::from_fn(|index| {
                self.ime_editor(ImeField::Display(index), &display.editors[index])
                    .clone()
            })
        });
        let view = self
            .display_view
            .as_ref()
            .ok_or("display view unavailable")?;
        view.update(DisplayFrame {
            editors: preview_editors.as_ref().unwrap_or(&display.editors),
            selected: display.selected,
            error: self
                .input_font_error
                .as_deref()
                .or(display.error.as_deref()),
            pending,
            hovered,
            armed,
        })?;
        view.set_input_font(self.input_font.clone());
        if view.dirty() || self.painted_reactive != Some(id) {
            view.compose(&mut self.scene, &mut self.hits)?;
            self.painted_reactive = Some(id);
        }
        self.render_scene()
    }
    fn draw_players_view(&mut self) -> Result<(), String> {
        let id = self
            .navigator
            .active_id()
            .ok_or("players instance unavailable")?;
        if self
            .players_view
            .as_ref()
            .is_none_or(|view| view.id() != id)
        {
            self.players_view = Some(PlayersView::new(id, WIDTH as u32, HEIGHT as u32)?);
            self.painted_reactive = None;
        }
        let local = self
            .local_setup
            .as_ref()
            .ok_or("players data unavailable")?;
        let settings = self
            .settings
            .as_ref()
            .ok_or("players settings unavailable")?;
        let mut frame = players_frame(local, settings, self.profile_io.is_some());
        frame.hovered = beatkernel_bms_runtime::ui::players::hit(&frame, self.point());
        frame.armed = (30..=37)
            .chain(20000..20064)
            .map(ControlId)
            .find(|&id| self.gesture.is_armed(id));
        let view = self
            .players_view
            .as_ref()
            .ok_or("players view unavailable")?;
        view.update(frame)?;
        if view.dirty() || self.painted_reactive != Some(id) {
            view.compose(&mut self.scene, &mut self.hits)?;
            self.painted_reactive = Some(id);
        }
        self.render_scene()
    }
    fn draw_devices_view(&mut self) -> Result<(), String> {
        let id = self
            .navigator
            .active_id()
            .ok_or("devices instance unavailable")?;
        if self
            .devices_view
            .as_ref()
            .is_none_or(|view| view.id() != id)
        {
            self.devices_view = Some(DevicesView::new(id, WIDTH as u32, HEIGHT as u32)?);
            self.painted_reactive = None;
        }
        let picker = self.picker.as_ref().ok_or("devices data unavailable")?;
        let settings = self
            .settings
            .as_ref()
            .ok_or("devices settings unavailable")?;
        let mut frame = devices_frame(picker, settings, self.profile_io.is_some());
        frame.hovered = beatkernel_bms_runtime::ui::devices::hit(&frame, self.point());
        frame.armed = (20..=24)
            .map(ControlId)
            .find(|&id| self.gesture.is_armed(id));
        let view = self
            .devices_view
            .as_ref()
            .ok_or("devices view unavailable")?;
        view.update(frame)?;
        if view.dirty() || self.painted_reactive != Some(id) {
            view.compose(&mut self.scene, &mut self.hits)?;
            self.painted_reactive = Some(id);
        }
        self.render_scene()
    }
    fn draw_records_view(&mut self) -> Result<(), String> {
        let id = self
            .navigator
            .active_id()
            .ok_or("records instance unavailable")?;
        if self
            .records_view
            .as_ref()
            .is_none_or(|view| view.id() != id)
        {
            self.records_view = Some(RecordsView::new(id, WIDTH as u32, HEIGHT as u32)?);
            self.painted_reactive = None;
        }
        let records = self.records.as_ref().ok_or("records data unavailable")?;
        let opponents = self
            .settings
            .as_ref()
            .map_or(0, |draft| saved_opponents(&draft.values));
        let mut frame = records_frame(
            records,
            self.profile_io.is_some(),
            opponents,
            self.settings.as_ref().map(|draft| &draft.values),
        );
        frame.directory = self.ime_editor(ImeField::RecordDirectory, &records.directory);
        frame.error = self.input_font_error.as_deref().or(frame.error);
        frame.hovered = beatkernel_bms_runtime::ui::records::hit(&frame, self.point());
        frame.armed = (50..=61)
            .chain(66..=68)
            .map(ControlId)
            .find(|&id| self.gesture.is_armed(id));
        let view = self
            .records_view
            .as_ref()
            .ok_or("records view unavailable")?;
        view.update(frame)?;
        view.set_input_font(self.input_font.clone());
        if view.dirty() || self.painted_reactive != Some(id) {
            view.compose(&mut self.scene, &mut self.hits)?;
            self.painted_reactive = Some(id);
        }
        self.render_scene()
    }
    fn draw_practice(&mut self) -> Result<(), String> {
        let id = self
            .navigator
            .active_id()
            .ok_or("practice instance unavailable")?;
        let hovered = self.hit();
        let armed = [ControlId(71), ControlId(72), ControlId(73), ControlId(76)]
            .into_iter()
            .find(|&id| self.gesture.is_armed(id));
        let practice = self.practice.as_ref().ok_or("practice data unavailable")?;
        if practice.view.id() != id {
            return Err("practice instance is stale".into());
        }
        practice.view.update(PracticeFrame {
            editor: self
                .ime_editor(ImeField::PracticeStart, &practice.editor)
                .clone(),
            end_editor: self
                .ime_editor(ImeField::PracticeEnd, &practice.end_editor)
                .clone(),
            end_focused: practice.end_focused,
            error: self
                .input_font_error
                .clone()
                .or_else(|| practice.error.clone()),
            hovered,
            armed,
        });
        practice.view.set_input_font(self.input_font.clone());
        if practice.view.dirty() || self.painted_reactive != Some(id) {
            practice.view.compose(&mut self.scene, &mut self.hits)?;
            self.painted_reactive = Some(id);
        }
        self.render_scene()
    }
    fn draw(&mut self) -> Result<(), String> {
        if self.startup.is_some()
            || self.renderer_pending()
            || self.navigator.phase() != ScreenPhase::Active
        {
            return Ok(());
        }
        let route = self.navigator.route();
        if route == ScreenRoute::LiveAudio {
            return self.draw_live_audio();
        }
        let live_audio_available =
            route == (ScreenRoute::Play { replay: false }) && self.live_output_available();
        let backgrounds = self.background_frames(route)?;
        if route == ScreenRoute::Selection {
            return self.draw_selection();
        }
        if route == ScreenRoute::Practice {
            return self.draw_practice();
        }
        if route == ScreenRoute::Settings {
            return self.draw_settings_view();
        }
        if route == ScreenRoute::Display {
            return self.draw_display_view();
        }
        if route == ScreenRoute::Records {
            return self.draw_records_view();
        }
        if route == ScreenRoute::Players {
            return self.draw_players_view();
        }
        if matches!(route, ScreenRoute::Devices { .. }) {
            return self.draw_devices_view();
        }
        self.painted_reactive = None;
        let point = self.point();
        self.invalidate_hits();
        self.scene.clear();
        let pixels = &mut self.scene;
        rect(pixels, 0, 0, WIDTH as i64, HEIGHT as i64, 0x10151e);
        text(pixels, 24, 20, "BEATKERNEL BMS PLAYER", 3, 0xf0f4ff);
        if self.game.as_ref().is_some_and(|game| game.replay) {
            text(pixels, 450, 26, "REPLAY", 2, 0x74e5c5);
        } else if self.game.as_ref().is_some_and(|game| {
            game.launch.args().chunks_exact(2).any(|pair| {
                pair[0] == "--start-ns" && pair[1].parse::<i64>().is_ok_and(|start| start > 0)
            })
        }) {
            text(pixels, 450, 26, "PRACTICE", 2, 0xd8b36b);
        }
        if matches!(
            route,
            ScreenRoute::Play { .. } | ScreenRoute::Results { .. }
        ) {
            let game = self
                .game
                .as_ref()
                .ok_or("session screen data unavailable")?;
            draw_game_with_background(pixels, game, self.options.lookahead, &backgrounds)?;
            if live_audio_available {
                text(pixels, 740, 26, "F2: AUDIO OUTPUT", 1, 0x74e5c5);
            }

            let room = game
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.room.as_ref());
            if let Some(room) = room {
                for (id, label, action, meaningful) in [
                    (ControlId(90), "ROOM SEAL", RoomUiAction::Seal, true),
                    (ControlId(91), "ROOM READY", RoomUiAction::Ready, true),
                    (ControlId(92), "ROOM LEAVE", RoomUiAction::Leave, true),
                    (
                        ControlId(93),
                        "ROOM PREVIOUS",
                        RoomUiAction::Page(room.page.saturating_sub(1)),
                        room.pages > 0 && room.page > 0,
                    ),
                    (
                        ControlId(94),
                        "ROOM NEXT",
                        RoomUiAction::Page(room.page.saturating_add(1)),
                        room.page + 1 < room.pages,
                    ),
                ] {
                    let bounds = room_control_bounds(id).expect("known room control");
                    if meaningful && game.room_action_allowed(action) {
                        control(
                            pixels,
                            &mut self.hits,
                            &self.gesture,
                            point,
                            id,
                            bounds,
                            label,
                        );
                    } else {
                        molecules::button(pixels, bounds, label, false, false);
                    }
                }
                if !game.joined {
                    if let Some(notice) = game.viewer.room_notice() {
                        let clip =
                            beatkernel_bms_runtime::scene::ClipRect::new([300, 646, 300, 17])?;
                        rect(pixels, 300, 646, 300, 17, 0x10151e);
                        beatkernel_bms_runtime::ui::atoms::text_clipped(
                            pixels, 312, 646, &notice, 1, 0x9bb1cf, clip,
                        )?;
                    }
                }
            }
            let pages = game.presentation_page_count();
            if pages > 1 {
                if game.local_page > 0 {
                    control(
                        pixels,
                        &mut self.hits,
                        &self.gesture,
                        point,
                        ControlId(6),
                        Bounds {
                            x: 620,
                            y: 658,
                            width: 150,
                            height: 34,
                        },
                        "PREVIOUS",
                    );
                }
                if game.local_page + 1 < pages {
                    control(
                        pixels,
                        &mut self.hits,
                        &self.gesture,
                        point,
                        ControlId(7),
                        Bounds {
                            x: 780,
                            y: 658,
                            width: 150,
                            height: 34,
                        },
                        "NEXT",
                    );
                }
            }
            if game.comparisons_available() {
                control(
                    pixels,
                    &mut self.hits,
                    &self.gesture,
                    point,
                    ControlId(8),
                    Bounds {
                        x: 550,
                        y: 65,
                        width: 180,
                        height: 34,
                    },
                    if game.local_comparisons {
                        "COMPARISONS *"
                    } else {
                        "COMPARISONS"
                    },
                );
            }
            if let Some(paused) = game.pause_target() {
                control(
                    pixels,
                    &mut self.hits,
                    &self.gesture,
                    point,
                    ControlId(62),
                    PAUSE_BOUNDS,
                    if paused { "PAUSE F9" } else { "RESUME F9" },
                );
            }
            if !game.replay && room.is_none() {
                let mark_bounds = Bounds {
                    x: 550,
                    y: 20,
                    width: 140,
                    height: 30,
                };
                let restart_bounds = Bounds {
                    x: 700,
                    y: 20,
                    width: 170,
                    height: 30,
                };
                if game.practice_position().is_some() {
                    control(
                        pixels,
                        &mut self.hits,
                        &self.gesture,
                        point,
                        ControlId(60),
                        mark_bounds,
                        "MARK F7",
                    );
                } else {
                    molecules::button(pixels, mark_bounds, "MARK F7", false, false);
                }
                if game.practice_restart_available() {
                    control(
                        pixels,
                        &mut self.hits,
                        &self.gesture,
                        point,
                        ControlId(61),
                        restart_bounds,
                        "RESTART F8",
                    );
                } else {
                    molecules::button(pixels, restart_bounds, "RESTART F8", false, false);
                }
                let caption = game.practice_loop.map_or_else(
                    || {
                        game.practice_bookmark.map_or_else(
                            || "F7 MARK / F8 RESTART MARK".to_owned(),
                            |start| format!("MARK {}  F8 RESTART", start.formatted()),
                        )
                    },
                    |region| {
                        format!(
                            "LOOP {} - {} {}",
                            region.start().formatted(),
                            region.end().formatted(),
                            if game.loop_enabled { "ON" } else { "OFF" }
                        )
                    },
                );
                text(pixels, 24, 102, &caption, 1, 0xd8b36b);
                for (id, bounds, label, enabled) in [
                    (
                        ControlId(63),
                        LOOP_END_BOUNDS,
                        "END F10",
                        game.loop_controls_available() && game.practice_bookmark.is_some(),
                    ),
                    (
                        ControlId(64),
                        LOOP_TOGGLE_BOUNDS,
                        if game.loop_enabled {
                            "LOOP ON F11"
                        } else {
                            "LOOP OFF F11"
                        },
                        game.loop_controls_available() && game.practice_loop.is_some(),
                    ),
                ] {
                    if enabled {
                        control(
                            pixels,
                            &mut self.hits,
                            &self.gesture,
                            point,
                            id,
                            bounds,
                            label,
                        );
                    } else {
                        molecules::button(pixels, bounds, label, false, false);
                    }
                }
            }
            let retry_bounds = Bounds {
                x: 410,
                y: 65,
                width: 130,
                height: 34,
            };
            if game.retry_available() {
                control(
                    pixels,
                    &mut self.hits,
                    &self.gesture,
                    point,
                    ControlId(9),
                    retry_bounds,
                    "RETRY F5",
                );
            } else {
                molecules::button(pixels, retry_bounds, "RETRY F5", false, false);
            }
            if game.joined {
                control(
                    pixels,
                    &mut self.hits,
                    &self.gesture,
                    point,
                    ControlId(3),
                    Bounds {
                        x: 750,
                        y: 65,
                        width: 180,
                        height: 34,
                    },
                    "RETURN",
                );
            } else if !game.cancelling {
                control(
                    pixels,
                    &mut self.hits,
                    &self.gesture,
                    point,
                    ControlId(2),
                    Bounds {
                        x: 750,
                        y: 65,
                        width: 180,
                        height: 34,
                    },
                    "CANCEL",
                );
            }
        } else {
            return Err("screen has no drawable presentation".into());
        }
        if let Some(error) = &self.failure {
            text(
                pixels,
                24,
                650,
                game_error_caption(self.game.as_ref()),
                2,
                0xff8e8e,
            );
            text(pixels, 24, 682, error, 1, 0xffaaaa);
        }
        self.render_scene()
    }
    fn render_scene(&mut self) -> Result<(), String> {
        let mut presented = false;
        if let Some(renderer) = &mut self.renderer {
            renderer.render(&self.scene)?;
            if renderer.needs_surface_recreation() {
                let instance = self.instance.as_ref().ok_or("missing GPU instance")?;
                let window = self
                    .window
                    .as_ref()
                    .ok_or("missing window for surface recreation")?;
                let surface = instance
                    .create_surface(window.clone())
                    .map_err(|error| error.to_string())?;
                renderer.replace_surface(surface)?;
            }
            presented = !renderer.needs_redraw();
        }
        if presented {
            self.publish_ime_cursor_area();
        } else {
            self.ime.cursor_area = None;
        }
        Ok(())
    }

    fn initialize_renderer(&mut self) -> Result<(), String> {
        if self.startup.is_some()
            || self.renderer.is_some()
            || self.renderer_startup.is_some()
            || self.closing()
            || self.is_suspended()
            || self.occluded
        {
            return Ok(());
        }
        let Some(window) = self.window.as_ref().cloned() else {
            return Ok(());
        };
        // Native instance/surface acquisition stays with the window owner.
        // Their owned handles move to the worker; WASM setup remains async.
        let instance = graphics::instance(self.active_backend)?;
        let surface = instance
            .create_surface(window)
            .map_err(|error| error.to_string())?;
        let presentation = self.options.presentation;
        let job = NativeCatalog::spawn_prepared(move |control| {
            control.checkpoint()?;
            let renderer = pollster::block_on(Renderer::new(surface, &instance, presentation))?;
            control.checkpoint()?;
            Ok(PreparedRenderer { instance, renderer })
        })?;
        self.renderer_startup = Some(RendererStartup {
            job,
            retired: false,
        });
        self.gesture.cancel();
        self.invalidate_hits();
        if let Some(window) = &self.window {
            window.set_title("BeatKernel BMS player — Preparing graphics…");
        }
        Ok(())
    }

    fn renderer_pending(&self) -> bool {
        self.renderer_startup.is_some() || (self.window.is_some() && self.renderer.is_none())
    }

    fn renderer_startup_busy(&self) -> bool {
        self.renderer_startup.as_ref().is_some_and(|startup| {
            !startup.job.is_finished()
                || startup.retired
                || self.closing()
                || (!self.is_suspended() && !self.occluded)
        })
    }

    fn retire_renderer_startup(&mut self) {
        if let Some(startup) = &mut self.renderer_startup {
            startup.retired = true;
            startup.job.cancel();
        }
    }

    fn collect_renderer(&mut self) {
        let Some(startup) = &self.renderer_startup else {
            return;
        };
        let retired = startup.retired;
        if !retired && !self.closing() && (self.is_suspended() || self.occluded) {
            return;
        }
        let Some(result) = self
            .renderer_startup
            .as_mut()
            .and_then(|startup| startup.job.poll())
        else {
            return;
        };
        self.renderer_startup = None;
        if retired || self.closing() {
            return;
        }
        let result = result.and_then(
            |PreparedRenderer {
                 instance,
                 mut renderer,
             }| {
                let window = self
                    .window
                    .as_ref()
                    .ok_or("renderer window is unavailable")?;
                // Use current UI size/atlas, never the values at worker admission.
                // No handle or font binding is published until all preflight succeeds.
                let size = window.inner_size();
                renderer.resize(size.width, size.height)?;
                let font_text = self
                    .title_font
                    .as_ref()
                    .map(|atlas| {
                        let texture = renderer.upload_texture(atlas.image())?;
                        FontText::new(Arc::clone(atlas), texture)
                    })
                    .transpose()?;
                self.instance = Some(instance);
                self.renderer = Some(renderer);
                self.bind_title_font(font_text);
                Ok(())
            },
        );
        if let Err(error) = result {
            self.fail(error);
            return;
        }
        self.next_frame = Instant::now();
        if let Some(window) = &self.window {
            window.set_title("BeatKernel BMS player");
            window.request_redraw();
        }
    }

    fn release_backgrounds(&mut self) {
        if let Some(renderer) = &mut self.renderer {
            if let Err(error) = self.bga_cache.clear(renderer) {
                self.failure = Some(error);
            }
        } else {
            self.bga_cache = BgaTextureCache::default();
        }
    }

    /// Only UI-owned renderer operations occur here; banks were prepared by
    /// the native game owner. Exact member clocks determine visible selections.
    fn background_frames(&mut self, route: ScreenRoute) -> Result<[BgaFrame; 4], String> {
        let Some(renderer) = &mut self.renderer else {
            self.bga_cache = BgaTextureCache::default();
            return Ok([BgaFrame::default(); 4]);
        };
        if !matches!(
            route,
            ScreenRoute::Play { .. } | ScreenRoute::Results { .. }
        ) {
            return self.bga_cache.sync(None, &[], renderer);
        }
        let Some(game) = &self.game else {
            return self.bga_cache.sync(None, &[], renderer);
        };
        if game.joined && !game.replay {
            return self.bga_cache.sync(None, &[], renderer);
        }
        let Some(snapshot) = &game.snapshot else {
            return self.bga_cache.sync(None, &[], renderer);
        };
        let (states, count) = background_presentations(snapshot, game.local_page)?;
        self.bga_cache
            .sync_presentations(snapshot.images.as_ref(), &states[..count], renderer)
    }
}
impl ApplicationHandler for Desktop {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.closing() {
            return;
        }
        self.navigator.resume();
        self.synchronize_panel_lifecycles();
        if self.window.is_none() {
            match event_loop.create_window(
                Window::default_attributes()
                    .with_title(if self.startup.is_some() {
                        "BeatKernel BMS player — Loading profile…"
                    } else {
                        "BeatKernel BMS player — Preparing graphics…"
                    })
                    .with_inner_size(LogicalSize::new(WIDTH as f64, HEIGHT as f64)),
            ) {
                Ok(window) => {
                    self.ime.cursor_area = None;
                    self.window = Some(Arc::new(window));
                }
                Err(error) => {
                    self.fail(error);
                    return;
                }
            }
        }
        self.active = self
            .window
            .as_ref()
            .is_some_and(|window| window.has_focus());
        self.collect_startup();
        self.collect_renderer();
        if self.startup.is_some() || self.closing() {
            return;
        }
        if let Err(error) = self.initialize_renderer() {
            self.fail(error);
            return;
        }
        self.next_frame = Instant::now();
        if let Some(window) = &self.window {
            if !self.renderer_pending() {
                window.request_redraw();
            }
        }
    }
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.retire_renderer_startup();
        self.catalog_wheel.reset();
        self.navigator.suspend();
        self.synchronize_panel_lifecycles();
        self.active = false;
        self.pointer = None;
        self.gesture.cancel();
        self.invalidate_hits();
        self.cancel();
        self.renderer = None;
        self.font_text = None;
        self.input_font = None;
        self.bga_cache = BgaTextureCache::default();
        self.instance = None;
    }
    fn window_event(&mut self, _event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.window.as_ref().is_none_or(|window| window.id() != id) {
            return;
        }
        let request_redraw = !matches!(&event, WindowEvent::RedrawRequested);
        match event {
            WindowEvent::CloseRequested => {
                self.request_close();
            }
            WindowEvent::Destroyed => {
                self.request_close();
                self.renderer = None;
                self.bga_cache = BgaTextureCache::default();
                self.window = None;
            }
            WindowEvent::Focused(active) => {
                self.catalog_wheel.reset();
                self.active = active;
                if !active {
                    self.pointer = None;
                    self.cancel();
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.catalog_wheel.reset();
                self.occluded = occluded;
                if occluded {
                    self.gesture.cancel();
                }
            }
            WindowEvent::Resized(size) => {
                self.catalog_wheel.reset();
                self.painted_reactive = None;
                self.gesture.cancel();
                self.pointer = None;
                self.invalidate_hits();
                if let Some(renderer) = &mut self.renderer {
                    if let Err(error) = renderer.resize(size.width, size.height) {
                        self.fail(error);
                    }
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                self.catalog_wheel.reset();
                self.gesture.cancel();
                self.pointer = None;
                // Winit resize sizes are already physical; do not multiply DPI.
                self.invalidate_hits();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = Some((position.x, position.y));
                if !self.over_catalog(self.point()) {
                    self.catalog_wheel.reset();
                }
            }
            WindowEvent::Ime(event) => self.ime_event(event),
            WindowEvent::CursorLeft { .. } => {
                self.catalog_wheel.reset();
                self.pointer = None;
                self.gesture.cancel();
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                if matches!(phase, TouchPhase::Started | TouchPhase::Cancelled) {
                    self.catalog_wheel.reset();
                }
                if phase != TouchPhase::Cancelled {
                    let physical = self.window.as_ref().map_or((0, 0), |window| {
                        let size = window.inner_size();
                        (size.width, size.height)
                    });
                    let lines = catalog_scroll_lines(delta, physical);
                    let over_catalog = self.over_catalog(self.point());
                    self.scroll_catalog(lines, over_catalog);
                }
                if phase == TouchPhase::Ended {
                    self.catalog_wheel.reset();
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if state == ElementState::Pressed && self.ime.composing {
                    self.ime = ImeDraft::default();
                    if let Some(window) = &self.window {
                        window.set_ime_allowed(false);
                    }
                    self.sync_ime();
                }
                self.catalog_wheel.reset();
                let hit = self.hit();
                match state {
                    ElementState::Pressed => self.gesture.press(hit),
                    ElementState::Released => {
                        if let Some(id) = self.gesture.release(hit) {
                            self.activate(id);
                        }
                    }
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers_changed(modifiers.state()),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                self.keyboard_input(
                    event.physical_key,
                    &event.logical_key,
                    event.text.as_deref(),
                    event.repeat,
                );
            }
            WindowEvent::RedrawRequested
                if self.startup.is_none()
                    && !self.renderer_pending()
                    && !self.is_suspended()
                    && !self.closing()
                    && !self.occluded =>
            {
                self.collect_game();
                if let Err(error) = self.draw() {
                    self.fail(error);
                }
                self.next_frame =
                    Instant::now() + Duration::from_secs_f64(1.0 / self.options.fps as f64);
            }
            _ => {}
        }
        self.sync_ime();
        if request_redraw {
            self.sync_input_font();
        }
        if request_redraw
            && self.startup.is_none()
            && !self.renderer_pending()
            && !self.closing()
            && !self.is_suspended()
            && !self.occluded
        {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.collect_startup();
        self.collect_renderer();
        if self.startup.is_some() {
            event_loop.set_control_flow(if self.startup_busy() {
                ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(4))
            } else {
                ControlFlow::Wait
            });
            return;
        }
        if self.renderer.is_none() && !self.closing() && !self.is_suspended() && !self.occluded {
            if let Err(error) = self.initialize_renderer() {
                self.fail(error);
            }
        }
        self.collect_catalog();
        self.collect_game();
        self.collect_profile();
        self.sync_ime();
        self.collect_clipboard();
        if self.closing()
            && self.game.as_ref().is_none_or(|game| game.joined)
            && self.profile_io.is_none()
            && self.startup.is_none()
            && self.renderer_startup.is_none()
            && self.catalog.is_none()
            && self
                .clipboard
                .as_ref()
                .is_none_or(ClipboardWorker::is_finished)
        {
            event_loop.exit();
            return;
        }
        if self.renderer_pending() {
            // A ready hidden result stays owned without continuous wakeups.
            // Retired/closing work still wakes until its real join is consumed.
            let busy = self.renderer_startup_busy()
                || self.catalog_busy()
                || self.clipboard_busy()
                || self.profile_io.is_some()
                || self.game.as_ref().is_some_and(|game| !game.joined)
                || self.closing();
            event_loop.set_control_flow(if busy {
                ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(4))
            } else {
                ControlFlow::Wait
            });
            return;
        }
        if self.reactive_waits_for_events() {
            event_loop.set_control_flow(ControlFlow::Wait);
        } else if self.closing()
            || self.is_suspended()
            || self.occluded
            || ((self.clipboard_busy() || self.catalog_busy()) && self.reactive_scene_idle())
        {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(4),
            ));
        } else {
            if Instant::now() >= self.next_frame {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                // Avoid a busy event loop before the queued redraw is dispatched.
                self.next_frame =
                    Instant::now() + Duration::from_secs_f64(1.0 / self.options.fps as f64);
            }
            let wake = if self.clipboard_busy() || self.catalog_busy() {
                self.next_frame
                    .min(Instant::now() + Duration::from_millis(4))
            } else {
                self.next_frame
            };
            event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
        }
    }
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.request_close();
        // Unexpected OS exit still joins native owners through Game::drop.
        self.game = None;
        self.release_backgrounds();
        self.profile_io = None;
        self.startup = None;
        self.renderer_startup = None;
        self.catalog = None;
        self.clipboard = None;
    }
}

fn players_frame<'a>(
    local: &'a LocalDraft,
    draft: &'a SettingsDraft,
    pending: bool,
) -> PlayersFrame<'a> {
    PlayersFrame {
        model: &local.model,
        selected: local.selected,
        first: local.first,
        pending,
        error: draft.error.as_deref(),
        message: draft.message.as_deref(),
        hovered: None,
        armed: None,
    }
}
fn devices_frame<'a>(
    picker: &'a DevicePicker,
    draft: &'a SettingsDraft,
    pending: bool,
) -> DevicesFrame<'a> {
    DevicesFrame {
        catalog: &picker.catalog,
        player: picker.player,
        selected: picker.selected,
        first: picker.first,
        pending,
        error: draft.error.as_deref(),
        hovered: None,
        armed: None,
    }
}
#[cfg(test)]
fn draw_local(
    scene: &mut Scene,
    local: &LocalDraft,
    draft: &SettingsDraft,
    hits: &mut Vec<(ControlId, Bounds)>,
    gesture: &Gesture,
    point: Option<(f64, f64)>,
    pending: bool,
) {
    let view = PlayersView::new(ScreenInstanceId(1), WIDTH as u32, HEIGHT as u32).unwrap();
    let mut frame = players_frame(local, draft, pending);
    frame.hovered = beatkernel_bms_runtime::ui::players::hit(&frame, point);
    frame.armed = (30..=37)
        .chain(20000..20064)
        .map(ControlId)
        .find(|&id| gesture.is_armed(id));
    view.update(frame).unwrap();
    view.compose(scene, hits).unwrap();
}

#[cfg(test)]
fn draw_display(
    scene: &mut Scene,
    display: &DisplayDraft,
    hits: &mut Vec<(ControlId, Bounds)>,
    gesture: &Gesture,
    point: Option<(f64, f64)>,
    pending: bool,
) {
    let view = DisplayView::new(ScreenInstanceId(1), WIDTH as u32, HEIGHT as u32).unwrap();
    let hovered = point.and_then(|point| {
        DISPLAY_BUTTONS
            .iter()
            .rev()
            .find(|(_, bounds, _)| bounds.contains(point))
            .map(|(id, _, _)| *id)
    });
    let armed = DISPLAY_BUTTONS
        .iter()
        .map(|(id, _, _)| *id)
        .find(|&id| gesture.is_armed(id));
    view.update(DisplayFrame {
        editors: &display.editors,
        selected: display.selected,
        error: display.error.as_deref(),
        pending,
        hovered,
        armed,
    })
    .unwrap();
    view.compose(scene, hits).unwrap();
}

fn saved_opponents(settings: &NativeSettings) -> usize {
    settings
        .fields()
        .iter()
        .filter(|field| {
            matches!(field.flag, "--ghost-self" | "--ghost-other") && !field.value.is_empty()
        })
        .count()
}
fn records_frame<'a>(
    records: &'a RecordsDraft,
    pending: bool,
    opponents: usize,
    settings: Option<&NativeSettings>,
) -> RecordsFrame<'a> {
    RecordsFrame {
        directory: &records.directory,
        details: records.details,
        grade_page: records.grade_page,
        directory_focused: records.directory_focused,
        catalog: records.catalog.as_ref(),
        selected: records.selected,
        first: records.first,
        preview: records.valid_preview(),
        pending,
        opponents,
        selected_opponents: settings
            .zip(records.selected_path().and_then(|path| path.to_str()))
            .map_or([0; 2], |(settings, path)| {
                [
                    settings.opponent_count(OpponentKind::Own, path),
                    settings.opponent_count(OpponentKind::Other, path),
                ]
            }),
        message: records.message.as_deref(),
        error: records.error.as_deref(),
        hovered: None,
        armed: None,
    }
}

#[cfg(test)]
fn draw_records(
    scene: &mut Scene,
    records: &RecordsDraft,
    hits: &mut Vec<(ControlId, Bounds)>,
    gesture: &Gesture,
    point: Option<(f64, f64)>,
    pending: bool,
    opponents: usize,
) {
    let id = ScreenNavigator::default().active_id().unwrap();
    let view = RecordsView::new(id, WIDTH as u32, HEIGHT as u32).unwrap();
    let mut frame = records_frame(records, pending, opponents, None);
    frame.hovered = beatkernel_bms_runtime::ui::records::hit(&frame, point);
    frame.armed = (50..=61)
        .chain(66..=68)
        .map(ControlId)
        .find(|&id| gesture.is_armed(id));
    view.update(frame).unwrap();
    view.compose(scene, hits).unwrap();
}

fn control(
    scene: &mut Scene,
    hits: &mut Vec<(ControlId, Bounds)>,
    gesture: &Gesture,
    point: Option<(f64, f64)>,
    id: ControlId,
    bounds: Bounds,
    label: &str,
) {
    molecules::button(
        scene,
        bounds,
        label,
        point.is_some_and(|point| bounds.contains(point)),
        gesture.is_armed(id),
    );
    hits.push((id, bounds));
}

fn room_control_bounds(id: ControlId) -> Option<Bounds> {
    Some(match id.0 {
        90 => Bounds {
            x: 550,
            y: 20,
            width: 110,
            height: 30,
        },
        91 => Bounds {
            x: 670,
            y: 20,
            width: 110,
            height: 30,
        },
        92 => Bounds {
            x: 790,
            y: 20,
            width: 140,
            height: 30,
        },
        93 => Bounds {
            x: 620,
            y: 695,
            width: 150,
            height: 24,
        },
        94 => Bounds {
            x: 780,
            y: 695,
            width: 150,
            height: 24,
        },
        _ => return None,
    })
}

fn window_title(title: &str, artist: &str) -> String {
    format!("{title} — {artist} — BeatKernel")
        .chars()
        .filter(|character| !character.is_control())
        .take(256)
        .collect()
}

fn game_error_caption(game: Option<&Game>) -> &'static str {
    match game {
        Some(game) if !game.joined && game.cancelling => "ERROR - WAITING FOR CLEANUP",
        Some(game) if !game.joined => "ERROR - SESSION CONTINUES",
        _ => "ERROR - ENTER RETURNS TO SELECTION",
    }
}
fn local_comparisons_available(players: &[player::LocalPlayerSnapshot]) -> bool {
    players.len() >= 2
        && players.iter().any(|player| {
            player.competition.as_ref().is_some_and(|comparisons| {
                !comparisons.ghosts.is_empty() || comparisons.network.is_some()
            })
        })
}
#[cfg(test)]
fn local_page(count: usize, current: usize, forward: bool) -> usize {
    let last = count
        .div_ceil(organisms::LOCAL_PLAYERS_PER_PAGE)
        .saturating_sub(1);
    let current = current.min(last);
    if forward {
        current.saturating_add(1).min(last)
    } else {
        current.saturating_sub(1)
    }
}

#[cfg(test)]
fn background_states(
    snapshot: &player::PlayerSnapshot,
    page: usize,
) -> Result<([BgaState; 4], usize), String> {
    let (presentations, count) = background_presentations(snapshot, page)?;
    Ok((presentations.map(|presentation| presentation.state), count))
}

fn background_presentations(
    snapshot: &player::PlayerSnapshot,
    page: usize,
) -> Result<
    (
        [beatkernel_bms_runtime::poor_background::BgaPresentation; 4],
        usize,
    ),
    String,
> {
    if snapshot.players.len() > 64 {
        return Err("local background roster exceeds capacity".into());
    }
    let mut states = [beatkernel_bms_runtime::poor_background::BgaPresentation::default(); 4];
    let count = if snapshot.players.len() >= 2 {
        let first = page
            .checked_mul(4)
            .filter(|first| *first < snapshot.players.len())
            .ok_or("local background page exceeds roster")?;
        let members = &snapshot.players[first..(first + 4).min(snapshot.players.len())];
        for (slot, member) in members.iter().enumerate() {
            if let (Some(chart), Some(now)) = (&member.chart, member.song_time) {
                states[slot] =
                    beatkernel_bms_runtime::poor_background::PoorBackgroundPolicy::default()
                        .select(chart, now, member.note_progress.as_ref())?;
            }
        }
        members.len()
    } else {
        if let (Some(chart), Some(now)) = (&snapshot.chart, snapshot.song_time) {
            states[0] = beatkernel_bms_runtime::poor_background::PoorBackgroundPolicy::default()
                .select(chart, now, snapshot.note_progress.as_ref())?;
        }
        1
    };
    Ok((states, count))
}

#[cfg(test)]
fn draw_game(pixels: &mut Scene, game: &Game, lookahead: i64) -> Result<(), String> {
    draw_game_with_background(pixels, game, lookahead, &[BgaFrame::default(); 4])
}

fn draw_game_with_background(
    pixels: &mut Scene,
    game: &Game,
    lookahead: i64,
    backgrounds: &[BgaFrame; 4],
) -> Result<(), String> {
    let Some(snapshot) = &game.snapshot else {
        text(pixels, 24, 80, "LOADING - ESC CANCEL", 2, 0x9bb1cf);
        return Ok(());
    };
    let status = if game.prepared_retry.is_some() && !game.joined {
        "RETRY WAITING FOR CLEANUP"
    } else if game.cancelling && !game.joined {
        "STOPPING"
    } else if game.joined && game.replay {
        "RECORD PREFIX RESULTS - ENTER RETURN"
    } else if game.joined {
        "RESULTS - ENTER RETURN"
    } else {
        match &snapshot.status {
            player::PlayerStatus::Loading => "LOADING",
            player::PlayerStatus::Playing if game.replay && snapshot.song_time.is_none() => {
                "NATIVE PRESENTATION UNAVAILABLE - ESC CANCEL"
            }
            player::PlayerStatus::Playing if snapshot.pause == player::PauseState::Pausing => {
                "PAUSING - NATIVE WAIT"
            }
            player::PlayerStatus::Playing if snapshot.pause == player::PauseState::Paused => {
                "PAUSED - F9 RESUME"
            }
            player::PlayerStatus::Playing if snapshot.pause == player::PauseState::Resuming => {
                "RESUMING - NATIVE WAIT"
            }
            player::PlayerStatus::Playing if game.replay => "WATCHING RECORD - ESC CANCEL",
            player::PlayerStatus::Playing => "PLAYING - ESC CANCEL",
            player::PlayerStatus::Stopping => "STOPPING",
            player::PlayerStatus::Finished => "FINISHING CLEANUP",
            player::PlayerStatus::Failed(_) => "FAILED - CLEANING UP",
        }
    };
    text(pixels, 24, 65, status, 2, 0x9bb1cf);
    if game.joined && !game.replay {
        if let Some(results) = &game.completed_results {
            results.compose_mode(pixels, game.local_page, game.local_comparisons)?;
        } else if let Some(error) = &game.completed_results_error {
            text(
                pixels,
                24,
                140,
                "COMPLETED RESULTS UNAVAILABLE",
                2,
                0xff8e8e,
            );
            text(pixels, 24, 175, error, 1, 0xff8e8e);
        } else {
            text(pixels, 24, 140, "NO COMPLETED PLAY", 2, 0x9bb1cf);
        }
        if let player::PlayerStatus::Failed(error) = &snapshot.status {
            text(pixels, 24, 620, "TECHNICAL ERROR", 1, 0xff8e8e);
            text(pixels, 150, 620, error, 1, 0xff8e8e);
        }
        if let Some(room) = &snapshot.room {
            organisms::room_presentation_footer(pixels, room)?;
        }
        return Ok(());
    }
    if backgrounds.iter().any(|frame| frame.unavailable != 0) {
        text(pixels, 750, 96, "BACKGROUND UNAVAILABLE", 1, 0xd8b36b);
    }
    if snapshot.players.len() >= 2 {
        organisms::local_players_with_background(
            pixels,
            &snapshot.players,
            lookahead,
            game.local_page,
            game.local_comparisons,
            backgrounds,
        )?;
        if snapshot.room.is_none() {
            text(
                pixels,
                24,
                665,
                &format!(
                    "PLAYERS {}  PAGE {}/{}  PGUP/PGDN{}",
                    snapshot.players.len(),
                    game.local_page + 1,
                    snapshot
                        .players
                        .len()
                        .div_ceil(organisms::LOCAL_PLAYERS_PER_PAGE),
                    if local_comparisons_available(&snapshot.players) {
                        "  C COMPARISONS"
                    } else {
                        ""
                    }
                ),
                1,
                0x9bb1cf,
            );
        }
        if let Some(room) = &snapshot.room {
            organisms::room_presentation_footer(pixels, room)?;
        }
        return Ok(());
    }
    molecules::gauge_hud(
        pixels,
        &snapshot.gauge,
        Bounds {
            x: 750,
            y: 110,
            width: 186,
            height: 18,
        },
    )?;
    if let Some(competition) = snapshot
        .players
        .first()
        .and_then(|player| player.competition.as_ref())
    {
        organisms::competition_scoreboard_with_bms_score(
            pixels,
            &snapshot.score,
            competition,
            snapshot.bms_score.as_ref(),
        )?;
    } else {
        organisms::scoreboard_with_bms_score(
            pixels,
            &snapshot.score,
            &snapshot.recent_results,
            snapshot.bms_score.as_ref(),
        );
    }
    if let (Some(chart), Some(now)) = (&snapshot.chart, snapshot.song_time) {
        organisms::playfield_with_background(
            pixels,
            chart,
            now,
            lookahead,
            &snapshot.recent_results,
            snapshot.pressed_lanes,
            snapshot.note_progress.as_ref(),
            backgrounds[0],
        )?;
        if snapshot.room.is_none() {
            text(
                pixels,
                24,
                665,
                &format!("SONG {:.3} S", now.as_nanos() as f64 / 1e9),
                2,
                0x9bb1cf,
            );
        }
        if let Some(event) = snapshot.last_judge {
            if (i128::from(now.as_nanos()) - i128::from(event.at.as_nanos())).abs() <= 700_000_000 {
                let (label, color) = beatkernel_bms_runtime::timing_display::judge_label(&event);
                text(pixels, 160, 550, &label, 2, color);
            }
        }
    }
    if let Some(room) = &snapshot.room {
        organisms::room_presentation_footer(pixels, room)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_game_composes_background_and_rejects_invalid_sprite_frame() {
        use beatkernel::time::Timestamp;
        use beatkernel_bms_runtime::{bga_render::BgaSprite, texture::TextureId};
        let source = beatkernel_bms::parse(
            "#BPM 120\n#WAV01 tap.wav\n#00011:01\n#00004:01\n",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        let chart = Arc::new(
            player_chart::PlayerChart::from_compiled(&source, &source.compile().unwrap().chart)
                .unwrap(),
        );
        let mut game = retry_fixture();
        game.snapshot = Some(player::PlayerSnapshot {
            chart: Some(chart),
            song_time: Some(Timestamp::ZERO),
            status: player::PlayerStatus::Playing,
            ..Default::default()
        });
        let mut scene = Scene::new(WIDTH as u32, HEIGHT as u32);
        draw_game(&mut scene, &game, 1_000_000_000).unwrap();
        scene.clear();
        // Pure geometry fixture uses an available builtin texture identity;
        // GPU upload/pixel correctness is deliberately not claimed here.
        let mut frames = [BgaFrame::default(); 4];
        frames[0] = BgaFrame {
            active: true,
            base: Some(BgaSprite {
                texture: TextureId::FONT,
                width: 2,
                height: 1,
            }),
            ..Default::default()
        };
        draw_game_with_background(&mut scene, &game, 1_000_000_000, &frames).unwrap();
        assert!(scene.status().is_ok());
        scene.clear();
        frames[0].base.as_mut().unwrap().height = 0;
        assert!(draw_game_with_background(&mut scene, &game, 1_000_000_000, &frames).is_err());
        scene.clear();
        frames[0] = BgaFrame {
            active: true,
            unavailable: 1,
            ..Default::default()
        };
        draw_game_with_background(&mut scene, &game, 1_000_000_000, &frames).unwrap();
    }

    #[test]
    fn native_background_presentations_preserve_all_three_poor_modes_per_member() {
        use beatkernel::{
            judge::{JudgeEvent, JudgeOutcome, JudgeStage, MissReason},
            time::Timestamp,
        };
        use beatkernel_bms::ImageId;
        let members: Vec<_> = [0,1,2].into_iter().map(|mode| {
            let source = beatkernel_bms::parse(&format!("#BPM 120\n#WAV01 tap.wav\n#00011:01\n#BMP00 poor.bmp\n#00004:01\n#00007:02\n#0000A:04\n#0000B:FF80402010080402\n#0000C:80\n#0000D:40\n#0000E:20\n#POORBGA {mode}"), beatkernel_bms::ParseOptions::default()).unwrap();
            let chart = Arc::new(player_chart::PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap());
            let mut progress = beatkernel_bms_runtime::note_progress::NoteProgress::new(chart.clone()).unwrap();
            progress.apply(&[JudgeEvent { object: chart.notes[0].object, stage: JudgeStage::Instant, outcome: JudgeOutcome::Miss { reason: MissReason::HeadTimeout }, at: Timestamp::ZERO, input: None }]);
            player::LocalPlayerSnapshot { bms_score: None, player: PlayerId(mode + 1), chart: Some(chart), song_time: Some(Timestamp::from_nanos(i64::from(mode) * 125_000_000)), score: Default::default(), mine_damage: Default::default(), gauge: Default::default(), last_judge: None, recent_results: vec![], pressed_lanes: 0, note_progress: Some(progress), competition: None }
        }).collect();
        let mut snapshot = player::PlayerSnapshot {
            players: members,
            ..Default::default()
        };
        let (shown, count) = background_presentations(&snapshot, 0).unwrap();
        assert_eq!(count, 3);
        assert_eq!(shown[0].opacity.base, 32); // Replace uses Poor opacity.
        assert_eq!(shown[1].opacity.base, 255); // Before the next marker at 250ms.
        assert_eq!(shown[2].opacity.base, 128); // Own original-song time at 250ms.
        for selection in &shown[..count] {
            assert_eq!(
                (
                    selection.opacity.layer,
                    selection.opacity.layer2,
                    selection.opacity.poor
                ),
                (128, 64, 32)
            );
        }
        assert_eq!(shown[0].state.base, Some(ImageId(0)));
        assert!(shown[0].state.layer.is_none() && shown[0].poor_overlay.is_none());
        assert!(shown[0].state.layer2.is_none());
        assert_eq!(shown[1].state.base, Some(ImageId(1)));
        assert_eq!(shown[1].state.layer, Some(ImageId(2)));
        assert_eq!(shown[1].state.layer2, Some(ImageId(4)));
        assert_eq!(shown[1].poor_overlay, Some(ImageId(0)));
        assert_eq!(shown[2].state.base, Some(ImageId(1)));
        assert_eq!(shown[2].state.layer, Some(ImageId(2)));
        assert_eq!(shown[2].state.layer2, Some(ImageId(4)));
        assert!(shown[2].poor_overlay.is_none());
        snapshot.pause = player::PauseState::Paused;
        assert_eq!(background_presentations(&snapshot, 0).unwrap().0, shown);
        let overlay = snapshot.players[1].clone();
        for member in &mut snapshot.players {
            member.song_time = Some(Timestamp::from_nanos(500_000_000));
        }
        assert!(
            background_presentations(&snapshot, 0)
                .unwrap()
                .0
                .iter()
                .all(|p| p.poor_overlay.is_none())
        );
        snapshot.players.clear();
        snapshot.chart = overlay.chart;
        snapshot.song_time = overlay.song_time;
        snapshot.note_progress = overlay.note_progress;
        assert_eq!(
            background_presentations(&snapshot, 0).unwrap().0[0],
            shown[1]
        );
    }
    #[test]
    fn poor_background_uses_each_members_prefix_and_solo_alias_without_wall_time() {
        use beatkernel::{
            judge::{JudgeEvent, JudgeOutcome, JudgeStage, MissReason},
            time::Timestamp,
        };
        use beatkernel_bms::ImageId;
        let source = beatkernel_bms::parse(
            "#BPM 120\n#WAV01 tap.wav\n#00011:01\n#BMP00 poor.bmp\n#00004:01\n#00007:02\n#00106:03",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        let chart = Arc::new(
            player_chart::PlayerChart::from_compiled(&source, &source.compile().unwrap().chart)
                .unwrap(),
        );
        let event = JudgeEvent {
            object: chart.notes[0].object,
            stage: JudgeStage::Instant,
            outcome: JudgeOutcome::Miss {
                reason: MissReason::HeadTimeout,
            },
            at: Timestamp::ZERO,
            input: None,
        };
        let mut missed =
            beatkernel_bms_runtime::note_progress::NoteProgress::new(chart.clone()).unwrap();
        missed.apply(&[event]);
        let mut snapshot = player::PlayerSnapshot {
            players: [7, u32::MAX, 2]
                .into_iter()
                .map(|id| player::LocalPlayerSnapshot {
                    bms_score: None,
                    player: PlayerId(id),
                    chart: Some(chart.clone()),
                    mine_damage: Default::default(),
                    gauge: Default::default(),
                    song_time: Some(Timestamp::ZERO),
                    score: Default::default(),
                    last_judge: None,
                    recent_results: vec![],
                    pressed_lanes: 0,
                    note_progress: Some(if id == u32::MAX {
                        beatkernel_bms_runtime::note_progress::NoteProgress::new(chart.clone())
                            .unwrap()
                    } else {
                        missed.clone()
                    }),
                    competition: None,
                })
                .collect(),
            ..Default::default()
        };
        snapshot.players[2].song_time = Some(Timestamp::from_nanos(499_999_999));
        let (states, count) = background_states(&snapshot, 0).unwrap();
        assert_eq!(count, 3);
        assert_eq!(states[0].base, Some(ImageId(0)));
        assert!(states[0].layer.is_none());
        assert_eq!(states[1].base, Some(ImageId(1)));
        assert_eq!(states[1].layer, Some(ImageId(2)));
        assert_eq!(states[2], states[0]);
        assert_eq!(states[3], BgaState::default());
        snapshot.pause = player::PauseState::Paused;
        assert_eq!(background_states(&snapshot, 0).unwrap().0, states);
        snapshot.players[0].song_time = Some(Timestamp::from_nanos(500_000_000));
        assert_eq!(background_states(&snapshot, 0).unwrap().0[0], states[1]);
        snapshot.players.clear();
        snapshot.chart = Some(chart.clone());
        snapshot.song_time = Some(Timestamp::ZERO);
        snapshot.note_progress = Some(missed);
        assert_eq!(background_states(&snapshot, 0).unwrap().0[0], states[0]);
        snapshot.note_progress = Some(
            beatkernel_bms_runtime::note_progress::NoteProgress::new(Arc::new(
                chart.as_ref().clone(),
            ))
            .unwrap(),
        );
        assert!(background_states(&snapshot, 0).is_err());
    }
    #[test]
    fn visible_background_states_use_exact_member_clock_and_page_without_wall_time() {
        use beatkernel::time::Timestamp;
        use beatkernel_bms::ImageId;
        let source = beatkernel_bms::parse(
            "#BPM 120\n#WAV01 tap.wav\n#00011:01\n#00004:01\n#00104:02\n#00107:03\n",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        let chart = Arc::new(
            player_chart::PlayerChart::from_compiled(&source, &source.compile().unwrap().chart)
                .unwrap(),
        );
        let mut snapshot = player::PlayerSnapshot {
            players: [7, u32::MAX, 2, 900, 41]
                .into_iter()
                .zip([
                    0,
                    2_000_000_000,
                    1_999_999_999,
                    3_000_000_000,
                    1_000_000_000,
                ])
                .map(|(id, ns)| player::LocalPlayerSnapshot {
                    bms_score: None,
                    mine_damage: Default::default(),
                    gauge: Default::default(),
                    player: PlayerId(id),
                    chart: Some(Arc::clone(&chart)),
                    song_time: Some(Timestamp::from_nanos(ns)),
                    score: Default::default(),
                    last_judge: None,
                    recent_results: vec![],
                    pressed_lanes: 0,
                    note_progress: None,
                    competition: None,
                })
                .collect(),
            ..Default::default()
        };
        let (states, count) = background_states(&snapshot, 0).unwrap();
        assert_eq!(count, 4);
        assert_eq!(
            states.map(|s| s.base),
            [
                Some(ImageId(1)),
                Some(ImageId(2)),
                Some(ImageId(1)),
                Some(ImageId(2))
            ]
        );
        assert_eq!(
            states.map(|s| s.layer),
            [None, Some(ImageId(3)), None, Some(ImageId(3))]
        );
        snapshot.pause = player::PauseState::Paused;
        assert_eq!(background_states(&snapshot, 0).unwrap().0, states);
        let (next, count) = background_states(&snapshot, 1).unwrap();
        assert_eq!(count, 1);
        assert_eq!(next[0].base, Some(ImageId(1)));
        assert_eq!(next[1..], [BgaState::default(); 3]);
        assert!(background_states(&snapshot, usize::MAX).is_err());
        snapshot.players[1].song_time = Some(Timestamp::ZERO);
        assert_eq!(
            background_states(&snapshot, 0).unwrap().0[1].base,
            Some(ImageId(1))
        );
        snapshot.players.clear();
        snapshot.chart = Some(chart);
        snapshot.song_time = Some(Timestamp::from_nanos(2_000_000_000));
        assert_eq!(
            background_states(&snapshot, 0).unwrap().0[0].layer,
            Some(ImageId(3))
        );
    }

    #[test]
    fn title_font_option_is_ui_owned_and_rejects_empty_or_duplicate_paths() {
        let args = [
            "--chart",
            "fixture.bms",
            "--title-font",
            "fonts/title.ttf",
            "--bind",
            "11:04",
        ]
        .map(String::from);
        let options = Options::parse(&args).unwrap();
        assert_eq!(options.title_font, Some(PathBuf::from("fonts/title.ttf")));
        assert_eq!(
            options.native,
            ["--chart", "fixture.bms", "--bind", "11:04"]
        );
        for args in [
            vec!["--title-font", ""],
            vec!["--title-font", "one.ttf", "--title-font", "two.ttf"],
            vec!["--title-font"],
        ] {
            assert!(
                Options::parse(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err()
            );
        }
        assert!(prepare_title_font(vec![0; 64], &[]).is_err());
    }
    #[test]
    fn selection_font_prepares_artist_prefix_and_rejects_invalid_artist_before_startup() {
        let items = [SelectionItem {
            title: "A".into(),
            artist: "가".into(),
        }];
        let atlas = prepare_title_font(font_fixture::font_bytes(), &items).unwrap();
        assert!(atlas.get('A').is_some());
        assert!(atlas.get('가').is_some());
        let items = [SelectionItem {
            title: "A".into(),
            artist: "가".repeat(MAX_TEXT_GLYPHS) + "\n",
        }];
        assert!(prepare_title_font(font_fixture::font_bytes(), &items).is_ok());
        let items = [SelectionItem {
            title: "A".into(),
            artist: "가\n".into(),
        }];
        assert!(prepare_title_font(font_fixture::font_bytes(), &items).is_err());
    }
    #[test]
    fn renderer_font_rebinding_discards_retained_titles_and_hits_but_keeps_search() {
        let mut app = lifecycle_fixture();
        app.search_editor.insert("FIX").unwrap();
        app.selection_view = Some(
            SelectionView::new(
                ScreenInstanceId(7),
                Arc::clone(&app.selection_items),
                Arc::from([]),
                WIDTH as u32,
                HEIGHT as u32,
            )
            .unwrap(),
        );
        app.hits.push((
            ControlId(100),
            Bounds {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
        ));
        app.bind_title_font(None);
        assert!(app.selection_view.is_none());
        assert!(app.hits.is_empty());
        assert!(app.painted_reactive.is_none());
        assert_eq!(app.search_editor.value(), "FIX");
        assert_eq!(app.entries[0].path, PathBuf::from("fixture.bms"));
    }
    #[test]
    fn field_font_preparation_preserves_edits_and_old_atlas_when_capacity_is_exhausted() {
        let mut app = lifecycle_fixture();
        app.title_font = Some(Arc::new(
            FontAtlas::new(font_fixture::font_bytes(), 14.0, 128, 128, 1).unwrap(),
        ));
        app.set_search_focus(true);
        app.edit_search(None, Some("A"));
        let before = Arc::clone(app.title_font.as_ref().unwrap());
        assert!(before.get('A').is_some());
        app.edit_search(None, Some("가"));
        assert_eq!(app.search_editor.value(), "A가");
        assert!(Arc::ptr_eq(&before, app.title_font.as_ref().unwrap()));
        assert!(
            app.input_font_error
                .as_ref()
                .unwrap()
                .contains("USING BITMAP")
        );
        assert!(app.input_font.is_none());
        app.draw_selection().unwrap();
        select_all_input(&mut app);
        app.modifiers_changed(ModifiersState::empty());
        pressed_input(&mut app, KeyCode::KeyA, Some("A"));
        assert_eq!(app.search_editor.value(), "A");
        assert!(app.input_font_error.is_none());
        assert!(Arc::ptr_eq(&before, app.title_font.as_ref().unwrap()));
        assert!(app.renderer.is_none());
    }
    #[test]
    fn actual_ime_and_draft_navigation_prepare_unicode_without_renderer_or_native_io() {
        let mut app = lifecycle_fixture();
        app.title_font = Some(Arc::new(
            FontAtlas::new(font_fixture::font_bytes(), 14.0, 128, 128, 128).unwrap(),
        ));
        app.set_search_focus(true);
        app.ime_event(Ime::Enabled);
        app.ime_event(Ime::Preedit("가".into(), Some((0, 3))));
        assert!(app.title_font.as_ref().unwrap().get('가').is_some());
        assert_eq!(app.search_editor.value(), "");
        let prepared = Arc::clone(app.title_font.as_ref().unwrap());
        app.ime_event(Ime::Preedit("".into(), None));
        assert!(Arc::ptr_eq(&prepared, app.title_font.as_ref().unwrap()));
        app.open_settings();
        app.settings.as_mut().unwrap().profile_focused = true;
        pressed_input(&mut app, KeyCode::KeyX, Some("別.profile"));
        for character in "別.profile".chars() {
            assert!(app.title_font.as_ref().unwrap().get(character).is_some());
        }
        app.open_display();
        pressed_input(&mut app, KeyCode::KeyX, Some("音"));
        assert!(app.title_font.as_ref().unwrap().get('音').is_some());
        app.back();
        app.open_practice();
        pressed_input(&mut app, KeyCode::KeyX, Some("語"));
        assert!(app.title_font.as_ref().unwrap().get('語').is_some());
        app.practice.as_mut().unwrap().end_focused = true;
        pressed_input(&mut app, KeyCode::KeyX, Some("終"));
        assert!(app.title_font.as_ref().unwrap().get('終').is_some());
        app.back();
        app.open_records();
        pressed_input(&mut app, KeyCode::KeyX, Some("録"));
        assert!(app.title_font.as_ref().unwrap().get('録').is_some());
        assert!(app.input_font_error.is_none());
        assert_eq!(app.options.native, ["--chart", "fixture.bms"]);
        assert!(app.profile_io.is_none());
        assert!(app.renderer.is_none());
        assert!(app.window.is_none());
        assert!(app.game.is_none());
    }
    fn select_all_input(app: &mut Desktop) {
        app.modifiers_changed(if cfg!(target_os = "macos") {
            ModifiersState::SUPER
        } else {
            ModifiersState::CONTROL
        });
        // Logical A on a different physical key must still select all.
        app.keyboard_input(
            PhysicalKey::Code(KeyCode::KeyQ),
            &Key::Character("a".into()),
            Some("a"),
            false,
        );
    }
    fn pressed_input(app: &mut Desktop, key: KeyCode, text: Option<&str>) {
        app.keyboard_input(
            PhysicalKey::Code(key),
            &Key::Character(text.unwrap_or("").into()),
            text,
            false,
        );
    }
    fn field_editor(app: &Desktop, field: usize) -> &LineEditor {
        match field {
            0 => &app.search_editor,
            1 => &app.settings.as_ref().unwrap().editor,
            2 => &app.settings.as_ref().unwrap().profile,
            3 => &app.display.as_ref().unwrap().editors[2],
            4 => &app.practice.as_ref().unwrap().editor,
            5 => &app.practice.as_ref().unwrap().end_editor,
            6 => &app.records.as_ref().unwrap().directory,
            _ => unreachable!(),
        }
    }
    #[test]
    fn selection_shortcuts_follow_platform_logical_keys_and_exclude_extra_modifiers() {
        for macos in [false, true] {
            let command = if macos {
                ModifiersState::SUPER
            } else {
                ModifiersState::CONTROL
            };
            let other = if macos {
                ModifiersState::CONTROL
            } else {
                ModifiersState::SUPER
            };
            let physical = PhysicalKey::Code(KeyCode::KeyQ);
            for character in ["a", "A"] {
                assert_eq!(
                    selection_command(physical, &Key::Character(character.into()), command, macos),
                    Some(SelectionCommand::All)
                );
            }
            for modifiers in [
                ModifiersState::empty(),
                other,
                command | other,
                command | ModifiersState::ALT,
                command | ModifiersState::SHIFT,
            ] {
                assert_eq!(
                    selection_command(physical, &Key::Character("a".into()), modifiers, macos),
                    None
                );
            }
            assert_eq!(
                selection_command(
                    PhysicalKey::Code(KeyCode::KeyA),
                    &Key::Character("q".into()),
                    command,
                    macos
                ),
                None
            );
            for (key, expected) in [
                (KeyCode::ArrowLeft, SelectionCommand::Left),
                (KeyCode::ArrowRight, SelectionCommand::Right),
                (KeyCode::Home, SelectionCommand::Home),
                (KeyCode::End, SelectionCommand::End),
            ] {
                let physical = PhysicalKey::Code(key);
                let logical = Key::Character("".into());
                assert_eq!(
                    selection_command(physical, &logical, ModifiersState::SHIFT, macos),
                    Some(expected)
                );
                for extra in [command, other, ModifiersState::ALT] {
                    assert_eq!(
                        selection_command(physical, &logical, ModifiersState::SHIFT | extra, macos),
                        None
                    );
                }
            }
        }
    }
    #[test]
    fn actual_keyboard_route_selects_and_replaces_each_editable_draft_without_inserting_shortcut() {
        for field in 0..7 {
            let mut app = lifecycle_fixture();
            if field == 0 {
                app.set_search_focus(true);
            } else {
                app.open_settings();
                match field {
                    1 => {
                        let draft = app.settings.as_mut().unwrap();
                        let index = draft
                            .values
                            .fields()
                            .iter()
                            .position(|field| field.flag == "--record-replay")
                            .unwrap();
                        draft.select(index).unwrap();
                    }
                    2 => app.settings.as_mut().unwrap().profile_focused = true,
                    3 => {
                        app.open_display();
                        app.display.as_mut().unwrap().selected = 2;
                    }
                    4 | 5 => {
                        app.open_practice();
                        app.practice.as_mut().unwrap().end_focused = field == 5;
                    }
                    6 => app.open_records(),
                    _ => unreachable!(),
                }
            }
            select_all_input(&mut app);
            app.modifiers_changed(ModifiersState::empty());
            pressed_input(&mut app, KeyCode::KeyX, Some("a別b"));
            assert_eq!(field_editor(&app, field).value(), "a別b", "field {field}");
            select_all_input(&mut app);
            assert_eq!(field_editor(&app, field).value(), "a別b");
            assert_eq!(field_editor(&app, field).selection(), Some((0, 5)));
            app.modifiers_changed(ModifiersState::empty());
            pressed_input(&mut app, KeyCode::ArrowLeft, None);
            assert_eq!(field_editor(&app, field).cursor(), 0);
            app.modifiers_changed(ModifiersState::SHIFT);
            pressed_input(&mut app, KeyCode::ArrowRight, None);
            pressed_input(&mut app, KeyCode::ArrowRight, None);
            assert_eq!(field_editor(&app, field).selection(), Some((0, 4)));
            app.modifiers_changed(ModifiersState::empty());
            pressed_input(&mut app, KeyCode::KeyE, Some("é"));
            assert_eq!(field_editor(&app, field).value(), "éb");
            assert_eq!(field_editor(&app, field).selection(), None);
            select_all_input(&mut app);
            app.modifiers_changed(ModifiersState::empty());
            pressed_input(&mut app, KeyCode::Backspace, None);
            assert_eq!(field_editor(&app, field).value(), "");
            assert_eq!(app.options.native, ["--chart", "fixture.bms"]);
            assert!(app.window.is_none());
            assert!(app.renderer.is_none());
            assert!(app.game.is_none());
        }
    }
    #[test]
    fn selected_ime_preview_cancels_or_replaces_once_and_owns_selection_shortcuts() {
        let mut app = lifecycle_fixture();
        app.set_search_focus(true);
        app.edit_search(None, Some("a別b"));
        select_all_input(&mut app);
        let original = app.search_editor.clone();
        let projection = app.catalog_search.indices();
        app.ime_event(Ime::Enabled);
        app.ime_event(Ime::Preedit("音".into(), Some((3, 3))));
        assert_eq!(app.ime.preview.as_ref().unwrap().value(), "音");
        assert_eq!(app.search_editor, original);
        select_all_input(&mut app);
        assert_eq!(app.search_editor, original);
        assert_eq!(app.ime.preview.as_ref().unwrap().value(), "音");
        assert!(Arc::ptr_eq(&projection, &app.catalog_search.indices()));
        app.ime_event(Ime::Preedit("".into(), None));
        assert!(app.ime.preview.is_none());
        assert_eq!(app.search_editor, original);
        app.ime_event(Ime::Preedit("音".into(), None));
        app.ime_event(Ime::Commit("音".into()));
        assert_eq!(app.search_editor.value(), "音");
        assert_eq!(app.search_editor.selection(), None);
        assert_eq!(app.search_editor.composition(), None);
        app.draw_selection().unwrap();
        assert!(!app.selection_view.as_ref().unwrap().dirty());
    }
    #[test]
    fn selection_modifiers_reset_on_unavailable_ui_and_drafts_keep_their_own_selection() {
        let mut app = lifecycle_fixture();
        app.open_settings();
        app.settings.as_mut().unwrap().profile_focused = true;
        pressed_input(&mut app, KeyCode::KeyX, Some("profile.bkp"));
        select_all_input(&mut app);
        let selected = app.settings.as_ref().unwrap().profile.clone();
        app.settings.as_mut().unwrap().profile_focused = false;
        app.sync_ime();
        assert!(!app.modifiers.is_empty()); // Field change retains held hardware modifiers.
        assert_eq!(app.settings.as_ref().unwrap().profile, selected);
        assert_eq!(app.settings.as_ref().unwrap().editor.selection(), None);
        for unavailable in 0..4 {
            app.modifiers_changed(ModifiersState::SHIFT);
            match unavailable {
                0 => app.active = false,
                1 => app.occluded = true,
                2 => app.navigator.suspend(),
                _ => {
                    let (owner, permit) = app.metadata_scope().unwrap();
                    app.profile_io = Some(ProfileOperation {
                        owner,
                        permit,
                        worker: None,
                    });
                }
            }
            app.sync_ime();
            assert!(app.modifiers.is_empty());
            let before = app.settings.as_ref().unwrap().editor.clone();
            select_all_input(&mut app);
            assert!(app.modifiers.is_empty());
            assert_eq!(app.settings.as_ref().unwrap().editor, before);
            assert_eq!(app.settings.as_ref().unwrap().profile, selected);
            app.active = true;
            app.occluded = false;
            app.profile_io = None;
            app.navigator.resume();
            app.sync_ime();
        }
        app.modifiers_changed(ModifiersState::SHIFT);
        app.request_close();
        assert!(app.modifiers.is_empty());
        let mut app = lifecycle_fixture();
        select_all_input(&mut app); // No focused search field.
        assert_eq!(app.search_editor.value(), "");
        assert_eq!(app.search_editor.selection(), None);
        assert!(app.game.is_none());
    }
    pub(super) fn lifecycle_fixture() -> Desktop {
        fn native_unavailable(_: &[String]) -> Result<(), Box<dyn Error>> {
            Err("fixture must not open native playback".into())
        }
        fn validate_only(_: &[String]) -> Result<(), Box<dyn Error>> {
            Ok(())
        }
        fn query_unavailable(_: DeviceRequest) -> Result<DeviceCatalog, Box<dyn Error>> {
            Err("fixture must not query devices".into())
        }
        Desktop {
            options: Options::parse(&["--chart".into(), "fixture.bms".into()]).unwrap(),
            active_backend: BackendChoice::Auto,
            display: None,
            display_view: None,
            players_view: None,
            devices_view: None,
            practice: None,
            records: None,
            records_view: None,
            native: native_unavailable,
            validate: validate_only,
            query_devices: query_unavailable,
            replay: native_unavailable,
            validate_replay: validate_only,
            picker: None,
            local_setup: None,
            accepted_local: None,
            live_audio: None,
            live_audio_view: None,
            settings: None,
            settings_view: None,
            profile_io: None,
            startup: None,
            catalog: None,
            catalog_progress: player_chart::ScanProgress::default(),
            catalog_message: None,
            entries: vec![Entry {
                path: PathBuf::from("fixture.bms"),
                title: "FIXTURE".into(),
                artist: String::new(),
            }],
            selected: 0,
            selection_items: vec![SelectionItem {
                title: "FIXTURE".into(),
                artist: String::new(),
            }]
            .into(),
            catalog_search: CatalogSearch::new(&[SelectionItem {
                title: "FIXTURE".into(),
                artist: String::new(),
            }])
            .unwrap(),
            search_editor: LineEditor::new("", 256).unwrap(),
            search_focused: false,
            catalog_wheel: WheelSteps::default(),
            ime: ImeDraft::default(),
            modifiers: ModifiersState::empty(),
            clipboard: None,
            pending_clipboard: None,
            title_font: None,
            font_text: None,
            input_font: None,
            input_font_error: None,
            selection_diagnostics: Arc::from([]),
            selection_view: None,
            painted_reactive: None,
            window: None,
            renderer: None,
            renderer_startup: None,
            bga_cache: BgaTextureCache::default(),
            instance: None,
            scene: Scene::new(WIDTH as u32, HEIGHT as u32),
            game: None,
            navigator: ScreenNavigator::default(),
            active: true,
            occluded: false,
            failure: None,
            fatal: None,
            next_frame: Instant::now(),
            pointer: None,
            gesture: Gesture::default(),
            hits: Vec::new(),
        }
    }
    #[test]
    fn ime_ranges_reach_retained_scenes_and_clear_without_committing_preview_text() {
        let mut app = lifecycle_fixture();
        app.set_search_focus(true);
        app.ime_event(Ime::Enabled);
        let projection = app.catalog_search.indices();
        for (end, columns) in [(3, 1), (5, 2)] {
            app.ime_event(Ime::Preedit("音é".into(), Some((0, end))));
            let line = app
                .ime_editor(ImeField::Search, &app.search_editor)
                .visible_line(40);
            assert_eq!(line.composition, Some((0, 2)));
            assert_eq!(line.selection, Some((0, columns)));
            assert!(line.caret_visible);
            app.draw_selection().unwrap();
            assert!(!app.selection_view.as_ref().unwrap().dirty());
            app.scene.status().unwrap();
            assert_eq!(app.search_editor.value(), "");
            assert!(Arc::ptr_eq(&projection, &app.catalog_search.indices()));
        }
        app.ime_event(Ime::Preedit("音é".into(), None));
        assert!(
            !app.ime_editor(ImeField::Search, &app.search_editor)
                .visible_line(40)
                .caret_visible
        );
        app.draw_selection().unwrap();
        app.ime_event(Ime::Commit("音é".into()));
        app.draw_selection().unwrap();
        assert_eq!(app.search_editor.value(), "音é");
        assert!(app.ime.preview.is_none());
        assert!(app.search_editor.composition().is_none());
        assert!(app.search_editor.visible_line(40).caret_visible);
        app.open_settings();
        app.settings.as_mut().unwrap().profile_focused = true;
        app.sync_ime();
        app.ime_event(Ime::Enabled);
        let args = app.settings.as_ref().unwrap().values.native_args();
        for switch_field in [false, true] {
            app.ime_event(Ime::Preedit("별é".into(), Some((0, 5))));
            let profile = &app.settings.as_ref().unwrap().profile;
            let line = app.ime_editor(ImeField::Profile, profile).visible_line(40);
            assert_eq!(
                (line.composition, line.selection),
                (Some((0, 2)), Some((0, 2)))
            );
            app.draw_settings_view().unwrap();
            assert!(!app.settings_view.as_ref().unwrap().dirty());
            app.scene.status().unwrap();
            if switch_field {
                app.settings.as_mut().unwrap().profile_focused = false;
                app.sync_ime();
            } else {
                app.ime_event(Ime::Disabled);
            }
            app.draw_settings_view().unwrap();
            assert!(app.ime.preview.is_none());
            let profile = &app.settings.as_ref().unwrap().profile;
            assert!(
                app.ime_editor(ImeField::Profile, profile)
                    .composition()
                    .is_none()
            );
            assert_eq!(profile.value(), "");
            assert_eq!(app.settings.as_ref().unwrap().values.native_args(), args);
            app.ime_event(Ime::Enabled);
        }
        assert!(app.renderer.is_none());
        assert!(app.window.is_none());
    }
    #[test]
    fn ime_preview_is_visual_and_commit_updates_only_acknowledged_search_or_setting() {
        let mut app = lifecycle_fixture();
        app.set_search_focus(true);
        let projection = app.catalog_search.indices();
        app.ime_event(Ime::Commit("ignored".into()));
        assert_eq!(app.search_editor.value(), "");
        app.ime_event(Ime::Enabled);
        app.ime_event(Ime::Preedit("音".into(), Some((3, 3))));
        assert!(app.ime_owns_keyboard());
        assert_eq!(
            app.ime_editor(ImeField::Search, &app.search_editor).value(),
            "音"
        );
        assert_eq!(app.search_editor.value(), "");
        assert!(Arc::ptr_eq(&projection, &app.catalog_search.indices()));
        app.key(KeyCode::Enter, false);
        assert!(app.search_focused);
        assert!(app.game.is_none());
        app.ime_event(Ime::Preedit("".into(), None));
        app.ime_event(Ime::Commit("音".into()));
        assert_eq!(app.search_editor.value(), "音");
        assert_eq!(
            app.ime_editor(ImeField::Search, &app.search_editor).value(),
            "音"
        );
        app.open_settings();
        let index = app
            .settings
            .as_ref()
            .unwrap()
            .values
            .fields()
            .iter()
            .position(|field| field.flag == "--record-replay")
            .unwrap();
        app.settings.as_mut().unwrap().select(index).unwrap();
        app.sync_ime();
        let before = app.settings.as_ref().unwrap().values.native_args();
        app.ime_event(Ime::Enabled);
        app.ime_event(Ime::Preedit("별.bkr".into(), Some((3, 3))));
        assert_eq!(app.settings.as_ref().unwrap().values.native_args(), before);
        app.key(KeyCode::Enter, false);
        assert_eq!(app.navigator.route(), ScreenRoute::Settings);
        app.ime_event(Ime::Commit("별.bkr".into()));
        let draft = app.settings.as_ref().unwrap();
        assert_eq!(draft.editor.value(), "별.bkr");
        assert_eq!(draft.values.fields()[index].value, "별.bkr");
        assert_eq!(app.options.native, ["--chart", "fixture.bms"]);
    }
    #[test]
    fn ime_field_and_lifecycle_changes_drop_preview_and_require_fresh_enable() {
        let mut app = lifecycle_fixture();
        app.open_settings();
        app.ime_event(Ime::Enabled);
        app.ime_event(Ime::Preedit("old".into(), None));
        app.settings.as_mut().unwrap().profile_focused = true;
        app.sync_ime();
        assert!(app.ime.preview.is_none());
        assert!(!app.ime.enabled);
        app.ime_event(Ime::Commit("stale".into()));
        assert_eq!(app.settings.as_ref().unwrap().profile.value(), "");
        app.ime_event(Ime::Enabled);
        app.ime_event(Ime::Commit("기록.json".into()));
        assert_eq!(app.settings.as_ref().unwrap().profile.value(), "기록.json");
        for inactive in 0..3 {
            app.ime_event(Ime::Preedit("pending".into(), None));
            match inactive {
                0 => app.active = false,
                1 => app.occluded = true,
                _ => app.navigator.suspend(),
            }
            app.sync_ime();
            app.ime_event(Ime::Enabled);
            app.ime_event(Ime::Commit("lost".into()));
            assert!(app.ime.target.is_none());
            assert!(app.ime.preview.is_none());
            app.active = true;
            app.occluded = false;
            app.navigator.resume();
            app.sync_ime();
            app.ime_event(Ime::Enabled);
        }
        assert_eq!(app.settings.as_ref().unwrap().profile.value(), "기록.json");
        app.back();
        app.ime_event(Ime::Commit("late".into()));
        assert!(app.ime.target.is_none());
        assert!(app.settings.is_none());
        assert!(app.game.is_none());
    }
    #[test]
    fn ime_invalid_preview_and_commit_preserve_settings_and_show_errors() {
        let mut app = lifecycle_fixture();
        app.open_settings();
        let before = app.settings.as_ref().unwrap().values.native_args();
        app.ime_event(Ime::Enabled);
        app.ime_event(Ime::Preedit("音".into(), Some((1, 3))));
        assert!(app.ime.preview.is_none());
        assert!(app.settings.as_ref().unwrap().error.is_some());
        for invalid in ["\n".into(), "x".repeat(4097)] {
            app.ime_event(Ime::Commit(invalid));
            assert_eq!(app.settings.as_ref().unwrap().values.native_args(), before);
            assert!(app.settings.as_ref().unwrap().error.is_some());
        }
        app.ime_event(Ime::Preedit("valid".into(), None));
        app.ime_event(Ime::Disabled);
        assert!(!app.ime_owns_keyboard());
        assert!(app.ime.preview.is_none());
        app.ime_event(Ime::Commit("ignored".into()));
        assert_eq!(app.settings.as_ref().unwrap().values.native_args(), before);
    }
    #[test]
    fn catalog_pages_and_edges_preserve_filtered_chart_and_search_caret() {
        let mut app = catalog_navigation_fixture();
        let projection = app.catalog_search.indices();
        app.key(KeyCode::PageDown, false);
        assert_eq!(app.selected, 30);
        app.key(KeyCode::End, false);
        assert_eq!(app.selected, 38);
        app.key(KeyCode::PageUp, true);
        assert_eq!(app.selected, 8);
        app.key(KeyCode::Home, false);
        assert_eq!(app.selected, 0);
        app.set_search_focus(true);
        app.key(KeyCode::End, false);
        assert_eq!(app.search_editor.cursor(), 4);
        assert_eq!(app.selected, 0);
        app.key(KeyCode::Home, false);
        assert_eq!(app.search_editor.cursor(), 0);
        assert_eq!(app.selected, 0);
        assert!(Arc::ptr_eq(&projection, &app.catalog_search.indices()));
        app.catalog_search.set_query("absent").unwrap();
        app.set_search_focus(false);
        for key in [
            KeyCode::Home,
            KeyCode::End,
            KeyCode::PageDown,
            KeyCode::PageUp,
        ] {
            app.key(key, false);
            assert_eq!(app.catalog_search.selected(), None);
        }
        assert!(app.game.is_none());
    }
    fn catalog_navigation_fixture() -> Desktop {
        let mut app = lifecycle_fixture();
        app.entries = (0..40)
            .map(|index| Entry {
                path: format!("chart{index}.bms").into(),
                title: format!("CHART {index}"),
                artist: if index % 2 == 0 { "even" } else { "odd" }.into(),
            })
            .collect();
        app.selection_items = app
            .entries
            .iter()
            .map(|entry| SelectionItem {
                title: entry.title.clone(),
                artist: entry.artist.clone(),
            })
            .collect::<Vec<_>>()
            .into();
        app.catalog_search = CatalogSearch::new(&app.selection_items).unwrap();
        app.catalog_search.set_query("even").unwrap();
        app.search_editor = LineEditor::new("even", 256).unwrap();
        app
    }
    #[test]
    fn catalog_wheel_admission_fraction_reset_and_click_cancellation() {
        let mut app = catalog_navigation_fixture();
        let projection = app.catalog_search.indices();
        app.gesture.press(Some(ControlId(100)));
        app.scroll_catalog(-0.75, true);
        assert_eq!(app.selected, 0);
        assert_eq!(app.gesture.release(Some(ControlId(100))), None);
        app.scroll_catalog(-0.25, true);
        assert_eq!(app.selected, 2);
        app.scroll_catalog(1.0, true);
        assert_eq!(app.selected, 0);
        for admission in 0..5 {
            app.scroll_catalog(-0.75, true);
            match admission {
                0 => app.scroll_catalog(-1.0, false),
                1 => {
                    app.active = false;
                    app.scroll_catalog(-1.0, true);
                    app.active = true;
                }
                2 => {
                    app.occluded = true;
                    app.scroll_catalog(-1.0, true);
                    app.occluded = false;
                }
                3 => {
                    app.open_settings();
                    app.back();
                }
                _ => {
                    app.set_search_focus(true);
                    app.set_search_focus(false);
                }
            }
            app.scroll_catalog(-0.25, true);
            assert_eq!(app.selected, 0);
            app.catalog_wheel.reset();
        }
        app.scroll_catalog(-0.75, true);
        app.scroll_catalog(f64::NAN, true);
        app.scroll_catalog(-0.25, true);
        assert_eq!(app.selected, 0);
        app.catalog_wheel.reset();
        app.scroll_catalog(-f64::MAX, true);
        assert_eq!(app.selected, 30); // At most one displayed page per event.
        app.scroll_catalog(0.0, true);
        assert_eq!(app.selected, 30);
        assert!(Arc::ptr_eq(&projection, &app.catalog_search.indices()));
        assert!(app.game.is_none());
    }
    #[test]
    fn consecutive_catalog_wheels_keep_admission_while_click_hits_wait_for_redraw() {
        let mut app = catalog_navigation_fixture();
        app.draw().unwrap();
        let point = Some((28.0, 140.0));
        for index in [2, 4, 6] {
            assert!(app.over_catalog(point));
            app.scroll_catalog(-1.0, app.over_catalog(point));
            assert_eq!(app.selected, index);
            assert!(app.hits.is_empty());
        }
        assert!(!app.over_catalog(Some((500.0, 134.0))));
        app.open_settings();
        assert!(!app.over_catalog(point));
        assert!(app.game.is_none());
    }
    #[test]
    fn catalog_pixel_scroll_uses_viewport_scale_and_rejects_zero_extent() {
        use winit::dpi::PhysicalPosition;
        assert_eq!(
            catalog_scroll_lines(MouseScrollDelta::LineDelta(99.0, -2.0), (0, 0)),
            -2.0
        );
        for height in [360, 720, 1440] {
            let pixel = ROW_HEIGHT as f64 * f64::from(height) / HEIGHT as f64;
            assert_eq!(
                catalog_scroll_lines(
                    MouseScrollDelta::PixelDelta(PhysicalPosition::new(999.0, pixel)),
                    (height / 3 * 4, height)
                ),
                1.0
            );
        }
        assert!(
            catalog_scroll_lines(
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 1.0)),
                (0, 0)
            )
            .is_nan()
        );
    }
    #[test]
    fn filtered_selection_preserves_catalog_identity_and_empty_results_cannot_play_or_open_records()
    {
        let mut app = lifecycle_fixture();
        app.entries.push(Entry {
            path: "other.bms".into(),
            title: "OTHER SONG".into(),
            artist: "ARTIST".into(),
        });
        app.selection_items = app
            .entries
            .iter()
            .map(|entry| SelectionItem {
                title: entry.title.clone(),
                artist: entry.artist.clone(),
            })
            .collect::<Vec<_>>()
            .into();
        app.catalog_search = CatalogSearch::new(&app.selection_items).unwrap();
        app.key(KeyCode::F3, false);
        app.edit_search(None, Some("other artist"));
        assert_eq!(app.catalog_search.selected(), Some(1));
        assert_eq!(app.selected, 1);
        app.draw().unwrap();
        assert!(app.hits.iter().any(|(id, _)| *id == ControlId(101)));
        assert!(!app.hits.iter().any(|(id, _)| *id == ControlId(100)));
        app.activate(ControlId(100));
        assert_eq!(app.selected, 1);
        app.key(KeyCode::Enter, false);
        assert!(!app.search_focused);
        assert!(app.game.is_none());
        app.open_settings();
        assert!(!app.search_focused);
        app.edit_search(None, Some("hidden"));
        assert_eq!(app.search_editor.value(), "other artist");
        app.back();
        app.draw().unwrap();
        assert_eq!(app.catalog_search.selected(), Some(1));
        app.key(KeyCode::F3, false);
        app.edit_search(None, Some(" missing"));
        app.draw().unwrap();
        assert_eq!(app.catalog_search.selected(), None);
        assert!(!app.hits.iter().any(|(id, _)| *id == ControlId(1)));
        assert!(app.start().is_err());
        app.open_settings();
        app.open_records();
        assert_eq!(app.navigator.route(), ScreenRoute::Settings);
        assert!(app.records.is_none());
        app.back();
        app.key(KeyCode::F3, false);
        app.key(KeyCode::Escape, false);
        assert_eq!(app.search_editor.value(), "");
        assert_eq!(app.catalog_search.selected(), Some(0));
        assert!(!app.closing());
        fn reject_start(_: &[String]) -> Result<(), Box<dyn Error>> {
            Err("START BUTTON REACHED PREFLIGHT".into())
        }
        app.validate = reject_start;
        app.set_search_focus(true);
        app.activate(ControlId(1));
        assert!(!app.search_focused);
        assert!(
            app.failure
                .as_deref()
                .unwrap()
                .contains("START BUTTON REACHED PREFLIGHT")
        );
        assert!(app.game.is_none());
    }

    #[test]
    fn selection_reuses_geometry_and_scope_after_settings_back() {
        let mut app = lifecycle_fixture();
        assert!(app.reactive_waits_for_events());
        app.draw().unwrap();
        let selection_id = app.selection_view.as_ref().unwrap().id();
        app.draw().unwrap();
        assert_eq!(app.painted_reactive, Some(selection_id));
        assert!(!app.selection_view.as_ref().unwrap().dirty());
        app.open_settings();
        assert!(app.reactive_waits_for_events());
        assert_eq!(app.selection_view.as_ref().unwrap().id(), selection_id);
        app.draw().unwrap();
        app.back();
        assert!(app.reactive_waits_for_events());
        assert_eq!(app.navigator.active_id(), Some(selection_id));
        assert!(app.painted_reactive.is_none());
        app.draw().unwrap();
        assert_eq!(app.painted_reactive, Some(selection_id));
        assert!(app.hits.iter().any(|(id, _)| *id == ControlId(100)));
        app.draw().unwrap();
        assert!(!app.selection_view.as_ref().unwrap().dirty());
        app.request_close();
        assert!(app.selection_view.is_none());
        assert!(!app.reactive_waits_for_events());
    }

    #[test]
    fn settings_view_is_retained_through_child_and_cleared_hits_restore_without_state_change() {
        let mut app = lifecycle_fixture();
        app.open_settings();
        app.draw().unwrap();
        let id = app.settings_view.as_ref().unwrap().id();
        assert!(app.reactive_waits_for_events());
        assert_eq!(app.painted_reactive, Some(id));
        assert!(!app.settings_view.as_ref().unwrap().dirty());
        // Up at the first row leaves every signal equal but invalidates hit regions.
        app.settings_key(KeyCode::ArrowUp, false);
        assert!(app.hits.is_empty());
        assert!(app.painted_reactive.is_none());
        app.draw().unwrap();
        assert!(app.hits.iter().any(|(id, _)| *id == ControlId(10)));
        assert!(!app.settings_view.as_ref().unwrap().dirty());
        app.open_practice();
        assert_eq!(app.settings_view.as_ref().unwrap().id(), id);
        app.draw().unwrap();
        app.back();
        assert_eq!(app.navigator.active_id(), Some(id));
        assert_eq!(app.settings_view.as_ref().unwrap().id(), id);
        app.draw().unwrap();
        assert!(app.hits.iter().any(|(id, _)| *id == ControlId(74)));
        app.back();
        assert!(app.settings_view.is_none());
        assert!(app.settings.is_none());
        assert_eq!(app.navigator.route(), ScreenRoute::Selection);
    }

    #[test]
    fn retained_player_view_survives_device_child_and_close_disposes_both() {
        let mut app = lifecycle_fixture();
        app.open_settings();
        app.open_local();
        app.local_setup.as_mut().unwrap().model.resize(2).unwrap();
        app.local_setup.as_mut().unwrap().selected = 1;
        app.draw().unwrap();
        let parent = app.players_view.as_ref().unwrap().id();
        assert!(app.reactive_waits_for_events());
        let attach_child = |app: &mut Desktop| {
            let next = app
                .prepare_route(ScreenRoute::Devices { players: true })
                .unwrap();
            app.commit_route(next);
            let id = app.navigator.active_id().unwrap();
            let catalog = DeviceCatalog::new(
                DeviceRequest::LinuxKeyboard,
                vec![beatkernel_bms_runtime::device_catalog::DeviceChoice {
                    id: "/dev/input/fixture".into(),
                    label: "FIXTURE KEYBOARD".into(),
                    detail: "METADATA ONLY".into(),
                    selectable: true,
                }],
            )
            .unwrap();
            let player = app.local_setup.as_ref().unwrap().model.players()[1].id;
            app.picker = Some(PanelScope::new(
                id,
                DevicePicker {
                    catalog,
                    player: Some(player),
                    first: 0,
                    selected: None,
                },
            ));
        };
        attach_child(&mut app);
        app.draw().unwrap();
        assert!(app.reactive_waits_for_events());
        assert_eq!(app.players_view.as_ref().unwrap().id(), parent);
        assert!(app.hits.iter().any(|(id, _)| *id == ControlId(10000)));
        assert!(!app.hits.iter().any(|(id, _)| *id == ControlId(20)));
        app.back();
        assert_eq!(app.navigator.active_id(), Some(parent));
        assert!(app.devices_view.is_none());
        assert_eq!(app.local_setup.as_ref().unwrap().selected, 1);
        app.draw().unwrap();
        assert!(!app.players_view.as_ref().unwrap().dirty());
        assert!(app.hits.iter().any(|(id, _)| *id == ControlId(20001)));
        attach_child(&mut app);
        app.draw().unwrap();
        app.request_close();
        assert!(app.players_view.is_none());
        assert!(app.devices_view.is_none());
        assert!(app.picker.is_none());
        assert!(app.local_setup.is_none());
    }

    #[test]
    fn retained_records_back_restores_parent_and_closing_releases_child() {
        let mut app = lifecycle_fixture();
        app.open_settings();
        app.draw().unwrap();
        let parent = app.navigator.active_id().unwrap();
        app.open_records();
        app.draw().unwrap();
        let child = app.records_view.as_ref().unwrap().id();
        assert!(app.reactive_waits_for_events());
        assert_eq!(app.painted_reactive, Some(child));
        app.invalidate_hits();
        app.draw().unwrap();
        assert!(!app.records_view.as_ref().unwrap().dirty());
        assert!(app.hits.iter().any(|(id, _)| *id == ControlId(58)));
        assert!(app.hits.iter().any(|(id, _)| *id == ControlId(50)));
        app.back();
        assert_eq!(app.navigator.active_id(), Some(parent));
        assert!(app.records_view.is_none());
        assert!(app.records.is_none());
        assert_eq!(app.settings_view.as_ref().unwrap().id(), parent);
        app.open_records();
        app.draw().unwrap();
        assert_ne!(app.records_view.as_ref().unwrap().id(), child);
        app.request_close();
        assert!(app.records_view.is_none());
        assert!(app.records.is_none());
        assert!(!app.reactive_waits_for_events());
    }

    #[test]
    fn retained_display_done_failure_back_and_close_preserve_draft_ownership() {
        let mut app = lifecycle_fixture();
        app.open_settings();
        let parent = app.navigator.active_id();
        app.open_display();
        app.draw().unwrap();
        let id = app.display_view.as_ref().unwrap().id();
        assert!(app.reactive_waits_for_events());
        app.display.as_mut().unwrap().editors[2] = LineEditor::new("180", 32).unwrap();
        app.draw().unwrap();
        assert_eq!(app.display_view.as_ref().unwrap().id(), id);
        assert!(!app.display_view.as_ref().unwrap().dirty());
        app.finish_display();
        assert_eq!(app.navigator.active_id(), parent);
        assert!(app.display_view.is_none());
        assert_eq!(app.settings.as_ref().unwrap().presentation.fps, 180);
        assert_eq!(app.options.fps, 120);
        app.open_display();
        let child = app.navigator.active_id();
        app.display.as_mut().unwrap().editors[2] = LineEditor::new("9", 32).unwrap();
        app.finish_display();
        assert_eq!(app.navigator.active_id(), child);
        assert!(app.display.as_ref().unwrap().error.is_some());
        assert_eq!(app.settings.as_ref().unwrap().presentation.fps, 180);
        app.back();
        app.open_display();
        assert_eq!(app.display.as_ref().unwrap().editors[2].value(), "180");
        app.draw().unwrap();
        app.request_close();
        assert!(app.display_view.is_none());
    }

    #[test]
    fn practice_done_commits_precise_start_only_to_parent_draft() {
        let mut app = lifecycle_fixture();
        app.open_settings();
        let parent = app.navigator.active_id();
        let settings = app.settings.as_mut().unwrap();
        let index = settings
            .values
            .fields()
            .iter()
            .position(|field| field.flag == "--start-ns")
            .unwrap();
        settings.select(index).unwrap();
        let previous = settings.values.native_args();
        app.open_practice();
        assert_eq!(app.navigator.route(), ScreenRoute::Practice);
        let child = app.navigator.active_id();
        app.practice.as_mut().unwrap().editor = LineEditor::new("168:00:00.000000001", 64).unwrap();
        app.draw().unwrap();
        assert!(app.reactive_waits_for_events());
        assert!(!app.practice.as_ref().unwrap().view.dirty());
        assert!(app.hits.iter().any(|(id, _)| *id == ControlId(71)));
        app.practice_key(KeyCode::Home, false);
        app.draw().unwrap();
        assert_eq!(app.navigator.active_id(), child);
        app.finish_practice();
        assert_eq!(app.navigator.active_id(), parent);
        assert!(app.practice.is_none());
        let settings = app.settings.as_ref().unwrap();
        assert_eq!(settings.values.fields()[index].value, "604800000000001");
        assert_eq!(settings.editor.value(), "604800000000001");
        assert!(
            settings
                .message
                .as_deref()
                .unwrap()
                .contains("APPLY IS SEPARATE")
        );
        assert_eq!(
            settings
                .values
                .native_args()
                .chunks_exact(2)
                .filter(|pair| pair[0] != "--start-ns")
                .flatten()
                .cloned()
                .collect::<Vec<_>>(),
            previous
        );
        assert!(!app.options.native.iter().any(|flag| flag == "--start-ns"));
    }

    #[test]
    fn practice_section_done_updates_selected_end_atomically_and_back_discards() {
        let mut app = lifecycle_fixture();
        app.open_settings();
        let parent = app.navigator.active_id();
        let settings = app.settings.as_mut().unwrap();
        let end_index = settings
            .values
            .fields()
            .iter()
            .position(|field| field.flag == "--end-ns")
            .unwrap();
        settings.select(end_index).unwrap();
        let before = settings.values.native_args();
        app.open_practice();
        let child = app.navigator.active_id();
        let draft = app.practice.as_mut().unwrap();
        draft.editor = LineEditor::new("168:00:00.000000001", 64).unwrap();
        draft.end_editor = LineEditor::new("168:00:00.000000002", 64).unwrap();
        app.finish_practice();
        assert_eq!(app.navigator.active_id(), parent);
        assert!(app.practice.is_none());
        let settings = app.settings.as_ref().unwrap();
        assert_eq!(settings.editor.value(), "604800000000002");
        assert_eq!(
            PracticeStart::from_settings(&settings.values)
                .unwrap()
                .nanoseconds(),
            604800000000001
        );
        assert_eq!(
            PracticeStart::section_end(&settings.values)
                .unwrap()
                .unwrap()
                .nanoseconds(),
            604800000000002
        );
        assert!(!app.options.native.iter().any(|arg| arg == "--end-ns"));
        let saved = settings.values.native_args();
        app.open_practice();
        assert_ne!(app.navigator.active_id(), child);
        assert_eq!(
            app.practice.as_ref().unwrap().end_editor.value(),
            "168:00:00.000000002"
        );
        app.practice.as_mut().unwrap().end_editor =
            LineEditor::new("168:00:00.000000001", 64).unwrap();
        let instance = app.navigator.active_id();
        app.finish_practice();
        assert_eq!(app.navigator.active_id(), instance);
        assert!(app.practice.as_ref().unwrap().error.is_some());
        assert_eq!(app.settings.as_ref().unwrap().values.native_args(), saved);
        app.back();
        assert_eq!(app.navigator.active_id(), parent);
        assert_eq!(app.settings.as_ref().unwrap().values.native_args(), saved);
        assert_ne!(saved, before);
    }
    #[test]
    fn practice_field_focus_and_end_clear_change_only_targeted_draft() {
        let mut app = lifecycle_fixture();
        app.open_settings();
        app.open_practice();
        app.draw().unwrap();
        for id in [70, 75, 71, 72, 73, 76] {
            assert!(app.hits.iter().any(|(hit, _)| hit.0 == id));
        }
        app.practice.as_mut().unwrap().editor = LineEditor::new("20:00:00", 64).unwrap();
        app.practice_key(KeyCode::Tab, false);
        assert!(app.practice.as_ref().unwrap().end_focused);
        app.practice.as_mut().unwrap().edit(None, Some("20:00:01"));
        assert_eq!(app.practice.as_ref().unwrap().editor.value(), "20:00:00");
        assert_eq!(
            app.practice.as_ref().unwrap().end_editor.value(),
            "20:00:01"
        );
        app.practice_key(KeyCode::Tab, true);
        assert!(app.practice.as_ref().unwrap().end_focused);
        app.activate(ControlId(70));
        assert!(!app.practice.as_ref().unwrap().end_focused);
        app.activate(ControlId(75));
        assert!(app.practice.as_ref().unwrap().end_focused);
        app.activate(ControlId(76));
        assert_eq!(app.practice.as_ref().unwrap().end_editor.value(), "");
        assert_eq!(app.practice.as_ref().unwrap().editor.value(), "20:00:00");
        app.finish_practice();
        let values = &app.settings.as_ref().unwrap().values;
        assert_eq!(
            PracticeStart::from_settings(values).unwrap().nanoseconds(),
            72000000000000
        );
        assert_eq!(PracticeStart::section_end(values).unwrap(), None);
        app.open_practice();
        app.practice.as_mut().unwrap().end_editor = LineEditor::new("21:00:00", 64).unwrap();
        app.activate(ControlId(73));
        let draft = app.practice.as_ref().unwrap();
        assert_eq!(draft.editor.value(), "0:00");
        assert_eq!(draft.end_editor.value(), "");
        assert!(!draft.end_focused);
        app.finish_practice();
        let values = &app.settings.as_ref().unwrap().values;
        assert_eq!(
            PracticeStart::from_settings(values).unwrap().nanoseconds(),
            0
        );
        assert_eq!(PracticeStart::section_end(values).unwrap(), None);
    }

    #[test]
    fn practice_invalid_done_back_and_reset_preserve_draft_boundaries() {
        let mut app = lifecycle_fixture();
        app.open_settings();
        let parent = app.navigator.active_id();
        let previous = app.settings.as_ref().unwrap().values.native_args();
        app.open_practice();
        let child = app.navigator.active_id();
        app.practice.as_mut().unwrap().editor = LineEditor::new("0:00.0000000001", 64).unwrap();
        app.finish_practice();
        assert_eq!(app.navigator.active_id(), child);
        assert!(app.practice.as_ref().unwrap().error.is_some());
        assert_eq!(
            app.settings.as_ref().unwrap().values.native_args(),
            previous
        );
        app.back();
        assert_eq!(app.navigator.active_id(), parent);
        assert_eq!(
            app.settings.as_ref().unwrap().values.native_args(),
            previous
        );
        app.open_practice();
        app.practice.as_mut().unwrap().editor = LineEditor::new("20:00:00", 64).unwrap();
        app.activate(ControlId(73));
        assert_eq!(app.practice.as_ref().unwrap().editor.value(), "0:00");
        assert_eq!(
            app.settings.as_ref().unwrap().values.native_args(),
            previous
        );
        app.finish_practice();
        assert_eq!(app.navigator.active_id(), parent);
        assert_eq!(
            PracticeStart::from_settings(&app.settings.as_ref().unwrap().values)
                .unwrap()
                .nanoseconds(),
            0
        );
        app.open_practice();
        app.request_close();
        assert!(app.practice.is_none());
    }

    #[test]
    fn desktop_parent_draft_is_retained_but_only_active_panel_receives_input() {
        let mut app = lifecycle_fixture();
        app.open_settings();
        let settings_id = app.settings.as_ref().unwrap().id();
        app.settings
            .as_mut()
            .unwrap()
            .profile
            .insert("kept-profile")
            .unwrap();
        let settings_permit = app.settings.as_ref().unwrap().task_permit();
        assert!(settings_permit.is_active());
        app.open_display();
        let display_id = app.display.as_ref().unwrap().id();
        let display_permit = app.display.as_ref().unwrap().task_permit();
        assert_eq!(
            settings_permit.phase(),
            beatkernel_bms_runtime::panel_scope::PanelPhase::Retained
        );
        assert!(!settings_permit.is_cancelled());
        assert!(display_permit.is_active());
        app.key(KeyCode::Tab, false);
        assert_eq!(app.display.as_ref().unwrap().selected, 1);
        assert_eq!(app.settings.as_ref().unwrap().selected, 0);
        app.key(KeyCode::F4, false); // Hidden Settings cannot open its Records child.
        assert_eq!(app.navigator.route(), ScreenRoute::Display);
        assert!(app.records.is_none());
        assert!(!app.navigator.accepts(settings_id));
        assert!(app.navigator.accepts(display_id));
        app.key(KeyCode::Escape, false);
        assert!(app.display.is_none());
        assert!(display_permit.is_cancelled());
        assert!(!settings_permit.is_cancelled());
        assert!(settings_permit.is_active());
        assert_eq!(app.settings.as_ref().unwrap().id(), settings_id);
        assert_eq!(
            app.settings.as_ref().unwrap().profile.value(),
            "kept-profile"
        );
        app.open_local();
        let before = app.navigator.clone();
        let count = app.local_setup.as_ref().unwrap().model.players().len();
        assert!(app.prepare_route(ScreenRoute::Records).is_err());
        assert_eq!(app.navigator, before);
        assert_eq!(
            app.local_setup.as_ref().unwrap().model.players().len(),
            count
        );
        let child_permit = app.local_setup.as_ref().unwrap().task_permit();
        app.request_close();
        assert!(child_permit.is_cancelled() && settings_permit.is_cancelled());
        assert!(app.settings.is_none() && app.local_setup.is_none());
        assert_eq!(app.navigator.route(), ScreenRoute::Closing);
    }

    #[test]
    fn panel_suspend_defers_admission_without_disposing_retained_parent_or_child() {
        use beatkernel_bms_runtime::panel_scope::PanelPhase;
        let mut app = lifecycle_fixture();
        app.open_settings();
        let parent = app.settings.as_ref().unwrap().task_permit();
        app.open_local();
        let child_id = app.local_setup.as_ref().unwrap().id();
        let child = app.local_setup.as_ref().unwrap().task_permit();
        assert_eq!(parent.phase(), PanelPhase::Retained);
        assert!(child.is_active());
        assert_eq!(app.metadata_scope().unwrap().0, child_id);

        app.navigator.suspend();
        app.synchronize_panel_lifecycles();
        assert_eq!(parent.phase(), PanelPhase::Suspended);
        assert_eq!(child.phase(), PanelPhase::Suspended);
        assert!(!parent.is_cancelled() && !child.is_cancelled());
        assert!(app.metadata_scope().is_err());
        app.back();
        assert_eq!(app.navigator.active_id(), Some(child_id));

        app.navigator.resume();
        app.synchronize_panel_lifecycles();
        assert_eq!(parent.phase(), PanelPhase::Retained);
        assert!(child.is_active());
        assert_eq!(app.metadata_scope().unwrap().0, child_id);
        app.back();
        assert!(child.is_cancelled());
        assert!(parent.is_active());
        app.request_close();
        assert!(parent.is_cancelled());
        app.synchronize_panel_lifecycles();
        assert!(parent.is_cancelled() && child.is_cancelled());
    }

    #[test]
    fn cancelled_metadata_never_starts_or_publishes_and_session_drain_is_separate() {
        use std::cell::Cell;
        let scope = PanelScope::new(ScreenInstanceId(99), ());
        let permit = scope.task_permit();
        drop(scope);
        let called = Cell::new(false);
        assert!(
            scoped_metadata(permit, || {
                called.set(true);
                Ok(ProfileResult::Saved)
            })
            .is_err()
        );
        assert!(!called.get());
        let scope = PanelScope::new(ScreenInstanceId(100), ());
        let permit = scope.task_permit();
        assert!(
            scoped_metadata(permit, || {
                called.set(true);
                drop(scope); // Cancellation after work starts suppresses its result.
                Ok(ProfileResult::Saved)
            })
            .is_err()
        );
        assert!(called.get());

        let mut app = lifecycle_fixture();
        let next = app
            .prepare_route(ScreenRoute::Play { replay: false })
            .unwrap();
        app.commit_route(next);
        app.game = Some(retry_fixture()); // No thread/device in this fixture.
        assert!(
            app.prepare_route(ScreenRoute::Results { replay: false })
                .is_err()
        );
        assert!(app.prepare_route(ScreenRoute::Selection).is_err());
        app.navigator.suspend();
        app.game.as_mut().unwrap().owner_finished(true);
        app.collect_game();
        assert_eq!(app.navigator.route(), ScreenRoute::Play { replay: false });
        app.navigator.resume();
        app.collect_game();
        assert_eq!(
            app.navigator.route(),
            ScreenRoute::Results { replay: false }
        );
        assert!(app.game.is_some());
        app.key(KeyCode::Escape, false);
        assert_eq!(app.navigator.route(), ScreenRoute::Selection);
        assert!(app.game.is_none());
    }

    #[test]
    fn pause_control_requires_stable_native_ack_for_live_or_replay_and_fences_cleanup() {
        let mut game = retry_fixture();
        for (phase, expected) in [
            (player::PauseState::Unavailable, None),
            (player::PauseState::Running, Some(true)),
            (player::PauseState::Pausing, None),
            (player::PauseState::Paused, Some(false)),
            (player::PauseState::Resuming, None),
        ] {
            game.accept_snapshot(player::PlayerSnapshot {
                status: player::PlayerStatus::Playing,
                pause: phase,
                ..Default::default()
            });
            assert_eq!(game.pause_target(), expected);
        }
        game.snapshot.as_mut().unwrap().pause = player::PauseState::Running;
        game.replay = true;
        assert_eq!(game.pause_target(), Some(true));
        game.snapshot.as_mut().unwrap().pause = player::PauseState::Paused;
        assert_eq!(game.pause_target(), Some(false));
        game.snapshot.as_mut().unwrap().pause = player::PauseState::Unavailable;
        assert_eq!(game.pause_target(), None);
        game.snapshot.as_mut().unwrap().pause = player::PauseState::Running;
        game.replay = false;
        game.prepared_retry = Some(game.launch.retry().unwrap());
        assert_eq!(game.pause_target(), None);
        game.prepared_retry = None;
        game.snapshot.as_mut().unwrap().cancelled = true;
        assert_eq!(game.pause_target(), None);
        game.snapshot.as_mut().unwrap().cancelled = false;
        game.cancelling = true;
        assert_eq!(game.pause_target(), None);
        game.cancelling = false;
        game.joined = true;
        assert_eq!(game.pause_target(), None);
    }

    #[test]
    fn watching_pins_record_and_retry_never_rewrites_capture_or_live_draft() {
        let values = NativeSettings::from_args(
            &["--record-replay".into(), "new.bkr".into()],
            settings_host(),
        )
        .unwrap();
        let original = values.native_args();
        let launch = record_launch(
            &values,
            std::path::Path::new("song.bms"),
            std::path::Path::new("old.bkr"),
        )
        .unwrap();
        assert!(!launch.args().iter().any(|s| s == "--record-replay"));
        let retry = launch.retry().unwrap();
        assert_eq!(retry.args(), launch.args());
        assert_eq!(retry.attempt(), 1);
        assert_eq!(values.native_args(), original);
        let mut game = retry_fixture();
        game.replay = true;
        game.launch = launch;
        game.prepared_retry = Some(retry);
        assert!(
            game.owner_finished(true)
                .unwrap()
                .args()
                .chunks_exact(2)
                .any(|p| p == ["--replay", "old.bkr"])
        );
        assert!(game.replay);
    }
    fn retry_fixture() -> Game {
        let (_publisher, viewer) = player::channel();
        Game {
            viewer,
            worker: None,
            snapshot: None,
            completed_results: None,
            completed_results_error: None,
            cancelling: false,
            joined: false,
            local_page: 3,
            local_comparisons: true,
            replay: false,
            launch: SessionLaunch::new(vec![
                "--chart".into(),
                "pinned.bms".into(),
                "--record-replay".into(),
                "run.bkr".into(),
            ])
            .unwrap(),
            prepared_retry: None,
            practice_bookmark: None,
            practice_loop: None,
            loop_enabled: false,
        }
    }
    #[test]
    fn practice_mark_uses_exact_accepted_native_time_and_keeps_previous_mark_on_rejection() {
        let mut game = retry_fixture();
        assert!(game.mark_practice().is_err());
        game.accept_snapshot(player::PlayerSnapshot {
            song_time: Some(beatkernel::time::Timestamp::from_nanos(604_800_000_000_001)),
            status: player::PlayerStatus::Playing,
            ..Default::default()
        });
        game.mark_practice().unwrap();
        let bookmark = game.practice_bookmark.unwrap();
        assert_eq!(bookmark.nanoseconds(), 604_800_000_000_001);
        assert!(game.practice_restart_available());
        game.snapshot.as_mut().unwrap().song_time =
            Some(beatkernel::time::Timestamp::from_nanos(-1));
        assert!(game.mark_practice().is_err());
        assert_eq!(game.practice_bookmark, Some(bookmark));
        game.snapshot.as_mut().unwrap().song_time =
            Some(beatkernel::time::Timestamp::from_nanos(i64::MAX));
        game.snapshot.as_mut().unwrap().cancelled = true;
        assert!(game.practice_position().is_none());
        game.snapshot.as_mut().unwrap().cancelled = false;
        game.replay = true;
        assert!(game.mark_practice().is_err());
        assert!(!game.practice_restart_available());
        game.replay = false;
        game.mark_practice().unwrap();
        assert_eq!(game.practice_bookmark.unwrap().nanoseconds(), i64::MAX);
        game.prepared_retry = Some(game.launch.retry_from(bookmark).unwrap());
        game.cancelling = true;
        assert!(game.practice_position().is_none());
        assert!(!game.practice_restart_available());
        assert!(game.owner_finished(false).is_none());
        assert!(game.prepared_retry.is_none());
        assert!(game.practice_restart_available());
        assert!(game.practice_position().is_none());
    }
    fn loop_fixture() -> Game {
        let mut game = retry_fixture();
        game.accept_snapshot(player::PlayerSnapshot {
            song_time: Some(beatkernel::time::Timestamp::from_nanos(10)),
            status: player::PlayerStatus::Playing,
            pause: player::PauseState::Running,
            ..Default::default()
        });
        game.mark_practice().unwrap();
        game.snapshot.as_mut().unwrap().song_time =
            Some(beatkernel::time::Timestamp::from_nanos(20));
        game.mark_loop_end().unwrap();
        game.toggle_loop().unwrap();
        game
    }
    #[test]
    fn loop_repeat_requires_exact_native_end_successful_cleanup_and_join() {
        let mut game = loop_fixture();
        assert!(!game.loop_due()); // Observed position already equals the region end.
        game.snapshot.as_mut().unwrap().song_time = Some(beatkernel::time::Timestamp::MAX);
        assert!(!game.loop_due()); // Coalesced UI time never authorizes stopping audio.
        game.snapshot.as_mut().unwrap().completed_end =
            Some(beatkernel::time::Timestamp::from_nanos(20));
        assert!(!game.loop_due()); // Owner is still Playing, not cleaned up.
        game.snapshot.as_mut().unwrap().status = player::PlayerStatus::Finished;
        assert!(!game.loop_due()); // Terminal publication alone is not a join.
        assert!(game.owner_finished(true).is_none());
        assert!(game.loop_due());
        game.viewer.request_pause(true);
        assert!(!game.loop_due());
        game.viewer.request_pause(false);
        for end in [
            None,
            Some(beatkernel::time::Timestamp::from_nanos(19)),
            Some(beatkernel::time::Timestamp::from_nanos(21)),
        ] {
            game.snapshot.as_mut().unwrap().completed_end = end;
            assert!(!game.loop_due());
        }
        game.snapshot.as_mut().unwrap().completed_end =
            Some(beatkernel::time::Timestamp::from_nanos(20));
        game.snapshot.as_mut().unwrap().cancelled = true;
        assert!(!game.loop_due());
        game.snapshot.as_mut().unwrap().cancelled = false;
        game.snapshot.as_mut().unwrap().status =
            player::PlayerStatus::Failed("cleanup fixture".into());
        assert!(!game.loop_due());
        game.snapshot.as_mut().unwrap().status = player::PlayerStatus::Finished;
        game.replay = true;
        assert!(!game.loop_due());
        game.replay = false;
        game.prepared_retry = Some(game.launch.retry().unwrap());
        assert!(!game.loop_due());
        game.prepared_retry = None;
        game.cancel();
        assert!(!game.loop_due());
        assert!(!game.loop_enabled);
        let mut marking = loop_fixture();
        let region = marking.practice_loop;
        marking.snapshot.as_mut().unwrap().song_time =
            Some(beatkernel::time::Timestamp::from_nanos(10));
        assert!(marking.mark_loop_end().is_err());
        assert_eq!(marking.practice_loop, region);
        marking.mark_practice().unwrap();
        assert!(marking.practice_loop.is_none());
        assert!(!marking.loop_enabled);
    }
    #[test]
    fn native_loop_enable_prepares_both_endpoints_and_waits_for_owner_cleanup() {
        let mut app = lifecycle_fixture();
        let next = app
            .prepare_route(ScreenRoute::Play { replay: false })
            .unwrap();
        app.commit_route(next);
        app.game = Some(loop_fixture());
        app.occluded = true;
        app.request_restart(true);
        assert!(app.game.as_ref().unwrap().prepared_retry.is_none());
        app.occluded = false;
        app.request_restart(true); // F11 enabling takes this same preflight path.
        let game = app.game.as_ref().unwrap();
        assert!(game.cancelling && !game.joined);
        let prepared = game.prepared_retry.as_ref().unwrap();
        assert_eq!(prepared.attempt(), 1);
        for pair in [
            ["--start-ns", "10"],
            ["--end-ns", "20"],
            ["--record-replay", "run.retry1.bkr"],
        ] {
            assert!(prepared.args().chunks_exact(2).any(|p| p == pair));
        }
        app.request_restart(true);
        assert_eq!(
            app.game
                .as_ref()
                .unwrap()
                .prepared_retry
                .as_ref()
                .unwrap()
                .attempt(),
            1
        );
        let old = app.game.as_mut().unwrap();
        let launch = old.owner_finished(true).unwrap();
        assert!(old.joined);
        let mut next = loop_fixture();
        next.launch = launch;
        assert!(!next.loop_due());
        next.snapshot.as_mut().unwrap().status = player::PlayerStatus::Finished;
        next.snapshot.as_mut().unwrap().completed_end =
            Some(beatkernel::time::Timestamp::from_nanos(20));
        next.owner_finished(true);
        assert!(next.loop_due());
        let launch = next.launch.retry_loop(next.practice_loop.unwrap()).unwrap();
        assert_eq!(launch.attempt(), 2);
        assert!(
            launch
                .args()
                .chunks_exact(2)
                .any(|p| p == ["--record-replay", "run.retry2.bkr"])
        );
        let pinned = launch.retry().unwrap();
        assert!(
            !pinned
                .args()
                .iter()
                .any(|arg| arg == "--start-ns" || arg == "--end-ns")
        );
    }
    #[test]
    fn rejected_loop_toggle_during_pause_transition_never_cancels_or_prepares_retry() {
        let mut app = lifecycle_fixture();
        let next = app
            .prepare_route(ScreenRoute::Play { replay: false })
            .unwrap();
        app.commit_route(next);
        app.game = Some(loop_fixture());
        for pause in [player::PauseState::Pausing, player::PauseState::Resuming] {
            app.game.as_mut().unwrap().snapshot.as_mut().unwrap().pause = pause;
            app.edit_loop(true);
            let game = app.game.as_ref().unwrap();
            assert!(game.loop_enabled);
            assert!(!game.cancelling);
            assert!(game.prepared_retry.is_none());
            assert!(app.failure.is_some());
        }
    }
    #[test]
    fn loop_preflight_failure_disables_without_cancel_and_f5_or_explicit_cancel_disarms() {
        fn reject(_: &[String]) -> Result<(), Box<dyn Error>> {
            Err("loop preflight fixture".into())
        }
        let mut app = lifecycle_fixture();
        let next = app
            .prepare_route(ScreenRoute::Play { replay: false })
            .unwrap();
        app.commit_route(next);
        app.game = Some(loop_fixture());
        app.validate = reject;
        app.request_restart(true);
        let game = app.game.as_ref().unwrap();
        assert!(!game.loop_enabled);
        assert!(!game.cancelling);
        assert!(game.prepared_retry.is_none());
        assert!(app.failure.as_deref().unwrap().contains("retry preflight"));
        app.game = Some(loop_fixture());
        app.validate = lifecycle_fixture().validate;
        app.request_retry();
        let game = app.game.as_ref().unwrap();
        assert!(!game.loop_enabled);
        assert!(
            !game
                .prepared_retry
                .as_ref()
                .unwrap()
                .args()
                .iter()
                .any(|a| a == "--start-ns")
        );
        app.cancel();
        assert!(app.game.as_ref().unwrap().prepared_retry.is_none());
        let mut game = loop_fixture();
        game.prepared_retry = Some(
            game.launch
                .retry_from(game.practice_bookmark.unwrap())
                .unwrap(),
        );
        assert!(game.owner_finished(false).is_none());
        assert!(!game.loop_enabled);
    }
    #[test]
    fn watch_pause_pointer_dispatch_changes_actual_bridge_desire_and_controls_do_not_overlap() {
        let mut app = lifecycle_fixture();
        let next = app
            .prepare_route(ScreenRoute::Play { replay: true })
            .unwrap();
        app.commit_route(next);
        let (publisher, viewer) = player::channel();
        let mut game = retry_fixture();
        game.viewer = viewer;
        game.replay = true;
        game.accept_snapshot(player::PlayerSnapshot {
            status: player::PlayerStatus::Playing,
            pause: player::PauseState::Running,
            ..Default::default()
        });
        app.game = Some(game);
        player::with_publisher(publisher, || {
            app.activate(ControlId(62));
            assert!(player::pause_requested());
            app.game.as_mut().unwrap().snapshot.as_mut().unwrap().pause =
                player::PauseState::Paused;
            app.activate(ControlId(62));
            assert!(!player::pause_requested());
            app.game.as_mut().unwrap().snapshot.as_mut().unwrap().pause =
                player::PauseState::Pausing;
            app.activate(ControlId(62));
            assert!(!player::pause_requested());
            app.activate(ControlId(63));
            app.activate(ControlId(64));
            assert!(app.game.as_ref().unwrap().practice_loop.is_none());
            Ok(())
        })
        .unwrap();
        let cancel = Bounds {
            x: 750,
            y: 65,
            width: 180,
            height: 34,
        };
        let rectangles = [cancel, PAUSE_BOUNDS, LOOP_END_BOUNDS, LOOP_TOGGLE_BOUNDS];
        for (index, a) in rectangles.iter().enumerate() {
            assert!(
                a.x >= 0
                    && a.y >= 0
                    && a.x + a.width <= WIDTH as i64
                    && a.y + a.height <= HEIGHT as i64
            );
            for b in &rectangles[index + 1..] {
                assert!(
                    a.x + a.width <= b.x
                        || b.x + b.width <= a.x
                        || a.y + a.height <= b.y
                        || b.y + b.height <= a.y
                );
            }
        }
        for bounds in [PAUSE_BOUNDS, LOOP_END_BOUNDS, LOOP_TOGGLE_BOUNDS] {
            assert!(bounds.y >= 640); // Local panel content ends at 640.
        }
    }
    #[test]
    fn bookmark_native_preflight_failure_does_not_cancel_or_mutate_live_session() {
        fn reject(_: &[String]) -> Result<(), Box<dyn Error>> {
            Err("fixture rejects before native ownership".into())
        }
        let mut app = lifecycle_fixture();
        let next = app
            .prepare_route(ScreenRoute::Play { replay: false })
            .unwrap();
        app.commit_route(next);
        let mut game = retry_fixture();
        game.accept_snapshot(player::PlayerSnapshot {
            song_time: Some(beatkernel::time::Timestamp::from_nanos(72_000_000_000_001)),
            status: player::PlayerStatus::Playing,
            ..Default::default()
        });
        game.mark_practice().unwrap();
        let bookmark = game.practice_bookmark;
        let original = game.launch.args().to_vec();
        app.game = Some(game);
        app.validate = reject;
        app.request_restart(true);
        let game = app.game.as_ref().unwrap();
        assert!(!game.cancelling);
        assert!(game.prepared_retry.is_none());
        assert_eq!(game.practice_bookmark, bookmark);
        assert_eq!(game.launch.args(), original);
        assert_eq!(game.launch.attempt(), 0);
        assert!(app.failure.as_deref().unwrap().contains("retry preflight"));
        app.request_close();
        assert!(app.game.as_ref().unwrap().prepared_retry.is_none());
    }

    #[test]
    fn retry_admission_waits_for_owner_success_and_cancel_discards_prepared_launch() {
        let mut game = retry_fixture();
        assert!(game.retry_available());
        game.prepared_retry = Some(game.launch.retry().unwrap());
        game.cancelling = true;
        assert!(!game.retry_available());
        let launch = game.owner_finished(true).unwrap();
        assert!(game.joined);
        assert_eq!(launch.attempt(), 1);
        assert_eq!(launch.args()[1], "pinned.bms");
        assert_eq!(launch.args()[3], "run.retry1.bkr");
        assert_eq!(game.local_page, 3);
        assert!(game.local_comparisons);
        let mut failed = retry_fixture();
        failed.prepared_retry = Some(failed.launch.retry().unwrap());
        assert!(failed.owner_finished(false).is_none());
        assert!(failed.prepared_retry.is_none());
        assert!(failed.retry_available()); // Explicit joined-results retry remains possible.
        let mut cancelled = retry_fixture();
        cancelled.prepared_retry = Some(cancelled.launch.retry().unwrap());
        cancelled.cancel();
        assert!(cancelled.prepared_retry.is_none());
        assert!(!cancelled.retry_available());
        assert!(cancelled.owner_finished(true).is_none());
    }
    #[test]
    fn retry_snapshot_paging_survives_loading_and_clamps_final_results() {
        let mut game = retry_fixture();
        game.accept_snapshot(player::PlayerSnapshot::default());
        assert_eq!(game.local_page, 3);
        assert_eq!(game_error_caption(Some(&game)), "ERROR - SESSION CONTINUES");
        let snapshot = player::PlayerSnapshot {
            players: (1..=5)
                .map(|id| player::LocalPlayerSnapshot {
                    bms_score: None,
                    player: PlayerId(id),
                    chart: None,
                    mine_damage: Default::default(),
                    gauge: Default::default(),
                    song_time: None,
                    score: Default::default(),
                    last_judge: None,
                    recent_results: Vec::new(),
                    competition: None,
                    pressed_lanes: 0,
                    note_progress: None,
                })
                .collect(),
            ..Default::default()
        };
        game.accept_snapshot(snapshot);
        assert_eq!(game.local_page, 1);
        game.cancelling = true;
        assert_eq!(
            game_error_caption(Some(&game)),
            "ERROR - WAITING FOR CLEANUP"
        );
        game.owner_finished(false);
        assert_eq!(
            game_error_caption(Some(&game)),
            "ERROR - ENTER RETURNS TO SELECTION"
        );
    }
    fn record_preview_fixture(path: PathBuf) -> RecordPreview {
        RecordPreview {
            path,
            records: 7,
            recorded_until: Some(beatkernel::time::Timestamp::from_nanos(1_000_000_000)),
            start: beatkernel::time::Timestamp::ZERO,
            end: None,
            historical: None,
            historical_score: None,
            bms_score: None,
            historical_bms_score: None,
            historical_comparison: None,
            archive_error: None,
            score: Default::default(),
        }
    }
    #[test]
    fn record_selection_paging_and_directory_edits_invalidate_attachment() {
        let mut records = RecordsDraft::new(PathBuf::from("song.bms")).unwrap();
        assert_eq!(records.directory.value(), ".");
        records.catalog = Some(RecordCatalog {
            entries: (0..26)
                .map(|index| PathBuf::from(format!("r{index}.bkr")))
                .collect(),
            truncated: true,
        });
        records.select(0);
        records.preview = Some(record_preview_fixture(PathBuf::from("r0.bkr")));
        assert!(records.valid_preview().is_some());
        records.page(true);
        assert_eq!(records.selected, Some(10));
        assert_eq!(records.first, 10);
        assert!(records.preview.is_none());
        records.page(true);
        assert_eq!(records.first, 20);
        records.page(true);
        assert_eq!(records.first, 20);
        records.page(false);
        assert_eq!(records.first, 10);
        records.preview = Some(record_preview_fixture(PathBuf::from("r0.bkr")));
        assert!(records.valid_preview().is_none());
        records.directory_focused = true;
        records.edit(None, Some("\n"));
        assert!(records.error.is_some());
        assert!(records.catalog.is_some());
        records.edit(Some(KeyCode::Home), None);
        records.edit(None, Some("archive/"));
        assert!(records.catalog.is_none());
        assert!(records.preview.is_none());
        assert_eq!(records.selected, None);
        assert_eq!(records.first, 0);
    }
    #[test]
    fn record_controls_require_current_preview_and_pending_fences_every_action() {
        let mut records = RecordsDraft::new(PathBuf::from("song.bms")).unwrap();
        records.catalog = Some(RecordCatalog {
            entries: vec![PathBuf::from("r.bkr")],
            truncated: false,
        });
        records.select(0);
        let mut scene = Scene::new(960, 720);
        let mut hits = Vec::new();
        draw_records(
            &mut scene,
            &records,
            &mut hits,
            &Gesture::default(),
            None,
            false,
            0,
        );
        assert!(!hits.iter().any(|(id, _)| matches!(id.0, 52 | 53 | 59)));
        records.preview = Some(record_preview_fixture(PathBuf::from("r.bkr")));
        scene.clear();
        hits.clear();
        draw_records(
            &mut scene,
            &records,
            &mut hits,
            &Gesture::default(),
            None,
            false,
            7,
        );
        assert!(hits.iter().any(|(id, _)| *id == ControlId(52)));
        assert!(hits.iter().any(|(id, _)| *id == ControlId(53)));
        assert!(hits.iter().any(|(id, _)| *id == ControlId(59)));
        assert!(
            hits.iter()
                .all(|(id, _)| matches!(id.0,50..=59|50000..=50255))
        );
        scene.clear();
        hits.clear();
        draw_records(
            &mut scene,
            &records,
            &mut hits,
            &Gesture::default(),
            None,
            true,
            8,
        );
        assert!(hits.is_empty());
    }
    #[test]
    fn selective_record_removal_refreshes_parent_editor_and_preserves_duplicates_and_accepted_state()
     {
        let mut app = lifecycle_fixture();
        app.open_settings();
        app.open_records();
        let path = "records/own.bkr";
        let draft = app.settings.as_mut().unwrap();
        draft.values.add_opponent(OpponentKind::Own, path).unwrap();
        draft.values.add_opponent(OpponentKind::Own, path).unwrap();
        draft
            .values
            .add_opponent(OpponentKind::Other, path)
            .unwrap();
        let row = draft
            .values
            .fields()
            .iter()
            .position(|row| row.flag == "--ghost-self" && row.value == path)
            .unwrap();
        draft.select(row).unwrap();
        let accepted = app.options.native.clone();
        let records = app.records.as_mut().unwrap();
        records.catalog = Some(RecordCatalog {
            entries: vec![path.into()],
            truncated: false,
        });
        records.selected = Some(0);
        assert!(records.preview.is_none());
        app.remove_record(OpponentKind::Own);
        let draft = app.settings.as_ref().unwrap();
        assert_eq!(draft.values.opponent_count(OpponentKind::Own, path), 1);
        assert_eq!(draft.values.opponent_count(OpponentKind::Other, path), 1);
        assert_eq!(draft.selected, row);
        assert_eq!(draft.editor.value(), "");
        assert_eq!(app.options.native, accepted);
        let records = app.records.as_ref().unwrap();
        let frame = records_frame(
            records,
            false,
            saved_opponents(&draft.values),
            Some(&draft.values),
        );
        assert_eq!(frame.selected_opponents, [1, 1]);
        app.remove_record(OpponentKind::Own);
        app.remove_record(OpponentKind::Own); // Missing targets report an error and preserve others.
        assert!(app.records.as_ref().unwrap().error.is_some());
        assert_eq!(
            app.settings
                .as_ref()
                .unwrap()
                .values
                .opponent_count(OpponentKind::Other, path),
            1
        );
        app.back();
        app.remove_record(OpponentKind::Other); // Hidden child cannot mutate parent draft.
        assert_eq!(
            app.settings
                .as_ref()
                .unwrap()
                .values
                .opponent_count(OpponentKind::Other, path),
            1
        );
    }
    #[test]
    fn cleared_ghost_field_refreshes_the_settings_editor_without_resurrecting_path() {
        let mut values = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
        values.add_opponent(OpponentKind::Own, "r.bkr").unwrap();
        let selected = values
            .fields()
            .iter()
            .position(|field| field.flag == "--ghost-self")
            .unwrap();
        let mut draft = SettingsDraft {
            values,
            presentation: Default::default(),
            cached_local: None,
            selected,
            editor: LineEditor::new("r.bkr", 4096).unwrap(),
            profile: LineEditor::new("", 4096).unwrap(),
            profile_focused: false,
            message: None,
            error: None,
        };
        assert_eq!(saved_opponents(&draft.values), 1);
        draft.values.clear_opponents();
        draft.refresh_selected().unwrap();
        assert_eq!(draft.editor.value(), "");
        assert_eq!(saved_opponents(&draft.values), 0);
    }
    #[test]
    fn display_modal_isolated_edits_back_and_pending_controls() {
        let original = PresentationSettings::default();
        let mut modal = DisplayDraft::new(original).unwrap();
        modal.selected = 2;
        modal.edit(Some(KeyCode::Home), None);
        modal.edit(None, Some("9"));
        assert!(modal.value().is_err());
        assert_eq!(original.fps, 120);
        // Back drops the isolated modal; reopening starts from the accepted draft.
        drop(modal);
        let modal = DisplayDraft::new(original).unwrap();
        assert_eq!(modal.value().unwrap(), original);
        let mut scene = Scene::new(960, 720);
        let mut hits = Vec::new();
        draw_display(
            &mut scene,
            &modal,
            &mut hits,
            &Gesture::default(),
            None,
            false,
        );
        assert_eq!(hits.len(), 6);
        assert!(hits.iter().any(|(id, _)| *id == ControlId(40)));
        assert!(
            hits.iter()
                .all(|(id, _)| matches!(id.0, 40 | 41 | 40000..=40003))
        );
        hits.clear();
        scene.clear();
        draw_display(
            &mut scene,
            &modal,
            &mut hits,
            &Gesture::default(),
            None,
            true,
        );
        assert!(hits.is_empty());
    }
    #[test]
    fn explicit_display_cli_overrides_preserve_profile_values_and_native_flag_values() {
        let options = Options::parse(&[
            "--chart".into(),
            "--ui-fps".into(),
            "--profile".into(),
            "profile.bkp".into(),
            "--present".into(),
            "mailbox".into(),
        ])
        .unwrap();
        assert_eq!(options.display_overrides, ["--present", "mailbox"]);
        assert_eq!(options.chart, Some(PathBuf::from("--ui-fps")));
        let stored = PresentationSettings {
            backend: BackendChoice::Gl,
            presentation: Presentation::Fifo,
            fps: 60,
            lookahead_ms: 3500,
        };
        let merged = stored.apply_overrides(&options.display_overrides).unwrap();
        assert_eq!(merged.backend, BackendChoice::Gl);
        assert_eq!(merged.fps, 60);
        assert_eq!(merged.lookahead_ms, 3500);
        assert_eq!(merged.presentation, Presentation::Mailbox);
        assert!(
            Options::parse(&[
                "--chart".into(),
                "song.bms".into(),
                "--ui-fps".into(),
                "120".into(),
                "--ui-fps".into(),
                "60".into()
            ])
            .is_err()
        );
    }
    #[test]
    fn comparison_toggle_is_available_only_for_groups_with_retained_comparisons() {
        let mut players: Vec<_> = (1..=2)
            .map(|id| player::LocalPlayerSnapshot {
                bms_score: None,
                player: beatkernel_bms_runtime::local_players::PlayerId(id),
                mine_damage: Default::default(),
                gauge: Default::default(),
                chart: None,
                song_time: None,
                score: Default::default(),
                last_judge: None,
                recent_results: Vec::new(),
                competition: None,
                pressed_lanes: 0,
                note_progress: None,
            })
            .collect();
        assert!(!local_comparisons_available(&players));
        players[0].competition = Some(player::CompetitionSnapshot {
            ghosts: Vec::new(),
            network: None,
        });
        assert!(!local_comparisons_available(&players));
        players[0].competition.as_mut().unwrap().network = Some(player::NetworkSnapshot {
            status: player::NetworkStatus::Disconnected,
            progress: None,
        });
        assert!(local_comparisons_available(&players));
        assert!(!local_comparisons_available(&players[..1]));
        // Retained ghost prefixes alone are sufficient, without a remote connection.
        let comparison = players[0].competition.as_mut().unwrap();
        comparison.network = None;
        comparison.ghosts.push(player::GhostSnapshot {
            kind: beatkernel_bms_runtime::competition::OpponentKind::Own,
            label: "past record".into(),
            hits: 1,
            misses: 0,
            combo: 1,
            max_combo: 1,
            recorded_until: Some(beatkernel::time::Timestamp::ZERO),
        });
        assert!(local_comparisons_available(&players));
    }
    #[test]
    fn local_pages_keep_three_four_together_and_bound_larger_rosters() {
        for count in [0, 1, 2, 3, 4] {
            assert_eq!(local_page(count, 0, true), 0);
            assert_eq!(local_page(count, usize::MAX, false), 0);
        }
        assert_eq!(local_page(5, 0, true), 1);
        assert_eq!(local_page(5, 1, true), 1);
        assert_eq!(local_page(64, 14, true), 15);
        assert_eq!(local_page(64, 15, true), 15);
        assert_eq!(local_page(64, 15, false), 14);
    }
    #[test]
    fn local_setup_solo_hides_assignment_and_pending_group_fences_controls() {
        let values = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
        let draft = SettingsDraft {
            values: values.clone(),
            presentation: PresentationSettings::default(),
            cached_local: None,
            selected: 0,
            editor: LineEditor::new("", 4096).unwrap(),
            profile: LineEditor::new("", 4096).unwrap(),
            profile_focused: false,
            message: None,
            error: None,
        };
        let mut local = LocalDraft {
            model: LocalSetup::from_settings(&values, SettingsHost::Linux).unwrap(),
            selected: 0,
            first: 0,
        };
        let mut scene = Scene::new(960, 720);
        let mut hits = Vec::new();
        draw_local(
            &mut scene,
            &local,
            &draft,
            &mut hits,
            &Gesture::default(),
            None,
            false,
        );
        assert!(!hits.iter().any(|(id, _)| matches!(id.0, 34 | 35)));
        assert!(scene.status().is_ok());
        local.model.resize(4).unwrap();
        hits.clear();
        scene.clear();
        draw_local(
            &mut scene,
            &local,
            &draft,
            &mut hits,
            &Gesture::default(),
            None,
            false,
        );
        assert_eq!(hits.iter().filter(|(id, _)| id.0 >= 20000).count(), 4);
        assert!(hits.iter().any(|(id, _)| id.0 == 34));
        hits.clear();
        scene.clear();
        draw_local(
            &mut scene,
            &local,
            &draft,
            &mut hits,
            &Gesture::default(),
            None,
            true,
        );
        assert!(hits.is_empty());
        assert!(scene.status().is_ok());
    }
    #[test]
    fn profile_option_is_ui_owned_and_native_overrides_replace_repeated_groups() {
        let args = [
            "--library",
            "charts",
            "--profile",
            "native profile.txt",
            "--bind",
            "11:04",
        ]
        .map(String::from);
        let options = Options::parse(&args).unwrap();
        assert_eq!(options.profile, Some(PathBuf::from("native profile.txt")));
        assert_eq!(options.native, ["--bind", "11:04"]);
        for args in [
            vec!["--library", "charts", "--profile", ""],
            vec![
                "--library",
                "charts",
                "--profile",
                "one",
                "--profile",
                "two",
            ],
        ] {
            assert!(
                Options::parse(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err()
            );
        }
        let base = ["--alsa", "hw:1", "--bind", "11:04", "--bind", "12:05"].map(String::from);
        let merged = beatkernel_bms_runtime::settings::overlay_native_args(
            &base,
            &options.native,
            SettingsHost::Linux,
        )
        .unwrap();
        assert_eq!(merged, ["--alsa", "hw:1", "--bind", "11:04"]);
    }
    #[test]
    fn settings_edits_update_the_draft_and_rejections_preserve_text_and_caret() {
        let values =
            NativeSettings::from_args(&["--evdev".into(), "device".into()], SettingsHost::Linux)
                .unwrap();
        let editor = LineEditor::new("device", 4096).unwrap();
        let mut draft = SettingsDraft {
            cached_local: None,
            values,
            presentation: PresentationSettings::default(),
            selected: 0,
            editor,
            profile: LineEditor::new("", 4096).unwrap(),
            profile_focused: false,
            message: None,
            error: None,
        };
        draft.edit(Some(KeyCode::Home), None);
        draft.edit(None, Some("별"));
        assert_eq!(draft.values.fields()[0].value, "별device");
        let before = (draft.editor.value().to_owned(), draft.editor.cursor());
        draft.edit(None, Some("\n"));
        assert!(draft.error.is_some());
        assert_eq!(
            (draft.editor.value().to_owned(), draft.editor.cursor()),
            before
        );
        assert_eq!(draft.values.fields()[0].value, "별device");
        draft.profile_focused = true;
        draft.edit(None, Some("내 설정.txt"));
        assert_eq!(draft.profile.value(), "내 설정.txt");
        assert_eq!(draft.values.fields()[0].value, "별device");
    }
    #[test]
    fn selection_replaces_chart_once_and_preserves_repeated_native_options() {
        let args = [
            "--chart",
            "old.bms",
            "--bind",
            "11:04",
            "--ghost-self",
            "one.bkr",
            "--bind",
            "12:05",
            "--ghost-self",
            "two.bkr",
        ]
        .map(String::from);
        let next = with_chart(&args, "selected.bms");
        assert_eq!(
            next.iter()
                .filter(|value| value.as_str() == "--chart")
                .count(),
            1
        );
        assert_eq!(next.last().unwrap(), "selected.bms");
        assert_eq!(without_chart(&next), without_chart(&args));
        assert!(!next.iter().any(|value| value == "old.bms"));
    }
    #[test]
    fn ui_options_leave_native_device_configuration_intact() {
        let args = [
            "--library",
            "charts",
            "--ui-fps",
            "60",
            "--device",
            "17",
            "--buffer",
            "128",
            "--bind",
            "A:1",
        ]
        .map(String::from);
        let options = Options::parse(&args).unwrap();
        assert_eq!(
            options.native,
            ["--device", "17", "--buffer", "128", "--bind", "A:1"]
        );
        assert_eq!(options.fps, 60);
        let reserved_value = ["--chart", "--ui-fps", "--device", "--library"].map(String::from);
        assert_eq!(
            Options::parse(&reserved_value).unwrap().native,
            reserved_value
        );
        for args in [
            vec!["--chart", ""],
            vec!["--library", ""],
            vec!["--chart", "a", "--ui-fps", "60", "--ui-fps", "120"],
            vec![
                "--chart",
                "a",
                "--ui-lookahead-ms",
                "100",
                "--ui-lookahead-ms",
                "200",
            ],
        ] {
            assert!(
                Options::parse(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err()
            );
        }
        assert!(
            Options::parse(&[
                "--library".into(),
                "charts".into(),
                "--chart".into(),
                "song.bms".into()
            ])
            .is_err()
        );
    }
    #[test]
    fn native_title_keeps_unicode_but_removes_control_characters_and_bounds_size() {
        assert!(window_title("곡\0제목", "아티스트").contains("곡제목"));
        assert!(
            !window_title("bad\nname", "\0")
                .chars()
                .any(char::is_control)
        );
        assert_eq!(window_title(&"A".repeat(1024), "").chars().count(), 256);
    }
    mod live_output {
        include!("desktop_live_output_fixtures.rs");
    }
}

#[cfg(test)]
#[path = "desktop_record_details_fixtures.rs"]
mod desktop_record_details_fixtures;

#[cfg(test)]
#[path = "desktop_grade_page_fixtures.rs"]
mod desktop_grade_page_fixtures;

#[cfg(test)]
#[path = "desktop_comparison_page_fixtures.rs"]
mod desktop_comparison_page_fixtures;
