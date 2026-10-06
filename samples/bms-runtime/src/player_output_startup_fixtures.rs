use super::*;
use crate::gameplay::output::domain::control::fixtures::capability;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn idle_reply_poll_does_not_acquire_the_control_lock() {
    let (publisher, viewer) = channel();
    publisher.advertise_output(Some(capability())).unwrap();
    let _held = publisher.0.output.lock().unwrap();
    assert!(!viewer.output_pending());
    assert!(viewer.take_output_reply().unwrap().is_none());
    assert!(viewer.output_supported());
}

#[test]
fn startup_advertisement_waits_for_ui_contention_and_publishes_once() {
    let (publisher, viewer) = channel();
    let held = publisher.0.output.lock().unwrap();
    let worker_publisher = publisher.clone();
    let (started, ready) = mpsc::channel();
    let (finished, result) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        started.send(()).unwrap();
        finished
            .send(worker_publisher.advertise_output(Some(capability())))
            .unwrap();
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let premature = result.recv_timeout(Duration::from_millis(50));
    drop(held);
    worker.join().unwrap();
    assert!(matches!(premature, Err(mpsc::RecvTimeoutError::Timeout)));
    result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(viewer.output_capability().unwrap(), Some(capability()));
    assert!(viewer.output_supported());
    assert!(!viewer.output_pending());
}

#[test]
fn cancellation_while_startup_waits_suppresses_advertisement_without_failure() {
    let (publisher, viewer) = channel();
    let held = publisher.0.output.lock().unwrap();
    let worker_publisher = publisher.clone();
    let (started, ready) = mpsc::channel();
    let (finished, result) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        started.send(()).unwrap();
        finished
            .send(worker_publisher.advertise_output(Some(capability())))
            .unwrap();
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let premature = result.recv_timeout(Duration::from_millis(50));
    viewer.cancel();
    drop(held);
    worker.join().unwrap();
    assert!(matches!(premature, Err(mpsc::RecvTimeoutError::Timeout)));
    result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert!(!viewer.output_supported());
    assert!(viewer.output_capability().unwrap().is_none());
    assert!(publisher.0.output.lock().unwrap().capability().is_none());
}

#[test]
fn closed_and_poisoned_uncancelled_channels_remain_startup_errors() {
    let (publisher, _) = channel();
    publisher.0.output_closed.store(true, Ordering::Release);
    assert_eq!(
        publisher
            .advertise_output(Some(capability()))
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotConnected
    );
    let (publisher, _) = channel();
    let poisoner = publisher.clone();
    assert!(
        std::panic::catch_unwind(move || {
            let _held = poisoner.0.output.lock().unwrap();
            panic!("controlled output lock poison");
        })
        .is_err()
    );
    assert_eq!(
        publisher
            .advertise_output(Some(capability()))
            .unwrap_err()
            .kind(),
        io::ErrorKind::Other
    );
}
