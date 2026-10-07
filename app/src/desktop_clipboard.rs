//! Desktop text clipboard ownership and blocking OS calls stay on one worker.
use beatkernel_bms_runtime::ui::{clipboard::ClipboardRequest, text_input::MAX_LINE_BYTES};
use std::{
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    thread::{self, JoinHandle},
};

pub(crate) trait TextClipboard: Send {
    fn read_text(&mut self) -> Result<String, String>;
    fn write_text(&mut self, value: &str) -> Result<(), String>;
}

#[derive(Default)]
struct NativeClipboard {
    clipboard: Option<arboard::Clipboard>,
}
impl NativeClipboard {
    fn clipboard(&mut self) -> Result<&mut arboard::Clipboard, String> {
        if self.clipboard.is_none() {
            // Called only by the worker. Failed initialization leaves None so
            // a later user request can retry without replacing the service.
            self.clipboard = Some(
                arboard::Clipboard::new()
                    .map_err(|error| format!("clipboard initialization failed: {error}"))?,
            );
        }
        Ok(self.clipboard.as_mut().expect("initialized clipboard"))
    }
}
impl TextClipboard for NativeClipboard {
    fn read_text(&mut self) -> Result<String, String> {
        self.clipboard()?
            .get_text()
            .map_err(|error| format!("clipboard read failed: {error}"))
    }
    fn write_text(&mut self, value: &str) -> Result<(), String> {
        self.clipboard()?
            .set_text(value)
            .map_err(|error| format!("clipboard write failed: {error}"))
    }
}

/// One accepted request remains busy until its response is polled. Both queues
/// have capacity one; OS access, retained native ownership and destruction all
/// belong to the worker. Reads are bounded only after the backend allocates.
/// Native operations have no hard timeout, so a stalled call can delay closing.
pub(crate) struct ClipboardWorker {
    requests: Option<SyncSender<ClipboardRequest>>,
    responses: Receiver<Result<Option<String>, String>>,
    worker: Option<JoinHandle<()>>,
    busy: bool,
}
impl ClipboardWorker {
    pub(crate) fn spawn(backend: impl TextClipboard + 'static) -> Result<Self, String> {
        let (requests, incoming) = mpsc::sync_channel(1);
        let (replies, responses) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("bms-clipboard".into())
            .spawn(move || {
                let mut backend = backend;
                while let Ok(request) = incoming.recv() {
                    let response = match request {
                        ClipboardRequest::Read => backend.read_text().and_then(|value| {
                            if value.len() > MAX_LINE_BYTES {
                                Err("clipboard text exceeds 4096 bytes".into())
                            } else {
                                Ok(Some(value))
                            }
                        }),
                        ClipboardRequest::Write(value) => backend.write_text(&value).map(|()| None),
                    };
                    if replies.send(response).is_err() {
                        break;
                    }
                }
            })
            .map_err(|error| format!("clipboard worker could not start: {error}"))?;
        Ok(Self {
            requests: Some(requests),
            responses,
            worker: Some(worker),
            busy: false,
        })
    }
    pub(crate) fn native() -> Result<Self, String> {
        Self::spawn(NativeClipboard::default())
    }
    pub(crate) fn submit(&mut self, request: ClipboardRequest) -> Result<(), String> {
        let sender = self.requests.as_ref().ok_or("clipboard worker is closed")?;
        if self.busy {
            return Err("clipboard worker is busy".into());
        }
        match sender.try_send(request) {
            Ok(()) => {
                self.busy = true;
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err("clipboard worker is busy".into()),
            Err(TrySendError::Disconnected(_)) => {
                self.requests = None;
                Err("clipboard worker disconnected".into())
            }
        }
    }
    pub(crate) fn poll(&mut self) -> Option<Result<Option<String>, String>> {
        if !self.busy {
            return None;
        }
        match self.responses.try_recv() {
            Ok(response) => {
                self.busy = false;
                Some(response)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.busy = false;
                self.requests = None;
                Some(Err("clipboard worker disconnected".into()))
            }
        }
    }
    pub(crate) fn is_busy(&self) -> bool {
        self.busy
    }
    pub(crate) fn begin_close(&mut self) {
        // Keep the response receiver alive: the one accepted operation can
        // publish its completion even when the UI has cancelled its edit.
        self.requests = None;
    }
    pub(crate) fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }
}
impl Drop for ClipboardWorker {
    fn drop(&mut self) {
        self.begin_close();
        if let Some(worker) = self.worker.take() {
            // Normal UI shutdown waits for is_finished first. This fallback
            // guarantees native ownership does not outlive an early return.
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc::Sender, thread::ThreadId, time::Duration};

    #[derive(Debug, PartialEq, Eq)]
    enum Call {
        Read(ThreadId),
        Write(ThreadId, String),
        Dropped(ThreadId),
    }
    struct FakeClipboard {
        calls: Sender<Call>,
        replies: Receiver<Result<String, String>>,
    }
    impl TextClipboard for FakeClipboard {
        fn read_text(&mut self) -> Result<String, String> {
            self.calls.send(Call::Read(thread::current().id())).unwrap();
            self.replies.recv().unwrap()
        }
        fn write_text(&mut self, value: &str) -> Result<(), String> {
            self.calls
                .send(Call::Write(thread::current().id(), value.into()))
                .unwrap();
            self.replies.recv().unwrap().map(|_| ())
        }
    }
    impl Drop for FakeClipboard {
        fn drop(&mut self) {
            let _ = self.calls.send(Call::Dropped(thread::current().id()));
        }
    }
    fn service() -> (
        ClipboardWorker,
        Receiver<Call>,
        SyncSender<Result<String, String>>,
    ) {
        let (calls, observed) = mpsc::channel();
        let (replies, pending) = mpsc::sync_channel(1);
        let worker = ClipboardWorker::spawn(FakeClipboard {
            calls,
            replies: pending,
        })
        .unwrap();
        (worker, observed, replies)
    }
    fn observed(calls: &Receiver<Call>) -> Call {
        calls.recv_timeout(Duration::from_secs(5)).unwrap()
    }
    fn completion(worker: &mut ClipboardWorker) -> Result<Option<String>, String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(reply) = worker.poll() {
                return reply;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "missing fake response"
            );
            thread::yield_now();
        }
    }

    #[test]
    fn worker_serializes_native_ownership_and_stays_busy_until_reply_is_polled() {
        let (mut worker, calls, replies) = service();
        assert!(!worker.is_busy());
        assert!(!worker.is_finished());
        assert_eq!(worker.poll(), None);
        worker
            .submit(ClipboardRequest::Write("가é".into()))
            .unwrap();
        let Call::Write(owner, value) = observed(&calls) else {
            panic!("expected write");
        };
        assert_ne!(owner, thread::current().id());
        assert_eq!(value, "가é");
        assert!(worker.is_busy());
        assert_eq!(worker.poll(), None);
        assert_eq!(
            worker.submit(ClipboardRequest::Read).unwrap_err(),
            "clipboard worker is busy"
        );
        replies.send(Ok(String::new())).unwrap();
        assert!(worker.is_busy());
        assert_eq!(completion(&mut worker), Ok(None));
        assert!(!worker.is_busy());
        worker.submit(ClipboardRequest::Read).unwrap();
        assert_eq!(observed(&calls), Call::Read(owner));
        replies.send(Ok("붙여넣기".into())).unwrap();
        worker.begin_close();
        // Worker destruction happens after its queued reply, proving close
        // drains that reply without requiring the UI to receive it first.
        assert_eq!(observed(&calls), Call::Dropped(owner));
        assert!(worker.is_busy());
        assert_eq!(worker.poll(), Some(Ok(Some("붙여넣기".into()))));
        assert_eq!(worker.poll(), None);
        assert_eq!(
            worker.submit(ClipboardRequest::Read).unwrap_err(),
            "clipboard worker is closed"
        );
        worker.worker.take().unwrap().join().unwrap();
        assert!(worker.is_finished());
    }

    #[test]
    fn errors_and_oversized_reads_are_drained_before_a_successful_retry() {
        let (mut worker, calls, replies) = service();
        for (reply, expected) in [
            (
                Err("temporarily unavailable".into()),
                Err("temporarily unavailable".into()),
            ),
            (
                Ok("가".repeat(1366)),
                Err("clipboard text exceeds 4096 bytes".into()),
            ),
            (Ok("A".repeat(4096)), Ok(Some("A".repeat(4096)))),
            (Ok(String::new()), Ok(Some(String::new()))),
        ] {
            worker.submit(ClipboardRequest::Read).unwrap();
            assert!(matches!(observed(&calls), Call::Read(_)));
            replies.send(reply).unwrap();
            assert_eq!(completion(&mut worker), expected);
            assert!(!worker.is_busy());
        }
        worker
            .submit(ClipboardRequest::Write("cut".into()))
            .unwrap();
        assert!(matches!(observed(&calls), Call::Write(_, value) if value == "cut"));
        replies.send(Err("write rejected".into())).unwrap();
        assert_eq!(completion(&mut worker), Err("write rejected".into()));
        worker
            .submit(ClipboardRequest::Write("retry".into()))
            .unwrap();
        assert!(matches!(observed(&calls), Call::Write(_, value) if value == "retry"));
        replies.send(Ok(String::new())).unwrap();
        assert_eq!(completion(&mut worker), Ok(None));
        worker.begin_close();
        assert!(matches!(observed(&calls), Call::Dropped(_)));
    }

    #[test]
    fn close_before_work_and_drop_with_an_unread_reply_release_the_backend() {
        let (mut idle, calls, _replies) = service();
        idle.begin_close();
        idle.begin_close();
        let Call::Dropped(owner) = observed(&calls) else {
            panic!("idle backend did not close");
        };
        assert_ne!(owner, thread::current().id());
        assert_eq!(idle.poll(), None);
        assert!(!idle.is_busy());
        assert!(idle.submit(ClipboardRequest::Read).is_err());
        drop(idle);

        let (mut worker, calls, replies) = service();
        worker.submit(ClipboardRequest::Read).unwrap();
        let Call::Read(owner) = observed(&calls) else {
            panic!("expected read");
        };
        replies.send(Ok("unread completion".into())).unwrap();
        drop(worker); // The capacity-one completion cannot block the join.
        assert_eq!(observed(&calls), Call::Dropped(owner));
    }

    #[test]
    fn closing_waits_for_the_accepted_operation_without_accepting_more_work() {
        let (mut worker, calls, replies) = service();
        worker.submit(ClipboardRequest::Read).unwrap();
        let Call::Read(owner) = observed(&calls) else {
            panic!("expected read");
        };
        // The fake backend is now blocked on its response channel, so these
        // assertions do not depend on a scheduler race or actual OS timing.
        worker.begin_close();
        assert!(worker.is_busy());
        assert!(!worker.is_finished());
        assert_eq!(worker.poll(), None);
        assert_eq!(
            worker
                .submit(ClipboardRequest::Write("late".into()))
                .unwrap_err(),
            "clipboard worker is closed"
        );
        replies.send(Ok("accepted before close".into())).unwrap();
        assert_eq!(observed(&calls), Call::Dropped(owner));
        assert_eq!(
            worker.poll(),
            Some(Ok(Some("accepted before close".into())))
        );
        assert!(!worker.is_busy());
    }

    #[test]
    fn disconnected_request_or_response_is_an_explicit_terminal_error() {
        let (requests, incoming) = mpsc::sync_channel(1);
        let (replies, responses) = mpsc::sync_channel(1);
        drop(incoming);
        let mut worker = ClipboardWorker {
            requests: Some(requests),
            responses,
            worker: None,
            busy: false,
        };
        assert_eq!(
            worker.submit(ClipboardRequest::Read).unwrap_err(),
            "clipboard worker disconnected"
        );
        assert!(!worker.is_busy());
        assert_eq!(worker.poll(), None);
        drop(replies);

        let (requests, _incoming) = mpsc::sync_channel(1);
        let (replies, responses) = mpsc::sync_channel(1);
        drop(replies);
        let mut worker = ClipboardWorker {
            requests: Some(requests),
            responses,
            worker: None,
            busy: true,
        };
        assert_eq!(
            worker.poll(),
            Some(Err("clipboard worker disconnected".into()))
        );
        assert!(!worker.is_busy());
        assert_eq!(worker.poll(), None);
        assert_eq!(
            worker.submit(ClipboardRequest::Read).unwrap_err(),
            "clipboard worker is closed"
        );
    }
}
