//! Native main-thread presentation of snapshots from the actual gameplay owner.
use beatkernel::judge::JudgeOutcome;
use beatkernel_bms_runtime::ui::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId, Gesture, logical_point},
    molecules, organisms,
    text_input::LineEditor,
};
use beatkernel_bms_runtime::{
    device_catalog::{DeviceCatalog, DeviceRequest},
    local_players::PlayerId,
    local_setup::LocalSetup,
    player, player_chart,
    presentation_settings::PresentationSettings,
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
) -> Result<(), Box<dyn Error>> {
    if args.len() == 1 && args[0] == "--help" {
        println!(
            "player (--library DIR | --chart PATH) [--profile PATH] [--ui-lookahead-ms 100..10000] [--ui-fps 30..240] [--gpu-backend auto|vulkan|dx12|metal|gl] [--present fifo|immediate|mailbox] NATIVE_OPTIONS\nSolo devices are automatic. Advanced native overrides and key bindings use flag-value pairs.\nF2: settings; Up/Down: select; Enter: play/return; PageUp/PageDown: local player pages; C: toggle local comparisons; Escape or focus loss: cancel; close: cancel and drain.\nUI keys do not provide gameplay input. Use the native play command's help for platform options."
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
    let event_loop = EventLoop::new()?;
    let active_backend = options.backend;
    let mut app = Desktop {
        options,
        active_backend,
        display: None,
        native,
        validate,
        query_devices,
        picker: None,
        local_setup: None,
        accepted_local: None,
        settings: None,
        profile_io: None,
        entries,
        diagnostics,
        selected: 0,
        window: None,
        renderer: None,
        instance: None,
        scene: Scene::new(WIDTH as u32, HEIGHT as u32),
        game: None,
        closing: false,
        active: false,
        occluded: false,
        suspended: false,
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
    Saved,
}
struct ProfileOperation(Option<JoinHandle<Result<ProfileResult, String>>>);
impl Drop for ProfileOperation {
    fn drop(&mut self) {
        if let Some(worker) = self.0.take() {
            let _ = worker.join();
        }
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

struct Desktop {
    options: Options,
    active_backend: BackendChoice,
    display: Option<DisplayDraft>,
    native: Native,
    validate: Native,
    query_devices: QueryDevices,
    picker: Option<DevicePicker>,
    local_setup: Option<LocalDraft>,
    accepted_local: Option<LocalSetup>,
    settings: Option<SettingsDraft>,
    profile_io: Option<ProfileOperation>,
    entries: Vec<Entry>,
    diagnostics: Vec<String>,
    selected: usize,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    instance: Option<wgpu::Instance>,
    scene: Scene,
    game: Option<Game>,
    closing: bool,
    active: bool,
    occluded: bool,
    suspended: bool,
    failure: Option<String>,
    fatal: Option<String>,
    next_frame: Instant,
    pointer: Option<(f64, f64)>,
    gesture: Gesture,
    hits: Vec<(ControlId, Bounds)>,
}
impl Desktop {
    fn open_settings(&mut self) {
        if self.game.is_some() || self.profile_io.is_some() {
            return;
        }
        let result = (|| {
            let values =
                NativeSettings::from_args(&without_chart(&self.options.native), settings_host())?;
            let editor = LineEditor::new(
                &values.fields().first().ok_or("no settings fields")?.value,
                4096,
            )?;
            Ok::<_, String>(SettingsDraft {
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
            })
        })();
        match result {
            Ok(draft) => {
                self.settings = Some(draft);
                self.failure = None;
            }
            Err(error) => self.failure = Some(error),
        }
        self.gesture.cancel();
        self.hits.clear();
    }
    fn open_local(&mut self) {
        if self.display.is_some()
            || self.profile_io.is_some()
            || self.game.is_some()
            || self.picker.is_some()
        {
            return;
        }
        let result = (|| {
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
            Ok::<_, String>(LocalDraft {
                model,
                selected: 0,
                first: 0,
            })
        })();
        match result {
            Ok(local) => {
                self.local_setup = Some(local);
                self.local_error(None);
            }
            Err(error) => self.local_error(Some(error)),
        }
        self.gesture.cancel();
        self.hits.clear();
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
        self.hits.clear();
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
            KeyCode::Escape if !repeat => self.local_setup = None,
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
        self.hits.clear();
    }
    fn finish_local(&mut self) {
        let result = (|| {
            let local = self.local_setup.as_ref().ok_or("local setup unavailable")?;
            let draft = self.settings.as_ref().ok_or("settings unavailable")?;
            let values = local.model.settings(&draft.values)?;
            self.validate_settings(&values)?;
            let editor = LineEditor::new(
                &values.fields().first().ok_or("settings empty")?.value,
                4096,
            )?;
            Ok::<_, String>((values, editor, local.model.clone()))
        })();
        match result {
            Ok((values, editor, model)) => {
                if let Some(draft) = &mut self.settings {
                    draft.values = values;
                    draft.cached_local = Some(model);
                    draft.selected = 0;
                    draft.editor = editor;
                    draft.profile_focused = false;
                    draft.error = None;
                    draft.message = Some("PLAYERS READY - APPLY TO USE".into());
                }
                self.local_setup = None;
            }
            Err(error) => self.local_error(Some(error)),
        }
        self.gesture.cancel();
        self.hits.clear();
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
        if self.display.is_some() {
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
        if self.profile_io.is_some() || self.game.is_some() {
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
            let query = self.query_devices;
            thread::Builder::new()
                .name("bms-devices".into())
                .spawn(move || {
                    query(request)
                        .map(|catalog| ProfileResult::Devices(catalog, player))
                        .map_err(|error| error.to_string())
                })
                .map_err(|error| error.to_string())
        })();
        match result {
            Ok(worker) => {
                self.profile_io = Some(ProfileOperation(Some(worker)));
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
        self.hits.clear();
    }
    fn use_device(&mut self) {
        let result = (|| {
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
                return Ok(());
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
            Ok::<(), String>(())
        })();
        match result {
            Ok(()) => self.picker = None,
            Err(error) => {
                if let Some(draft) = &mut self.settings {
                    draft.error = Some(error);
                }
            }
        }
        self.gesture.cancel();
        self.hits.clear();
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
        self.hits.clear();
    }
    fn picker_key(&mut self, key: KeyCode, repeat: bool) {
        match key {
            KeyCode::Escape if !repeat => {
                self.picker = None;
                self.hits.clear();
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
                self.hits.clear();
            }
            _ => {}
        }
    }
    fn profile_request(&mut self, save: bool) {
        if self.profile_io.is_some()
            || self.game.is_some()
            || self.display.is_some()
            || self.picker.is_some()
            || self.local_setup.is_some()
        {
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
            thread::Builder::new()
                .name("bms-profile".into())
                .spawn(move || {
                    if save {
                        beatkernel_bms_runtime::settings_profile::save_player_profile(
                            &path, &values, host,
                        )
                        .map(|()| ProfileResult::Saved)
                        .map_err(|error| error.to_string())
                    } else {
                        beatkernel_bms_runtime::settings_profile::load_player_profile(&path, host)
                            .map(ProfileResult::Loaded)
                            .map_err(|error| error.to_string())
                    }
                })
                .map_err(|error| error.to_string())
        })();
        match result {
            Ok(worker) => {
                self.profile_io = Some(ProfileOperation(Some(worker)));
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
        self.hits.clear();
    }
    fn collect_profile(&mut self) {
        if !self.profile_io.as_ref().is_some_and(|operation| {
            operation
                .0
                .as_ref()
                .is_some_and(|worker| worker.is_finished())
        }) {
            return;
        }
        let mut operation = self.profile_io.take().expect("finished profile operation");
        let result = operation
            .0
            .take()
            .expect("profile worker")
            .join()
            .unwrap_or_else(|_| Err("profile worker panicked".into()));
        if let Some(draft) = &mut self.settings {
            match result {
                Ok(ProfileResult::Devices(catalog, player)) => {
                    self.picker = Some(DevicePicker {
                        catalog,
                        player,
                        first: 0,
                        selected: None,
                    });
                    draft.error = None;
                    draft.message = None;
                }
                Ok(ProfileResult::Saved) => {
                    draft.error = None;
                    draft.message = Some("PROFILE SAVED - APPLY IS SEPARATE".into());
                }
                Ok(ProfileResult::Loaded(profile)) => {
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
                Err(error) => draft.error = Some(error),
            }
        }
        self.gesture.cancel();
        self.hits.clear();
    }
    fn apply_settings(&mut self) {
        if self.profile_io.is_some()
            || self.game.is_some()
            || self.display.is_some()
            || self.picker.is_some()
            || self.local_setup.is_some()
        {
            return;
        }
        let Some(draft) = &self.settings else {
            return;
        };
        let result = (|| {
            let args = self.validate_settings(&draft.values)?;
            let presentation = draft.presentation;
            presentation.validate()?;
            graphics::instance_descriptor(presentation.backend)?;
            let cached_local = draft.cached_local.clone();
            let profile =
                (!draft.profile.value().is_empty()).then(|| PathBuf::from(draft.profile.value()));
            Ok::<_, String>((args, presentation, cached_local, profile))
        })();
        // All drafts and host/build validation are complete before GPU mutation.
        // No options are committed if the current surface rejects this mode.
        let result = result.and_then(|(args, presentation, cached_local, profile)| {
            if presentation.presentation != self.options.presentation {
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_presentation(presentation.presentation)?;
                }
            }
            Ok((args, presentation, cached_local, profile))
        });
        match result {
            Ok((args, presentation, cached_local, profile)) => {
                self.options.native = args;
                self.options.set_display(presentation);
                self.next_frame = Instant::now();
                self.accepted_local = cached_local;
                self.options.profile = profile;
                self.settings = None;
                self.failure = None;
            }
            Err(error) => {
                if let Some(draft) = &mut self.settings {
                    draft.error = Some(error);
                }
            }
        }
        self.gesture.cancel();
        self.hits.clear();
    }
    fn open_display(&mut self) {
        if self.game.is_some()
            || self.profile_io.is_some()
            || self.picker.is_some()
            || self.local_setup.is_some()
            || self.display.is_some()
        {
            return;
        }
        if let Some(draft) = &mut self.settings {
            match DisplayDraft::new(draft.presentation) {
                Ok(display) => self.display = Some(display),
                Err(error) => draft.error = Some(error),
            }
        }
        self.gesture.cancel();
        self.hits.clear();
    }
    fn finish_display(&mut self) {
        if self.profile_io.is_some() || self.game.is_some() {
            return;
        }
        let Some(display) = &mut self.display else {
            return;
        };
        match display.value() {
            Ok(value) => {
                if let Some(draft) = &mut self.settings {
                    draft.presentation = value;
                    draft.error = None;
                    draft.message = Some("DISPLAY DRAFT UPDATED - APPLY IS SEPARATE".into());
                }
                self.display = None;
            }
            Err(error) => display.error = Some(error),
        }
        self.gesture.cancel();
        self.hits.clear();
    }
    fn display_key(&mut self, key: KeyCode, repeat: bool) {
        match key {
            KeyCode::Escape if !repeat => self.display = None,
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
        self.hits.clear();
    }
    fn settings_key(&mut self, key: KeyCode, repeat: bool) {
        match key {
            KeyCode::Escape if !repeat => {
                self.settings = None;
                self.hits.clear();
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
                        self.hits.clear();
                        return;
                    }
                    let length = draft.values.fields().len();
                    if key == KeyCode::Tab && draft.selected + 1 == length {
                        draft.profile_focused = true;
                        self.hits.clear();
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
                    self.hits.clear();
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
        if !self.active || self.closing || self.suspended || self.occluded {
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
        if self.profile_io.is_some() {
            return;
        }
        if self.display.is_some() {
            match id.0 {
                40 => self.finish_display(),
                41 => self.display = None,
                40000..=40003 => {
                    self.display.as_mut().expect("display routing").selected =
                        (id.0 - 40000) as usize
                }
                _ => {}
            }
            self.gesture.cancel();
            self.hits.clear();
            return;
        }
        if self.picker.is_some() {
            match id.0 {
                20 => self.use_device(),
                21 => {
                    self.picker = None;
                    self.gesture.cancel();
                    self.hits.clear();
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
                    self.hits.clear();
                }
                _ => {}
            }
            return;
        }
        if self.local_setup.is_some() {
            match id.0 {
                30 => self.finish_local(),
                31 => self.local_setup = None,
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
            self.hits.clear();
            return;
        }
        if self.settings.is_some() {
            match id.0 {
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
                    self.hits.clear();
                }
                10 => self.apply_settings(),
                11 => {
                    self.settings = None;
                    self.gesture.cancel();
                    self.hits.clear();
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
                    self.hits.clear();
                }
                row if row >= 1000 => {
                    if let Some(draft) = &mut self.settings {
                        if let Err(error) = draft.select((row - 1000) as usize) {
                            draft.error = Some(error);
                        }
                    }
                    self.gesture.cancel();
                    self.hits.clear();
                }
                _ => {}
            }
            return;
        }
        match id.0 {
            6 if self.game.is_some() => self.change_local_page(false),
            7 if self.game.is_some() => self.change_local_page(true),
            8 if self.game.is_some() => self.toggle_local_comparisons(),
            5 if self.game.is_none() => self.open_settings(),
            1 if self.game.is_none() && !self.entries.is_empty() => self.key(KeyCode::Enter, false),
            2 if self.game.as_ref().is_some_and(|game| !game.joined) => self.cancel(),
            3 if self.game.as_ref().is_some_and(|game| game.joined) => {
                self.key(KeyCode::Enter, false)
            }
            4 if self.game.is_none() => {
                self.closing = true;
                self.cancel();
            }
            row if row >= 100 && self.game.is_none() => {
                if let Ok(index) = usize::try_from(row - 100) {
                    if index < self.entries.len() {
                        self.selected = index;
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
            self.hits.clear();
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
                self.hits.clear();
            }
        }
    }
    fn cancel(&mut self) {
        self.gesture.cancel();
        if let Some(game) = &mut self.game {
            if !game.joined {
                game.viewer.cancel();
                game.cancelling = true;
            }
        }
    }
    fn fail(&mut self, error: impl ToString) {
        self.fatal = Some(error.to_string());
        self.closing = true;
        self.cancel();
    }
    fn collect_game(&mut self) {
        if let Some(game) = &mut self.game {
            if let Some(snapshot) = game.viewer.take_latest() {
                if let (Some(window), Some(chart)) = (&self.window, &snapshot.chart) {
                    window.set_title(&window_title(&chart.title, &chart.artist));
                }
                game.snapshot = Some(snapshot);
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
                if let Some(snapshot) = game.viewer.take_latest() {
                    game.snapshot = Some(snapshot);
                }
                if let Err(error) = result {
                    self.failure = Some(error);
                }
                game.joined = true;
            }
        }
    }
    fn key(&mut self, key: KeyCode, repeat: bool) {
        self.gesture.cancel();
        if !self.active || self.closing || self.profile_io.is_some() {
            return;
        }
        if self.display.is_some() {
            self.display_key(key, repeat);
            return;
        }
        if self.picker.is_some() {
            self.picker_key(key, repeat);
            return;
        }
        if self.local_setup.is_some() {
            self.local_key(key, repeat);
            return;
        }
        if self.settings.is_some() {
            self.settings_key(key, repeat);
            return;
        }
        if self.game.is_some() && !repeat && matches!(key, KeyCode::PageUp | KeyCode::PageDown) {
            self.change_local_page(key == KeyCode::PageDown);
            return;
        }
        if self.game.is_some() && !repeat && key == KeyCode::KeyC {
            self.toggle_local_comparisons();
            return;
        }
        if self.game.as_ref().is_some_and(|game| game.joined) {
            if !repeat && matches!(key, KeyCode::Enter | KeyCode::Escape) {
                self.game = None;
                self.failure = None;
                if let Some(window) = &self.window {
                    window.set_title("BeatKernel BMS player");
                }
            }
        } else if self.game.is_some() {
            if key == KeyCode::Escape && !repeat {
                self.cancel();
            }
        } else {
            match key {
                KeyCode::F2 if !repeat => self.open_settings(),
                KeyCode::Escape if !repeat => self.closing = true,
                KeyCode::ArrowUp => self.selected = self.selected.saturating_sub(1),
                KeyCode::ArrowDown if !self.entries.is_empty() => {
                    self.selected = (self.selected + 1).min(self.entries.len() - 1)
                }
                KeyCode::Enter if !repeat && !self.entries.is_empty() => {
                    if let Err(error) = self.start() {
                        self.failure = Some(error);
                    }
                }
                _ => {}
            }
        }
    }
    fn start(&mut self) -> Result<(), String> {
        if self.profile_io.is_some() {
            return Err("profile operation is pending".into());
        }
        let entry = &self.entries[self.selected];
        let path = entry
            .path
            .to_str()
            .ok_or("native chart path must be UTF-8")?;
        let args = with_chart(&self.options.native, path);
        (self.validate)(&args).map_err(|error| error.to_string())?;
        let (publisher, viewer) = player::channel();
        let native = self.native;
        let worker = thread::Builder::new()
            .name("bms-game".into())
            .spawn(move || {
                player::with_publisher(publisher, || {
                    native(&args).map_err(|error| error.to_string())
                })
            })
            .map_err(|error| error.to_string())?;
        if let Some(window) = &self.window {
            window.set_title(&window_title(&entry.title, &entry.artist));
        }
        self.failure = None;
        self.game = Some(Game {
            viewer,
            worker: Some(worker),
            snapshot: None,
            cancelling: false,
            joined: false,
            local_page: 0,
            local_comparisons: false,
        });
        Ok(())
    }
    fn draw(&mut self) -> Result<(), String> {
        let point = self.point();
        self.hits.clear();
        self.scene.clear();
        let pixels = &mut self.scene;
        rect(pixels, 0, 0, WIDTH as i64, HEIGHT as i64, 0x10151e);
        text(pixels, 24, 20, "BEATKERNEL BMS PLAYER", 3, 0xf0f4ff);
        if let Some(display) = &self.display {
            draw_display(
                pixels,
                display,
                &mut self.hits,
                &self.gesture,
                point,
                self.profile_io.is_some(),
            );
        } else if let Some(picker) = &self.picker {
            draw_devices(
                pixels,
                picker,
                self.settings.as_ref().expect("picker has draft"),
                &mut self.hits,
                &self.gesture,
                point,
                self.profile_io.is_some(),
            );
        } else if let Some(local) = &self.local_setup {
            draw_local(
                pixels,
                local,
                self.settings.as_ref().expect("local has draft"),
                &mut self.hits,
                &self.gesture,
                point,
                self.profile_io.is_some(),
            );
        } else if let Some(draft) = &self.settings {
            draw_settings(
                pixels,
                draft,
                &mut self.hits,
                &self.gesture,
                point,
                self.profile_io.is_some(),
            )?;
        } else if let Some(game) = &self.game {
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
            text(
                pixels,
                24,
                65,
                "UP/DOWN SELECT  ENTER PLAY  F2 SETTINGS",
                2,
                0x9bb1cf,
            );
            text(
                pixels,
                24,
                96,
                &format!(
                    "{} CHARTS   {} SCAN DIAGNOSTICS",
                    self.entries.len(),
                    self.diagnostics.len()
                ),
                2,
                0xd8b36b,
            );
            let first = self.selected.saturating_sub(8);
            for (row, entry) in self.entries.iter().enumerate().skip(first).take(15) {
                let y = 140 + (row - first) * 34;
                if row == self.selected {
                    rect(pixels, 18, y as i64 - 6, 924, 30, 0x263d59);
                }
                text(pixels, 28, y, &entry.title, 2, 0xf0f4ff);
                self.hits.push((
                    ControlId(100 + row as u64),
                    Bounds {
                        x: 18,
                        y: y as i64 - 6,
                        width: 924,
                        height: 30,
                    },
                ));
            }
            for (index, diagnostic) in self.diagnostics.iter().take(2).enumerate() {
                text(pixels, 24, 654 + index * 22, diagnostic, 1, 0xd8b36b);
            }
            if self.entries.is_empty() {
                text(pixels, 24, 150, "NO SUPPORTED CHARTS FOUND", 2, 0xff8e8e);
            }
            if !self.entries.is_empty() {
                control(
                    pixels,
                    &mut self.hits,
                    &self.gesture,
                    point,
                    ControlId(1),
                    Bounds {
                        x: 550,
                        y: 65,
                        width: 180,
                        height: 34,
                    },
                    "START",
                );
            }
            control(
                pixels,
                &mut self.hits,
                &self.gesture,
                point,
                ControlId(5),
                Bounds {
                    x: 750,
                    y: 102,
                    width: 180,
                    height: 30,
                },
                "SETTINGS",
            );
            control(
                pixels,
                &mut self.hits,
                &self.gesture,
                point,
                ControlId(4),
                Bounds {
                    x: 750,
                    y: 65,
                    width: 180,
                    height: 34,
                },
                "EXIT",
            );
        }
        if self.game.is_none()
            && self.settings.is_none()
            && self.active_backend != self.options.backend
        {
            text(
                pixels,
                24,
                700,
                "GPU BACKEND PENDING - SAVE PROFILE AND RESTART",
                1,
                0xd8b36b,
            );
        }
        if let Some(error) = &self.failure {
            text(
                pixels,
                24,
                650,
                "ERROR - ENTER RETURNS TO SELECTION",
                2,
                0xff8e8e,
            );
            text(pixels, 24, 682, error, 1, 0xffaaaa);
        }
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
        if self.closing {
            return;
        }
        self.suspended = false;
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
        self.next_frame = Instant::now();
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.suspended = true;
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
        match event {
            WindowEvent::CloseRequested => {
                self.closing = true;
                self.cancel();
            }
            WindowEvent::Destroyed => {
                self.closing = true;
                self.cancel();
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
                self.gesture.cancel();
                self.pointer = None;
                self.hits.clear();
                if let Some(renderer) = &mut self.renderer {
                    if let Err(error) = renderer.resize(size.width, size.height) {
                        self.fail(error);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = Some((position.x, position.y))
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
                let editing = self.settings.is_some();
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
                    )
                );
                if let PhysicalKey::Code(key) = event.physical_key {
                    self.key(key, event.repeat);
                }
                if editing
                    && !navigation
                    && self.active
                    && !self.closing
                    && self.profile_io.is_none()
                    && self.picker.is_none()
                    && self.local_setup.is_none()
                {
                    let value = event.text.as_deref().or_else(|| match &event.logical_key {
                        Key::Character(value) => Some(value.as_str()),
                        _ => None,
                    });
                    if let Some(value) = value {
                        if let Some(display) = &mut self.display {
                            display.edit(None, Some(value));
                        } else if let Some(draft) = &mut self.settings {
                            draft.edit(None, Some(value));
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested if !self.suspended && !self.closing && !self.occluded => {
                self.collect_game();
                if let Err(error) = self.draw() {
                    self.fail(error);
                }
                self.next_frame =
                    Instant::now() + Duration::from_secs_f64(1.0 / self.options.fps as f64);
            }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.collect_game();
        self.collect_profile();
        if self.closing
            && self.game.as_ref().is_none_or(|game| game.joined)
            && self.profile_io.is_none()
        {
            event_loop.exit();
            return;
        }
        if self.closing || self.suspended || self.occluded {
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
        self.cancel();
        // Unexpected OS exit still joins native owners through Game::drop.
        self.game = None;
        self.profile_io = None;
    }
}

fn draw_local(
    scene: &mut Scene,
    local: &LocalDraft,
    draft: &SettingsDraft,
    hits: &mut Vec<(ControlId, Bounds)>,
    gesture: &Gesture,
    point: Option<(f64, f64)>,
    pending: bool,
) {
    text(
        scene,
        24,
        65,
        "PLAYERS - +/- COUNT - SPACE ASSIGN - ENTER DONE - ESC BACK",
        1,
        0x9bb1cf,
    );
    let players = local.model.players();
    let solo = players.len() == 1;
    text(
        scene,
        24,
        91,
        &format!(
            "{} PLAYERS  ROWS {}-{} OF {}",
            players.len(),
            local.first + 1,
            (local.first + SETTINGS_ROWS).min(players.len()),
            players.len()
        ),
        1,
        0xd8b36b,
    );
    for (index, player) in players
        .iter()
        .enumerate()
        .skip(local.first)
        .take(SETTINGS_ROWS)
    {
        let bounds = Bounds {
            x: 24,
            y: 120 + (index - local.first) as i64 * 39,
            width: 906,
            height: 34,
        };
        let label = if solo {
            "SOLO - INPUT AUTOMATIC".to_owned()
        } else {
            format!(
                "P{}  {}",
                player.id.0,
                player.input().unwrap_or("NO KEYBOARD ASSIGNED")
            )
        };
        if local.selected == index {
            rect(scene, bounds.x - 4, bounds.y, 4, bounds.height, 0x74e5c5);
        }
        if pending {
            molecules::button(scene, bounds, &label, false, false);
        } else {
            control(
                scene,
                hits,
                gesture,
                point,
                ControlId(20000 + index as u64),
                bounds,
                &label,
            );
        }
    }
    text(
        scene,
        24,
        535,
        if solo {
            "SOLO STARTS WITHOUT DEVICE SELECTION"
        } else {
            "ASSIGN A DISTINCT KEYBOARD TO EACH PLAYER"
        },
        1,
        0x9bb1cf,
    );
    for (id, x, label) in [(36, 620, "PREVIOUS"), (37, 780, "NEXT")] {
        let available = if id == 36 {
            local.first > 0
        } else {
            local.first + SETTINGS_ROWS < players.len()
        };
        if available {
            let bounds = Bounds {
                x,
                y: 550,
                width: 150,
                height: 34,
            };
            if pending {
                molecules::button(scene, bounds, label, false, false);
            } else {
                control(scene, hits, gesture, point, ControlId(id), bounds, label);
            }
        }
    }
    for (id, x, label) in [
        (30, 24, "DONE"),
        (31, 174, "BACK"),
        (32, 324, "REMOVE"),
        (33, 474, "ADD"),
        (34, 624, "ASSIGN KEYBOARD"),
        (35, 784, "CLEAR"),
    ] {
        if solo && matches!(id, 34 | 35) {
            continue;
        }
        let bounds = Bounds {
            x,
            y: 620,
            width: if id >= 34 { 150 } else { 140 },
            height: 34,
        };
        let unavailable = pending
            || (id == 32 && solo)
            || (id == 33
                && players.len() == beatkernel_bms_runtime::local_players::MAX_LOCAL_PLAYERS);
        if unavailable {
            molecules::button(scene, bounds, label, false, false);
        } else {
            control(scene, hits, gesture, point, ControlId(id), bounds, label);
        }
    }
    if pending {
        text(scene, 24, 590, "LOADING KEYBOARDS", 1, 0xd8b36b);
    }
    if let Some(error) = &draft.error {
        text(scene, 24, 690, error, 1, 0xff8e8e);
    } else if let Some(message) = &draft.message {
        text(scene, 24, 690, message, 1, 0x74e5c5);
    }
}

fn draw_devices(
    scene: &mut Scene,
    picker: &DevicePicker,
    draft: &SettingsDraft,
    hits: &mut Vec<(ControlId, Bounds)>,
    gesture: &Gesture,
    point: Option<(f64, f64)>,
    pending: bool,
) {
    text(
        scene,
        24,
        65,
        if picker.catalog.request().is_keyboard() {
            "KEYBOARD DEVICES - UP/DOWN SELECT - ENTER USE - ESC BACK"
        } else {
            "AUDIO OUTPUT DEVICES - UP/DOWN SELECT - ENTER USE - ESC BACK"
        },
        1,
        0x9bb1cf,
    );
    if let Some(player) = picker.player {
        text(scene, 690, 65, &format!("FOR P{}", player.0), 1, 0x74e5c5);
    }
    let first = picker.first;
    text(
        scene,
        24,
        91,
        &format!(
            "{} DEVICE ENTRIES - NO AUTOMATIC SELECTION",
            picker.catalog.choices().len()
        ),
        1,
        0xd8b36b,
    );
    organisms::device_list(
        scene,
        &picker.catalog,
        picker.selected,
        first,
        SETTINGS_ROWS,
    );
    if !pending {
        for (index, choice) in picker
            .catalog
            .choices()
            .iter()
            .enumerate()
            .skip(first)
            .take(SETTINGS_ROWS)
        {
            if choice.selectable {
                hits.push((
                    ControlId(10000 + index as u64),
                    Bounds {
                        x: 24,
                        y: 120 + (index - first) as i64 * 39,
                        width: 906,
                        height: 34,
                    },
                ));
            }
        }
    }
    for (id, x, label) in [
        (20, 24, "USE DEVICE"),
        (21, 212, "BACK"),
        (22, 400, "REFRESH"),
        (23, 588, "PREV"),
        (24, 776, "NEXT"),
    ] {
        let bounds = Bounds {
            x,
            y: 620,
            width: 170,
            height: 34,
        };
        if pending || (id == 20 && picker.selected.is_none()) {
            molecules::button(scene, bounds, label, false, false);
        } else {
            control(scene, hits, gesture, point, ControlId(id), bounds, label);
        }
    }
    if pending {
        text(scene, 24, 665, "LOADING DEVICES", 1, 0xd8b36b);
    }
    if let Some(error) = &draft.error {
        text(scene, 24, 690, error, 1, 0xff8e8e);
    }
}

fn draw_display(
    scene: &mut Scene,
    display: &DisplayDraft,
    hits: &mut Vec<(ControlId, Bounds)>,
    gesture: &Gesture,
    point: Option<(f64, f64)>,
    pending: bool,
) {
    text(
        scene,
        24,
        65,
        "DISPLAY - ENTER DONE - ESC BACK",
        2,
        0x9bb1cf,
    );
    for (index, label) in ["GPU BACKEND", "PRESENT MODE", "UI FPS", "LOOKAHEAD MS"]
        .iter()
        .enumerate()
    {
        let y = 130 + index as i64 * 75;
        text(scene, 24, (y + 10) as usize, label, 1, 0xf0f4ff);
        let bounds = Bounds {
            x: 280,
            y,
            width: 650,
            height: 34,
        };
        molecules::text_field(
            scene,
            &display.editors[index],
            bounds,
            index == display.selected && !pending,
        );
        if !pending {
            hits.push((ControlId(40000 + index as u64), bounds));
        }
    }
    for (y, hint) in [
        (445, "BACKEND: AUTO / VULKAN / DX12 / METAL / GL"),
        (460, "PRESENT: FIFO / IMMEDIATE / MAILBOX"),
        (475, "UI FPS: 30..240   LOOKAHEAD: 100..10000 MS"),
        (500, "SAVE PROFILE + RESTART FOR GPU BACKEND"),
        (515, "DONE UPDATES DRAFT - APPLY IS SEPARATE"),
    ] {
        text(scene, 24, y, hint, 1, 0x9bb1cf);
    }
    for (id, x, label) in [(40, 24, "DONE"), (41, 212, "BACK")] {
        let bounds = Bounds {
            x,
            y: 620,
            width: 170,
            height: 34,
        };
        if pending {
            molecules::button(scene, bounds, label, false, false);
        } else {
            control(scene, hits, gesture, point, ControlId(id), bounds, label);
        }
    }
    if let Some(error) = &display.error {
        text(scene, 24, 690, error, 1, 0xff8e8e);
    }
}

fn draw_settings(
    scene: &mut Scene,
    draft: &SettingsDraft,
    hits: &mut Vec<(ControlId, Bounds)>,
    gesture: &Gesture,
    point: Option<(f64, f64)>,
    pending: bool,
) -> Result<(), String> {
    text(scene, 24, 65, "SETTINGS - APPLY / BACK", 1, 0x9bb1cf);
    for (id, x, label) in [
        (18, 355, "DISPLAY"),
        (17, 550, "PLAYERS"),
        (16, 745, "AUDIO"),
    ] {
        let bounds = Bounds {
            x,
            y: 60,
            width: 185,
            height: 34,
        };
        if !pending {
            control(scene, hits, gesture, point, ControlId(id), bounds, label);
        } else {
            molecules::button(scene, bounds, label, false, false);
        }
    }
    let first = draft.selected / SETTINGS_ROWS * SETTINGS_ROWS;
    text(
        scene,
        24,
        91,
        &format!(
            "FIELDS {}-{} OF {}   UP/DOWN OR TAB SELECT",
            first + 1,
            (first + SETTINGS_ROWS).min(draft.values.fields().len()),
            draft.values.fields().len()
        ),
        1,
        0xd8b36b,
    );
    for (index, field) in draft
        .values
        .fields()
        .iter()
        .enumerate()
        .skip(first)
        .take(SETTINGS_ROWS)
    {
        let y = 120 + (index - first) as i64 * 39;
        text(scene, 24, (y + 10) as usize, field.label, 1, 0xf0f4ff);
        let bounds = Bounds {
            x: 280,
            y,
            width: 650,
            height: 32,
        };
        if index == draft.selected {
            molecules::text_field(scene, &draft.editor, bounds, !draft.profile_focused);
        } else {
            molecules::text_field_value(scene, &field.value, bounds);
        }
        if !pending {
            hits.push((ControlId(1000 + index as u64), bounds));
        }
    }
    if let Some(field) = draft.values.fields().get(draft.selected) {
        text(scene, 24, 525, field.hint, 1, 0x9bb1cf);
    }
    let profile_bounds = Bounds {
        x: 160,
        y: 558,
        width: 770,
        height: 34,
    };
    text(scene, 24, 570, "PROFILE PATH", 1, 0xf0f4ff);
    molecules::text_field(scene, &draft.profile, profile_bounds, draft.profile_focused);
    if !pending {
        hits.push((ControlId(15), profile_bounds));
    }
    for (id, x, label) in [
        (10, 24, "APPLY"),
        (11, 212, "BACK"),
        (12, 400, "ADD BINDING"),
        (13, 588, "LOAD"),
        (14, 776, "SAVE"),
    ] {
        if pending {
            molecules::button(
                scene,
                Bounds {
                    x,
                    y: 620,
                    width: 170,
                    height: 34,
                },
                label,
                false,
                false,
            );
            continue;
        }
        control(
            scene,
            hits,
            gesture,
            point,
            ControlId(id),
            Bounds {
                x,
                y: 620,
                width: 170,
                height: 34,
            },
            label,
        );
    }
    if pending {
        text(scene, 24, 665, "LOADING DEVICES", 1, 0xd8b36b);
    } else if let Some(message) = &draft.message {
        text(scene, 24, 665, message, 1, 0x74e5c5);
    }
    if let Some(error) = &draft.error {
        text(scene, 24, 690, error, 1, 0xff8e8e);
    }
    Ok(())
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
    let status = if game.cancelling && !game.joined {
        "STOPPING"
    } else if game.joined {
        "RESULTS - ENTER RETURNS TO SELECTION"
    } else {
        match &snapshot.status {
            player::PlayerStatus::Loading => "LOADING",
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
