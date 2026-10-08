//! Portable fixtures for the helper used by the actual Windows Raw Input drain.
//! These do not execute HWND, registration, QPC or native message queue behavior.
use super::finish_raw_input;
use std::cell::{Cell, RefCell};

#[test]
fn foreground_cleanup_precedes_successful_admission_and_occurs_exactly_once() {
    let order = RefCell::new(Vec::new());
    let original = vec![4_u8, 8, 15, 16, 23, 42];
    let expected = original.clone();
    let result = finish_raw_input::<_, &'static str>(
        Ok(original),
        true,
        || order.borrow_mut().push("cleanup"),
        |received| {
            assert_eq!(received, expected);
            order.borrow_mut().push("admit");
            Ok(())
        },
    );
    assert_eq!(result, Ok(()));
    assert_eq!(*order.borrow(), ["cleanup", "admit"]);
}

#[test]
fn foreground_decode_error_is_preserved_after_exactly_one_cleanup_without_admission() {
    let cleanup_count = Cell::new(0);
    let admitted = Cell::new(false);
    let result = finish_raw_input::<(), _>(
        Err("decode failed"),
        true,
        || cleanup_count.set(cleanup_count.get() + 1),
        |_| {
            admitted.set(true);
            Ok(())
        },
    );
    assert_eq!(result, Err("decode failed"));
    assert_eq!(cleanup_count.get(), 1);
    assert!(!admitted.get());
}

#[test]
fn foreground_publication_error_does_not_skip_or_repeat_cleanup() {
    let order = RefCell::new(Vec::new());
    let result = finish_raw_input(
        Ok(7),
        true,
        || order.borrow_mut().push("cleanup"),
        |received| {
            assert_eq!(received, 7);
            order.borrow_mut().push("publish failed");
            Err("transport full")
        },
    );
    assert_eq!(result, Err("transport full"));
    assert_eq!(*order.borrow(), ["cleanup", "publish failed"]);
}

#[test]
fn background_input_never_performs_foreground_cleanup_in_any_result_path() {
    let cleanup_count = Cell::new(0);
    for (decoded, admit_error, expected) in [
        (Ok(9), false, Ok(())),
        (Err("decode failed"), false, Err("decode failed")),
        (Ok(9), true, Err("transport full")),
    ] {
        let result = finish_raw_input(
            decoded,
            false,
            || cleanup_count.set(cleanup_count.get() + 1),
            |received| {
                assert_eq!(received, 9);
                if admit_error {
                    Err("transport full")
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(result, expected);
        assert_eq!(cleanup_count.get(), 0);
    }
}

#[test]
fn filtered_or_empty_foreground_batch_still_requires_cleanup() {
    let order = RefCell::new(Vec::new());
    let result = finish_raw_input::<_, &'static str>(
        Ok(Vec::<u8>::new()),
        true,
        || order.borrow_mut().push("cleanup"),
        |events| {
            assert!(events.is_empty());
            order.borrow_mut().push("empty batch");
            Ok(())
        },
    );
    assert_eq!(result, Ok(()));
    assert_eq!(*order.borrow(), ["cleanup", "empty batch"]);
}
