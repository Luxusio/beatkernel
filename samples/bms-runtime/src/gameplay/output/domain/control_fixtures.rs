//! Deferred pure command state and real player-channel lifecycle fixtures.
use super::*;
pub(crate) fn args() -> Vec<String> {
    [
        "--alsa",
        "default",
        "--buffer-frames",
        "128",
        "--period-frames",
        "32",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}
pub(crate) fn capability() -> OutputCapability {
    OutputCapability {
        host: crate::settings::SettingsHost::Linux,
        current_args: args(),
    }
}
#[test]
fn unsupported_invalid_and_busy_requests_do_not_consume_identity_and_reply_consumption_releases_exact_one_slot()
 {
    let mut c = OutputControls::new();
    assert!(c.request(args()).is_err());
    assert_eq!(c.next, Some(1));
    c.advertise(Some(capability())).unwrap();
    for input in [
        vec!["--alsa".into()],
        vec!["--sample-rate".into(), "48000".into()],
        vec!["--alsa".into(), "bad\0value".into()],
        vec![
            "--alsa".into(),
            "default".into(),
            "--alsa".into(),
            "default".into(),
        ],
    ] {
        assert!(c.request(input).is_err());
        assert_eq!(c.next, Some(1));
        assert!(!c.pending());
    }
    assert_eq!(c.request(args()).unwrap(), 1);
    assert_eq!(c.next, Some(2));
    assert!(c.request(args()).is_err());
    assert_eq!(c.next, Some(2));
    let request = c.take_request().unwrap();
    assert_eq!(request.id, 1);
    assert_eq!(request.args, args());
    assert!(c.take_request().is_none());
    assert!(c.request(args()).is_err());
    c.reply(&OutputReply {
        id: 1,
        result: Ok(capability()),
    })
    .unwrap();
    assert!(c.pending());
    assert!(c.request(args()).is_err());
    assert_eq!(c.take_reply().unwrap().id, 1);
    assert!(!c.pending());
    assert_eq!(c.request(args()).unwrap(), 2);
}
#[test]
fn maximum_u64_identity_is_issued_once_and_never_renewed_after_reply_or_validation_refusal() {
    let mut c = OutputControls::new();
    c.advertise(Some(capability())).unwrap();
    c.next = Some(u64::MAX);
    assert!(
        c.request(vec!["--chart".into(), "other.bms".into()])
            .is_err()
    );
    assert_eq!(c.next, Some(u64::MAX));
    assert_eq!(c.request(args()).unwrap(), u64::MAX);
    assert_eq!(c.next, None);
    c.take_request().unwrap();
    c.reply(&OutputReply {
        id: u64::MAX,
        result: Err("bounded refusal".into()),
    })
    .unwrap();
    assert_eq!(c.take_reply().unwrap().id, u64::MAX);
    assert!(c.request(args()).is_err());
    assert_eq!(c.next, None);
    assert!(!c.pending());
}
#[test]
fn malformed_or_uncorrelated_reply_is_atomic_and_owner_close_preserves_already_accepted_reply() {
    let mut c = OutputControls::new();
    c.advertise(Some(capability())).unwrap();
    c.request(args()).unwrap();
    c.take_request().unwrap();
    for reply in [
        OutputReply {
            id: 2,
            result: Ok(capability()),
        },
        OutputReply {
            id: 1,
            result: Err("bad\nmessage".into()),
        },
        OutputReply {
            id: 1,
            result: Err("x".repeat(4097)),
        },
        OutputReply {
            id: 1,
            result: Ok(OutputCapability {
                host: crate::settings::SettingsHost::Linux,
                current_args: vec!["--evdev".into(), "keyboard".into()],
            }),
        },
    ] {
        assert!(c.reply(&reply).is_err());
        assert_eq!(c.flight, Some(1));
        assert!(c.reply.is_none());
        assert_eq!(c.capability(), Some(&capability()));
    }
    let accepted = OutputReply {
        id: 1,
        result: Ok(capability()),
    };
    c.reply(&accepted).unwrap();
    c.close("later owner failure");
    assert_eq!(c.take_reply(), Some(accepted));
    assert!(c.capability().is_none());
    assert!(c.request(args()).is_err());
}
#[test]
fn queued_and_inflight_cancellation_settle_one_bounded_correlated_reply_and_keep_controls_closed() {
    for in_flight in [false, true] {
        let mut c = OutputControls::new();
        c.advertise(Some(capability())).unwrap();
        c.request(args()).unwrap();
        if in_flight {
            c.take_request().unwrap();
        }
        c.close(&format!("cancelled\n{}", "界".repeat(2000)));
        let reply = c.take_reply().unwrap();
        assert_eq!(reply.id, 1);
        let message = reply.result.unwrap_err();
        assert!(!message.chars().any(char::is_control));
        assert!(message.chars().count() <= 512);
        assert!(message.len() <= 4096);
        assert!(c.take_reply().is_none());
        assert!(c.take_request().is_none());
        assert!(!c.pending());
        assert!(c.advertise(Some(capability())).is_err());
    }
}
macro_rules! player_channel_tests {
    () => {
        #[test]
        fn decided_output_reply_survives_cancel_poll_and_terminal_owner_failure() {
            for poll in [false, true] {
                let (publisher, viewer) = super::channel();
                let cap = crate::live_output_control::fixtures::capability();
                publisher.advertise_output(Some(cap.clone())).unwrap();
                let id = viewer.request_output(crate::live_output_control::fixtures::args()).unwrap();
                publisher.take_output_request().unwrap().unwrap();
                let reply = crate::live_output_control::OutputReply { id, result: Ok(cap) };
                super::with_publisher(publisher.clone(), || {
                    viewer.cancel();
                    if poll {
                        assert!(viewer.take_output_reply().unwrap().is_none());
                        assert!(viewer.output_pending());
                    }
                    publisher.reply_output(&reply).unwrap();
                    Err::<(), String>("later owner failure".into())
                }).unwrap_err();
                assert_eq!(viewer.take_output_reply().unwrap(), Some(reply));
                assert!(viewer.take_output_reply().unwrap().is_none());
                assert!(!viewer.output_pending());
                assert!(!viewer.output_supported());
            }
        }
        #[test]
        fn cancelled_inflight_output_without_decision_waits_for_owner_and_settles_once() {
            let (publisher, viewer) = super::channel();
            publisher.advertise_output(Some(crate::live_output_control::fixtures::capability())).unwrap();
            let id = viewer.request_output(crate::live_output_control::fixtures::args()).unwrap();
            publisher.take_output_request().unwrap().unwrap();
            super::with_publisher(publisher, || {
                viewer.cancel();
                assert!(viewer.take_output_reply().unwrap().is_none());
                assert!(viewer.request_output(crate::live_output_control::fixtures::args()).is_err());
                assert!(viewer.take_output_reply().unwrap().is_none());
                assert!(viewer.output_pending());
                Err::<(), String>("original owner failure".into())
            }).unwrap_err();
            let reply = viewer.take_output_reply().unwrap().unwrap();
            assert_eq!(reply.id, id);
            assert_eq!(reply.result.unwrap_err(), "original owner failure");
            assert!(viewer.take_output_reply().unwrap().is_none());
            assert!(!viewer.output_pending());
        }
        #[test]
        fn actual_player_output_adapter_commits_decided_reply_before_return_despite_ui_contention() {
            use crate::gameplay_output_ui::OutputUiPort;
            use crate::gameplay::output::adapters::player::PlayerOutputUi;
            use std::{sync::mpsc, time::Duration};
            let (publisher, viewer) = super::channel();
            let mut applied = crate::live_output_control::fixtures::capability();
            publisher.advertise_output(Some(applied.clone())).unwrap();
            let id = viewer.request_output(crate::live_output_control::fixtures::args()).unwrap();
            publisher.take_output_request().unwrap().unwrap();
            applied.current_args[1] = "actual-new-endpoint".into();
            let reply = crate::live_output_control::OutputReply { id, result: Ok(applied) };
            let decided = reply.clone();
            let owner = publisher.clone();
            let locked = publisher.0.output.lock().unwrap();
            let (started_tx, started_rx) = mpsc::channel();
            let (done_tx, done_rx) = mpsc::channel();
            let worker = std::thread::spawn(move || {
                let result = super::with_publisher(owner, || {
                    started_tx.send(()).unwrap();
                    PlayerOutputUi.reply(&decided).map_err(|e| e.to_string())?;
                    Err::<(), String>("later terminal error".into())
                });
                done_tx.send(result).unwrap();
            });
            started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
            viewer.cancel();
            assert_eq!(viewer.take_output_reply().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
            drop(locked);
            assert_eq!(done_rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap_err(), "later terminal error");
            worker.join().unwrap();
            assert_eq!(viewer.take_output_reply().unwrap(), Some(reply));
            assert!(viewer.take_output_reply().unwrap().is_none());
            assert!(!viewer.output_pending());
            assert!(!viewer.output_supported());
        }
        #[test]
        fn actual_output_channels_are_session_isolated_and_reply_contention_retains_pending_identity() {
            use crate::live_output_control::{OutputReply};
            let (a,av)=super::channel(); let (b,bv)=super::channel(); let cap=crate::live_output_control::fixtures::capability(); let args=crate::live_output_control::fixtures::args();
            a.advertise_output(Some(cap.clone())).unwrap(); b.advertise_output(Some(cap.clone())).unwrap(); assert_eq!(av.request_output(args.clone()).unwrap(),1); assert_eq!(bv.request_output(args).unwrap(),1); assert_eq!(a.take_output_request().unwrap().unwrap().id,1); assert_eq!(b.take_output_request().unwrap().unwrap().id,1);
            let reply=OutputReply { id:1,result:Ok(cap) }; let locked=a.0.output.lock().unwrap(); assert_eq!(a.reply_output(&reply).unwrap_err().kind(),std::io::ErrorKind::WouldBlock); assert!(av.output_pending()); assert!(bv.take_output_reply().unwrap().is_none()); drop(locked);
            a.reply_output(&reply).unwrap(); assert!(av.output_pending()); assert_eq!(av.take_output_reply().unwrap(),Some(reply)); assert!(!av.output_pending()); assert!(bv.output_pending()); b.reply_output(&OutputReply { id:1,result:Err("other session refusal".into()) }).unwrap(); assert_eq!(bv.take_output_reply().unwrap().unwrap().result.unwrap_err(),"other session refusal");
        }
        #[test]
        fn actual_viewer_cancel_and_normal_owner_return_settle_accepted_requests_without_success_ack() {
            for cancel in [false,true] {
                let (publisher,viewer)=super::channel(); publisher.advertise_output(Some(crate::live_output_control::fixtures::capability())).unwrap(); let id=viewer.request_output(crate::live_output_control::fixtures::args()).unwrap();
                if cancel { viewer.cancel(); assert!(publisher.take_output_request().unwrap().is_none()); }
                else { super::with_publisher(publisher,||Ok::<(),String>(())).unwrap(); }
                let reply=viewer.take_output_reply().unwrap().unwrap(); assert_eq!(reply.id,id); assert!(reply.result.is_err()); assert!(!viewer.output_pending()); assert!(viewer.output_capability().unwrap().is_none()); assert!(viewer.request_output(crate::live_output_control::fixtures::args()).is_err());
            }
        }
        #[test]
        fn actual_owner_unwind_closes_inflight_request_even_when_control_lock_prevents_immediate_settlement() {
            let (publisher,viewer)=super::channel(); publisher.advertise_output(Some(crate::live_output_control::fixtures::capability())).unwrap(); let id=viewer.request_output(crate::live_output_control::fixtures::args()).unwrap(); publisher.take_output_request().unwrap().unwrap();
            let locked=publisher.0.output.lock().unwrap(); let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||super::with_publisher(publisher.clone(),||->Result<(),String> { panic!("controlled owner unwind") }))); assert!(result.is_err()); drop(locked);
            let reply=viewer.take_output_reply().unwrap().unwrap(); assert_eq!(reply.id,id); assert!(reply.result.is_err()); assert!(!viewer.output_pending()); assert!(viewer.output_capability().unwrap().is_none()); assert!(viewer.request_output(crate::live_output_control::fixtures::args()).is_err());
        }
    };
}
pub(crate) use player_channel_tests;
