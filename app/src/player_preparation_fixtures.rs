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
