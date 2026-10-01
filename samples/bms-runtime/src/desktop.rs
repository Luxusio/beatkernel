//! Native main-thread presentation of snapshots from the actual gameplay owner.
use beatkernel::judge::JudgeOutcome;
use beatkernel_bms_runtime::ui::{
    atoms::{rect, text},
    catalog_search::CatalogSearch,
    devices::{DevicesFrame, DevicesView},
    display::{BUTTONS as DISPLAY_BUTTONS, DisplayFrame, DisplayView},
    interaction::{Bounds, ControlId, Gesture, logical_point},
    molecules, organisms,
    players::{PlayersFrame, PlayersView},
    practice::{PracticeFrame, PracticeView},
    records::{RecordsFrame, RecordsView},
    selection::{SelectionFrame, SelectionItem, SelectionView},
    settings::{BUTTONS as SETTINGS_BUTTONS, SettingsFrame, SettingsView},
    text_input::LineEditor,
};
use beatkernel_bms_runtime::{
    competition::OpponentKind,
    device_catalog::{DeviceCatalog, DeviceRequest},
    local_players::PlayerId,
    local_setup::LocalSetup,
    panel_scope::{PanelScope, TaskPermit},
    player, player_chart,
    practice::PracticeStart,
    presentation_settings::PresentationSettings,
    record_catalog::{RecordCatalog, RecordPreview},
    screen_lifecycle::{ScreenInstanceId, ScreenNavigator, ScreenPhase, ScreenRoute},
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
    path::PathBuf,
    sync::Arc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

const WIDTH: usize = 960;
const HEIGHT: usize = 720;
type Native = fn(&[String]) -> Result<(), Box<dyn Error>>;
type QueryDevices = fn(DeviceRequest) -> Result<DeviceCatalog, Box<dyn Error>>;

struct Options {
    native: Vec<String>,
    library: Option<PathBuf>,
    chart: Option<PathBuf>,
    lookahead: i64,
    fps: usize,
    backend: BackendChoice,
    presentation: Presentation,
    profile: Option<PathBuf>,
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
                    | "--ui-lookahead-ms"
                    | "--ui-fps"
                    | "--gpu-backend"
                    | "--present"
            ) {
                match flag {
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
struct Game {
    viewer: player::PlayerViewer,
    worker: Option<JoinHandle<Result<(), String>>>,
    snapshot: Option<player::PlayerSnapshot>,
    cancelling: bool,
    joined: bool,
    local_page: usize,
    local_comparisons: bool,
    replay: bool,
    launch: SessionLaunch,
    prepared_retry: Option<SessionLaunch>,
    practice_bookmark: Option<PracticeStart>,
}

impl Game {
    fn accept_snapshot(&mut self, snapshot: player::PlayerSnapshot) {
        let count = snapshot.players.len();
        if count > 0 {
            self.local_page = self.local_page.min(
                count
                    .div_ceil(organisms::LOCAL_PLAYERS_PER_PAGE)
                    .saturating_sub(1),
            );
        }
        self.snapshot = Some(snapshot);
    }
    fn retry_available(&self) -> bool {
        self.prepared_retry.is_none() && (self.joined || !self.cancelling)
    }
    fn practice_position(&self) -> Option<PracticeStart> {
        if self.replay || self.joined || self.cancelling || self.prepared_retry.is_some() {
            return None;
        }
        let snapshot = self.snapshot.as_ref()?;
        if snapshot.status != player::PlayerStatus::Playing || snapshot.cancelled {
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
        Ok(())
    }
    fn practice_restart_available(&self) -> bool {
        !self.replay && self.practice_bookmark.is_some() && self.retry_available()
    }
    fn cancel(&mut self) {
        self.prepared_retry = None;
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
        cancelling: false,
        joined: false,
        local_page,
        local_comparisons,
        replay,
        launch,
        prepared_retry: None,
        practice_bookmark: None,
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
            "player (--library DIR | --chart PATH) [--profile PATH] [--ui-lookahead-ms 100..10000] [--ui-fps 30..240] [--gpu-backend auto|vulkan|dx12|metal|gl] [--present fifo|immediate|mailbox] NATIVE_OPTIONS\nSolo devices are automatic. Advanced native overrides and key bindings use flag-value pairs.\nF2: settings; F3 in selection: search; F4 in settings: records; F6 in settings: practice;  W in records list: watch; Up/Down: select; Enter: play/return; PageUp/PageDown: local player pages; C: toggle local comparisons; F5: retry pinned start; F7: mark live position; F8: restart mark after cleanup; Escape or focus loss: cancel; close: cancel and drain.\nUI keys do not provide gameplay input. Use the native play command's help for platform options."
        );
        return Ok(());
    }
    let mut options = Options::parse(args)?;
    if let Some(path) = &options.profile {
        let profile =
            beatkernel_bms_runtime::settings_profile::load_player_profile(path, settings_host())?;
        options.native = beatkernel_bms_runtime::settings::overlay_native_args(
            &profile.native.native_args(),
            &without_chart(&options.native),
            settings_host(),
        )?;
        options.set_display(
            profile
                .presentation
                .apply_overrides(&options.display_overrides)?,
        );
    }
    let (entries, diagnostics) = if let Some(root) = &options.library {
        let library = match player_chart::scan_library(root) {
            Ok(library) => library,
            Err(error) => player_chart::ChartLibrary {
                entries: Vec::new(),
                diagnostics: vec![error.to_string()],
            },
        };
        (
            library
                .entries
                .into_iter()
                .map(|entry| Entry {
                    path: entry.path,
                    title: entry.title,
                    artist: entry.artist,
                })
                .collect::<Vec<_>>(),
            library.diagnostics,
        )
    } else {
        let path = options
            .chart
            .as_ref()
            .expect("validated chart selection")
            .clone();
        let title = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        (
            vec![Entry {
                path,
                title,
                artist: String::new(),
            }],
            Vec::<String>::new(),
        )
    };
    let selection_items: Arc<[SelectionItem]> = entries
        .iter()
        .map(|entry| SelectionItem {
            title: entry.title.clone(),
            artist: entry.artist.clone(),
        })
        .collect::<Vec<_>>()
        .into();
    let selection_diagnostics = diagnostics.into();
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
        settings: None,
        settings_view: None,
        profile_io: None,
        entries,
        selected: 0,
        selection_items,
        catalog_search,
        search_editor,
        search_focused: false,
        selection_diagnostics,
        selection_view: None,
        painted_reactive: None,
        window: None,
        renderer: None,
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
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.fatal {
        return Err(error.into());
    }
    Ok(())
}

const SETTINGS_ROWS: usize = 10;
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
    error: Option<String>,
    view: PracticeView,
}
impl PracticeDraft {
    fn edit(&mut self, key: Option<KeyCode>, value: Option<&str>) {
        self.error = edit_line(&mut self.editor, key, value).err();
    }
    fn reset(&mut self) {
        match LineEditor::new("0:00", 64) {
            Ok(editor) => {
                self.editor = editor;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
}

struct RecordsDraft {
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
        if self
            .catalog
            .as_ref()
            .is_some_and(|catalog| index < catalog.entries.len())
        {
            if self.selected != Some(index) {
                self.preview = None;
                self.message = None;
            }
            self.selected = Some(index);
            self.directory_focused = false;
            self.first = index / SETTINGS_ROWS * SETTINGS_ROWS;
        }
    }
    fn page(&mut self, forward: bool) {
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
        if !self.directory_focused {
            return;
        }
        let before = self.directory.value().to_owned();
        self.error = edit_line(&mut self.directory, key, value).err();
        if self.directory.value() != before {
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
    if panel
        .as_ref()
        .is_some_and(|panel| !navigator.retains(panel.id()))
    {
        *panel = None;
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
    settings: Option<PanelScope<SettingsDraft>>,
    settings_view: Option<SettingsView>,
    profile_io: Option<ProfileOperation>,
    entries: Vec<Entry>,
    selected: usize,
    selection_items: Arc<[SelectionItem]>,
    catalog_search: CatalogSearch,
    search_editor: LineEditor,
    search_focused: bool,
    selection_diagnostics: Arc<[String]>,
    selection_view: Option<SelectionView>,
    painted_reactive: Option<ScreenInstanceId>,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
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
    fn invalidate_hits(&mut self) {
        self.hits.clear();
        // A retained scene must restore hit regions even if its signals are equal.
        self.painted_reactive = None;
    }
    fn closing(&self) -> bool {
        self.navigator.phase() == ScreenPhase::Exiting
    }
    fn is_suspended(&self) -> bool {
        self.navigator.phase() == ScreenPhase::Suspended
    }
    fn ui_ready(&self) -> bool {
        self.active
            && self.navigator.phase() == ScreenPhase::Active
            && !self.occluded
            && self.profile_io.is_none()
    }
    /// Prepare an atomic route change before data preparation or thread spawn.
    fn prepare_route(&self, to: ScreenRoute) -> Result<ScreenNavigator, String> {
        if to != ScreenRoute::Closing
            && !matches!(to, ScreenRoute::Play { .. } | ScreenRoute::Results { .. })
            && self.game.as_ref().is_some_and(|game| !game.joined)
        {
            return Err("navigation waits for native session cleanup".into());
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
        if !matches!(
            self.navigator.route(),
            ScreenRoute::Play { .. } | ScreenRoute::Results { .. } | ScreenRoute::Closing
        ) {
            self.game = None;
        }
    }
    fn navigate(&mut self, to: ScreenRoute) -> Result<(), String> {
        let next = self.prepare_route(to)?;
        self.commit_route(next);
        Ok(())
    }
    fn back(&mut self) {
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
        if !self.navigator.accepts(scope.0) {
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
        if operation.permit.is_cancelled() || !self.navigator.accepts(operation.owner) {
            return;
        }
        // Completion must wake event-driven menus before polling switches to Wait.
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        if self.navigator.route() == ScreenRoute::Records {
            if let Some(records) = &mut self.records {
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
                            records.preview = Some(preview);
                            records.error = None;
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
        self.ui_ready()
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
    fn records_key(&mut self, key: KeyCode, repeat: bool) {
        match key {
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
            let practice = PracticeDraft {
                editor: LineEditor::new(&start.formatted(), 64)?,
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
            let settings = self.settings.as_ref().ok_or("settings unavailable")?;
            let mut values = settings.values.clone();
            start.apply_to(&mut values)?;
            let editor = if values
                .fields()
                .get(settings.selected)
                .is_some_and(|field| field.flag == "--start-ns")
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
    }
    fn hit(&self) -> Option<ControlId> {
        if !self.active || self.closing() || self.is_suspended() || self.occluded {
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
        if self.navigator.route() == ScreenRoute::Records {
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
                71 => self.finish_practice(),
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
            let count = game.snapshot.as_ref().map_or(0, |s| s.players.len());
            game.local_page = local_page(count, game.local_page, forward);
            self.gesture.cancel();
            self.invalidate_hits();
        }
    }
    fn toggle_local_comparisons(&mut self) {
        if let Some(game) = &mut self.game {
            if game
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| local_comparisons_available(&snapshot.players))
            {
                game.local_comparisons = !game.local_comparisons;
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
        let prepared = if from_bookmark {
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
                if self.game.as_ref().is_some_and(|game| game.joined) {
                    self.replace_joined_game(launch);
                } else if let Some(game) = &mut self.game {
                    game.prepared_retry = Some(launch);
                    game.viewer.cancel();
                    game.cancelling = true;
                    self.failure = None;
                }
            }
            Err(error) => self.failure = Some(format!("retry preflight: {error}")),
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
                return;
            }
        };
        // Spawn is the only fallible step; retain joined results on failure.
        let native = if old.replay { self.replay } else { self.native };
        let bookmark = old.practice_bookmark;
        match spawn_game(
            native,
            launch,
            old.local_page,
            old.local_comparisons,
            old.replay,
        ) {
            Ok(mut game) => {
                game.practice_bookmark = bookmark;
                self.commit_route(next);
                self.game = Some(game);
                self.failure = None;
            }
            Err(error) => self.failure = Some(format!("retry spawn: {error}")),
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
        let mut retry = None;
        if let Some(game) = &mut self.game {
            if let Some(snapshot) = game.viewer.take_latest() {
                if let (Some(window), Some(chart)) = (&self.window, &snapshot.chart) {
                    window.set_title(&window_title(&chart.title, &chart.artist));
                }
                game.accept_snapshot(snapshot);
            }
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
        if self.navigator.phase() == ScreenPhase::Active {
            if let Some(game) = &self.game {
                if game.joined && matches!(self.navigator.route(), ScreenRoute::Play { .. }) {
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
    fn set_search_focus(&mut self, focused: bool) {
        if self.search_focused != focused {
            self.search_focused = focused;
            if let Some(window) = &self.window {
                window.set_ime_allowed(focused);
            }
        }
    }
    fn edit_search(&mut self, key: Option<KeyCode>, value: Option<&str>) {
        if !self.ui_ready()
            || self.navigator.route() != ScreenRoute::Selection
            || !self.search_focused
        {
            return;
        }
        let mut editor = self.search_editor.clone();
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
    fn key(&mut self, key: KeyCode, repeat: bool) {
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
                    self.catalog_search.step(key == KeyCode::ArrowDown);
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
        let id = self
            .navigator
            .active_id()
            .ok_or("Selection instance unavailable")?;
        if self
            .selection_view
            .as_ref()
            .is_none_or(|view| view.id() != id)
        {
            self.selection_view = Some(SelectionView::new(
                id,
                Arc::clone(&self.selection_items),
                Arc::clone(&self.selection_diagnostics),
                WIDTH as u32,
                HEIGHT as u32,
            )?);
            self.painted_reactive = None;
        }
        let frame = SelectionFrame {
            selected: self.selected,
            hovered: self.hit(),
            armed: [ControlId(1), ControlId(5), ControlId(4)]
                .into_iter()
                .find(|&id| self.gesture.is_armed(id)),
            error: self.failure.clone(),
            backend_pending: self.active_backend != self.options.backend,
        };
        let view = self
            .selection_view
            .as_ref()
            .ok_or("Selection view unavailable")?;
        view.set_projection(self.catalog_search.indices(), self.catalog_search.cursor())?;
        view.set_search(&self.search_editor, self.search_focused)?;
        view.update(frame);
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
        let view = self
            .settings_view
            .as_ref()
            .ok_or("settings view unavailable")?;
        view.update(SettingsFrame {
            fields: settings.values.fields(),
            selected: settings.selected,
            editor: &settings.editor,
            profile: &settings.profile,
            profile_focused: settings.profile_focused,
            message: settings.message.as_deref(),
            error: settings.error.as_deref(),
            pending,
            hovered,
            armed,
        })?;
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
        let view = self
            .display_view
            .as_ref()
            .ok_or("display view unavailable")?;
        view.update(DisplayFrame {
            editors: &display.editors,
            selected: display.selected,
            error: display.error.as_deref(),
            pending,
            hovered,
            armed,
        })?;
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
        let mut frame = records_frame(records, self.profile_io.is_some(), opponents);
        frame.hovered = beatkernel_bms_runtime::ui::records::hit(&frame, self.point());
        frame.armed = (50..=59)
            .map(ControlId)
            .find(|&id| self.gesture.is_armed(id));
        let view = self
            .records_view
            .as_ref()
            .ok_or("records view unavailable")?;
        view.update(frame)?;
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
        let armed = [ControlId(71), ControlId(72), ControlId(73)]
            .into_iter()
            .find(|&id| self.gesture.is_armed(id));
        let practice = self.practice.as_ref().ok_or("practice data unavailable")?;
        if practice.view.id() != id {
            return Err("practice instance is stale".into());
        }
        practice.view.update(PracticeFrame {
            editor: practice.editor.clone(),
            error: practice.error.clone(),
            hovered,
            armed,
        });
        if practice.view.dirty() || self.painted_reactive != Some(id) {
            practice.view.compose(&mut self.scene, &mut self.hits)?;
            self.painted_reactive = Some(id);
        }
        self.render_scene()
    }
    fn draw(&mut self) -> Result<(), String> {
        if self.navigator.phase() != ScreenPhase::Active {
            return Ok(());
        }
        let route = self.navigator.route();
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
            draw_game(pixels, game, self.options.lookahead)?;
            let count = game.snapshot.as_ref().map_or(0, |s| s.players.len());
            if count > organisms::LOCAL_PLAYERS_PER_PAGE {
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
                if game.local_page + 1 < count.div_ceil(organisms::LOCAL_PLAYERS_PER_PAGE) {
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
            if game
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| local_comparisons_available(&snapshot.players))
            {
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
            if !game.replay {
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
                let caption = game.practice_bookmark.map_or_else(
                    || "F7 MARK / F8 RESTART MARK".to_owned(),
                    |start| format!("MARK {}  F8 RESTART", start.formatted()),
                );
                text(pixels, 24, 102, &caption, 1, 0xd8b36b);
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
        }
        Ok(())
    }
}
impl ApplicationHandler for Desktop {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.closing() {
            return;
        }
        self.navigator.resume();
        if self.window.is_none() {
            match event_loop.create_window(
                Window::default_attributes()
                    .with_title("BeatKernel BMS player")
                    .with_inner_size(LogicalSize::new(WIDTH as f64, HEIGHT as f64)),
            ) {
                Ok(window) => self.window = Some(Arc::new(window)),
                Err(error) => {
                    self.fail(error);
                    return;
                }
            }
        }
        if self.renderer.is_none() {
            let window = self.window.as_ref().expect("created window").clone();
            // Native startup only. Reusable Renderer::new stays async for WASM hosts.
            let result = (|| -> Result<(wgpu::Instance, Renderer), String> {
                let instance = graphics::instance(self.active_backend)?;
                let surface = instance
                    .create_surface(window.clone())
                    .map_err(|error| error.to_string())?;
                let mut renderer = pollster::block_on(Renderer::new(
                    surface,
                    &instance,
                    self.options.presentation,
                ))?;
                let size = window.inner_size();
                renderer.resize(size.width, size.height)?;
                Ok((instance, renderer))
            })();
            match result {
                Ok((instance, renderer)) => {
                    self.instance = Some(instance);
                    self.renderer = Some(renderer);
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
        self.next_frame = Instant::now();
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.navigator.suspend();
        self.active = false;
        self.pointer = None;
        self.cancel();
        self.renderer = None;
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
                self.window = None;
            }
            WindowEvent::Focused(active) => {
                self.active = active;
                if !active {
                    self.pointer = None;
                    self.cancel();
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                if occluded {
                    self.gesture.cancel();
                }
            }
            WindowEvent::Resized(size) => {
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
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = Some((position.x, position.y))
            }
            WindowEvent::Ime(winit::event::Ime::Commit(value)) => {
                if self.navigator.route() == ScreenRoute::Selection && self.search_focused {
                    self.edit_search(None, Some(&value));
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer = None;
                self.gesture.cancel();
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
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
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let editing = self.navigator.active_id();
                let navigation = matches!(
                    event.physical_key,
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
                if let PhysicalKey::Code(key) = event.physical_key {
                    self.key(key, event.repeat);
                }
                if editing.is_some_and(|id| self.navigator.accepts(id))
                    && !navigation
                    && self.ui_ready()
                {
                    let value = event.text.as_deref().or_else(|| match &event.logical_key {
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
                            ScreenRoute::Settings => {
                                if let Some(draft) = &mut self.settings {
                                    draft.edit(None, Some(value));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested
                if !self.is_suspended() && !self.closing() && !self.occluded =>
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
        if request_redraw && !self.closing() && !self.is_suspended() && !self.occluded {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.collect_game();
        self.collect_profile();
        if self.closing()
            && self.game.as_ref().is_none_or(|game| game.joined)
            && self.profile_io.is_none()
        {
            event_loop.exit();
            return;
        }
        if self.reactive_waits_for_events() {
            event_loop.set_control_flow(ControlFlow::Wait);
        } else if self.closing() || self.is_suspended() || self.occluded {
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
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
        }
    }
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.request_close();
        // Unexpected OS exit still joins native owners through Game::drop.
        self.game = None;
        self.profile_io = None;
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
fn records_frame(records: &RecordsDraft, pending: bool, opponents: usize) -> RecordsFrame<'_> {
    RecordsFrame {
        directory: &records.directory,
        directory_focused: records.directory_focused,
        catalog: records.catalog.as_ref(),
        selected: records.selected,
        first: records.first,
        preview: records.valid_preview(),
        pending,
        opponents,
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
    let mut frame = records_frame(records, pending, opponents);
    frame.hovered = beatkernel_bms_runtime::ui::records::hit(&frame, point);
    frame.armed = (50..=59).map(ControlId).find(|&id| gesture.is_armed(id));
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

fn draw_game(pixels: &mut Scene, game: &Game, lookahead: i64) -> Result<(), String> {
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
            player::PlayerStatus::Playing if game.replay => "WATCHING RECORD - ESC CANCEL",
            player::PlayerStatus::Playing => "PLAYING - ESC CANCEL",
            player::PlayerStatus::Stopping => "STOPPING",
            player::PlayerStatus::Finished => "FINISHING CLEANUP",
            player::PlayerStatus::Failed(_) => "FAILED - CLEANING UP",
        }
    };
    text(pixels, 24, 65, status, 2, 0x9bb1cf);
    if snapshot.players.len() >= 2 {
        organisms::local_players_with_competition(
            pixels,
            &snapshot.players,
            lookahead,
            game.local_page,
            game.local_comparisons,
        )?;
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
        return Ok(());
    }
    if let Some(competition) = snapshot
        .players
        .first()
        .and_then(|player| player.competition.as_ref())
    {
        organisms::competition_scoreboard(pixels, &snapshot.score, competition)?;
    } else {
        organisms::scoreboard(pixels, &snapshot.score, &snapshot.recent_results);
    }
    if let (Some(chart), Some(now)) = (&snapshot.chart, snapshot.song_time) {
        organisms::playfield(pixels, chart, now, lookahead)?;
        text(
            pixels,
            24,
            665,
            &format!("SONG {:.3} S", now.as_nanos() as f64 / 1e9),
            2,
            0x9bb1cf,
        );
        if let Some(event) = snapshot.last_judge {
            if (i128::from(now.as_nanos()) - i128::from(event.at.as_nanos())).abs() <= 700_000_000 {
                let (label, color) = match event.outcome {
                    JudgeOutcome::Hit { grade, delta } => (
                        format!("HIT G{} {:+.2} MS", grade.0, delta.as_nanos() as f64 / 1e6),
                        0x74e5c5,
                    ),
                    JudgeOutcome::Miss { .. } => ("MISS".into(), 0xff8e8e),
                };
                text(pixels, 160, 550, &label, 2, color);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn lifecycle_fixture() -> Desktop {
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
            options: Options::parse(&[]).unwrap(),
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
            settings: None,
            settings_view: None,
            profile_io: None,
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
            selection_diagnostics: Arc::from([]),
            selection_view: None,
            painted_reactive: None,
            window: None,
            renderer: None,
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
        app.open_display();
        let display_id = app.display.as_ref().unwrap().id();
        let display_permit = app.display.as_ref().unwrap().task_permit();
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
                    player: PlayerId(id),
                    chart: None,
                    song_time: None,
                    score: Default::default(),
                    last_judge: None,
                    recent_results: Vec::new(),
                    competition: None,
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
            8,
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
                player: beatkernel_bms_runtime::local_players::PlayerId(id),
                chart: None,
                song_time: None,
                score: Default::default(),
                last_judge: None,
                recent_results: Vec::new(),
                competition: None,
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
}
