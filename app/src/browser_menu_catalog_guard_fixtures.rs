//! Cold guard-state arrangement; refusal runs through the actual public setter.
use super::*;

fn records_owner_with_parent() -> (BrowserMenu, MenuSnapshot) {
    let mut menu = BrowserMenu::new(u64::MAX).unwrap();
    menu.navigate_with_fields(
        menu.token(),
        ScreenRoute::Settings,
        vec!["original parent draft".into()],
    )
    .unwrap();
    let parent = menu.snapshot();
    menu.navigate_with_fields(
        menu.token(),
        ScreenRoute::Records,
        ["records/alpha.bkr", "records/曲.bkr", "records/omega.bkr"]
            .map(str::to_owned)
            .to_vec(),
    )
    .unwrap();
    menu.select(menu.token(), 1).unwrap();
    assert_eq!(
        menu.accept_action(menu.token(), 7, 50).unwrap(),
        MenuEffect::Bridge(50)
    );
    (menu, parent)
}

#[test]
fn pending_records_catalog_refuses_changed_and_unchanged_setters_without_consuming_owner_state() {
    let (mut menu, parent) = records_owner_with_parent();
    menu.pending = true;
    let waiting = menu.snapshot();
    assert_eq!(waiting.fields[waiting.selected], "records/曲.bkr");
    for fields in [
        [
            "records/new.bkr",
            "records/alpha.bkr",
            "records/曲.bkr",
            "records/omega.bkr",
        ]
        .map(str::to_owned)
        .to_vec(),
        waiting.fields.clone(),
        Vec::new(),
    ] {
        assert!(menu.set_fields(waiting.token, fields).is_err());
        assert_eq!(menu.snapshot(), waiting);
        assert_eq!(menu.token(), waiting.token);
        assert_eq!(menu.last_action, 7);
    }
    menu.pending = false;
    menu.set_fields(
        menu.token(),
        [
            "records/new.bkr",
            "records/alpha.bkr",
            "records/曲.bkr",
            "records/omega.bkr",
        ]
        .map(str::to_owned)
        .to_vec(),
    )
    .unwrap();
    assert_eq!(menu.selected(), 2);
    assert_eq!(menu.fields()[2], "records/曲.bkr");
    assert!(menu.accept_action(menu.token(), 7, 50).is_err());
    assert_eq!(
        menu.accept_action(menu.token(), 8, 50).unwrap(),
        MenuEffect::Bridge(50)
    );
    menu.back(menu.token()).unwrap();
    let restored = menu.snapshot();
    assert_eq!(restored.route, ScreenRoute::Settings);
    assert_eq!(restored.token.screen, parent.token.screen);
    assert_eq!(restored.fields, parent.fields);
    assert_eq!(restored.selected, parent.selected);
}

#[test]
fn exhausted_records_catalog_refuses_before_identity_reselection_and_preserves_parent_history() {
    let (mut menu, parent) = records_owner_with_parent();
    let recoverable_revision = menu.token().revision;
    menu.revision = u64::MAX;
    let exhausted = menu.snapshot();
    for fields in [
        [
            "records/new.bkr",
            "records/alpha.bkr",
            "records/曲.bkr",
            "records/omega.bkr",
        ]
        .map(str::to_owned)
        .to_vec(),
        exhausted.fields.clone(),
        Vec::new(),
    ] {
        assert!(menu.set_fields(exhausted.token, fields).is_err());
        assert_eq!(menu.snapshot(), exhausted);
        assert_eq!(menu.token().revision, u64::MAX);
        assert_eq!(menu.selected(), 1);
        assert_eq!(menu.fields()[1], "records/曲.bkr");
        assert_eq!(menu.last_action, 7);
    }
    // Restore only the arranged counter to inspect the preserved navigation
    // owner; production never recovers an exhausted revision this way.
    menu.revision = recoverable_revision;
    assert_eq!(
        menu.accept_action(menu.token(), 8, 50).unwrap(),
        MenuEffect::Bridge(50)
    );
    menu.back(menu.token()).unwrap();
    let restored = menu.snapshot();
    assert_eq!(restored.route, ScreenRoute::Settings);
    assert_eq!(restored.token.screen, parent.token.screen);
    assert_eq!(restored.fields, parent.fields);
    assert_eq!(restored.selected, parent.selected);
}
