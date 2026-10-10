//! Detached preparation ownership never holds the Session borrow or UI snapshot lock.
use super::*;
use crate::preparation::{
    PreparationCache, PreparationCacheLimits, PreparationProgress, PreparationStage,
};

fn cache() -> Arc<Mutex<PreparationCache>> {
    Arc::new(Mutex::new(
        PreparationCache::new(PreparationCacheLimits::default()).unwrap(),
    ))
}
fn progress(stage: PreparationStage) -> PreparationProgress {
    PreparationProgress {
        stage,
        completed_assets: 0,
        total_assets: 1,
    }
}

fn prepared_mailbox_retry() -> crate::session_launch::SessionLaunch {
    crate::session_launch::SessionLaunch::new(
        [
            "--chart",
            "mailbox.bms",
            "--record-replay",
            "records/mailbox.bkr",
            "--rate",
            "48000",
        ]
        .map(str::to_owned)
        .to_vec(),
    )
    .unwrap()
    .prepare_with(|requested| {
        let mut accepted = requested.to_vec();
        accepted.extend(
            [
                "--channel-policy",
                "exact",
                "--output",
                "accepted-output",
                "--buffer-frames",
                "128",
            ]
            .map(str::to_owned),
        );
        Ok(accepted)
    })
    .unwrap()
    .retry()
    .unwrap()
    .retry()
    .unwrap()
}

fn assert_same_mailbox_retry(
    actual: &crate::session_launch::SessionLaunch,
    expected: &crate::session_launch::SessionLaunch,
) {
    assert!(actual.is_prepared());
    assert_eq!(actual.original_args(), expected.original_args());
    assert_eq!(
        actual.prepared_original_args(),
        expected.prepared_original_args()
    );
    assert_eq!(actual.args(), expected.args());
    assert_eq!(actual.attempt(), 2);
    assert_eq!(actual.args()[3], "records/mailbox.retry2.bkr");
}

#[test]
fn native_launch_mailbox_contention_is_pending_and_exact_retry_drains_once_after_release() {
    let expected = prepared_mailbox_retry();
    let (publisher, viewer) = channel();
    with_publisher(publisher, || pin_native_launch(expected.clone())).unwrap();
    let producer_guard = viewer.0.native_launch.lock().unwrap();
    assert_same_mailbox_retry(producer_guard.as_ref().unwrap(), &expected);
    assert!(viewer.take_native_launch().unwrap().is_none());
    assert_same_mailbox_retry(producer_guard.as_ref().unwrap(), &expected);
    drop(producer_guard);
    let drained = viewer.take_native_launch().unwrap().unwrap();
    assert_same_mailbox_retry(&drained, &expected);
    assert!(viewer.take_native_launch().unwrap().is_none());
}

#[test]
fn poisoned_native_launch_mailbox_refuses_without_consuming_queued_accepted_retry() {
    let expected = prepared_mailbox_retry();
    let (publisher, viewer) = channel();
    with_publisher(publisher, || pin_native_launch(expected.clone())).unwrap();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let producer_guard = viewer.0.native_launch.lock().unwrap();
        assert_same_mailbox_retry(producer_guard.as_ref().unwrap(), &expected);
        panic!("fixture producer failed while publishing its native invocation");
    }));
    assert!(panic.is_err());
    let error = viewer.take_native_launch().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
    let diagnostic = error.to_string();
    assert!(diagnostic.contains("native"), "{diagnostic}");
    assert!(diagnostic.contains("publication"), "{diagnostic}");
    let retained = viewer.0.native_launch.lock().unwrap_err().into_inner();
    assert_same_mailbox_retry(retained.as_ref().unwrap(), &expected);
}

struct CancelVisualSource {
    viewer: PlayerViewer,
    calls: Arc<Mutex<Vec<String>>>,
    cancel_resolve: bool,
    cancel_read: bool,
    native_movie: Option<std::path::PathBuf>,
}

impl crate::asset_source::AssetSource for CancelVisualSource {
    fn resolve(
        &self,
        name: &str,
        _policy: crate::asset_paths::AssetPathPolicy,
    ) -> io::Result<std::path::PathBuf> {
        self.calls.lock().unwrap().push(format!("resolve:{name}"));
        if self.cancel_resolve {
            self.viewer.cancel();
        }
        Ok(self.native_movie.clone().unwrap_or_else(|| name.into()))
    }
    fn read<'a>(
        &'a self,
        key: &std::path::Path,
        max_bytes: usize,
    ) -> io::Result<std::borrow::Cow<'a, [u8]>> {
        assert!(
            self.native_movie.is_none(),
            "NativeFile must not copy movie bytes"
        );
        self.calls
            .lock()
            .unwrap()
            .push(format!("read:{}", key.display()));
        if self.cancel_read {
            self.viewer.cancel();
        }
        let mut bitmap = vec![0; 58];
        bitmap[..2].copy_from_slice(b"BM");
        bitmap[2..6].copy_from_slice(&58u32.to_le_bytes());
        bitmap[10..14].copy_from_slice(&54u32.to_le_bytes());
        bitmap[14..18].copy_from_slice(&40u32.to_le_bytes());
        bitmap[18..22].copy_from_slice(&1i32.to_le_bytes());
        bitmap[22..26].copy_from_slice(&1i32.to_le_bytes());
        bitmap[26..28].copy_from_slice(&1u16.to_le_bytes());
        bitmap[28..30].copy_from_slice(&24u16.to_le_bytes());
        bitmap[34..38].copy_from_slice(&4u32.to_le_bytes());
        bitmap[54..57].copy_from_slice(&[0, 0, 255]);
        assert!(bitmap.len() <= max_bytes);
        Ok(std::borrow::Cow::Owned(bitmap))
    }
}

fn assert_visual_unpublished(viewer: &PlayerViewer) -> PlayerSnapshot {
    let snapshot = viewer.take_latest().unwrap();
    assert!(snapshot.chart.is_none());
    assert!(snapshot.images.is_none());
    #[cfg(not(target_arch = "wasm32"))]
    assert!(snapshot.movies.is_none());
    assert!(snapshot.players.is_empty());
    assert!(viewer
        .preparation_progress()
        .is_none_or(|p| p.stage != PreparationStage::Ready));
    snapshot
}

#[test]
fn registration_cancelled_during_first_image_resource_does_not_process_later_resources_or_publish()
{
    for cancel_on_read in [false, true] {
        let source = beatkernel_bms::parse(
            "#BPM 120\n#BMP01 first.bmp\n#BMP02 later.bmp\n#00004:0102\n",
            Default::default(),
        )
        .unwrap();
        let compiled = source.compile().unwrap();
        let (publisher, viewer) = channel();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let assets = CancelVisualSource {
            viewer: PlayerViewer(viewer.0.clone()),
            calls: calls.clone(),
            cancel_resolve: !cancel_on_read,
            cancel_read: cancel_on_read,
            native_movie: None,
        };
        with_publisher(publisher, || {
            assert!(register_chart_with_assets(
                &source,
                &compiled.chart,
                &[PlayerId(1)],
                None,
                None,
                Some(&assets),
                #[cfg(not(target_arch = "wasm32"))]
                |_| panic!("cancelled image registration must not construct a movie service"),
            )
            .is_err());
            assert!(cancelled());
            SESSION.with(|session| assert!(!session.borrow().as_ref().unwrap().chart_published));
            Ok(())
        })
        .unwrap();
        let expected = if cancel_on_read {
            vec!["resolve:first.bmp", "read:first.bmp"]
        } else {
            vec!["resolve:first.bmp"]
        };
        assert_eq!(*calls.lock().unwrap(), expected);
        assert_visual_unpublished(&viewer);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_file_movie_registration_cancelled_during_first_resolve_never_acquires_or_publishes() {
    let source = beatkernel_bms::parse(
        "#BPM 120\n#BMP01 first.mp4\n#BMP02 later.mp4\n#00004:0102\n",
        Default::default(),
    )
    .unwrap();
    let compiled = source.compile().unwrap();
    let (publisher, viewer) = channel();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let assets = CancelVisualSource {
        viewer: PlayerViewer(viewer.0.clone()),
        calls: calls.clone(),
        cancel_resolve: true,
        cancel_read: false,
        native_movie: Some("missing-first.mp4".into()),
    };
    with_publisher(publisher, || {
        let result = register_chart_with_assets(
            &source,
            &compiled.chart,
            &[PlayerId(1)],
            None,
            None,
            Some(&assets),
            |_| panic!("cancelled movie registration must not construct a service"),
        );
        assert!(result.unwrap_err().to_string().contains("cancelled"));
        assert!(cancelled());
        Ok(())
    })
    .unwrap();
    assert_eq!(*calls.lock().unwrap(), vec!["resolve:first.mp4"]);
    assert_visual_unpublished(&viewer);
}

#[cfg(not(target_arch = "wasm32"))]
struct MovieFile(std::path::PathBuf);
#[cfg(not(target_arch = "wasm32"))]
impl MovieFile {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "beatkernel-staged-movie-{}-{}.mp4",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, b"fixture native descriptor; no decoder is invoked").unwrap();
        Self(path)
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl Drop for MovieFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cancelled_staged_movie_service_joins_before_worker_terminal_publication() {
    let movie = MovieFile::new();
    let (publisher, viewer) = channel();
    let assets = CancelVisualSource {
        viewer: PlayerViewer(viewer.0.clone()),
        calls: Arc::new(Mutex::new(Vec::new())),
        cancel_resolve: false,
        cancel_read: false,
        native_movie: Some(movie.0.clone()),
    };
    let cancel_viewer = PlayerViewer(viewer.0.clone());
    let (joining_tx, joining) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release) = std::sync::mpsc::sync_channel(1);
    let (finished_tx, finished) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let result = with_publisher(publisher, || {
            let source = beatkernel_bms::parse(
                "#BPM 120\n#BMP01 first.mp4\n#00004:01\n",
                Default::default(),
            )
            .unwrap();
            let compiled = source.compile().unwrap();
            register_chart_with_assets(
                &source,
                &compiled.chart,
                &[PlayerId(1)],
                None,
                None,
                Some(&assets),
                move |descriptors| {
                    assert!(!descriptors.is_empty());
                    let bank =
                        crate::video_native_bank::NativeVideoBank::prepare_with_service_for_test(
                            descriptors,
                            move |shutdown| {
                                let deadline =
                                    std::time::Instant::now() + std::time::Duration::from_secs(5);
                                while !*shutdown.borrow() {
                                    assert!(
                                        std::time::Instant::now() < deadline,
                                        "staged bank never requested shutdown"
                                    );
                                    std::thread::yield_now();
                                }
                                joining_tx.send(()).unwrap();
                                release
                                    .recv_timeout(std::time::Duration::from_secs(5))
                                    .map_err(|e| e.to_string())?;
                                Ok(())
                            },
                        )?;
                    cancel_viewer.cancel();
                    Ok(bank)
                },
            )
            .map_err(|error| error.to_string())
        });
        finished_tx.send(result).unwrap();
    });
    joining
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(matches!(
        finished.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    assert!(!worker.is_finished());
    let pending = viewer.take_latest().unwrap();
    assert_eq!(pending.status, PlayerStatus::Stopping);
    assert!(pending.chart.is_none());
    assert!(pending.movies.is_none());
    assert!(viewer
        .preparation_progress()
        .is_none_or(|p| p.stage != PreparationStage::Ready));
    release_tx.send(()).unwrap();
    assert!(finished
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap()
        .is_err());
    worker.join().unwrap();
    let terminal = assert_visual_unpublished(&viewer);
    assert!(terminal.cancelled);
    assert!(matches!(terminal.status, PlayerStatus::Failed(_)));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn installed_movie_service_transfers_cleanup_to_session_and_ui_arc_drop_does_not_join() {
    let movie = MovieFile::new();
    let (publisher, viewer) = channel();
    let assets = CancelVisualSource {
        viewer: PlayerViewer(viewer.0.clone()),
        calls: Arc::new(Mutex::new(Vec::new())),
        cancel_resolve: false,
        cancel_read: false,
        native_movie: Some(movie.0.clone()),
    };
    let (installed_tx, installed) = std::sync::mpsc::sync_channel(1);
    let (finish_tx, finish) = std::sync::mpsc::sync_channel(1);
    let (joining_tx, joining) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release) = std::sync::mpsc::sync_channel(1);
    let (finished_tx, finished) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let result = with_publisher(publisher, || {
            let source = beatkernel_bms::parse(
                "#BPM 120\n#BMP01 first.mp4\n#00004:01\n",
                Default::default(),
            )
            .unwrap();
            let compiled = source.compile().unwrap();
            register_chart_with_assets(
                &source,
                &compiled.chart,
                &[PlayerId(1)],
                None,
                None,
                Some(&assets),
                move |descriptors| {
                    crate::video_native_bank::NativeVideoBank::prepare_with_service_for_test(
                        descriptors,
                        move |shutdown| {
                            let deadline =
                                std::time::Instant::now() + std::time::Duration::from_secs(5);
                            while !*shutdown.borrow() {
                                assert!(
                                    std::time::Instant::now() < deadline,
                                    "installed bank never requested shutdown"
                                );
                                std::thread::yield_now();
                            }
                            joining_tx.send(()).unwrap();
                            release
                                .recv_timeout(std::time::Duration::from_secs(5))
                                .map_err(|e| e.to_string())?;
                            Ok(())
                        },
                    )
                },
            )
            .map_err(|error| error.to_string())?;
            SESSION.with(|session| {
                let session = session.borrow();
                let current = session.as_ref().unwrap();
                assert!(current.chart_published);
                assert!(current.snapshot.movies.is_some());
            });
            installed_tx.send(()).unwrap();
            finish
                .recv_timeout(std::time::Duration::from_secs(5))
                .map_err(|e| e.to_string())?;
            Ok(())
        });
        finished_tx.send(result).unwrap();
    });
    installed
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let snapshot = viewer.take_latest().unwrap();
    assert!(snapshot.chart.is_some());
    assert!(snapshot.movies.is_some());
    // UI releases its actual published Arc while the game still owns the service.
    drop(snapshot);
    assert!(matches!(
        joining.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    finish_tx.send(()).unwrap();
    joining
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(matches!(
        finished.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    assert!(!worker.is_finished());
    release_tx.send(()).unwrap();
    finished
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    assert_eq!(viewer.take_latest().unwrap().status, PlayerStatus::Finished);
}

#[test]
fn preparation_context_is_detached_from_tls_and_snapshot_contention_is_not_failure() {
    let (publisher, viewer) = channel();
    let owner = cache();
    with_publisher(publisher, || {
        pin_native_launch_with_preparation(
            crate::session_launch::SessionLaunch::new(vec![
                "--chart".into(),
                "fixture.bms".into(),
            ])?,
            Some(owner.clone()),
        )?;
        let context = preparation_context().unwrap();
        assert!(Arc::ptr_eq(&context.cache().unwrap(), &owner));
        let locked = viewer.0.latest.lock().unwrap();
        SESSION.with(|session| {
            let _borrow = session.borrow_mut();
            context
                .check(progress(PreparationStage::Decode))
                .map_err(|error| error.to_string())
        })?;
        drop(locked);
        context
            .check(progress(PreparationStage::Ready))
            .map_err(|error| error.to_string())?;
        assert_eq!(
            viewer.preparation_progress().unwrap().stage,
            PreparationStage::Ready
        );
        assert!(!cancelled());
        assert!(matches!(
            SESSION.with(|session| session.borrow().as_ref().unwrap().snapshot.status.clone()),
            PlayerStatus::Loading
        ));
        Ok(())
    })
    .unwrap();
}

#[test]
fn retained_preparation_context_refuses_after_its_session_ends_without_cancelling_cache_owner() {
    let owner = cache();
    let (publisher, _) = channel();
    let mut detached = None;
    with_publisher(publisher, || {
        pin_native_launch_with_preparation(
            crate::session_launch::SessionLaunch::new(vec![
                "--chart".into(),
                "fixture.bms".into(),
            ])?,
            Some(owner.clone()),
        )?;
        detached = preparation_context();
        detached
            .as_ref()
            .unwrap()
            .check(progress(PreparationStage::Parse))
            .map_err(|error| error.to_string())?;
        Ok(())
    })
    .unwrap();
    assert!(detached
        .as_ref()
        .unwrap()
        .check(progress(PreparationStage::Ready))
        .is_err());
    let (publisher, _) = channel();
    with_publisher(publisher, || {
        pin_native_launch_with_preparation(
            crate::session_launch::SessionLaunch::new(vec![
                "--chart".into(),
                "fixture.bms".into(),
            ])?,
            Some(owner.clone()),
        )?;
        assert!(Arc::ptr_eq(
            &preparation_context().unwrap().cache().unwrap(),
            &owner
        ));
        preparation_context()
            .unwrap()
            .check(progress(PreparationStage::Parse))
            .map_err(|error| error.to_string())
    })
    .unwrap();
}

#[test]
fn cancellation_refuses_every_preparation_stage_and_legacy_context_has_no_cache() {
    for stage in [
        PreparationStage::Parse,
        PreparationStage::Compile,
        PreparationStage::Resolve,
        PreparationStage::Read,
        PreparationStage::Decode,
        PreparationStage::Channels,
        PreparationStage::Admit,
        PreparationStage::Visual,
        PreparationStage::Ready,
    ] {
        let (publisher, viewer) = channel();
        with_publisher(publisher, || {
            pin_native_launch(crate::session_launch::SessionLaunch::new(vec![
                "--chart".into(),
                "fixture.bms".into(),
            ])?)?;
            let context = preparation_context().unwrap();
            assert!(context.cache().is_none());
            viewer.cancel();
            assert!(context.check(progress(stage)).is_err());
            assert!(cancelled());
            Ok(())
        })
        .unwrap();
    }
}
