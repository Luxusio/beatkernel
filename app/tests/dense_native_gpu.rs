#![cfg(all(feature = "desktop", target_os = "linux"))]
//! Actual Vulkan/X11 presentation, complementary to the internal dense geometry oracle.
//! Run with an owned Xvfb DISPLAY:
//! BEATKERNEL_TEST_NATIVE_UI_WINDOW=1 cargo test -p beatkernel-bms-runtime \
//!   --test dense_native_gpu --locked -- --ignored --test-threads=1
//! In this repository's local Linux runtime, prepend
//! target/toolchain/native-ui-runtime/usr/lib/x86_64-linux-gnu to LD_LIBRARY_PATH.
//! Xvfb and Mesa llvmpipe provide the software display/Vulkan verification tier.
//! This observes successful surface presentations, not pixels or physical GPU timing.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use beatkernel::time::Timestamp;
use beatkernel_bms_runtime::{
    graphics::{self, BackendChoice, Presentation, Renderer},
    note_progress::NoteProgress,
    player_chart::PlayerChart,
    scene::Scene,
    ui::{interaction::Bounds, organisms::playfield_in_with_progress},
};
use winit::{
    dpi::LogicalSize, event_loop::EventLoop, platform::x11::EventLoopBuilderExtX11, window::Window,
};

const WINDOW_NS: i64 = 4_000_000_000;
const BEHIND_NS: i64 = 150_000_000;

fn chart(dense: bool) -> Arc<PlayerChart> {
    let text = if dense {
        format!(
            "#BPM 60\n#WAV01 tone\n#LNTYPE 1\n#00051:{}\n#00012:{}\n#000D3:{}",
            "0101".repeat(2304),
            "01".repeat(2304),
            "1E".repeat(2304)
        )
    } else {
        "#BPM 60\n#WAV01 tone\n#00011:01".to_owned()
    };
    let source = beatkernel_bms::parse(&text, Default::default()).unwrap();
    Arc::new(PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap())
}

// Independent ordered linear membership oracle. Exact emitted primitive geometry is
// covered by dense_playfield_fixtures inside the library: Scene's frames are private.
fn assert_membership(chart: &PlayerChart, now: i64) -> (usize, usize, usize) {
    let mut notes = Vec::new();
    let mut mines = Vec::new();
    chart
        .visible_note_indices_checked(Timestamp::from_nanos(now), WINDOW_NS, BEHIND_NS, &mut notes)
        .unwrap();
    chart
        .visible_mine_indices_checked(Timestamp::from_nanos(now), WINDOW_NS, BEHIND_NS, &mut mines)
        .unwrap();
    let upper = i128::from(now) + i128::from(WINDOW_NS);
    let lower = i128::from(now) - i128::from(BEHIND_NS);
    let expected_notes: Vec<_> = chart
        .notes
        .iter()
        .enumerate()
        .filter(|(_, note)| {
            i128::from(note.start.as_nanos()) <= upper
                && i128::from(note.end.unwrap_or(note.start).as_nanos()) >= lower
        })
        .map(|(index, _)| index)
        .collect();
    let expected_mines: Vec<_> = chart
        .mines()
        .iter()
        .enumerate()
        .filter(|(_, mine)| {
            i128::from(mine.at.as_nanos()) >= lower && i128::from(mine.at.as_nanos()) <= upper
        })
        .map(|(index, _)| index)
        .collect();
    assert_eq!(notes, expected_notes);
    assert_eq!(mines, expected_mines);
    let primitives = notes
        .iter()
        .map(|&index| {
            if chart.notes[index].end.is_some() {
                3
            } else {
                1
            }
        })
        .sum::<usize>()
        + mines.len();
    (notes.len(), mines.len(), primitives)
}

fn paint(scene: &mut Scene, chart: &PlayerChart, now: i64, progress: Option<&NoteProgress>) {
    playfield_in_with_progress(
        scene,
        chart,
        Timestamp::from_nanos(now),
        WINDOW_NS,
        Bounds {
            x: 80,
            y: 64,
            width: 640,
            height: 528,
        },
        &[],
        0,
        progress,
    )
    .unwrap();
    scene.status().unwrap();
}

fn present(renderer: &mut Renderer, scene: &Scene) {
    let before = renderer.presentation_count();
    let deadline = Instant::now() + Duration::from_secs(10);
    while renderer.presentation_count() == before {
        renderer.render(scene).unwrap();
        assert!(
            Instant::now() < deadline,
            "no successful native presentation"
        );
        if renderer.presentation_count() == before {
            std::thread::sleep(Duration::from_millis(16));
        }
    }
    assert_eq!(renderer.presentation_count(), before + 1);
}

#[test]
#[ignore = "requires BEATKERNEL_TEST_NATIVE_UI_WINDOW=1 and owned X11 display/Vulkan surface"]
#[allow(deprecated)]
fn dense_native_gpu_grows_warms_seeks_resizes_and_rejects_late_candidate() {
    assert_eq!(
        std::env::var("BEATKERNEL_TEST_NATIVE_UI_WINDOW").as_deref(),
        Ok("1")
    );
    assert!(
        std::env::var_os("DISPLAY").is_some(),
        "owned DISPLAY required"
    );
    let event_loop = EventLoop::<()>::with_user_event()
        .with_x11()
        .with_any_thread(true)
        .build()
        .unwrap();
    let window = Arc::new(
        event_loop
            .create_window(
                Window::default_attributes()
                    .with_title("BeatKernel dense native GPU regression")
                    .with_inner_size(LogicalSize::new(960.0, 720.0)),
            )
            .unwrap(),
    );
    let instance = graphics::instance(BackendChoice::Vulkan).unwrap();
    let surface = instance.create_surface(window.clone()).unwrap();
    let mut renderer =
        pollster::block_on(Renderer::new(surface, &instance, Presentation::Fifo)).unwrap();
    let size = window.inner_size();
    renderer.resize(size.width, size.height).unwrap();
    let description = renderer.description();

    let sparse = chart(false);
    let dense = chart(true);
    assert_eq!(dense.lanes, [0x11, 0x12, 0x13]);
    assert_eq!(
        dense.notes.iter().filter(|note| note.end.is_some()).count(),
        2304
    );
    assert_eq!(
        dense.notes.iter().filter(|note| note.end.is_none()).count(),
        2304
    );
    assert_eq!(assert_membership(&dense, 0), (4608, 2304, 11520));
    let progress = NoteProgress::new(dense.clone()).unwrap();
    let mut scene = Scene::new(960, 720);
    paint(&mut scene, &sparse, 0, None);
    present(&mut renderer, &scene);

    // The same retained Scene/Renderer moves from one primitive to >8192.
    scene.clear();
    paint(&mut scene, &dense, 0, Some(&progress));
    present(&mut renderer, &scene);
    let identity = Arc::clone(scene.geometry_stamp().0);
    let epoch = scene.geometry_stamp().1;
    for _ in 0..4 {
        present(&mut renderer, &scene);
        assert!(Arc::ptr_eq(&identity, scene.geometry_stamp().0));
        assert_eq!(scene.geometry_stamp().1, epoch);
    }
    // Recomposition exercises retained playfield caches as well as unchanged redraws.
    scene.clear();
    paint(&mut scene, &dense, 0, Some(&progress));
    present(&mut renderer, &scene);

    let seek_membership = assert_membership(&dense, 2_000_000_000);
    assert!(seek_membership.0 > 2048);
    assert!(seek_membership.0 < 4608);
    scene.clear();
    paint(&mut scene, &dense, 2_000_000_000, Some(&progress));
    present(&mut renderer, &scene);
    renderer.resize(800, 600).unwrap();
    scene = Scene::new(800, 600);
    paint(&mut scene, &dense, 0, Some(&progress));
    present(&mut renderer, &scene);

    // Public-seam negative path: the fourth field has invalid prepared-index extent.
    // This tests sticky Scene rejection, not an injected GPU device-limit failure.
    scene.clear();
    for _ in 0..3 {
        paint(&mut scene, &dense, 0, Some(&progress));
    }
    let mut invalid = (*dense).clone();
    invalid.notes.push(invalid.notes[0].clone());
    assert!(playfield_in_with_progress(
        &mut scene,
        &invalid,
        Timestamp::from_nanos(0),
        WINDOW_NS,
        Bounds {
            x: 80,
            y: 64,
            width: 640,
            height: 528
        },
        &[],
        0,
        None,
    )
    .is_err());
    assert!(scene.status().is_err());
    let before_rejection = renderer.presentation_count();
    for _ in 0..2 {
        assert!(renderer.render(&scene).is_err());
        assert_eq!(renderer.presentation_count(), before_rejection);
    }
    scene.clear();
    paint(&mut scene, &dense, 0, Some(&progress));
    present(&mut renderer, &scene);
    eprintln!(
        "dense native GPU: {description}; initial_extent={}x{}; resized_surface=800x600; \
         dense_membership=4608notes/2304mines; expected_primitives=11520; \
         seek_membership={seek_membership:?}; successful_presentations={}; \
         rejected_candidate_counter={before_rejection}",
        size.width,
        size.height,
        renderer.presentation_count()
    );
    drop(renderer);
    drop(window);
    drop(event_loop);
}
