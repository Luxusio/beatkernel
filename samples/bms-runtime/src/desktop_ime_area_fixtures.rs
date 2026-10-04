//! Deferred candidate-area fixtures over actual composed Desktop field geometry.
//! No native window or candidate-popup submission is created by these fixtures.
use super::*;

fn field_app(field: TextField) -> Desktop {
    let mut app = super::tests::lifecycle_fixture();
    if field == TextField::Search {
        app.set_search_focus(true);
    } else {
        app.open_settings();
        match field {
            TextField::Setting(index) => app.settings.as_mut().unwrap().select(index).unwrap(),
            TextField::Profile => app.settings.as_mut().unwrap().profile_focused = true,
            TextField::Display(index) => {
                app.open_display();
                app.display.as_mut().unwrap().selected = index;
            }
            TextField::PracticeStart | TextField::PracticeEnd => {
                app.open_practice();
                app.practice.as_mut().unwrap().end_focused = field == TextField::PracticeEnd;
            }
            TextField::RecordDirectory => app.open_records(),
            TextField::Search => unreachable!(),
        }
    }
    app.sync_ime();
    assert_eq!(app.text_target().unwrap().field, field);
    app
}

#[test]
fn every_current_field_projects_its_actual_hit_rectangle_through_centered_bars() {
    // Literal layout bounds are independent expectations for the actual composer.
    // Width and height remain unchanged in the two one-to-one letterbox cases.
    for (field, control, identity, wide, tall) in [
        (
            TextField::Search,
            80,
            [440, 102, 490, 34],
            [920, 102, 490, 34],
            [440, 462, 490, 34],
        ),
        (
            TextField::Setting(0),
            1000,
            [280, 120, 650, 32],
            [760, 120, 650, 32],
            [280, 480, 650, 32],
        ),
        (
            TextField::Setting(11),
            1011,
            [280, 159, 650, 32],
            [760, 159, 650, 32],
            [280, 519, 650, 32],
        ),
        (
            TextField::Profile,
            15,
            [160, 558, 770, 34],
            [640, 558, 770, 34],
            [160, 918, 770, 34],
        ),
        (
            TextField::Display(0),
            40000,
            [280, 130, 650, 34],
            [760, 130, 650, 34],
            [280, 490, 650, 34],
        ),
        (
            TextField::Display(1),
            40001,
            [280, 205, 650, 34],
            [760, 205, 650, 34],
            [280, 565, 650, 34],
        ),
        (
            TextField::Display(2),
            40002,
            [280, 280, 650, 34],
            [760, 280, 650, 34],
            [280, 640, 650, 34],
        ),
        (
            TextField::Display(3),
            40003,
            [280, 355, 650, 34],
            [760, 355, 650, 34],
            [280, 715, 650, 34],
        ),
        (
            TextField::PracticeStart,
            70,
            [24, 150, 906, 40],
            [504, 150, 906, 40],
            [24, 510, 906, 40],
        ),
        (
            TextField::PracticeEnd,
            75,
            [24, 280, 906, 40],
            [504, 280, 906, 40],
            [24, 640, 906, 40],
        ),
        (
            TextField::RecordDirectory,
            58,
            [160, 108, 770, 34],
            [640, 108, 770, 34],
            [160, 468, 770, 34],
        ),
    ] {
        let mut app = field_app(field);
        let committed = app.text_editor(field).unwrap().clone();
        assert_eq!(
            app.ime_cursor_area([960, 720]),
            None,
            "composition must precede positioning"
        );
        app.draw().unwrap();
        let bounds = app
            .hits
            .iter()
            .find(|(id, _)| *id == ControlId(control))
            .unwrap()
            .1;
        assert_eq!(
            [bounds.x, bounds.y, bounds.width, bounds.height],
            identity.map(i64::from)
        );
        assert_eq!(app.ime_cursor_area([960, 720]), Some(identity));
        assert_eq!(app.ime_cursor_area([1920, 720]), Some(wide));
        assert_eq!(app.ime_cursor_area([960, 1440]), Some(tall));
        assert!(
            !app.ime.enabled,
            "position does not manufacture an OS enable receipt"
        );

        // Native composition endpoints do not substitute a guessed text caret
        // for the current field rectangle or change the committed draft.
        app.ime_event(Ime::Enabled);
        app.ime_event(Ime::Preedit("e\u{301}音".into(), Some((1, 3))));
        assert_eq!(app.ime_cursor_area([960, 720]), None);
        app.draw().unwrap();
        assert_eq!(app.ime_cursor_area([960, 720]), Some(identity));
        assert_eq!(app.text_editor(field).unwrap(), &committed);
        assert!(app.ime.cursor_area.is_none());
        assert!(app.window.is_none() && app.renderer.is_none() && app.game.is_none());
    }
}

#[test]
fn actual_search_bounds_round_outward_on_odd_surfaces_and_refuse_unrepresentable_native_positions()
{
    let mut app = field_app(TextField::Search);
    app.draw().unwrap();
    for (physical, expected) in [
        ([1001, 751], [458, 106, 512, 36]),
        ([777, 1001], [356, 291, 397, 28]),
        ([961, 721], [440, 102, 491, 34]),
        ([480, 360], [220, 51, 245, 17]),
        ([1, 1], [0, 0, 1, 1]),
        ([2, 2], [0, 0, 2, 1]),
    ] {
        assert_eq!(app.ime_cursor_area(physical), Some(expected));
        let [x, y, width, height] = expected;
        assert!(width > 0 && height > 0);
        assert!(u64::from(x) + u64::from(width) <= u64::from(physical[0]));
        assert!(u64::from(y) + u64::from(height) <= u64::from(physical[1]));
    }
    for physical in [[0, 0], [0, 720], [960, 0]] {
        assert_eq!(app.ime_cursor_area(physical), None);
    }
    // Search's projected origin still fits i32 here, but its right edge does
    // not; validating only the native position would admit an invalid area.
    assert_eq!(app.ime_cursor_area([u32::MAX, u32::MAX]), None);
    let mut profile = field_app(TextField::Profile);
    profile.draw().unwrap();
    // This genuine field's top is below the native signed-coordinate limit
    // only at ordinary sizes. A huge valid u32 surface must not wrap it.
    assert_eq!(profile.ime_cursor_area([u32::MAX, u32::MAX]), None);
    assert_eq!(
        profile.ime_cursor_area([960, 720]),
        Some([160, 558, 770, 34])
    );
    assert!(profile.ime.cursor_area.is_none());
}

#[test]
fn changed_targets_missing_hits_and_unavailable_lifecycles_require_fresh_composed_geometry() {
    let mut app = field_app(TextField::Search);
    app.draw().unwrap();
    let selection = app.navigator.active_id().unwrap();
    let old_hits = app.hits.clone();
    app.hits.retain(|(id, _)| *id != ControlId(80));
    assert_eq!(app.ime_cursor_area([960, 720]), None);
    app.invalidate_hits();
    app.draw().unwrap();
    assert_eq!(app.ime_cursor_area([960, 720]), Some([440, 102, 490, 34]));
    app.edit_search(None, Some("no chart matches this filter"));
    assert_eq!(app.ime_cursor_area([960, 720]), None);
    app.draw().unwrap();
    assert!(app.catalog_search.selected().is_none());
    assert_eq!(app.ime_cursor_area([960, 720]), Some([440, 102, 490, 34]));
    app.set_search_focus(false);
    assert_eq!(app.ime_cursor_area([960, 720]), None);
    app.open_settings();
    // Replay only previously composed evidence, never invented field bounds.
    app.hits = old_hits;
    app.painted_reactive = Some(selection);
    assert_eq!(app.ime_cursor_area([960, 720]), None);
    app.draw().unwrap();
    assert_eq!(app.ime_cursor_area([960, 720]), Some([280, 120, 650, 32]));
    app.settings.as_mut().unwrap().select(11).unwrap();
    assert_eq!(
        app.ime_cursor_area([960, 720]),
        None,
        "old target is not the selected field"
    );
    app.sync_ime();
    assert_eq!(app.ime_cursor_area([960, 720]), None);
    app.draw().unwrap();
    assert_eq!(app.ime_cursor_area([960, 720]), Some([280, 159, 650, 32]));

    for unavailable in 0..5 {
        let mut app = field_app(TextField::RecordDirectory);
        app.draw().unwrap();
        assert!(app.ime_cursor_area([960, 720]).is_some());
        match unavailable {
            0 => app.active = false,
            1 => app.occluded = true,
            2 => app.navigator.suspend(),
            3 => {
                let (owner, permit) = app.metadata_scope().unwrap();
                app.profile_io = Some(ProfileOperation {
                    owner,
                    permit,
                    worker: None,
                });
            }
            _ => app.request_close(),
        }
        assert_eq!(app.ime_cursor_area([960, 720]), None);
        app.sync_ime();
        assert!(app.ime.target.is_none() && app.ime.cursor_area.is_none());
        if unavailable != 4 {
            app.active = true;
            app.occluded = false;
            app.profile_io = None;
            app.navigator.resume();
            app.sync_ime();
            assert_eq!(app.ime_cursor_area([960, 720]), None);
            app.draw().unwrap();
            assert_eq!(app.ime_cursor_area([960, 720]), Some([160, 108, 770, 34]));
        }
        assert!(app.window.is_none() && app.renderer.is_none() && app.game.is_none());
    }
}
