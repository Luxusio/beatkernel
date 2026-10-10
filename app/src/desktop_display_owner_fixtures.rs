//! Actual Display scope owns both draft and retained geometry without native IO.
use super::*;

fn display_fixture() -> Desktop {
    let mut app = tests::lifecycle_fixture();
    app.open_settings();
    app.sync_ime();
    app.draw().unwrap();
    app.open_display();
    app.sync_ime();
    assert_eq!(app.navigator.route(), ScreenRoute::Display);
    app
}

#[test]
fn display_owner_mounts_lazily_with_common_id_and_preserves_edited_draft() {
    let mut app = display_fixture();
    let screen = app.navigator.active_id().unwrap();
    assert_eq!(app.display.as_ref().unwrap().id(), screen);
    assert!(app.display.as_ref().unwrap().view.is_none());
    app.display.as_mut().unwrap().draft.selected = 2;
    app.sync_ime();
    *app.text_editor_mut(TextField::Display(2)).unwrap() = LineEditor::new("180", 32).unwrap();
    app.draw().unwrap();
    let owner = app.display.as_ref().unwrap();
    assert_eq!(owner.view.as_ref().unwrap().id(), screen);
    assert_eq!(owner.draft.selected, 2);
    assert_eq!(owner.draft.editors[2].value(), "180");
    assert_eq!(app.text_target().unwrap().screen, screen);
    let geometry = Arc::clone(app.scene.geometry_stamp().0);
    let revision = app.scene.geometry_stamp().1;
    app.draw().unwrap();
    assert!(Arc::ptr_eq(&geometry, app.scene.geometry_stamp().0));
    assert_eq!(revision, app.scene.geometry_stamp().1);
    assert!(!app.display.as_ref().unwrap().view.as_ref().unwrap().dirty());
    assert!(app.window.is_none() && app.renderer.is_none() && app.game.is_none());
}

#[test]
fn display_back_disposes_child_and_preserves_retained_settings_geometry_and_draft() {
    let mut app = display_fixture();
    let parent = app.settings.as_ref().unwrap().id();
    let parent_permit = app.settings.as_ref().unwrap().task_permit();
    let original = app.settings.as_ref().unwrap().presentation;
    let child = app.display.as_ref().unwrap().id();
    let child_permit = app.display.as_ref().unwrap().task_permit();
    assert!(!parent_permit.is_active());
    assert!(!parent_permit.is_cancelled());
    app.draw().unwrap();
    assert_eq!(
        app.display.as_ref().unwrap().view.as_ref().unwrap().id(),
        child
    );
    app.display.as_mut().unwrap().draft.editors[2] = LineEditor::new("180", 32).unwrap();
    app.back();
    assert!(app.display.is_none());
    assert!(child_permit.is_cancelled());
    assert!(parent_permit.is_active());
    assert_eq!(app.settings.as_ref().unwrap().id(), parent);
    assert_eq!(app.settings_view.as_ref().unwrap().id(), parent);
    assert_eq!(app.settings.as_ref().unwrap().presentation, original);
    app.open_display();
    assert_ne!(app.display.as_ref().unwrap().id(), child);
    assert!(app.display.as_ref().unwrap().view.is_none());
    assert_eq!(
        app.display.as_ref().unwrap().draft.editors[2].value(),
        original.fps.to_string()
    );
}

#[test]
fn display_finish_updates_only_settings_draft_and_cancels_owner_permit() {
    let mut app = display_fixture();
    let before = app.options.display();
    let permit = app.display.as_ref().unwrap().task_permit();
    app.display.as_mut().unwrap().draft.editors[2] = LineEditor::new("180", 32).unwrap();
    app.draw().unwrap();
    app.finish_display();
    assert_eq!(app.navigator.route(), ScreenRoute::Settings);
    assert!(app.display.is_none());
    assert!(permit.is_cancelled());
    assert_eq!(app.settings.as_ref().unwrap().presentation.fps, 180);
    assert_eq!(app.options.display(), before);
    app.back();
    assert_eq!(app.navigator.route(), ScreenRoute::Selection);
    assert_eq!(
        app.options.display(),
        before,
        "Settings Back cancels the unapplied Display draft"
    );
}

#[test]
fn display_failed_finish_keeps_same_draft_and_view_owner_until_close() {
    let mut app = display_fixture();
    app.draw().unwrap();
    let screen = app.display.as_ref().unwrap().id();
    let permit = app.display.as_ref().unwrap().task_permit();
    let original = app.settings.as_ref().unwrap().presentation;
    app.display.as_mut().unwrap().draft.editors[2] = LineEditor::new("invalid", 32).unwrap();
    app.finish_display();
    assert_eq!(app.navigator.route(), ScreenRoute::Display);
    let owner = app.display.as_ref().unwrap();
    assert_eq!(owner.id(), screen);
    assert_eq!(owner.view.as_ref().unwrap().id(), screen);
    assert_eq!(owner.draft.editors[2].value(), "invalid");
    assert!(owner.draft.error.is_some());
    assert!(permit.is_active());
    assert_eq!(app.settings.as_ref().unwrap().presentation, original);
    app.request_close();
    assert!(app.display.is_none());
    assert!(permit.is_cancelled());
}

#[test]
fn display_finished_draft_reaches_options_only_after_explicit_settings_apply() {
    let mut app = display_fixture();
    let before = app.options.display();
    let child_permit = app.display.as_ref().unwrap().task_permit();
    let parent_permit = app.settings.as_ref().unwrap().task_permit();
    app.display.as_mut().unwrap().draft.editors[2] = LineEditor::new("180", 32).unwrap();
    app.finish_display();
    assert!(child_permit.is_cancelled());
    assert!(parent_permit.is_active());
    assert_eq!(app.options.display(), before);
    app.apply_settings();
    assert_eq!(app.navigator.route(), ScreenRoute::Selection);
    assert_eq!(app.options.display().fps, 180);
    assert!(app.display.is_none() && app.settings.is_none());
    assert!(parent_permit.is_cancelled());
}

#[test]
fn display_suspension_keeps_owned_draft_and_view_but_rejects_stale_ime() {
    let mut app = display_fixture();
    app.draw().unwrap();
    let screen = app.display.as_ref().unwrap().id();
    let value = app.display.as_ref().unwrap().draft.editors[0].clone();
    app.ime_event(Ime::Enabled);
    app.ime_event(Ime::Preedit("old".into(), None));
    assert!(app.ime.preview.is_some());
    app.navigator.suspend();
    app.synchronize_panel_lifecycles();
    app.sync_ime();
    assert!(app.ime.target.is_none() && app.ime.preview.is_none());
    assert_eq!(
        app.display.as_ref().unwrap().view.as_ref().unwrap().id(),
        screen
    );
    app.navigator.resume();
    app.synchronize_panel_lifecycles();
    app.sync_ime();
    app.ime_event(Ime::Commit("stale".into()));
    assert_eq!(app.display.as_ref().unwrap().draft.editors[0], value);
    app.draw().unwrap();
    assert_eq!(app.navigator.active_id(), Some(screen));
    assert_eq!(
        app.display.as_ref().unwrap().view.as_ref().unwrap().id(),
        screen
    );
    assert!(app.display.as_ref().unwrap().task_permit().is_active());
}
