//! AC-006 public cache/feed contracts. Recorder assertions do not claim GPU execution.
use crate::{
    asset_source::{MemoryAssetLimits, MemoryFiles},
    bga::BgaState,
    bga_opacity::BgaOpacity,
    bga_render::{paint, BgaFrame, BgaSprite, BgaTextureCache, MovieTextureCache, TextureOwner},
    image_assets::{ImageAssetLimits, ImageAssets},
    scene::Scene,
    texture::{RgbaImage, TextureId},
    ui::interaction::Bounds,
    video::{VideoDecodeEvent, VideoFrame, VideoSessionKey},
    video_assets::{VideoAssetLimits, VideoAssets, VideoTransform},
    video_native_bank::{
        NativeVideoBank, NativeVideoBankLimits, NativeVideoBudget, NativeVideoFrames,
        NativeVideoReservation,
    },
};
use beatkernel::time::Timestamp;
use beatkernel_bms::{BgaChannel, ImageId, ParseOptions};
use std::sync::Arc;

fn ns(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}

#[test]
fn native_bank_shutdown_fences_requests_and_join_observes_service_completion() {
    // No external codec or elapsed-time assumption: join synchronizes with the
    // real bank service's final decoder-cleanup path before returning.
    let bank = NativeVideoBank::prepare(Arc::new(VideoAssets::default())).unwrap();
    let mut demand = session(1);
    demand.content = bank.content();
    bank.shutdown();
    assert_eq!(
        bank.request(0, demand, ns(0)).unwrap_err(),
        "movie bank is shut down"
    );
    assert!(bank.try_next(0).is_none());
    bank.join().unwrap();
    bank.join().unwrap();
    assert!(bank.request(0, demand, ns(0)).is_err());
}

#[test]
fn native_session_joins_movie_service_before_publishing_terminal_snapshot() {
    struct Directory(std::path::PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = Directory(std::env::temp_dir().join(format!(
        "beatkernel-movie-session-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )));
    std::fs::create_dir(&dir.0).unwrap();
    let text = "#BPM 120\n#WAV01 key.wav\n#BMP01 clip.mp4\n#00004:01\n#00011:01";
    let path = dir.0.join("chart.bms");
    std::fs::write(&path, text).unwrap();
    std::fs::write(dir.0.join("clip.mp4"), b"movie-registration-only").unwrap();
    let source = beatkernel_bms::parse(text, ParseOptions::default()).unwrap();
    let compiled = source.compile().unwrap().chart;
    let (publisher, viewer) = crate::player::channel();
    let mut retained = None;
    crate::player::with_publisher(publisher, || {
        crate::player::publish_native_chart(
            &path,
            &source,
            &compiled,
            &[crate::local_players::PlayerId(1)],
        )
        .map_err(|error| error.to_string())?;
        retained = viewer.take_latest().unwrap().movies;
        Ok(())
    })
    .unwrap();
    let terminal = viewer.take_latest().unwrap();
    assert_eq!(terminal.status, crate::player::PlayerStatus::Finished);
    let bank = retained.unwrap();
    assert!(Arc::ptr_eq(&bank, terminal.movies.as_ref().unwrap()));
    let mut demand = session(1);
    demand.content = bank.content();
    assert_eq!(
        bank.request(0, demand, ns(0)).unwrap_err(),
        "movie bank is shut down"
    );
    // The game-owner join already consumed this service, even though both UI
    // snapshots kept its Arc alive. Repeating cleanup remains harmless.
    bank.join().unwrap();
}

fn raw_reservation(width: u32, height: u32) -> NativeVideoReservation {
    NativeVideoReservation::for_extent(
        width,
        height,
        VideoTransform {
            crop: None,
            canvas: None,
            keyed: false,
        },
    )
    .unwrap()
}

#[test]
fn native_stream_configuration_honors_exact_caller_byte_and_count_limits() {
    let reservation = raw_reservation(16, 16);
    let mut config = crate::video_native::FfmpegDecoderConfig::default();
    config.frame_limits.max_frame_bytes = 1024;
    config.frame_limits.max_bytes = 3071;
    assert!(reservation.decoder_config(&config).is_err());
    config.frame_limits.max_bytes = 3072;
    let derived = reservation.decoder_config(&config).unwrap();
    assert_eq!(derived.frame_limits.max_bytes, 3072);
    assert_eq!(derived.frame_limits.max_frame_bytes, 1024);
    assert_eq!(derived.frame_limits.max_frames, 3);
    config.frame_limits.max_bytes = 4096;
    config.frame_limits.max_frames = 12;
    let derived = reservation.decoder_config(&config).unwrap();
    assert_eq!(derived.frame_limits.max_bytes, 3072);
    assert_eq!(derived.frame_limits.max_frames, 3);
    config.frame_limits.max_frames = 2;
    assert!(reservation.decoder_config(&config).is_err());
    assert!(NativeVideoBank::prepare_with_config(
        Arc::new(VideoAssets::default()),
        NativeVideoBankLimits::default(),
        config,
    )
    .is_err());
}

#[test]
fn native_default_budget_admits_single_4k_and_two_hd_without_fixed_slot_partitions() {
    let limits = NativeVideoBankLimits::default();
    assert_eq!(limits.max_working_bytes, 512 * 1024 * 1024);
    let mut budget = NativeVideoBudget::new(limits).unwrap();
    let large = raw_reservation(3840, 2160);
    budget.reserve(0, large).unwrap();
    assert_eq!(budget.used_bytes(), large.working_bytes);
    budget.release(0);
    assert_eq!(budget.used_bytes(), 0);
    let hd = raw_reservation(1920, 1080);
    budget.reserve(0, hd).unwrap();
    budget.reserve(1, hd).unwrap();
    assert_eq!(budget.used_bytes(), hd.working_bytes * 2);
    budget.release(0);
    assert_eq!(budget.used_bytes(), hd.working_bytes);
    budget.release(1);
    assert_eq!(budget.used_bytes(), 0);
}

#[test]
fn native_budget_admits_sixteen_small_streams_and_rejects_extra_slot_without_charging() {
    let small = raw_reservation(64, 64);
    let mut budget = NativeVideoBudget::new(NativeVideoBankLimits {
        max_working_bytes: small.working_bytes * 16,
    })
    .unwrap();
    for slot in 0..16 {
        budget.reserve(slot, small).unwrap();
    }
    assert_eq!(budget.used_bytes(), small.working_bytes * 16);
    assert!(budget.reserve(16, small).is_err());
    assert_eq!(budget.used_bytes(), small.working_bytes * 16);
    for slot in 0..16 {
        budget.release(slot);
    }
    budget.release(0); // repeated retirement cannot free another slot's ownership
    assert_eq!(budget.used_bytes(), 0);
}

#[test]
fn native_budget_failed_replacement_is_atomic_and_release_allows_retry() {
    let small = raw_reservation(64, 64);
    let large = raw_reservation(128, 128);
    let mut budget = NativeVideoBudget::new(NativeVideoBankLimits {
        max_working_bytes: large.working_bytes,
    })
    .unwrap();
    budget.reserve(0, small).unwrap();
    budget.reserve(1, small).unwrap();
    assert!(budget.reserve(0, large).is_err());
    assert_eq!(budget.used_bytes(), small.working_bytes * 2);
    budget.release(1);
    assert_eq!(budget.used_bytes(), small.working_bytes);
    budget.reserve(0, large).unwrap();
    assert_eq!(budget.used_bytes(), large.working_bytes);
    budget.release(0);
    assert_eq!(budget.used_bytes(), 0);
    assert!(NativeVideoBudget::new(NativeVideoBankLimits {
        max_working_bytes: 0
    })
    .is_err());
}
fn session(generation: u64) -> VideoSessionKey {
    VideoSessionKey {
        content: 7,
        generation,
        image: ImageId(1),
        channel: BgaChannel::Base,
        activated_at: ns(100),
        ordinal: Some(3),
    }
}
fn frame(key: VideoSessionKey, pts: i64, revision: u64, width: u32, color: u8) -> VideoFrame {
    VideoFrame {
        session: key,
        pts: ns(pts),
        revision,
        image: Arc::new(
            RgbaImage::new(width, 1, [color, 0, 0, 255].repeat(width as usize)).unwrap(),
        ),
    }
}
#[derive(Default)]
struct Owner {
    live: bool,
    actions: Vec<&'static str>,
    pixels: Vec<Vec<u8>>,
    fail_upload: bool,
    fail_update: bool,
    fail_remove: bool,
}
impl TextureOwner for Owner {
    fn upload(&mut self, image: &RgbaImage) -> Result<TextureId, String> {
        self.actions.push("upload");
        if self.fail_upload {
            return Err("upload failed".into());
        }
        assert!(
            !self.live,
            "one-entry recorder requires release before upload"
        );
        self.live = true;
        self.pixels.push(image.pixels().to_vec());
        Ok(TextureId::WHITE)
    }
    fn update(&mut self, id: TextureId, image: &RgbaImage) -> Result<(), String> {
        self.actions.push("update");
        assert!(self.live);
        assert_eq!(id, TextureId::WHITE);
        if self.fail_update {
            return Err("update failed".into());
        }
        self.pixels.push(image.pixels().to_vec());
        Ok(())
    }
    fn remove(&mut self, id: TextureId) -> Result<(), String> {
        self.actions.push("remove");
        assert!(self.live);
        assert_eq!(id, TextureId::WHITE);
        if self.fail_remove {
            return Err("remove failed".into());
        }
        self.live = false;
        Ok(())
    }
}

#[test]
fn same_extent_revision_updates_keep_texture_identity_and_unchanged_frames_do_no_work() {
    let mut owner = Owner::default();
    let mut cache = MovieTextureCache::default();
    let first = frame(session(1), 0, 1, 1, 10);
    let original = cache.sync(&[Some(&first)], &mut owner).unwrap()[0].unwrap();
    let second = frame(session(1), 20, 2, 1, 20);
    let updated = cache.sync(&[Some(&second)], &mut owner).unwrap()[0].unwrap();
    assert_eq!(updated, original);
    cache.sync(&[Some(&second)], &mut owner).unwrap();
    assert_eq!(owner.actions, ["upload", "update"]);
    assert_eq!(owner.pixels, [vec![10, 0, 0, 255], vec![20, 0, 0, 255]]);
    cache.clear(&mut owner).unwrap();
    assert!(!owner.live);
}

#[test]
fn shared_session_views_upload_once_and_resize_releases_before_reallocation() {
    let mut owner = Owner::default();
    let mut cache = MovieTextureCache::default();
    let first = frame(session(1), 0, 1, 1, 10);
    let sprites = cache
        .sync(&[Some(&first), Some(&first)], &mut owner)
        .unwrap();
    assert_eq!(sprites[0], sprites[1]);
    assert_eq!(owner.actions, ["upload"]);
    let resized = frame(session(1), 1, 2, 2, 20);
    let sprite = cache.sync(&[Some(&resized)], &mut owner).unwrap()[0].unwrap();
    assert_eq!((sprite.width, sprite.height), (2, 1));
    assert_eq!(owner.actions, ["upload", "remove", "upload"]);
    cache.clear(&mut owner).unwrap();
}

#[test]
fn new_content_and_generation_release_previous_movie_ownership() {
    let mut owner = Owner::default();
    let mut cache = MovieTextureCache::default();
    let first = frame(session(1), 0, 1, 1, 10);
    cache.sync(&[Some(&first)], &mut owner).unwrap();
    let mut key = session(2);
    key.content = 8;
    let replacement = frame(key, 0, 1, 1, 30);
    cache.sync(&[Some(&replacement)], &mut owner).unwrap();
    assert_eq!(owner.actions, ["upload", "remove", "upload"]);
    cache.sync(&[], &mut owner).unwrap();
    assert!(!owner.live);
}

#[test]
fn failed_updates_retry_without_committing_the_failed_revision() {
    let mut owner = Owner::default();
    let mut cache = MovieTextureCache::default();
    let first = frame(session(1), 0, 1, 1, 10);
    cache.sync(&[Some(&first)], &mut owner).unwrap();
    let second = frame(session(1), 1, 2, 1, 20);
    owner.fail_update = true;
    assert!(cache.sync(&[Some(&second)], &mut owner).is_err());
    assert!(owner.live);
    assert_eq!(owner.pixels, [vec![10, 0, 0, 255]]);
    owner.fail_update = false;
    cache.sync(&[Some(&second)], &mut owner).unwrap();
    cache.sync(&[Some(&second)], &mut owner).unwrap();
    assert_eq!(owner.actions, ["upload", "update", "update"]);
    cache.clear(&mut owner).unwrap();
}

#[test]
fn resize_remove_and_upload_errors_preserve_retryable_ownership() {
    let mut owner = Owner::default();
    let mut cache = MovieTextureCache::default();
    let first = frame(session(1), 0, 1, 1, 10);
    cache.sync(&[Some(&first)], &mut owner).unwrap();
    let resized = frame(session(1), 1, 2, 2, 20);
    owner.fail_remove = true;
    assert!(cache.sync(&[Some(&resized)], &mut owner).is_err());
    assert!(owner.live);
    owner.fail_remove = false;
    owner.fail_upload = true;
    assert!(cache.sync(&[Some(&resized)], &mut owner).is_err());
    assert!(!owner.live);
    owner.fail_upload = false;
    cache.sync(&[Some(&resized)], &mut owner).unwrap();
    owner.fail_remove = true;
    assert!(cache.clear(&mut owner).is_err());
    assert!(owner.live);
    owner.fail_remove = false;
    cache.clear(&mut owner).unwrap();
    assert!(!owner.live);
}

#[test]
fn movie_cache_view_limit_is_checked_before_gpu_mutation() {
    let mut owner = Owner::default();
    let mut cache = MovieTextureCache::default();
    let first = frame(session(1), 0, 1, 1, 10);
    assert!(cache.sync(&[Some(&first); 17], &mut owner).is_err());
    assert!(owner.actions.is_empty());
    let admitted = cache.sync(&[Some(&first); 16], &mut owner).unwrap();
    assert!(admitted.iter().all(|sprite| *sprite == admitted[0]));
    assert_eq!(owner.actions, ["upload"]);
    cache.clear(&mut owner).unwrap();
}

#[test]
fn scene_keeps_base_layer_layer2_poor_order_fit_opacity_and_clip() {
    let sprite = |width, height| {
        Some(BgaSprite {
            texture: TextureId::WHITE,
            width,
            height,
        })
    };
    let frame = BgaFrame {
        active: true,
        base: sprite(4, 1),
        layer: sprite(1, 2),
        layer2: sprite(1, 1),
        poor_overlay: sprite(2, 1),
        unavailable: 0,
        opacity: BgaOpacity {
            base: 10,
            layer: 20,
            layer2: 30,
            poor: 40,
        },
    };
    let mut scene = Scene::new(300, 300);
    paint(
        &mut scene,
        frame,
        Bounds {
            x: 20,
            y: 30,
            width: 200,
            height: 200,
        },
    )
    .unwrap();
    let rectangles = scene.rectangles();
    assert_eq!(rectangles.len(), 5);
    assert_eq!(rectangles[0].bounds, [20., 30., 200., 200.]);
    assert_eq!(rectangles[1].bounds, [20., 105., 200., 50.]);
    assert_eq!(rectangles[2].bounds, [70., 30., 100., 200.]);
    assert_eq!(rectangles[3].bounds, [20., 30., 200., 200.]);
    assert_eq!(rectangles[4].bounds, [20., 80., 200., 100.]);
    for (rectangle, alpha) in rectangles[1..].iter().zip([10, 20, 30, 40]) {
        assert_eq!(rectangle.uv, [0., 0., 1., 1.]);
        assert_eq!(
            rectangle.color,
            [96. / 255., 96. / 255., 96. / 255., alpha as f32 / 255.]
        );
    }
    scene.rect(0, 0, 5, 5, 0xffffff);
    assert_eq!(scene.rectangles().last().unwrap().bounds, [0., 0., 5., 5.]);
}

fn static_bmp() -> Vec<u8> {
    let mut data = vec![0; 58];
    data[..2].copy_from_slice(b"BM");
    data[2..6].copy_from_slice(&58u32.to_le_bytes());
    data[10..14].copy_from_slice(&54u32.to_le_bytes());
    data[14..18].copy_from_slice(&40u32.to_le_bytes());
    data[18..22].copy_from_slice(&1i32.to_le_bytes());
    data[22..26].copy_from_slice(&1i32.to_le_bytes());
    data[26..28].copy_from_slice(&1u16.to_le_bytes());
    data[28..30].copy_from_slice(&24u16.to_le_bytes());
    data[54..57].copy_from_slice(&[3, 2, 1]);
    data
}

#[test]
fn actual_static_crop_aliases_share_cache_and_keep_canvas_scene_geometry() {
    let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    files
        .insert("song/chart.bms", b"#BPM 120".to_vec())
        .unwrap();
    files.insert("song/still.bmp", static_bmp()).unwrap();
    let source = files.scope("song/chart.bms").unwrap();
    let chart = beatkernel_bms::parse("#BMP01 still.bmp\n#BMP02 ./still.bmp\n#BGA01 01 0 0 1 1 1 0\n#@BGA02 02 0 0 1 1 1 0\n#CANVASSIZE 4 2\n#00004:01\n#00007:02", ParseOptions::default()).unwrap();
    let bank = Arc::new(
        ImageAssets::prepare_from_source(&source, &chart, ImageAssetLimits::default()).unwrap(),
    );
    assert!(Arc::ptr_eq(
        bank.get(ImageId(1)).unwrap(),
        bank.get_layer(ImageId(2)).unwrap()
    ));
    let mut owner = Owner::default();
    let mut cache = BgaTextureCache::default();
    let states = [BgaState {
        base: Some(ImageId(1)),
        layer: Some(ImageId(2)),
        ..BgaState::default()
    }];
    let frame = cache.sync(Some(&bank), &states, &mut owner).unwrap()[0];
    assert_eq!(frame.base, frame.layer);
    assert_eq!(owner.actions, ["upload"]);
    cache.sync(Some(&bank), &states, &mut owner).unwrap();
    assert_eq!(owner.actions, ["upload"]);
    let mut scene = Scene::new(100, 100);
    paint(
        &mut scene,
        frame,
        Bounds {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        },
    )
    .unwrap();
    assert_eq!(scene.rectangles()[1].bounds, [0., 25., 100., 50.]);
    assert_eq!(scene.rectangles()[2].bounds, [0., 25., 100., 50.]);
    cache.clear(&mut owner).unwrap();
}

#[test]
fn feed_uses_exact_original_member_target_and_watermark_not_arrival_time() {
    let mut feed = NativeVideoFrames::default();
    let key = session(1);
    let song = ns(1_000_000_003);
    let target = key.target(song).unwrap();
    assert_eq!(target, ns(999_999_903));
    feed.request(0, key, target).unwrap();
    feed.admit(
        0,
        VideoDecodeEvent::Frame(frame(key, 999_999_902, 1, 1, 10)),
    )
    .unwrap();
    feed.admit(
        0,
        VideoDecodeEvent::Frame(frame(key, 999_999_904, 2, 1, 20)),
    )
    .unwrap();
    assert!(feed.selected(0).is_none());
    feed.admit(
        0,
        VideoDecodeEvent::Watermark {
            session: key,
            through: target,
        },
    )
    .unwrap();
    assert_eq!(feed.selected(0).unwrap().pts, ns(999_999_902));
    feed.request(0, key, target).unwrap(); // pause repeats exactly the committed member target
    assert_eq!(feed.selected(0).unwrap().revision, 1);
}

#[test]
fn feed_seek_requires_generation_and_fences_stale_frame_watermark_and_end() {
    let mut feed = NativeVideoFrames::default();
    let old = session(1);
    feed.request(0, old, ns(100)).unwrap();
    feed.admit(0, VideoDecodeEvent::Frame(frame(old, 90, 1, 1, 10)))
        .unwrap();
    feed.admit(
        0,
        VideoDecodeEvent::Watermark {
            session: old,
            through: ns(100),
        },
    )
    .unwrap();
    assert!(feed.request(0, old, ns(20)).is_err());
    let current = session(2);
    feed.request(0, current, ns(20)).unwrap();
    feed.admit(0, VideoDecodeEvent::Frame(frame(old, 0, 99, 1, 99)))
        .unwrap();
    feed.admit(
        0,
        VideoDecodeEvent::Watermark {
            session: old,
            through: ns(1000),
        },
    )
    .unwrap();
    feed.admit(
        0,
        VideoDecodeEvent::End {
            session: old,
            end: None,
        },
    )
    .unwrap();
    assert!(feed.selected(0).is_none());
    feed.admit(0, VideoDecodeEvent::Frame(frame(current, 10, 1, 1, 20)))
        .unwrap();
    feed.admit(
        0,
        VideoDecodeEvent::End {
            session: current,
            end: Some(ns(15)),
        },
    )
    .unwrap();
    assert_eq!(feed.selected(0).unwrap().image.pixels(), &[20, 0, 0, 255]);
    feed.request(0, current, ns(1000)).unwrap();
    assert_eq!(feed.selected(0).unwrap().pts, ns(10));
    feed.retire(0);
    assert!(feed.selected(0).is_none());
}

#[test]
fn movie_registration_and_poor_miss_stamp_reach_feed_without_host_clock() {
    let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    files
        .insert("song/chart.bms", b"#BPM 120".to_vec())
        .unwrap();
    files
        .insert("song/clip.mp4", b"encoded-acquisition-fixture".to_vec())
        .unwrap();
    let chart = beatkernel_bms::parse(
        "#BMP00 clip.mp4\n#BMP01 ./clip.mp4\n#00004:01",
        ParseOptions::default(),
    )
    .unwrap();
    let bank = VideoAssets::prepare_from_source(
        &files.scope("song/chart.bms").unwrap(),
        &chart,
        VideoAssetLimits::default(),
    )
    .unwrap();
    assert_eq!(
        bank.get(ImageId(0)).unwrap().resource,
        bank.get(ImageId(1)).unwrap().resource
    );
    let mut poor = session(1);
    poor.channel = BgaChannel::Poor;
    poor.image = ImageId(0);
    poor.activated_at = ns(4_000_000_001); // accepted miss, not initial BMP00 selection
    poor.ordinal = None;
    let target = poor.target(ns(4_000_000_009)).unwrap();
    assert_eq!(target, ns(8));
    let mut feed = NativeVideoFrames::default();
    feed.request(3, poor, target).unwrap();
    feed.admit(3, VideoDecodeEvent::Frame(frame(poor, 7, 1, 1, 30)))
        .unwrap();
    feed.admit(
        3,
        VideoDecodeEvent::Watermark {
            session: poor,
            through: target,
        },
    )
    .unwrap();
    assert_eq!(
        feed.selected(3).unwrap().session.activated_at,
        poor.activated_at
    );
    assert_eq!(feed.selected(3).unwrap().pts, ns(7));
    assert!(feed.selected(0).is_none());
    assert!(feed.request(16, poor, target).is_err());
}
