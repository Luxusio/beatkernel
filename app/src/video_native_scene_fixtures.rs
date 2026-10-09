//! Actual external codec → prepared player snapshot → native feed → Scene proof.
//! Recorder tier asserts decoded pixels and production Scene composition, not GPU.
//! The separate native-window tier uses the actual Renderer and surface; it does
//! reads the presented window back through external X11 capture, without adding
//! a production Renderer test API.
use crate::{
    bga_render::{paint, BgaFrame, MovieTextureCache, TextureOwner},
    local_players::PlayerId,
    player::{self, PlayerSnapshot},
    scene::Scene,
    texture::{RgbaImage, TextureId},
    ui::interaction::Bounds,
    video::{VideoFrame, VideoSessionKey},
    video_native_bank::NativeVideoController,
};
use beatkernel::time::Timestamp;
use beatkernel_bms::ParseOptions;
use std::{
    path::PathBuf,
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

fn ns(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> Fixture {
    let ffmpeg = std::env::var_os("BEATKERNEL_TEST_FFMPEG")
        .expect("actual codec tier requires BEATKERNEL_TEST_FFMPEG");
    let ffprobe = std::env::var_os("BEATKERNEL_TEST_FFPROBE")
        .expect("actual codec tier requires BEATKERNEL_TEST_FFPROBE");
    assert!(std::path::Path::new(&ffprobe).is_file());
    // The production bank discovers these tools via PATH, rather than a test-only
    // injection. Root must prepend the explicitly selected tool directory.
    let selected = std::path::Path::new(&ffmpeg).parent().unwrap();
    assert!(
        std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .any(|directory| directory == selected),
        "prepend selected FFmpeg directory to PATH"
    );
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "beatkernel-native-scene-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )));
    std::fs::create_dir(&fixture.0).unwrap();
    let colors = [
        [255, 0, 0, 255],
        [255, 255, 0, 255],
        [0, 255, 0, 255],
        [0, 255, 255, 255],
        [0, 0, 255, 255],
    ];
    let mut pixels = Vec::with_capacity(16 * 16 * 4 * colors.len());
    for color in colors {
        for _ in 0..256 {
            pixels.extend_from_slice(&color);
        }
    }
    std::fs::write(fixture.0.join("colors.rgba"), pixels).unwrap();
    let output = Command::new(ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            "16x16",
            "-framerate",
            "10",
            "-i",
        ])
        .arg(fixture.0.join("colors.rgba"))
        .args([
            "-vf",
            "setpts=PTS+5/TB",
            "-fps_mode",
            "passthrough",
            "-c:v",
            "ffv1",
            "-threads",
            "1",
        ])
        .arg(fixture.0.join("clip.mkv"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "encode failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // A real PCM resource backs the terminal playable note. The movie fixture
    // uses normal chart admission rather than accepting an undefined WAV01.
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&52u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&8000u32.to_le_bytes());
    wav.extend_from_slice(&16000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&[0u8; 16]);
    std::fs::write(fixture.0.join("silence.wav"), wav).unwrap();
    std::fs::write(
        fixture.0.join("chart.bms"),
        "#BPM 120\n#WAV01 silence.wav\n#BMP01 clip.mkv\n#00104:01\n#00311:01\n",
    )
    .unwrap();
    fixture
}

fn selected(
    controller: &mut NativeVideoController,
    snapshot: &mut PlayerSnapshot,
    song: i64,
    expected_pts: i64,
) -> VideoFrame {
    snapshot.song_time = Some(ns(song));
    let chart = snapshot.chart.as_ref().unwrap();
    let bank = snapshot.movies.as_ref().unwrap();
    let activation = chart.bga_activations(snapshot.song_time.unwrap())[0].unwrap();
    assert_eq!(activation.activated_at, ns(2_000_000_000));
    let session = VideoSessionKey::from_activation(bank.content(), 0, activation, None).unwrap();
    let target = session.target(snapshot.song_time.unwrap()).unwrap();
    assert_eq!(target, ns(song - 2_000_000_000));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let frames = controller
            .sync(snapshot.movies.as_ref(), &[Some((session, target))])
            .unwrap();
        assert!(
            controller.unavailable.is_empty(),
            "movie unavailable: {:?}",
            controller.unavailable
        );
        if let Some(frame) = frames.into_iter().next().flatten() {
            assert!(
                frame.pts <= target,
                "future decoded frame must never be displayed"
            );
            if frame.pts == ns(expected_pts) {
                return frame;
            }
        }
        assert!(
            Instant::now() < deadline,
            "no eligible frame at song {song}, expected PTS {expected_pts}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn assert_color(frame: &VideoFrame, color: [u8; 4]) {
    assert_eq!([frame.image.width(), frame.image.height()], [16, 16]);
    assert!(frame
        .image
        .pixels()
        .chunks_exact(4)
        .all(|pixel| pixel == color));
}
fn compose(
    owner: &mut impl TextureOwner,
    cache: &mut MovieTextureCache,
    frame: &VideoFrame,
) -> (TextureId, Scene) {
    let sprite = cache.sync(&[Some(frame)], owner).unwrap()[0].unwrap();
    let mut scene = Scene::new(64, 64);
    paint(
        &mut scene,
        BgaFrame {
            active: true,
            base: Some(sprite),
            ..Default::default()
        },
        Bounds {
            x: 0,
            y: 0,
            width: 64,
            height: 64,
        },
    )
    .unwrap();
    assert_eq!(
        scene.rectangles().len(),
        2,
        "actual BGA composition includes black then movie"
    );
    assert_eq!(scene.batches().last().unwrap().texture, sprite.texture);
    (sprite.texture, scene)
}
fn run_chain<O: TextureOwner>(owner: &mut O, mut present: impl FnMut(&mut O, &Scene)) {
    let fixture = fixture();
    let text = std::fs::read_to_string(fixture.0.join("chart.bms")).unwrap();
    let source = beatkernel_bms::parse(&text, ParseOptions::default()).unwrap();
    let compiled = source.compile().unwrap().chart;
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_native_chart(
            &fixture.0.join("chart.bms"),
            &source,
            &compiled,
            &[PlayerId(1)],
        )
        .map_err(|error| error.to_string())?;
        let mut snapshot = viewer
            .take_latest()
            .expect("actual registered native snapshot");
        assert!(snapshot.movies.is_some());
        let mut controller = NativeVideoController::default();
        let mut cache = MovieTextureCache::default();
        let first = selected(&mut controller, &mut snapshot, 2_000_000_000, 0);
        assert_color(&first, [255, 0, 0, 255]);
        let (first_texture, scene) = compose(owner, &mut cache, &first);
        present(owner, &scene);
        let later = selected(&mut controller, &mut snapshot, 2_250_000_000, 200_000_000);
        assert_color(&later, [0, 255, 0, 255]);
        let (later_texture, scene) = compose(owner, &mut cache, &later);
        assert_eq!(
            later_texture, first_texture,
            "same extent advances via update_texture"
        );
        present(owner, &scene);
        // Multiple host polls with an unchanged committed song timestamp represent
        // pause. They must retain PTS, revision, session and GPU texture identity.
        for _ in 0..4 {
            let paused = selected(&mut controller, &mut snapshot, 2_250_000_000, 200_000_000);
            assert_eq!(
                (paused.session, paused.revision),
                (later.session, later.revision)
            );
            assert_color(&paused, [0, 255, 0, 255]);
            assert_eq!(compose(owner, &mut cache, &paused).0, first_texture);
        }
        let eof = selected(&mut controller, &mut snapshot, 3_000_000_000, 400_000_000);
        assert_color(&eof, [0, 0, 255, 255]);
        let (eof_texture, scene) = compose(owner, &mut cache, &eof);
        assert_eq!(eof_texture, first_texture);
        present(owner, &scene);
        let backwards = selected(&mut controller, &mut snapshot, 2_000_000_000, 0);
        assert_color(&backwards, [255, 0, 0, 255]);
        assert_ne!(backwards.session.generation, eof.session.generation);
        let (back_texture, scene) = compose(owner, &mut cache, &backwards);
        assert_ne!(
            back_texture, first_texture,
            "backward seek fences the old texture session"
        );
        present(owner, &scene);
        controller.retire();
        cache.clear(owner)?;
        snapshot.movies.as_ref().unwrap().join()?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        viewer.take_latest().unwrap().status,
        player::PlayerStatus::Finished
    );
}
#[derive(Default)]
struct Recorder {
    live: Option<TextureId>,
    uploads: usize,
    updates: usize,
    removes: usize,
}
impl TextureOwner for Recorder {
    fn upload(&mut self, _: &RgbaImage) -> Result<TextureId, String> {
        assert!(self.live.is_none());
        let texture = TextureId::allocate()?;
        self.live = Some(texture);
        self.uploads += 1;
        Ok(texture)
    }
    fn update(&mut self, texture: TextureId, _: &RgbaImage) -> Result<(), String> {
        assert_eq!(self.live, Some(texture));
        self.updates += 1;
        Ok(())
    }
    fn remove(&mut self, texture: TextureId) -> Result<(), String> {
        assert_eq!(self.live.take(), Some(texture));
        self.removes += 1;
        Ok(())
    }
}
#[test]
#[ignore = "requires explicit FFmpeg/FFprobe environment and selected codec directory on PATH"]
fn encoded_resource_through_prepared_native_snapshot_feed_cache_and_scene() {
    let mut recorder = Recorder::default();
    run_chain(&mut recorder, |_, scene| scene.status().unwrap());
    assert_eq!(
        (recorder.uploads, recorder.updates, recorder.removes),
        (2, 2, 2)
    );
    assert!(recorder.live.is_none());
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
fn capture_window(window: &winit::window::Window, path: &std::path::Path) -> Vec<u8> {
    let position = window.inner_position().unwrap();
    let display = std::env::var("DISPLAY").expect("actual surface tier requires DISPLAY");
    let input = format!("{display}+{},{}", position.x, position.y);
    let mut child = Command::new(std::env::var_os("BEATKERNEL_TEST_FFMPEG").unwrap())
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "x11grab",
            "-draw_mouse",
            "0",
            "-video_size",
            "64x64",
            "-i",
        ])
        .arg(input)
        .args([
            "-frames:v",
            "1",
            "-threads",
            "1",
            "-pix_fmt",
            "rgba",
            "-f",
            "rawvideo",
        ])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "actual X11 frame capture failed; selected FFmpeg must support x11grab"
            );
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("actual X11 capture did not complete within its deadline");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let pixels = std::fs::read(path).unwrap();
    assert_eq!(pixels.len(), 64 * 64 * 4);
    pixels
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
#[test]
#[ignore = "requires BEATKERNEL_TEST_NATIVE_UI_WINDOW=1, selected codecs, actual X11 display and GPU surface"]
#[allow(deprecated)]
fn encoded_native_scene_presents_on_actual_renderer_surface() {
    use winit::{
        event::{Event, WindowEvent},
        event_loop::EventLoop,
        platform::{pump_events::EventLoopExtPumpEvents, x11::EventLoopBuilderExtX11},
        window::Window,
    };
    assert_eq!(
        std::env::var("BEATKERNEL_TEST_NATIVE_UI_WINDOW").as_deref(),
        Ok("1")
    );
    let mut event_loop = EventLoop::builder()
        .with_x11()
        .with_any_thread(true)
        .build()
        .unwrap();
    let window = Arc::new(
        event_loop
            .create_window(
                Window::default_attributes()
                    .with_title("BeatKernel native movie fixture")
                    .with_inner_size(winit::dpi::PhysicalSize::new(64, 64)),
            )
            .unwrap(),
    );
    window.request_redraw();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut redraw_observed = false;
    while !redraw_observed {
        assert!(
            Instant::now() < deadline,
            "native window did not become drawable"
        );
        event_loop.pump_events(Some(Duration::from_millis(10)), |event, _| {
            if matches!(event, Event::WindowEvent { window_id, event: WindowEvent::RedrawRequested }
                if window_id == window.id())
            {
                redraw_observed = true;
            }
        });
    }
    let capture = Fixture(std::env::temp_dir().join(format!(
        "beatkernel-native-scene-capture-{}",
        std::process::id()
    )));
    std::fs::create_dir(&capture.0).unwrap();
    let instance = crate::graphics::instance(crate::graphics::BackendChoice::Vulkan).unwrap();
    let surface = instance.create_surface(window.clone()).unwrap();
    let mut renderer = pollster::block_on(crate::graphics::Renderer::new(
        surface,
        &instance,
        crate::graphics::Presentation::Fifo,
    ))
    .unwrap();
    let size = window.inner_size();
    renderer.resize(size.width, size.height).unwrap();
    let before = renderer.presentation_count();
    let expected = [[96u8, 0, 0], [0, 96, 0], [0, 0, 96], [96, 0, 0]];
    let mut presentation = 0;
    run_chain(&mut renderer, |renderer, scene| {
        renderer.render(scene).unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            event_loop.pump_events(Some(Duration::ZERO), |_, _| {});
            let pixels = capture_window(&window, &capture.0.join("presented.rgba"));
            let center = &pixels[(32 * 64 + 32) * 4..][..3];
            if center
                .iter()
                .zip(expected[presentation])
                .all(|(&actual, wanted)| actual.abs_diff(wanted) <= 3)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "presented movie color {center:?} did not reach {:?}",
                expected[presentation]
            );
        }
        presentation += 1;
    });
    assert_eq!(presentation, 4);
    assert_eq!(renderer.presentation_count(), before + 4);
}
