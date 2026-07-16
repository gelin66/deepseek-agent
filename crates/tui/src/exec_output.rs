use std::fmt;
use std::io::{self, Write};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::core::termination::RunTerminationReason;

/// Machine-facing status paired with a typed runtime termination reason.
///
/// `status` is deliberately not accepted from callers: it is derived from the
/// reason so Headless output cannot serialize combinations such as
/// `completed` + `unresolved` or `failed` + `resolved`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecTerminalStatus {
    Completed,
    Interrupted,
    Failed,
}

/// The single terminal projection flattened into both Headless JSON formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(crate) struct ExecTerminalReceipt {
    status: ExecTerminalStatus,
    termination_reason: RunTerminationReason,
}

impl ExecTerminalReceipt {
    #[must_use]
    pub(crate) const fn from_reason(termination_reason: RunTerminationReason) -> Self {
        let status = match termination_reason {
            RunTerminationReason::Resolved => ExecTerminalStatus::Completed,
            RunTerminationReason::Canceled => ExecTerminalStatus::Interrupted,
            RunTerminationReason::Unresolved
            | RunTerminationReason::Stuck
            | RunTerminationReason::Timeout
            | RunTerminationReason::BudgetExhausted
            | RunTerminationReason::ApprovalRequired
            | RunTerminationReason::ModelError
            | RunTerminationReason::ToolError
            | RunTerminationReason::InfrastructureError
            | RunTerminationReason::EvidenceMissing => ExecTerminalStatus::Failed,
        };
        Self {
            status,
            termination_reason,
        }
    }

    #[must_use]
    pub(crate) const fn termination_reason(self) -> RunTerminationReason {
        self.termination_reason
    }
}

/// Owns all blocking output for one headless execution.
///
/// A write future may be cancelled while it is waiting for queue capacity or
/// acknowledgement. Once its command has entered the queue, cancellation does
/// not retract the bytes: the writer still emits them in queue order.
pub(crate) struct ExecOutput {
    tx: mpsc::Sender<WriteCommand>,
    writer: JoinHandle<()>,
    first_write_error: Arc<Mutex<Option<OutputWriteError>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutputWriteError {
    pub(crate) stream: OutputStream,
    pub(crate) kind: io::ErrorKind,
    pub(crate) message: String,
}

impl fmt::Display for OutputWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} write failed: {}", self.stream, self.message)
    }
}

impl std::error::Error for OutputWriteError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecOutputError {
    QueueClosed,
    AcknowledgementDropped,
    Write(OutputWriteError),
}

impl fmt::Display for ExecOutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueClosed => f.write_str("output writer queue is closed"),
            Self::AcknowledgementDropped => {
                f.write_str("output writer stopped before acknowledging the write")
            }
            Self::Write(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ExecOutputError {}

pub(crate) struct ExecOutputAck {
    acknowledged: oneshot::Receiver<Result<(), OutputWriteError>>,
}

impl ExecOutputAck {
    pub(crate) async fn wait(self) -> Result<(), ExecOutputError> {
        self.acknowledged
            .await
            .map_err(|_| ExecOutputError::AcknowledgementDropped)?
            .map_err(ExecOutputError::Write)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutputCloseReport {
    pub(crate) joined: bool,
    pub(crate) unjoined: bool,
    pub(crate) write_error: Option<OutputWriteError>,
    pub(crate) join_error: Option<String>,
}

impl OutputCloseReport {
    #[cfg(test)]
    pub(crate) fn is_clean(&self) -> bool {
        self.joined && self.write_error.is_none() && self.join_error.is_none()
    }
}

struct WriteCommand {
    stream: OutputStream,
    bytes: Vec<u8>,
    ack: oneshot::Sender<Result<(), OutputWriteError>>,
}

impl ExecOutput {
    pub(crate) fn new(queue_capacity: NonZeroUsize) -> Self {
        Self::with_writers(queue_capacity, io::stdout(), io::stderr())
    }

    fn with_writers<Stdout, Stderr>(
        queue_capacity: NonZeroUsize,
        stdout: Stdout,
        stderr: Stderr,
    ) -> Self
    where
        Stdout: Write + Send + 'static,
        Stderr: Write + Send + 'static,
    {
        let (tx, mut rx) = mpsc::channel(queue_capacity.get());
        let first_write_error = Arc::new(Mutex::new(None));
        let writer_error = Arc::clone(&first_write_error);

        let writer = tokio::task::spawn_blocking(move || {
            let mut stdout = stdout;
            let mut stderr = stderr;
            let mut stdout_error = None;
            let mut stderr_error = None;

            while let Some(command) = rx.blocking_recv() {
                let WriteCommand { stream, bytes, ack } = command;
                let result = match stream {
                    OutputStream::Stdout => write_one(
                        stream,
                        &mut stdout,
                        &bytes,
                        &mut stdout_error,
                        &writer_error,
                    ),
                    OutputStream::Stderr => write_one(
                        stream,
                        &mut stderr,
                        &bytes,
                        &mut stderr_error,
                        &writer_error,
                    ),
                };
                let _ = ack.send(result);
            }
        });

        Self {
            tx,
            writer,
            first_write_error,
        }
    }

    pub(crate) async fn write_stdout(
        &self,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<(), ExecOutputError> {
        self.enqueue_stdout(bytes).await?.wait().await
    }

    #[cfg(test)]
    async fn write_stderr(&self, bytes: impl Into<Vec<u8>>) -> Result<(), ExecOutputError> {
        self.enqueue_stderr(bytes).await?.wait().await
    }

    pub(crate) async fn enqueue_stdout(
        &self,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<ExecOutputAck, ExecOutputError> {
        self.enqueue(OutputStream::Stdout, bytes.into()).await
    }

    pub(crate) async fn enqueue_stderr(
        &self,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<ExecOutputAck, ExecOutputError> {
        self.enqueue(OutputStream::Stderr, bytes.into()).await
    }

    async fn enqueue(
        &self,
        stream: OutputStream,
        bytes: Vec<u8>,
    ) -> Result<ExecOutputAck, ExecOutputError> {
        let (ack, acknowledged) = oneshot::channel();
        self.tx
            .send(WriteCommand { stream, bytes, ack })
            .await
            .map_err(|_| ExecOutputError::QueueClosed)?;
        Ok(ExecOutputAck { acknowledged })
    }

    /// Closes the queue, drains accepted writes, and waits at most `timeout`
    /// for the blocking writer to finish.
    pub(crate) async fn close_and_join(mut self, timeout: Duration) -> OutputCloseReport {
        drop(self.tx);

        let join_result = tokio::time::timeout(timeout, &mut self.writer).await;
        let (joined, join_error) = match join_result {
            Ok(Ok(())) => (true, None),
            Ok(Err(error)) => (true, Some(error.to_string())),
            Err(_) => (false, None),
        };

        OutputCloseReport {
            joined,
            unjoined: !joined,
            write_error: lock_unpoisoned(&self.first_write_error).clone(),
            join_error,
        }
    }
}

fn write_one<W: Write>(
    stream: OutputStream,
    writer: &mut W,
    bytes: &[u8],
    stream_error: &mut Option<OutputWriteError>,
    first_write_error: &Mutex<Option<OutputWriteError>>,
) -> Result<(), OutputWriteError> {
    if let Some(error) = stream_error {
        return Err(error.clone());
    }

    let result = writer.write_all(bytes).and_then(|()| writer.flush());
    result.map_err(|error| {
        let error = OutputWriteError {
            stream,
            kind: error.kind(),
            message: error.to_string(),
        };
        *stream_error = Some(error.clone());

        let mut first = lock_unpoisoned(first_write_error);
        if first.is_none() {
            *first = Some(error.clone());
        }
        error
    })
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc as std_mpsc;

    type RecordedWrites = Arc<Mutex<Vec<(OutputStream, Vec<u8>)>>>;

    #[derive(Clone)]
    struct RecordingSink {
        stream: OutputStream,
        writes: RecordedWrites,
    }

    impl Write for RecordingSink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            lock_unpoisoned(&self.writes).push((self.stream, bytes.to_vec()));
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct BrokenSink;

    impl Write for BrokenSink {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed reader"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct GatedSink {
        started: Option<std_mpsc::Sender<()>>,
        release: std_mpsc::Receiver<()>,
        finished: Option<std_mpsc::Sender<()>>,
    }

    impl Write for GatedSink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if let Some(started) = self.started.take() {
                let _ = started.send(());
            }
            self.release
                .recv()
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "test gate closed"))?;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Drop for GatedSink {
        fn drop(&mut self) {
            if let Some(finished) = self.finished.take() {
                let _ = finished.send(());
            }
        }
    }

    fn recording_pair() -> (RecordingSink, RecordingSink, RecordedWrites) {
        let writes = Arc::new(Mutex::new(Vec::new()));
        (
            RecordingSink {
                stream: OutputStream::Stdout,
                writes: Arc::clone(&writes),
            },
            RecordingSink {
                stream: OutputStream::Stderr,
                writes: Arc::clone(&writes),
            },
            writes,
        )
    }

    #[test]
    fn terminal_receipt_derives_a_stable_status_from_the_typed_reason() {
        for (reason, expected_status) in [
            (RunTerminationReason::Resolved, "completed"),
            (RunTerminationReason::Canceled, "interrupted"),
            (RunTerminationReason::Timeout, "failed"),
            (RunTerminationReason::ToolError, "failed"),
        ] {
            let receipt = ExecTerminalReceipt::from_reason(reason);
            let json = serde_json::to_value(receipt).expect("terminal receipt serializes");

            assert_eq!(json["status"], expected_status);
            assert_eq!(
                json["termination_reason"],
                serde_json::to_value(reason).expect("typed reason serializes")
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn writes_both_streams_in_queue_order_and_joins_cleanly() {
        let (stdout, stderr, writes) = recording_pair();
        let output = ExecOutput::with_writers(NonZeroUsize::new(2).unwrap(), stdout, stderr);

        output.write_stdout(b"one\n".to_vec()).await.unwrap();
        output.write_stderr(b"two\n".to_vec()).await.unwrap();
        output.write_stdout(b"three\n".to_vec()).await.unwrap();

        let report = output.close_and_join(Duration::from_secs(1)).await;
        assert!(report.is_clean(), "{report:?}");
        assert_eq!(
            *lock_unpoisoned(&writes),
            vec![
                (OutputStream::Stdout, b"one\n".to_vec()),
                (OutputStream::Stderr, b"two\n".to_vec()),
                (OutputStream::Stdout, b"three\n".to_vec()),
            ]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn acknowledges_write_failure_and_preserves_the_other_stream() {
        let (_, stderr, writes) = recording_pair();
        let output = ExecOutput::with_writers(NonZeroUsize::new(2).unwrap(), BrokenSink, stderr);

        let error = output.write_stdout(b"lost\n".to_vec()).await.unwrap_err();
        assert!(matches!(
            error,
            ExecOutputError::Write(OutputWriteError {
                stream: OutputStream::Stdout,
                kind: io::ErrorKind::BrokenPipe,
                ..
            })
        ));
        output.write_stderr(b"diagnostic\n".to_vec()).await.unwrap();

        let report = output.close_and_join(Duration::from_secs(1)).await;
        assert!(report.joined);
        assert!(!report.unjoined);
        assert_eq!(
            report.write_error.as_ref().map(|error| error.kind),
            Some(io::ErrorKind::BrokenPipe)
        );
        assert_eq!(
            *lock_unpoisoned(&writes),
            vec![(OutputStream::Stderr, b"diagnostic\n".to_vec())]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_ack_wait_does_not_retract_an_accepted_write() {
        let (started_tx, started_rx) = std_mpsc::channel();
        let (release_tx, release_rx) = std_mpsc::channel();
        let (finished_tx, _finished_rx) = std_mpsc::channel();
        let (_, stderr, _) = recording_pair();
        let output = ExecOutput::with_writers(
            NonZeroUsize::new(1).unwrap(),
            GatedSink {
                started: Some(started_tx),
                release: release_rx,
                finished: Some(finished_tx),
            },
            stderr,
        );

        let mut write = Box::pin(output.write_stdout(b"accepted\n".to_vec()));
        tokio::select! {
            result = &mut write => panic!("write unexpectedly completed: {result:?}"),
            started = tokio::task::spawn_blocking(move || started_rx.recv_timeout(Duration::from_secs(1))) => {
                started.unwrap().unwrap();
            }
        }
        drop(write);
        release_tx.send(()).unwrap();

        let report = output.close_and_join(Duration::from_secs(1)).await;
        assert!(report.is_clean(), "{report:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn close_deadline_reports_a_blocked_writer_as_unjoined() {
        let (started_tx, started_rx) = std_mpsc::channel();
        let (release_tx, release_rx) = std_mpsc::channel();
        let (finished_tx, finished_rx) = std_mpsc::channel();
        let (_, stderr, _) = recording_pair();
        let output = ExecOutput::with_writers(
            NonZeroUsize::new(1).unwrap(),
            GatedSink {
                started: Some(started_tx),
                release: release_rx,
                finished: Some(finished_tx),
            },
            stderr,
        );

        let mut write = Box::pin(output.write_stdout(b"blocked\n".to_vec()));
        tokio::select! {
            result = &mut write => panic!("write unexpectedly completed: {result:?}"),
            started = tokio::task::spawn_blocking(move || started_rx.recv_timeout(Duration::from_secs(1))) => {
                started.unwrap().unwrap();
            }
        }
        drop(write);

        let report = output.close_and_join(Duration::from_millis(25)).await;
        assert!(!report.joined);
        assert!(report.unjoined);

        release_tx.send(()).unwrap();
        tokio::task::spawn_blocking(move || finished_rx.recv_timeout(Duration::from_secs(1)))
            .await
            .unwrap()
            .unwrap();
    }
}
