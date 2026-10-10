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
