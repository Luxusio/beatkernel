//! Opt-in native loading evidence; each trial runs in a fresh process.
//! Current-only instrumentation separates renderer submission from admission.
//! Preserved-binary comparisons observe owned X11 windows externally and report
//! upper bounds. Both routes use actual X11 input rather than direct keys.
#![cfg(target_os = "linux")]
use super::*;
use std::{fs, process::Command, sync::mpsc};
use winit::platform::x11::EventLoopBuilderExtX11;

const TEST: &str = "desktop::loading_measurement_fixtures::actual_native_loading_current_five_fresh_process_trials";
const WAIT: Duration = Duration::from_secs(40);

fn hash(path: &std::path::Path) -> String {
    let output = Command::new("sha256sum").arg(path).output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}
fn json_string(value: &str) -> String {
    use std::fmt::Write;
    let mut quoted = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            character if character <= '\u{1f}' => {
                write!(&mut quoted, "\\u{:04x}", u32::from(character)).unwrap()
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}
fn xdo(args: &[&str]) -> Result<String, String> {
    let output = Command::new("timeout")
        .args(["5s", "xdotool"])
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}
fn capture(directory: &std::path::Path, name: &str, extent: [u32; 2]) {
    let helper = std::env::var_os("BEATKERNEL_TEST_NATIVE_UI_CAPTURE")
        .expect("owned X11 capture helper required");
    let path = directory.join(format!("{name}.png"));
    let status = Command::new("timeout")
        .arg("5s")
        .arg(helper)
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success(), "owned window capture failed");
    let image = image::open(&path).unwrap().to_rgba8();
    assert!(image.width() >= extent[0] && image.height() >= extent[1]);
    assert!(
        image
            .pixels()
            .any(|pixel| pixel.0[..3].iter().any(|channel| *channel != 0)),
        "actual window capture is black"
    );
}

enum DriverEvent {
    KeyRequested(Instant),
    ResizeRequested(Instant),
    Finished(Result<(), String>),
}
fn driver(
    title: String,
    start: mpsc::Receiver<()>,
    events: mpsc::Sender<DriverEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        if start.recv_timeout(WAIT).is_err() {
            return;
        }
        let run = || -> Result<(), String> {
            let matches = xdo(&["search", "--name", &format!("^{title}$")])?;
            let id = matches
                .lines()
                .next()
                .ok_or("owned window title was not found")?;
            xdo(&["windowfocus", "--sync", id])?;
            events
                .send(DriverEvent::KeyRequested(Instant::now()))
                .map_err(|error| error.to_string())?;
            xdo(&["key", "--clearmodifiers", "Down"])?;
            events
                .send(DriverEvent::ResizeRequested(Instant::now()))
                .map_err(|error| error.to_string())?;
            xdo(&["windowsize", "--sync", id, "900", "680"])?;
            Ok(())
        };
        let result = run();
        let _ = events.send(DriverEvent::Finished(result));
    })
}

struct Trial {
    app: Desktop,
    origin: Instant,
    directory: PathBuf,
    gate: Option<mpsc::SyncSender<()>>,
    font_entered: mpsc::Receiver<()>,
    start_driver: Option<mpsc::SyncSender<()>>,
    driver_events: mpsc::Receiver<DriverEvent>,
    selectable: Option<Instant>,
    first_frame: Option<Instant>,
    key_requested: Option<Instant>,
    key_delivered: Option<Instant>,
    resize_requested: Option<Instant>,
    resize_delivered: Option<Instant>,
    resized_frame: Option<Instant>,
    font_released: Option<Instant>,
    font_ready: Option<Instant>,
    final_frame: Option<Instant>,
    driver_finished: bool,
    entered: bool,
    original_selection: Option<PathBuf>,
    selected_after_key: Option<PathBuf>,
    submission_before_resize: u64,
    submission_before_font: u64,
}
impl Trial {
    fn selected(&self) -> Option<PathBuf> {
        self.app
            .catalog_search
            .selected()
            .map(|index| self.app.entries[index].path.clone())
    }
    fn submissions(&self) -> u64 {
        self.app
            .renderer
            .as_ref()
            .map_or(0, |renderer| renderer.presentation_count())
    }
    fn progress(&mut self, event_loop: &ActiveEventLoop) {
        assert!(self.origin.elapsed() < WAIT, "owned native trial timed out");
        assert!(
            self.app.fatal.is_none(),
            "actual Desktop failed: {:?}",
            self.app.fatal
        );
        while let Ok(event) = self.driver_events.try_recv() {
            match event {
                DriverEvent::KeyRequested(at) => self.key_requested = Some(at),
                DriverEvent::ResizeRequested(at) => self.resize_requested = Some(at),
                DriverEvent::Finished(result) => {
                    result.unwrap();
                    self.driver_finished = true;
                }
            }
        }
        if !self.entered && self.font_entered.try_recv().is_ok() {
            self.entered = true;
        }
        if self.selectable.is_none() && !self.app.entries.is_empty() {
            self.selectable = Some(Instant::now());
        }
        if self.first_frame.is_some() && self.entered && self.app.entries.len() >= 2 {
            if let Some(start) = self.start_driver.take() {
                assert!(self.app.title_font.is_none());
                assert!(self
                    .app
                    .catalog
                    .as_ref()
                    .is_some_and(|job| !job.is_finished()));
                self.original_selection = self.selected();
                start.send(()).unwrap();
            }
        }
        if self.key_delivered.is_some()
            && self.resize_delivered.is_some()
            && self.resized_frame.is_some()
            && self.driver_finished
            && self.font_released.is_none()
        {
            assert!(
                self.app.title_font.is_none(),
                "font gate must still hold during real input/resize"
            );
            assert_ne!(self.original_selection, self.selected_after_key);
            capture(&self.directory, "held-font-input-resized", [900, 680]);
            self.submission_before_font = self.submissions();
            self.font_released = Some(Instant::now());
            self.gate.take().unwrap().send(()).unwrap();
        }
        if self.font_released.is_some()
            && self.font_ready.is_none()
            && self.app.title_font.is_some()
            && self.app.catalog.is_none()
        {
            self.font_ready = Some(Instant::now());
            assert_eq!(self.selected(), self.selected_after_key);
            self.app.window.as_ref().unwrap().request_redraw();
        }
        if self.final_frame.is_some() {
            capture(&self.directory, "font-completed", [900, 680]);
            self.app.request_close();
            // Production exit owns explicit cancellation/join; no unfinished
            // catalog is detached by this fixture's ordinary polling path.
            event_loop.exit();
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(4),
            ));
        }
    }
}
impl ApplicationHandler<DesktopUiCommand> for Trial {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.app.resumed(event_loop);
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: DesktopUiCommand) {
        self.app.user_event(event_loop, event);
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let owned = self
            .app
            .window
            .as_ref()
            .is_some_and(|window| window.id() == id);
        let key = owned
            && matches!(&event, WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed && event.physical_key == PhysicalKey::Code(KeyCode::ArrowDown));
        let resize = owned
            && matches!(&event, WindowEvent::Resized(size) if [size.width, size.height] == [900, 680]);
        let redraw = owned && matches!(&event, WindowEvent::RedrawRequested);
        let before = self.submissions();
        let delivered = Instant::now();
        self.app.window_event(event_loop, id, event);
        if key && self.key_delivered.is_none() {
            assert!(
                self.font_released.is_none(),
                "input was delivered after releasing the held preparation"
            );
            self.key_delivered = Some(delivered);
            self.selected_after_key = self.selected();
            self.submission_before_resize = self.submissions();
        }
        if resize && self.resize_delivered.is_none() {
            self.resize_delivered = Some(delivered);
        }
        let after = self.submissions();
        if redraw && after > before {
            if self.first_frame.is_none()
                && !self.app.entries.is_empty()
                && self.app.hits.iter().any(|(id, _)| id.0 >= 100)
            {
                self.first_frame = Some(Instant::now());
            }
            if self.resize_delivered.is_some()
                && self.resized_frame.is_none()
                && after > self.submission_before_resize
            {
                self.resized_frame = Some(Instant::now());
            }
            if self.font_ready.is_some() && after > self.submission_before_font {
                self.final_frame = Some(Instant::now());
            }
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.app.about_to_wait(event_loop);
        self.progress(event_loop);
    }
    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(gate) = self.gate.take() {
            let _ = gate.send(());
        }
        self.app.exiting(event_loop);
    }
}
impl Drop for Trial {
    fn drop(&mut self) {
        if let Some(gate) = self.gate.take() {
            let _ = gate.send(());
        }
        self.app.request_close();
        self.app.join_catalog_cleanup();
    }
}

#[test]
#[ignore = "requires owned X11/software GPU, xdotool, timeout, sha256sum and direct PNG capture helper"]
#[allow(deprecated)]
fn actual_native_loading_current_five_fresh_process_trials() {
    assert_eq!(
        std::env::var("BEATKERNEL_TEST_NATIVE_UI_WINDOW").as_deref(),
        Ok("1")
    );
    let artifacts = PathBuf::from(
        std::env::var_os("BEATKERNEL_TEST_NATIVE_UI_ARTIFACT_DIR")
            .expect("artifact directory required"),
    );
    fs::create_dir_all(&artifacts).unwrap();
    let executable = std::env::current_exe().unwrap();
    let Some(trial) = std::env::var("BEATKERNEL_TEST_NATIVE_LOADING_TRIAL").ok() else {
        for trial in 0..5 {
            let output = Command::new("timeout")
                .arg("60s")
                .arg(&executable)
                .args([
                    TEST,
                    "--exact",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("BEATKERNEL_TEST_NATIVE_LOADING_TRIAL", trial.to_string())
                .output()
                .unwrap();
            fs::write(
                artifacts.join(format!("trial-{trial}.stdout.log")),
                &output.stdout,
            )
            .unwrap();
            fs::write(
                artifacts.join(format!("trial-{trial}.stderr.log")),
                &output.stderr,
            )
            .unwrap();
            assert!(
                output.status.success(),
                "fresh native trial {trial} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        eprintln!("Five fresh-process native current-only trials completed; raw records are separate from baseline-comparable loading_report input.");
        return;
    };
    let trial: u32 = trial.parse().unwrap();
    assert!(trial < 5);
    let directory = artifacts.join(format!("trial-{trial}"));
    fs::create_dir_all(&directory).unwrap();
    let library = directory.join("library");
    fs::create_dir_all(&library).unwrap();
    fs::write(
        library.join("a.bms"),
        "#TITLE ALPHA\n#BPM 120\n#WAV01 absent.wav\n#00011:01\n",
    )
    .unwrap();
    fs::write(
        library.join("z.bms"),
        "#TITLE ZULU\n#BPM 120\n#WAV01 absent.wav\n#00011:01\n",
    )
    .unwrap();
    let font = directory.join("original-fixture.ttf");
    fs::write(&font, font_fixture::font_bytes()).unwrap();
    let setup = Instant::now();
    let event_loop = EventLoop::<DesktopUiCommand>::with_user_event()
        .with_x11()
        .with_any_thread(true)
        .build()
        .unwrap();
    let title = format!("BeatKernel loading measurement trial {trial}");
    let window = Arc::new(
        event_loop
            .create_window(
                Window::default_attributes()
                    .with_title(&title)
                    .with_decorations(false)
                    .with_position(winit::dpi::LogicalPosition::new(0.0, 0.0))
                    .with_inner_size(LogicalSize::new(960.0, 720.0)),
            )
            .unwrap(),
    );
    let size = window.inner_size();
    assert_eq!(
        [size.width, size.height],
        [960, 720],
        "owned display must use scale one"
    );
    let instance = graphics::instance(BackendChoice::Auto).unwrap();
    let surface = instance.create_surface(window.clone()).unwrap();
    let mut renderer =
        pollster::block_on(Renderer::new(surface, &instance, Presentation::Fifo)).unwrap();
    renderer.resize(960, 720).unwrap();
    let renderer_description = renderer.description().to_owned();
    let renderer_setup_ns = setup.elapsed().as_nanos();
    let mut app = tests::lifecycle_fixture();
    app.options.library = Some(library.clone());
    app.entries.clear();
    app.selection_items = Arc::from([]);
    app.catalog_search = CatalogSearch::new(&[]).unwrap();
    app.window = Some(window);
    app.renderer = Some(renderer);
    app.instance = Some(instance);
    app.catalog_generation = 1;
    app.catalog_screen = app.navigator.active_id();
    let (entered, font_entered) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let origin = Instant::now();
    app.catalog = Some(
        NativeCatalog::spawn_streamed(library.clone(), 1, move |library, control| {
            entered.send(()).unwrap();
            gate.recv_timeout(WAIT).map_err(|error| error.to_string())?;
            prepare_catalog(library, Some(font), Vec::new(), control)
        })
        .unwrap(),
    );
    let (start, start_receiver) = mpsc::sync_channel(1);
    let (events, driver_events) = mpsc::channel();
    let driver = driver(title, start_receiver, events);
    let mut observer = Trial {
        app,
        origin,
        directory: directory.clone(),
        gate: Some(release),
        font_entered,
        start_driver: Some(start),
        driver_events,
        selectable: None,
        first_frame: None,
        key_requested: None,
        key_delivered: None,
        resize_requested: None,
        resize_delivered: None,
        resized_frame: None,
        font_released: None,
        font_ready: None,
        final_frame: None,
        driver_finished: false,
        entered: false,
        original_selection: None,
        selected_after_key: None,
        submission_before_resize: 0,
        submission_before_font: 0,
    };
    event_loop.run_app(&mut observer).unwrap();
    driver.join().unwrap();
    let elapsed = |at: Option<Instant>| {
        at.expect("actual event must be observed")
            .duration_since(origin)
            .as_nanos()
    };
    let selectable = elapsed(observer.selectable);
    let usable = elapsed(observer.first_frame);
    let key_requested = elapsed(observer.key_requested);
    let key_delivered = elapsed(observer.key_delivered);
    let resize_requested = elapsed(observer.resize_requested);
    let resize_delivered = elapsed(observer.resize_delivered);
    let font_released = elapsed(observer.font_released);
    assert!(selectable <= usable && usable <= key_requested && key_requested <= key_delivered);
    assert!(
        resize_requested <= resize_delivered
            && resize_delivered < font_released
            && key_delivered < font_released
    );
    assert!(observer.app.catalog.is_none() && observer.app.retired_catalogs.is_empty());
    let source = std::env::var("BEATKERNEL_TEST_NATIVE_UI_SOURCE_SHA")
        .ok()
        .map(|value| json_string(&value))
        .unwrap_or_else(|| "null".into());
    let raw = format!(
        r#"{{
  "schema": "beatkernel-native-connected-loading-current/v1",
  "variant": "current-only", "temperature": "cold", "trial": {trial},
  "process_id": {process}, "library_id": {library}, "cache_owner_reused": false,
  "executable_sha256": "{executable_hash}", "source_provenance": {source},
  "fixture_sha256": {{"a.bms": "{a_hash}", "z.bms": "{z_hash}", "font": "{font_hash}"}},
  "renderer": {renderer}, "renderer_setup_ns": {renderer_setup_ns},
  "events_ns": {{"request": 0, "catalog_selectable": {selectable}, "first_submitted_usable": {usable},
    "key_requested": {key_requested}, "key_delivered": {key_delivered},
    "resize_requested": {resize_requested}, "resize_delivered": {resize_delivered},
    "resized_submitted": {resized}, "font_gate_released": {font_released}, "font_ready": {font_ready}, "font_submitted": {font_submitted}}},
  "baseline_comparison": "unavailable", "prepared_ready": null, "audio_ready": null,
  "limits": "X11 injected input and submitted software frames; no physical input/audio/display latency or native before/after speedup claim"
}}"#,
        process = std::process::id(),
        library = json_string(library.to_str().unwrap()),
        executable_hash = hash(&executable),
        a_hash = hash(&directory.join("library/a.bms")),
        z_hash = hash(&directory.join("library/z.bms")),
        font_hash = hash(&directory.join("original-fixture.ttf")),
        renderer = json_string(&renderer_description),
        resized = elapsed(observer.resized_frame),
        font_ready = elapsed(observer.font_ready),
        font_submitted = elapsed(observer.final_frame)
    );
    fs::write(directory.join("events.json"), raw).unwrap();
    eprintln!(
        "Native current-only raw trial: {}",
        directory.join("events.json").display()
    );
}

// Test-owned ctypes boundary. The helper never discovers/focuses/captures a
// window unless its _NET_WM_PID equals the explicitly armed child process.
// Per-poll acquisition is a tiny XGetImage; PNG encoding runs only after proof.
const X11_CATALOG_OBSERVER: &str = r#"
import ctypes as c, ctypes.util, hashlib, json, os, struct, sys, time, zlib
x=c.CDLL(ctypes.util.find_library('X11'))
t=c.CDLL(ctypes.util.find_library('Xtst'))
U=c.c_ulong; I=c.c_int; P=c.c_void_p
class Attributes(c.Structure):
    _fields_=[('x',I),('y',I),('width',I),('height',I),('border_width',I),('depth',I),('visual',P),('root',U),('class_',I),('bit_gravity',I),('win_gravity',I),('backing_store',I),('backing_planes',U),('backing_pixel',U),('save_under',I),('colormap',U),('map_installed',I),('map_state',I),('all_event_masks',c.c_long),('your_event_mask',c.c_long),('do_not_propagate_mask',c.c_long),('override_redirect',I),('screen',P)]
class Image(c.Structure):
    _fields_=[('width',I),('height',I),('xoffset',I),('format',I),('data',P),('byte_order',I),('bitmap_unit',I),('bitmap_bit_order',I),('bitmap_pad',I),('depth',I),('bytes_per_line',I),('bits_per_pixel',I),('red_mask',U),('green_mask',U),('blue_mask',U),('obdata',P)]
class XError(c.Structure):
    _fields_=[('type',I),('display',P),('resourceid',U),('serial',U),('error_code',c.c_ubyte),('request_code',c.c_ubyte),('minor_code',c.c_ubyte)]
def bind(lib,name,result,args):
    f=getattr(lib,name); f.restype=result; f.argtypes=args; return f
open_display=bind(x,'XOpenDisplay',P,[c.c_char_p]); close_display=bind(x,'XCloseDisplay',I,[P])
root_window=bind(x,'XDefaultRootWindow',U,[P]); atom=bind(x,'XInternAtom',U,[P,c.c_char_p,I])
query=bind(x,'XQueryTree',I,[P,U,c.POINTER(U),c.POINTER(U),c.POINTER(c.POINTER(U)),c.POINTER(c.c_uint)])
property_=bind(x,'XGetWindowProperty',I,[P,U,U,c.c_long,c.c_long,I,U,c.POINTER(U),c.POINTER(I),c.POINTER(U),c.POINTER(U),c.POINTER(P)])
free=bind(x,'XFree',I,[P]); attrs=bind(x,'XGetWindowAttributes',I,[P,U,c.POINTER(Attributes)])
get_image=bind(x,'XGetImage',P,[P,U,I,I,c.c_uint,c.c_uint,U,I]); get_pixel=bind(x,'XGetPixel',U,[P,I,I]); destroy_image=bind(x,'XDestroyImage',I,[P])
focus=bind(x,'XSetInputFocus',I,[P,U,I,U]); sync=bind(x,'XSync',I,[P,I]); flush=bind(x,'XFlush',I,[P])
keysym=bind(x,'XStringToKeysym',U,[c.c_char_p]); keycode=bind(x,'XKeysymToKeycode',c.c_ubyte,[P,U])
fake_key=bind(t,'XTestFakeKeyEvent',I,[P,c.c_uint,I,U])
ErrorHandler=c.CFUNCTYPE(I,P,c.POINTER(XError))
set_error_handler=bind(x,'XSetErrorHandler',P,[ErrorHandler])
d=open_display(None)
if not d: raise RuntimeError('owned X11 DISPLAY is unavailable')
errors=[]; discovering=False
@ErrorHandler
def record_error(display,event):
    # C callbacks only record scalar data. Refusal/skip happens on Python's
    # ordinary stack after synchronization, never by raising through C.
    error=event.contents
    errors.append((error.error_code,error.request_code,error.resourceid,error.serial,discovering))
    return 0
set_error_handler(record_error)
def take_errors():
    pending=errors[:]; errors.clear(); return pending
def strict_errors():
    sync(d,0)
    pending=take_errors()
    if pending: raise RuntimeError('X11 operation refused: '+repr(pending))
def discovery_call(w,operation):
    global discovering
    # Any earlier error remains fatal, even if the next operation is discovery.
    strict_errors()
    discovering=True
    try:
        result=operation(); sync(d,0)
    finally: discovering=False
    pending=take_errors()
    if pending:
        if all(code==3 and request in (3,15,20) and resource==w and scoped for code,request,resource,serial,scoped in pending):
            return None,True  # A visited window vanished during discovery only.
        raise RuntimeError('X11 discovery operation refused: '+repr(pending))
    return result,False
pid=None; window=None; origin=None; pid_atom=atom(d,b'_NET_WM_PID',0)
def owner(w):
    typ=U(); fmt=I(); count=U(); after=U(); data=P()
    property_(d,w,pid_atom,0,1,0,0,c.byref(typ),c.byref(fmt),c.byref(count),c.byref(after),c.byref(data))
    try: return c.cast(data,c.POINTER(U))[0] if data.value and fmt.value==32 and count.value==1 else None
    finally:
        if data.value: free(data)
def owned():
    global window
    if window is not None:
        current_owner=owner(window); strict_errors()
        if current_owner!=pid: raise RuntimeError('owned window PID changed')
        return window
    pending=[root_window(d)]; examined=0
    while pending:
        w=pending.pop(); examined+=1
        if examined>4096: raise RuntimeError('X11 window inventory limit exceeded')
        current_owner,stale=discovery_call(w,lambda: owner(w))
        if stale: continue
        if current_owner==pid:
            a=Attributes()
            admitted,stale=discovery_call(w,lambda: attrs(d,w,c.byref(a)))
            if stale: continue
            if admitted and a.map_state==2:
                window=w; return w
        root=U(); parent=U(); children=c.POINTER(U)(); count=c.c_uint()
        admitted,stale=discovery_call(w,lambda: query(d,w,c.byref(root),c.byref(parent),c.byref(children),c.byref(count)))
        if stale: continue
        if admitted:
            try: pending.extend(children[i] for i in range(count.value))
            finally:
                if children: free(c.cast(children,P))
    return None
def now(): return time.monotonic_ns()-origin
def sample():
    start=now(); w=owned()
    if w is None: return ['sample',start,now(),0,0,0,0,0,pid]
    a=Attributes(); attrs(d,w,c.byref(a)); strict_errors()
    if (a.width,a.height)!=(960,720): raise RuntimeError('owned window must be 960x720 at scale one')
    image=get_image(d,w,18,134,4,64,U(-1).value,2)
    if not image: raise RuntimeError('owned window framebuffer unavailable')
    try: values=[get_pixel(image,2,2),get_pixel(image,2,36)]
    finally: destroy_image(image)
    strict_errors()
    return ['sample',start,now(),w,*values,a.width,a.height,pid]
def key(name):
    w=owned()
    if w is None: raise RuntimeError('no mapped window owned by child')
    focus(d,w,2,0); strict_errors()
    code=keycode(d,keysym(name.encode('ascii')))
    if not code: raise RuntimeError('X11 keysym has no keycode')
    at=now()
    if not fake_key(d,code,1,0) or not fake_key(d,code,0,0): raise RuntimeError('XTEST input refused')
    flush(d); strict_errors()
    return ['input',at,name]
def png(path):
    start=now(); w=owned(); a=Attributes(); attrs(d,w,c.byref(a)); strict_errors()
    image=get_image(d,w,0,0,a.width,a.height,U(-1).value,2)
    if not image: raise RuntimeError('owned screenshot acquisition refused')
    try:
        header=c.cast(image,c.POINTER(Image)).contents
        if header.bits_per_pixel!=32 or header.byte_order!=0 or (header.red_mask,header.green_mask,header.blue_mask)!=(0xff0000,0xff00,0xff): raise RuntimeError('PNG fixture requires ordinary little-endian X11 TrueColor32')
        payload=c.string_at(header.data,header.bytes_per_line*a.height); rows=[]
        for y in range(a.height):
            data=payload[y*header.bytes_per_line:y*header.bytes_per_line+a.width*4]
            rgb=bytearray(a.width*3); rgb[0::3]=data[2::4]; rgb[1::3]=data[1::4]; rgb[2::3]=data[0::4]
            rows.append(b'\0'+rgb)
    finally: destroy_image(image)
    strict_errors()
    def chunk(name,data): return struct.pack('>I',len(data))+name+data+struct.pack('>I',zlib.crc32(name+data)&0xffffffff)
    with open(path,'wb') as f: f.write(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',a.width,a.height,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(b''.join(rows)))+chunk(b'IEND',b''))
    return ['captured',start,now(),w,pid]
log_file=None; log_pending=b''; log_bytes=0; markers=[]
def logs():
    global log_pending,log_bytes
    start=now()
    data=log_file.read(65536); log_bytes+=len(data)
    if log_bytes>1048576: raise RuntimeError('production stdout exceeded bounded observation limit')
    log_pending+=data
    if len(log_pending)>65536: raise RuntimeError('production stdout line exceeded observation limit')
    while b'\n' in log_pending:
        line,log_pending=log_pending.split(b'\n',1)
        if b'prepared practice section=' in line:
            if not line.startswith(b'prepared practice section=') or line.count(b'prepared practice section=')!=1: raise RuntimeError('ambiguous preparation marker')
            markers.append((now(),line.hex()))
            if len(markers)>2: raise RuntimeError('unexpected extra preparation marker')
    return ['logs',start,now(),len(markers),';'.join(str(at)+':'+line for at,line in markers)]
def matches(args):
    start=now(); w=owned()
    if w is None: return ['match',start,now(),0,pid,0]
    left,top,width,height=map(int,args[:4]); mask=args[4]
    if not (0<width<=960 and 0<height<=32 and len(mask)==width*height and set(mask)<={'0','1'}): raise RuntimeError('invalid bounded text probe')
    a=Attributes(); attrs(d,w,c.byref(a)); strict_errors()
    if (a.width,a.height)!=(960,720): raise RuntimeError('text probe requires owned scale-one window')
    image=get_image(d,w,left,top,width,height,U(-1).value,2)
    if not image: raise RuntimeError('owned text framebuffer unavailable')
    try:
        matched=True
        for y in range(height):
            for col in range(width):
                pixel=get_pixel(image,col,y); red=(pixel>>16)&255; green=(pixel>>8)&255; blue=pixel&255
                foreground=red>green+15 and red>blue+15 and green>30
                if foreground!=(mask[y*width+col]=='1'): matched=False
    finally: destroy_image(image)
    strict_errors()
    return ['match',start,now(),w,pid,int(matched)]
try:
    for line in sys.stdin:
        command,*args=line.rstrip('\n').split('\t')
        try:
            if command=='begin': origin=time.monotonic_ns(); reply=['begin',origin]
            elif command=='arm': pid=int(args[0]); window=None; reply=['armed',pid]
            elif command=='sample': reply=sample()
            elif command=='key': reply=key(args[0])
            elif command=='capture': reply=png(args[0])
            elif command=='watch':
                if log_file is not None: raise RuntimeError('stdout observer already attached')
                log_file=open(args[0],'rb'); reply=['watching',now()]
            elif command=='logs': reply=logs()
            elif command=='match': reply=matches(args)
            elif command=='quit': print('quit',flush=True); break
            else: raise RuntimeError('unknown observer command')
            print('\t'.join(map(str,reply)),flush=True)
        except Exception as e: print('error\t'+repr(e),flush=True)
finally:
    if log_file is not None: log_file.close()
    close_display(d)
"#;

struct OwnedProcess(std::process::Child);
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
struct FrameObserver {
    process: OwnedProcess,
    input: std::process::ChildStdin,
    replies: mpsc::Receiver<String>,
    reader: Option<thread::JoinHandle<()>>,
}
impl FrameObserver {
    fn spawn(helper: &std::path::Path, directory: &std::path::Path) -> Self {
        use std::io::{BufRead, BufReader};
        let mut child = Command::new("python3")
            .args(["-u"])
            .arg(helper)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(fs::File::create(directory.join("observer.stderr.log")).unwrap())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (sender, replies) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                match line {
                    Ok(line) => {
                        if sender.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            process: OwnedProcess(child),
            input,
            replies,
            reader: Some(reader),
        }
    }
    fn command(&mut self, command: &str) -> (Vec<String>, u128) {
        use std::io::Write;
        let started = Instant::now();
        writeln!(self.input, "{command}").unwrap();
        self.input.flush().unwrap();
        let reply = self
            .replies
            .recv_timeout(Duration::from_secs(5))
            .expect("owned framebuffer helper exceeded response deadline");
        assert!(!reply.starts_with("error\t"), "{reply}");
        (
            reply.split('\t').map(str::to_owned).collect(),
            started.elapsed().as_nanos(),
        )
    }
}
impl Drop for FrameObserver {
    fn drop(&mut self) {
        if self.process.0.try_wait().ok().flatten().is_none() {
            let _ = self.process.0.kill();
        }
        let _ = self.process.0.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
fn checked_binary(path_variable: &str, hash_variable: &str) -> (PathBuf, String) {
    let path = PathBuf::from(
        std::env::var_os(path_variable).expect("explicit preserved executable path required"),
    );
    let expected =
        std::env::var(hash_variable).expect("explicit preserved executable SHA256 required");
    assert!(expected.len() == 64 && expected.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(hash(&path), expected, "preserved executable drifted");
    (path, expected)
}

#[test]
#[ignore = "requires preserved baseline/current production executables and owned scale-one X11 TrueColor32/software GPU"]
fn preserved_native_catalog_five_paired_fresh_process_trials() {
    assert_eq!(
        std::env::var("BEATKERNEL_TEST_NATIVE_UI_WINDOW").as_deref(),
        Ok("1")
    );
    let artifacts = PathBuf::from(
        std::env::var_os("BEATKERNEL_TEST_NATIVE_UI_ARTIFACT_DIR")
            .expect("owned artifact directory required"),
    );
    fs::create_dir_all(&artifacts).unwrap();
    let (baseline, baseline_hash) = checked_binary(
        "BEATKERNEL_TEST_NATIVE_LOADING_BASELINE_EXE",
        "BEATKERNEL_TEST_NATIVE_LOADING_BASELINE_SHA",
    );
    let (current, current_hash) = checked_binary(
        "BEATKERNEL_TEST_NATIVE_LOADING_CURRENT_EXE",
        "BEATKERNEL_TEST_NATIVE_LOADING_CURRENT_SHA",
    );
    let helper = artifacts.join("owned-x11-catalog-observer.py");
    fs::write(&helper, X11_CATALOG_OBSERVER).unwrap();
    let library = artifacts.join("paired-catalog-library");
    fs::create_dir_all(&library).unwrap();
    fs::write(
        library.join("a.bms"),
        "#TITLE ALPHA\n#BPM 120\n#WAV01 absent.wav\n#00011:01\n",
    )
    .unwrap();
    fs::write(
        library.join("z.bms"),
        "#TITLE ZULU\n#BPM 120\n#WAV01 absent.wav\n#00011:01\n",
    )
    .unwrap();
    let font = artifacts.join("paired-original-fixture.ttf");
    fs::write(&font, font_fixture::font_bytes()).unwrap();
    let args = vec![
        "player".to_owned(),
        "--library".into(),
        library.to_str().unwrap().into(),
        "--title-font".into(),
        font.to_str().unwrap().into(),
        "--gpu-backend".into(),
        "vulkan".into(),
        "--present".into(),
        "fifo".into(),
        "--ui-fps".into(),
        "120".into(),
    ];
    let fixture_a = hash(&library.join("a.bms"));
    let fixture_z = hash(&library.join("z.bms"));
    let fixture_font = hash(&font);
    let observer_hash = hash(&helper);
    for trial in 0..5 {
        for (variant, executable, binary_hash) in [
            ("baseline", &baseline, &baseline_hash),
            ("current", &current, &current_hash),
        ] {
            let directory = artifacts.join(format!("paired-{variant}-{trial}"));
            fs::create_dir_all(&directory).unwrap();
            let mut observer = FrameObserver::spawn(&helper, &directory);
            observer.command("begin");
            let child = Command::new(executable)
                .args(&args)
                .stdout(fs::File::create(directory.join("player.stdout.log")).unwrap())
                .stderr(fs::File::create(directory.join("player.stderr.log")).unwrap())
                .spawn()
                .unwrap();
            let mut child = OwnedProcess(child);
            let pid = child.0.id();
            observer.command(&format!("arm\t{pid}"));
            let deadline = Instant::now() + Duration::from_secs(30);
            let mut probes = Vec::new();
            let mut window = 0u64;
            let mut sample = || -> (u64, [u64; 2]) {
                assert!(
                    Instant::now() < deadline,
                    "owned production catalog observation timed out"
                );
                assert!(
                    child.0.try_wait().unwrap().is_none(),
                    "owned production player exited before usability proof"
                );
                let (fields, roundtrip) = observer.command("sample");
                assert_eq!(fields[0], "sample");
                let number = |index: usize| fields[index].parse::<u64>().unwrap();
                assert_eq!(number(8), u64::from(pid));
                if number(3) != 0 {
                    if window == 0 {
                        window = number(3);
                    }
                    assert_eq!(window, number(3));
                }
                probes.push((number(1), number(2), roundtrip));
                (number(2), [number(4), number(5)])
            };
            let (candidate, original) = loop {
                let observation = sample();
                if observation.1[0] != observation.1[1]
                    && observation.1.iter().all(|pixel| *pixel != 0)
                {
                    break observation;
                }
                thread::sleep(Duration::from_millis(1));
            };
            drop(sample);
            let (down, _) = observer.command("key\tDown");
            let down_request = down[1].parse::<u64>().unwrap();
            let mut observe_match = |target: [u64; 2]| -> u64 {
                loop {
                    assert!(
                        Instant::now() < deadline,
                        "owned selection pixel transition timed out"
                    );
                    assert!(child.0.try_wait().unwrap().is_none());
                    let (fields, roundtrip) = observer.command("sample");
                    assert_eq!(fields[0], "sample");
                    let number = |index: usize| fields[index].parse::<u64>().unwrap();
                    assert_eq!(number(3), window);
                    assert_eq!(number(8), u64::from(pid));
                    probes.push((number(1), number(2), roundtrip));
                    if [number(4), number(5)] == target {
                        return number(2);
                    }
                    thread::sleep(Duration::from_millis(1));
                }
            };
            let changed = observe_match([original[1], original[0]]);
            drop(observe_match);
            let (up, _) = observer.command("key\tUp");
            let up_request = up[1].parse::<u64>().unwrap();
            let restored = loop {
                assert!(Instant::now() < deadline);
                assert!(child.0.try_wait().unwrap().is_none());
                let (fields, roundtrip) = observer.command("sample");
                let number = |index: usize| fields[index].parse::<u64>().unwrap();
                assert_eq!(fields[0], "sample");
                assert_eq!(number(3), window);
                assert_eq!(number(8), u64::from(pid));
                probes.push((number(1), number(2), roundtrip));
                if [number(4), number(5)] == original {
                    break number(2);
                }
                thread::sleep(Duration::from_millis(1));
            };
            assert!(
                candidate <= down_request
                    && down_request <= changed
                    && changed <= up_request
                    && up_request <= restored
            );
            let (capture, _) = observer.command(&format!(
                "capture\t{}",
                directory.join("confirmed-owned-window.png").display()
            ));
            observer.command("key\tEscape");
            observer.command("quit");
            let cleanup_deadline = Instant::now() + Duration::from_secs(8);
            let status = loop {
                if let Some(status) = child.0.try_wait().unwrap() {
                    break status;
                }
                assert!(
                    Instant::now() < cleanup_deadline,
                    "owned player did not exit after Escape"
                );
                thread::sleep(Duration::from_millis(1));
            };
            assert!(status.success());
            let probe_json = probes
                .iter()
                .map(|(start, end, roundtrip)| {
                    format!(
                        "{{\"start_ns\":{start},\"end_ns\":{end},\"round_trip_ns\":{roundtrip}}}"
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let config = args
                .iter()
                .map(|arg| json_string(arg))
                .collect::<Vec<_>>()
                .join(",");
            let raw = format!(
                r#"{{
 "schema":"beatkernel-native-external-catalog-paired/v1","variant":"{variant}","trial":{trial},"temperature":"fresh-process-cache","process_id":{pid},"owned_window_id":{window},
 "executable_sha256":"{binary_hash}","observer_sha256":"{observer_hash}","fixture_sha256":{{"a.bms":"{fixture_a}","z.bms":"{fixture_z}","font":"{fixture_font}"}},"config":[{config}],
 "clock":"Python time.monotonic_ns, one helper origin before actual child spawn","events_ns":{{"request":0,"candidate_selected_frame_observed":{candidate},"down_requested":{down_request},"down_changed_frame_observed":{changed},"up_requested":{up_request},"first_proven_usable_frame_observed_upper_bound":{restored}}},
 "input_response_upper_bound_ns":{{"Down":{down_latency},"Up":{up_latency}}},"sampling_interval_configured_ns":1000000,"sampling":[{probe_json}],"capture_cost_ns":{capture_cost},
 "renderer_submission_time":null,"prepared_ready":null,"audio_ready":null,"os_page_cache_controlled":false,
 "limits":"Catalog-only observed window upper bounds include process launch, input probes, sampling and observer overhead; no native playable/section/replay/replacement or physical latency claim"
}}"#,
                down_latency = changed - down_request,
                up_latency = restored - up_request,
                capture_cost =
                    capture[2].parse::<u64>().unwrap() - capture[1].parse::<u64>().unwrap()
            );
            fs::write(directory.join("events.json"), raw).unwrap();
            drop(observer);
            drop(child);
        }
    }
}

// Match the complete original bitmap caption, including its background holes.
// This observes production pixels; it does not call Desktop readiness helpers.
fn error_text_probe(text: &str, y: usize, scale: usize) -> String {
    let width = text.len() * 6 * scale;
    assert!(24 + width <= 960);
    let mut mask = vec![b'0'; width * 7 * scale];
    let atlas = beatkernel_bms_runtime::font::atlas().unwrap();
    for (index, character) in text.chars().enumerate() {
        let code = character.to_ascii_uppercase() as usize;
        assert!(code < 128);
        for row in 0..7 {
            for column in 0..5 {
                let x = code % 16 * 8 + column;
                let y = code / 16 * 8 + row;
                if atlas.pixels()[(y * 128 + x) * 4 + 3] != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            mask[(row * scale + dy) * width
                                + index * 6 * scale
                                + column * scale
                                + dx] = b'1';
                        }
                    }
                }
            }
        }
    }
    format!(
        "match\t24\t{y}\t{width}\t{}\t{}",
        7 * scale,
        String::from_utf8(mask).unwrap()
    )
}

fn observed_text(
    observer: &mut FrameObserver,
    command: &str,
    pid: u32,
    window: u64,
) -> (u64, bool, u128) {
    let (reply, cost) = observer.command(command);
    assert_eq!(reply[0], "match");
    assert_eq!(reply[3].parse::<u64>().unwrap(), window);
    assert_eq!(reply[4].parse::<u32>().unwrap(), pid);
    (reply[2].parse().unwrap(), reply[5] == "1", cost)
}

#[test]
#[ignore = "requires preserved baseline/current Linux production executables, ALSA null and owned scale-one X11/software GPU"]
fn preserved_native_preparation_five_paired_fresh_process_trials() {
    use std::io::Write;
    assert_eq!(
        std::env::var("BEATKERNEL_TEST_NATIVE_UI_WINDOW").as_deref(),
        Ok("1")
    );
    let artifacts = PathBuf::from(
        std::env::var_os("BEATKERNEL_TEST_NATIVE_UI_ARTIFACT_DIR")
            .expect("owned artifact directory required"),
    );
    fs::create_dir_all(&artifacts).unwrap();
    let (baseline, baseline_hash) = checked_binary(
        "BEATKERNEL_TEST_NATIVE_LOADING_BASELINE_EXE",
        "BEATKERNEL_TEST_NATIVE_LOADING_BASELINE_SHA",
    );
    let (current, current_hash) = checked_binary(
        "BEATKERNEL_TEST_NATIVE_LOADING_CURRENT_EXE",
        "BEATKERNEL_TEST_NATIVE_LOADING_CURRENT_SHA",
    );
    let helper = artifacts.join("owned-x11-preparation-observer.py");
    fs::write(&helper, X11_CATALOG_OBSERVER).unwrap();
    let library = artifacts.join("paired-preparation-library");
    fs::create_dir_all(&library).unwrap();
    // Four logical aliases of one actual 16 MiB, non-silent mono PCM16 WAV.
    // Decoded logical PCM is 128 MiB, inside unchanged native admission limits.
    let wav = library.join("large.wav");
    let payload_bytes = 16u32 * 1024 * 1024;
    let mut output = fs::File::create(&wav).unwrap();
    output.write_all(b"RIFF").unwrap();
    output
        .write_all(&(36 + payload_bytes).to_le_bytes())
        .unwrap();
    output.write_all(b"WAVEfmt ").unwrap();
    output.write_all(&16u32.to_le_bytes()).unwrap();
    output.write_all(&1u16.to_le_bytes()).unwrap();
    output.write_all(&1u16.to_le_bytes()).unwrap();
    output.write_all(&44100u32.to_le_bytes()).unwrap();
    output.write_all(&88200u32.to_le_bytes()).unwrap();
    output.write_all(&2u16.to_le_bytes()).unwrap();
    output.write_all(&16u16.to_le_bytes()).unwrap();
    output.write_all(b"data").unwrap();
    output.write_all(&payload_bytes.to_le_bytes()).unwrap();
    let block: Vec<u8> = (0..32768i32)
        .flat_map(|sample| (((sample % 511) - 255) as i16).to_le_bytes())
        .collect();
    for _ in 0..payload_bytes as usize / block.len() {
        output.write_all(&block).unwrap();
    }
    drop(output);
    fs::write(library.join("a.bms"), "#TITLE ALPHA\n#BPM 120\n#WAV01 large.wav\n#WAV02 large.wav\n#WAV03 large.wav\n#WAV04 large.wav\n#00011:01020304\n").unwrap();
    fs::write(
        library.join("z.bms"),
        "#TITLE ZULU\n#BPM 120\n#WAV01 large.wav\n#00011:01\n",
    )
    .unwrap();
    let font = artifacts.join("paired-preparation-font.ttf");
    fs::write(&font, font_fixture::font_bytes()).unwrap();
    let args: Vec<String> = vec![
        "player",
        "--library",
        library.to_str().unwrap(),
        "--title-font",
        font.to_str().unwrap(),
        "--gpu-backend",
        "vulkan",
        "--present",
        "fifo",
        "--ui-fps",
        "120",
        "--evdev",
        "/dev/null",
        "--alsa",
        "null",
        "--rate",
        "44100",
        "--channels",
        "1",
        "--channel-policy",
        "exact",
        "--period-frames",
        "256",
        "--buffer-frames",
        "1024",
        "--bind",
        "11:04",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let fixture_hashes = format!(
        "{{\"a.bms\":{},\"z.bms\":{},\"large.wav\":{},\"font\":{}}}",
        json_string(&hash(&library.join("a.bms"))),
        json_string(&hash(&library.join("z.bms"))),
        json_string(&hash(&wav)),
        json_string(&hash(&font))
    );
    let joined_probe = error_text_probe("ERROR - ENTER RETURNS TO SELECTION", 650, 2);
    // The actual collector error wraps the /dev/null mandatory ioctl errno.
    let ioctl_probe = error_text_probe(
        "native input collector: Initialization(\"Linux backend: Io(Os { code: 25,",
        682,
        1,
    );
    let pinned_failure_probe =
        error_text_probe("native invocation differs from pinned launch", 682, 1);
    for trial in 0..5 {
        for (variant, executable, binary_hash) in [
            ("baseline", &baseline, &baseline_hash),
            ("current", &current, &current_hash),
        ] {
            let directory = artifacts.join(format!("preparation-{variant}-{trial}"));
            fs::create_dir_all(&directory).unwrap();
            let mut observer = FrameObserver::spawn(&helper, &directory);
            observer.command("begin");
            let stdout_path = directory.join("player.stdout.log");
            let mut child = OwnedProcess(
                Command::new(executable)
                    .args(&args)
                    .env("LC_ALL", "C")
                    .stdout(fs::File::create(&stdout_path).unwrap())
                    .stderr(fs::File::create(directory.join("player.stderr.log")).unwrap())
                    .spawn()
                    .unwrap(),
            );
            let pid = child.0.id();
            observer.command(&format!("arm\t{pid}"));
            observer.command(&format!("watch\t{}", stdout_path.display()));
            let deadline = Instant::now() + Duration::from_secs(120);
            let mut costs = Vec::new();
            let mut next_sample = |observer: &mut FrameObserver| {
                assert!(Instant::now() < deadline);
                assert!(child.0.try_wait().unwrap().is_none());
                let (reply, cost) = observer.command("sample");
                assert_eq!(reply[0], "sample");
                assert_eq!(reply[8].parse::<u32>().unwrap(), pid);
                costs.push(cost);
                (
                    reply[3].parse::<u64>().unwrap(),
                    [
                        reply[4].parse::<u64>().unwrap(),
                        reply[5].parse::<u64>().unwrap(),
                    ],
                )
            };
            let (window, pixels) = loop {
                let observation = next_sample(&mut observer);
                if observation.0 != 0
                    && observation.1[0] != observation.1[1]
                    && observation.1.iter().all(|value| *value != 0)
                {
                    break observation;
                }
                thread::sleep(Duration::from_millis(1));
            };
            observer.command("key\tDown");
            loop {
                let observation = next_sample(&mut observer);
                assert_eq!(observation.0, window);
                if observation.1 == [pixels[1], pixels[0]] {
                    break;
                }
                thread::sleep(Duration::from_millis(1));
            }
            observer.command("key\tUp");
            loop {
                let observation = next_sample(&mut observer);
                assert_eq!(observation.0, window);
                if observation.1 == pixels {
                    break;
                }
                thread::sleep(Duration::from_millis(1));
            }
            drop(next_sample);
            let (before, _) = observer.command("logs");
            assert_eq!(before[3], "0", "preparation marker preceded Enter");
            let mut records = Vec::new();
            let mut previous_markers = String::new();
            let mut cold_marker = None;
            for (attempt, key) in [(0usize, "Return"), (1usize, "F5")] {
                let (input, input_cost) = observer.command(&format!("key\t{key}"));
                let requested = input[1].parse::<u64>().unwrap();
                let mut cleared = attempt == 0;
                let mut clear_at = None;
                let (prepared, marker_hex, joined_at, error_at, exact_markers) = loop {
                    assert!(
                        Instant::now() < deadline,
                        "preparation/join observation deadline exceeded"
                    );
                    assert!(child.0.try_wait().unwrap().is_none());
                    let (logs, log_cost) = observer.command("logs");
                    costs.push(log_cost);
                    let count = logs[3].parse::<usize>().unwrap();
                    if count == attempt {
                        let (_, pinned_failure, cost) =
                            observed_text(&mut observer, &pinned_failure_probe, pid, window);
                        costs.push(cost);
                        assert!(
                            !pinned_failure,
                            "native launch configuration changed after pinning"
                        );
                    }
                    assert!(
                        count >= attempt && count <= attempt + 1,
                        "missing/extra/late marker for owned attempt"
                    );
                    assert!(
                        logs[4].starts_with(&previous_markers),
                        "previous observed marker changed"
                    );
                    let (joined_at, joined, caption_cost) =
                        observed_text(&mut observer, &joined_probe, pid, window);
                    let (error_at, ioctl, error_cost) =
                        observed_text(&mut observer, &ioctl_probe, pid, window);
                    costs.extend([caption_cost, error_cost]);
                    if !joined || !ioctl {
                        cleared = true;
                        clear_at.get_or_insert(joined_at);
                    }
                    if count == attempt + 1 && joined && ioctl && cleared {
                        let marker = logs[4].split(';').last().unwrap();
                        let (stamp, content) = marker.split_once(':').unwrap();
                        let prepared = stamp.parse::<u64>().unwrap();
                        assert!(
                            prepared >= requested,
                            "new marker observed before its input"
                        );
                        break (
                            prepared,
                            content.to_owned(),
                            joined_at,
                            error_at,
                            logs[4].clone(),
                        );
                    }
                    thread::sleep(Duration::from_millis(1));
                };
                assert!(prepared <= joined_at && requested <= error_at);
                assert!(cleared, "retry never visibly retired previous error frame");
                if let Some(cold) = &cold_marker {
                    assert_eq!(cold, &marker_hex, "retry changed prepared section marker");
                } else {
                    cold_marker = Some(marker_hex.clone());
                }
                previous_markers = exact_markers;
                let frame = directory.join(if attempt == 0 {
                    "first-joined-ioctl-error.png"
                } else {
                    "retry-joined-ioctl-error.png"
                });
                let (capture, capture_cost) =
                    observer.command(&format!("capture\t{}", frame.display()));
                assert_eq!(capture[3].parse::<u64>().unwrap(), window);
                assert_eq!(capture[4].parse::<u32>().unwrap(), pid);
                records.push(format!("{{\"attempt\":{attempt},\"input\":{},\"requested_ns\":{requested},\"prepared_log_observed_ns\":{prepared},\"input_to_native_prepared_log_observed_upper_bound_ns\":{},\"joined_error_caption_observed_ns\":{joined_at},\"required_ioctl_errno25_text_observed_ns\":{error_at},\"previous_error_cleared_observed_ns\":{},\"exact_marker_hex\":{},\"input_observer_roundtrip_ns\":{input_cost},\"capture_roundtrip_ns\":{capture_cost}}}", json_string(key), prepared-requested, clear_at.map_or("null".into(), |at| at.to_string()), json_string(&marker_hex)));
            }
            let (final_logs, _) = observer.command("logs");
            assert_eq!(final_logs[3], "2");
            assert_eq!(final_logs[4], previous_markers);
            observer.command("key\tEscape");
            // Results Escape returns to Selection; a second Escape closes it.
            observer.command("key\tEscape");
            let cleanup = Instant::now() + Duration::from_secs(8);
            let status = loop {
                if let Some(status) = child.0.try_wait().unwrap() {
                    break status;
                }
                assert!(
                    Instant::now() < cleanup,
                    "owned production player failed bounded cleanup"
                );
                thread::sleep(Duration::from_millis(1));
            };
            assert!(status.success());
            let (post_exit_logs, _) = observer.command("logs");
            assert_eq!(
                post_exit_logs[3], "2",
                "late preparation marker at shutdown"
            );
            assert_eq!(post_exit_logs[4], previous_markers);
            observer.command("quit");
            let config = args
                .iter()
                .map(|arg| json_string(arg))
                .collect::<Vec<_>>()
                .join(",");
            let raw = format!("{{\"schema\":\"beatkernel-native-external-preparation-paired/v1\",\"variant\":{},\"trial\":{trial},\"process_id\":{pid},\"owned_window_id\":{window},\"executable_sha256\":{},\"observer_sha256\":{},\"fixture_sha256\":{fixture_hashes},\"config\":[{config}],\"environment\":{{\"LC_ALL\":\"C\",\"output\":\"ALSA null\",\"input\":\"/dev/null mandatory ioctl failure\"}},\"clock\":\"one Python time.monotonic_ns origin before production spawn\",\"marker_count\":2,\"attempts\":[{}],\"observer_roundtrip_ns\":{:?},\"sampling_interval_configured_ns\":1000000,\"deadline_ns\":120000000000,\"stdout_read_max_bytes_per_probe\":65536,\"stdout_total_limit_bytes\":1048576,\"playable\":false,\"audio_ready\":null,\"os_page_cache_controlled\":false,\"limits\":\"Input-to-prepared-log observed upper bounds include delivery, derived completion/compile and stdout observation; joined error pixels prove technical failure, never gameplay/output readiness. No codec-only counters, section/replay/replacement comparative claim.\"}}", json_string(variant), json_string(binary_hash), json_string(&hash(&helper)), records.join(","), costs);
            fs::write(directory.join("events.json"), raw).unwrap();
        }
    }
}
