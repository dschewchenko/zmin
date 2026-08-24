use std::error::Error as StdError;
use std::fmt;
use std::fs::File;
use std::future::Future;
use std::io::{self, Read};
use std::pin::Pin;
use std::sync::mpsc::{self as std_mpsc, Receiver as StdReceiver};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll};
use std::thread::{self, JoinHandle, ThreadId};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::future::{select, Either};
use http::{Request, Response};
use http_body::{Body, Frame, SizeHint};
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper_util::client::legacy::{Client, Error as HyperClientError};
use hyper_util::rt::{TokioExecutor, TokioTimer};
use sha2::{Digest, Sha256};
use tokio::runtime::{Builder, Handle};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::oneshot::error::TryRecvError;
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;

use crate::connector::{ConnectError, ExactConnector};
use crate::{
    OpenRequestBody, RequestBodyCancellation, TransportError, BODY_BRIDGE_CHUNK_BYTES,
    REQUEST_BODY_BRIDGE_QUEUE_DEPTH, RESPONSE_BODY_BRIDGE_QUEUE_DEPTH,
};

const BODY_WORKER_COUNT: usize = 8;
const HTTP_RUNTIME_THREAD_STACK_BYTES: usize = 512 * 1024;

pub(crate) type RuntimeClientInner = Client<ExactConnector, RequestBody>;

#[derive(Clone)]
pub(crate) struct RuntimeClient {
    client: RuntimeClientInner,
    connector: ExactConnector,
}

impl RuntimeClient {
    pub(crate) fn new(
        connector: ExactConnector,
        pool_idle_timeout: Duration,
        http1_read_buffer_bytes: usize,
    ) -> Self {
        let mut builder = Client::builder(TokioExecutor::new());
        builder
            .pool_idle_timeout(pool_idle_timeout)
            .pool_max_idle_per_host(8)
            .pool_timer(TokioTimer::new())
            .http1_max_buf_size(http1_read_buffer_bytes);
        Self {
            client: builder.build(connector.clone()),
            connector,
        }
    }

    pub(crate) fn forward_proxy_authorization(
        &self,
        target: &http::Uri,
    ) -> Option<http::HeaderValue> {
        self.connector.forward_proxy_authorization(target)
    }
}

pub(crate) struct RuntimeBridge {
    inner: Arc<RuntimeInner>,
}

struct RuntimeInner {
    handle: Handle,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    thread_id: ThreadId,
    body_pool: BodyPool,
}

impl fmt::Debug for RuntimeBridge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RuntimeBridge(<shared-event-loop>)")
    }
}

impl Clone for RuntimeBridge {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl RuntimeBridge {
    pub(crate) fn new() -> Result<Self, TransportError> {
        let (ready_sender, ready_receiver) = std_mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("zmin-http-runtime".to_owned())
            .stack_size(HTTP_RUNTIME_THREAD_STACK_BYTES)
            .spawn(move || {
                let runtime = Builder::new_current_thread()
                    .enable_all()
                    .max_blocking_threads(8)
                    .build();
                let Ok(runtime) = runtime else {
                    let _ = ready_sender.send(None);
                    return;
                };
                let (shutdown_sender, shutdown_receiver) = oneshot::channel();
                let handle = runtime.handle().clone();
                let thread_id = thread::current().id();
                if ready_sender
                    .send(Some((handle, shutdown_sender, thread_id)))
                    .is_err()
                {
                    return;
                }
                runtime.block_on(async {
                    let _ = shutdown_receiver.await;
                });
            })
            .map_err(|error| TransportError::Request(error.kind()))?;
        let Some((handle, shutdown, thread_id)) = ready_receiver
            .recv()
            .map_err(|_| TransportError::Request(io::ErrorKind::Other))?
        else {
            let _ = thread.join();
            return Err(TransportError::Request(io::ErrorKind::Other));
        };
        Ok(Self {
            inner: Arc::new(RuntimeInner {
                handle,
                shutdown: Mutex::new(Some(shutdown)),
                thread: Mutex::new(Some(thread)),
                thread_id,
                body_pool: BodyPool::new()?,
            }),
        })
    }

    #[cfg(test)]
    pub(crate) fn thread_id(&self) -> ThreadId {
        self.inner.thread_id
    }

    pub(crate) fn execute(
        &self,
        client: RuntimeClient,
        request: Request<RequestBody>,
        upload_completion: Option<oneshot::Receiver<Result<(), io::ErrorKind>>>,
        deadline: Option<Instant>,
        cancellation: Option<RequestBodyCancellation>,
    ) -> PendingResponse {
        let (sender, receiver) = std_mpsc::sync_channel(1);
        let runtime_guard = self.clone();
        let task = self.inner.handle.spawn(async move {
            let response = request_with_deadline(
                &client.client,
                request,
                upload_completion,
                deadline,
                cancellation.clone(),
            )
            .await;
            let result = match response {
                Ok(response) => Ok(RuntimeResponse::new(
                    response,
                    deadline,
                    cancellation,
                    runtime_guard,
                )),
                Err(error) => Err(error),
            };
            let _ = sender.send(result);
        });
        PendingResponse {
            receiver,
            abort: task.abort_handle(),
        }
    }

    pub(crate) fn request_body(
        &self,
        source: OpenRequestBody,
        deadline: Option<Instant>,
        request_cancellation: Option<RequestBodyCancellation>,
    ) -> Result<PreparedRequestBody, TransportError> {
        let length = match &source {
            OpenRequestBody::Bytes(bytes) => bytes.len() as u64,
            OpenRequestBody::VerifiedRegularFile { length, .. } => *length,
        };
        if length == 0 {
            match source {
                OpenRequestBody::Bytes(_) => {}
                OpenRequestBody::VerifiedRegularFile {
                    file,
                    expected_sha256,
                    cancellation,
                    ..
                } => verify_empty_regular_file(
                    &file,
                    expected_sha256,
                    &cancellation,
                    request_cancellation.as_ref(),
                )?,
            }
            return Ok(PreparedRequestBody::empty());
        }
        let (sender, body, completion) = RequestBody::channel(length);
        match source {
            OpenRequestBody::Bytes(bytes) => {
                self.inner
                    .handle
                    .spawn(pump_bytes(bytes, sender, deadline, request_cancellation));
            }
            OpenRequestBody::VerifiedRegularFile {
                file,
                length,
                expected_sha256,
                cancellation,
            } => self.inner.body_pool.submit(BodyJob {
                file,
                length,
                expected_sha256,
                cancellation,
                request_cancellation,
                sender,
                deadline,
            })?,
        }
        Ok(PreparedRequestBody {
            body,
            completion: Some(completion),
        })
    }
}

pub(crate) struct PreparedRequestBody {
    pub(crate) body: RequestBody,
    pub(crate) completion: Option<oneshot::Receiver<Result<(), io::ErrorKind>>>,
}

impl PreparedRequestBody {
    pub(crate) fn empty() -> Self {
        Self {
            body: RequestBody::empty(),
            completion: None,
        }
    }
}

impl Drop for RuntimeInner {
    fn drop(&mut self) {
        if let Ok(shutdown) = self.shutdown.get_mut() {
            if let Some(shutdown) = shutdown.take() {
                let _ = shutdown.send(());
            }
        }
        if thread::current().id() == self.thread_id {
            return;
        }
        if let Ok(thread) = self.thread.get_mut() {
            if let Some(thread) = thread.take() {
                let _ = thread.join();
            }
        }
    }
}

pub(crate) struct PendingResponse {
    receiver: StdReceiver<Result<RuntimeResponse, TransportError>>,
    abort: AbortHandle,
}

impl PendingResponse {
    pub(crate) fn wait(self) -> Result<RuntimeResponse, TransportError> {
        self.receiver
            .recv()
            .map_err(|_| TransportError::Request(io::ErrorKind::Other))?
    }
}

impl Drop for PendingResponse {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

pub(crate) struct RuntimeResponse {
    pub(crate) status: u16,
    pub(crate) headers: http::HeaderMap,
    pub(crate) expected_length: Option<u64>,
    pub(crate) body: RuntimeBody,
}

impl RuntimeResponse {
    fn new(
        response: Response<Incoming>,
        deadline: Option<Instant>,
        cancellation: Option<RequestBodyCancellation>,
        runtime_guard: RuntimeBridge,
    ) -> Self {
        let status = response.status().as_u16();
        let expected_length = response.body().size_hint().exact();
        let (parts, body) = response.into_parts();
        let (sender, receiver) = mpsc::channel(RESPONSE_BODY_BRIDGE_QUEUE_DEPTH);
        tokio::spawn(pump_response_body(
            body,
            sender,
            deadline,
            cancellation.clone(),
        ));
        Self {
            status,
            headers: parts.headers,
            expected_length,
            body: RuntimeBody {
                receiver,
                current: Bytes::new(),
                ended: false,
                cancellation,
                _runtime_guard: runtime_guard,
            },
        }
    }
}

enum DownloadMessage {
    Data(Bytes),
    End,
    Error(io::Error),
}

pub(crate) struct RuntimeBody {
    receiver: mpsc::Receiver<DownloadMessage>,
    current: Bytes,
    ended: bool,
    cancellation: Option<RequestBodyCancellation>,
    _runtime_guard: RuntimeBridge,
}

#[cfg(test)]
impl RuntimeBody {
    pub(crate) fn queued_messages(&self) -> usize {
        self.receiver.len()
    }
}

impl Read for RuntimeBody {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self
            .cancellation
            .as_ref()
            .is_some_and(RequestBodyCancellation::is_cancelled)
        {
            return Err(cancelled_read_error());
        }
        if self.current.is_empty() {
            let message = self.receiver.blocking_recv();
            if self
                .cancellation
                .as_ref()
                .is_some_and(RequestBodyCancellation::is_cancelled)
            {
                return Err(cancelled_read_error());
            }
            match message {
                Some(DownloadMessage::Data(chunk)) => self.current = chunk,
                Some(DownloadMessage::End) => {
                    self.ended = true;
                    return Ok(0);
                }
                Some(DownloadMessage::Error(error))
                    if error.kind() == io::ErrorKind::Interrupted =>
                {
                    return Err(cancelled_read_error());
                }
                Some(DownloadMessage::Error(error)) => return Err(error),
                None if self.ended => return Ok(0),
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "HTTP response body pump ended without a terminal frame",
                    ));
                }
            }
        }
        let amount = self.current.len().min(buffer.len());
        buffer[..amount].copy_from_slice(&self.current.split_to(amount));
        Ok(amount)
    }
}

async fn request_with_deadline(
    client: &RuntimeClientInner,
    request: Request<RequestBody>,
    mut upload_completion: Option<oneshot::Receiver<Result<(), io::ErrorKind>>>,
    deadline: Option<Instant>,
    cancellation: Option<RequestBodyCancellation>,
) -> Result<Response<Incoming>, TransportError> {
    let response = wait_with_deadline_and_cancellation(
        client.request(request),
        deadline,
        cancellation.as_ref(),
    )
    .await?
    .map_err(map_hyper_error)?;
    if response.status().is_success() {
        if let Some(completion) = upload_completion.as_mut() {
            match completion.try_recv() {
                Ok(completed) => completed.map_err(TransportError::Request)?,
                Err(TryRecvError::Empty) => {
                    if let Some(cancellation) = cancellation {
                        cancellation.cancel();
                    }
                    return Err(TransportError::PrematureSuccessResponse);
                }
                Err(TryRecvError::Closed) => {
                    return Err(TransportError::Request(io::ErrorKind::BrokenPipe));
                }
            }
        }
    }
    Ok(response)
}

async fn pump_response_body(
    mut body: Incoming,
    sender: mpsc::Sender<DownloadMessage>,
    deadline: Option<Instant>,
    cancellation: Option<RequestBodyCancellation>,
) {
    loop {
        let next = next_response_frame(&mut body, &sender);
        let next = match wait_with_deadline_and_cancellation(next, deadline, cancellation.as_ref())
            .await
        {
            Ok(next) => next,
            Err(TransportError::Cancelled) => {
                try_send_download_error(
                    &sender,
                    io::ErrorKind::Interrupted,
                    "HTTP request cancelled",
                );
                return;
            }
            Err(TransportError::Timeout) => {
                send_download_error(&sender, io::ErrorKind::TimedOut, "HTTP request timed out")
                    .await;
                return;
            }
            Err(_) => unreachable!("deadline helper only returns timeout or cancellation"),
        };
        let frame = match next {
            ResponseFramePoll::Closed => return,
            ResponseFramePoll::Frame(frame) => frame,
        };
        let Some(frame) = frame else {
            let _ = sender.send(DownloadMessage::End).await;
            return;
        };
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                let kind = if error
                    .source()
                    .and_then(|source| source.downcast_ref::<io::Error>())
                    .is_some_and(|error| error.kind() == io::ErrorKind::TimedOut)
                {
                    io::ErrorKind::TimedOut
                } else {
                    io::ErrorKind::Other
                };
                let _ = sender
                    .send(DownloadMessage::Error(io::Error::new(
                        kind,
                        "HTTP response body failed",
                    )))
                    .await;
                return;
            }
        };
        let Ok(mut data) = frame.into_data() else {
            continue;
        };
        while !data.is_empty() {
            let amount = data.len().min(BODY_BRIDGE_CHUNK_BYTES);
            let send = sender.send(DownloadMessage::Data(data.split_to(amount)));
            match wait_with_deadline_and_cancellation(send, deadline, cancellation.as_ref()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => return,
                Err(TransportError::Cancelled) => {
                    try_send_download_error(
                        &sender,
                        io::ErrorKind::Interrupted,
                        "HTTP request cancelled",
                    );
                    return;
                }
                Err(TransportError::Timeout) => {
                    send_download_error(&sender, io::ErrorKind::TimedOut, "HTTP request timed out")
                        .await;
                    return;
                }
                Err(_) => unreachable!("deadline helper only returns timeout or cancellation"),
            }
        }
    }
}

async fn send_download_error(
    sender: &mpsc::Sender<DownloadMessage>,
    kind: io::ErrorKind,
    message: &'static str,
) {
    let _ = sender
        .send(DownloadMessage::Error(io::Error::new(kind, message)))
        .await;
}

fn try_send_download_error(
    sender: &mpsc::Sender<DownloadMessage>,
    kind: io::ErrorKind,
    message: &'static str,
) {
    let _ = sender.try_send(DownloadMessage::Error(io::Error::new(kind, message)));
}

fn cancelled_read_error() -> io::Error {
    // `Read::read_to_end` and `io::copy` transparently retry Interrupted.
    // Preserve Interrupted as the internal terminal pump message, then expose
    // a non-retryable body error so cooperative cancellation cannot spin.
    io::Error::new(io::ErrorKind::ConnectionAborted, "HTTP request cancelled")
}

async fn wait_with_deadline_and_cancellation<F>(
    future: F,
    deadline: Option<Instant>,
    cancellation: Option<&RequestBodyCancellation>,
) -> Result<F::Output, TransportError>
where
    F: Future,
{
    let cancellable = async move {
        let Some(cancellation) = cancellation else {
            return Ok(future.await);
        };
        if cancellation.is_cancelled() {
            return Err(TransportError::Cancelled);
        }
        let cancelled = Box::pin(cancellation.cancelled());
        let future = Box::pin(future);
        match select(cancelled, future).await {
            Either::Left(_) => Err(TransportError::Cancelled),
            Either::Right((output, _)) => Ok(output),
        }
    };
    match deadline {
        Some(deadline) => {
            tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), cancellable)
                .await
                .map_err(|_| TransportError::Timeout)?
        }
        None => cancellable.await,
    }
}

enum ResponseFramePoll {
    Closed,
    Frame(Option<Result<Frame<Bytes>, hyper::Error>>),
}

async fn next_response_frame(
    body: &mut Incoming,
    sender: &mpsc::Sender<DownloadMessage>,
) -> ResponseFramePoll {
    let closed = Box::pin(sender.closed());
    let frame = Box::pin(body.frame());
    match select(closed, frame).await {
        Either::Left(_) => ResponseFramePoll::Closed,
        Either::Right((frame, _)) => ResponseFramePoll::Frame(frame),
    }
}

pub(crate) struct RequestBody {
    receiver: mpsc::Receiver<UploadMessage>,
    remaining: u64,
    ended: bool,
    completion: Option<oneshot::Sender<Result<(), io::ErrorKind>>>,
}

impl RequestBody {
    fn channel(
        length: u64,
    ) -> (
        mpsc::Sender<UploadMessage>,
        Self,
        oneshot::Receiver<Result<(), io::ErrorKind>>,
    ) {
        let (sender, receiver) = mpsc::channel(REQUEST_BODY_BRIDGE_QUEUE_DEPTH);
        let (completion, completed) = oneshot::channel();
        let initially_ended = length == 0;
        let completion = if initially_ended {
            let _ = completion.send(Ok(()));
            None
        } else {
            Some(completion)
        };
        (
            sender,
            Self {
                receiver,
                remaining: length,
                ended: initially_ended,
                completion,
            },
            completed,
        )
    }

    fn empty() -> Self {
        Self {
            receiver: mpsc::channel(1).1,
            remaining: 0,
            ended: true,
            completion: None,
        }
    }

    fn complete(&mut self, result: Result<(), io::ErrorKind>) {
        if let Some(completion) = self.completion.take() {
            let _ = completion.send(result);
        }
    }
}

enum UploadMessage {
    Data(Bytes),
    End,
    Error(io::Error),
}

impl Body for RequestBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match self.receiver.poll_recv(context) {
            Poll::Ready(Some(UploadMessage::Data(bytes))) => {
                if bytes.len() as u64 > self.remaining {
                    self.complete(Err(io::ErrorKind::InvalidData));
                    return Poll::Ready(Some(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "HTTP request body exceeds declared length",
                    ))));
                }
                self.remaining -= bytes.len() as u64;
                if self.remaining == 0 {
                    self.ended = true;
                    self.complete(Ok(()));
                }
                Poll::Ready(Some(Ok(Frame::data(bytes))))
            }
            Poll::Ready(Some(UploadMessage::End)) if self.remaining == 0 => {
                self.ended = true;
                self.complete(Ok(()));
                Poll::Ready(None)
            }
            Poll::Ready(Some(UploadMessage::End)) => {
                self.complete(Err(io::ErrorKind::UnexpectedEof));
                Poll::Ready(Some(Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "HTTP request body is shorter than declared length",
                ))))
            }
            Poll::Ready(Some(UploadMessage::Error(error))) => {
                self.complete(Err(error.kind()));
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) if self.ended => Poll::Ready(None),
            Poll::Ready(None) => {
                self.complete(Err(io::ErrorKind::UnexpectedEof));
                Poll::Ready(Some(Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "HTTP request body pump ended without a terminal frame",
                ))))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.ended
    }

    fn size_hint(&self) -> SizeHint {
        let mut hint = SizeHint::new();
        hint.set_exact(self.remaining);
        hint
    }
}

impl Drop for RequestBody {
    fn drop(&mut self) {
        if !self.ended {
            self.complete(Err(io::ErrorKind::BrokenPipe));
        }
    }
}

async fn pump_bytes(
    bytes: Arc<[u8]>,
    sender: mpsc::Sender<UploadMessage>,
    deadline: Option<Instant>,
    cancellation: Option<RequestBodyCancellation>,
) {
    for chunk in bytes.chunks(BODY_BRIDGE_CHUNK_BYTES) {
        if !send_upload_message(
            &sender,
            UploadMessage::Data(Bytes::copy_from_slice(chunk)),
            deadline,
            cancellation.as_ref(),
        )
        .await
        {
            return;
        }
    }
    let _ = send_upload_message(&sender, UploadMessage::End, deadline, cancellation.as_ref()).await;
}

async fn send_upload_message(
    sender: &mpsc::Sender<UploadMessage>,
    message: UploadMessage,
    deadline: Option<Instant>,
    cancellation: Option<&RequestBodyCancellation>,
) -> bool {
    wait_with_deadline_and_cancellation(sender.send(message), deadline, cancellation)
        .await
        .is_ok_and(|result| result.is_ok())
}

struct BodyJob {
    file: File,
    length: u64,
    expected_sha256: [u8; 32],
    cancellation: RequestBodyCancellation,
    request_cancellation: Option<RequestBodyCancellation>,
    sender: mpsc::Sender<UploadMessage>,
    deadline: Option<Instant>,
}

struct BodyPool {
    active: Arc<(Mutex<usize>, Condvar)>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl BodyPool {
    fn new() -> Result<Self, TransportError> {
        Ok(Self {
            active: Arc::new((Mutex::new(0), Condvar::new())),
            workers: Mutex::new(Vec::new()),
        })
    }

    fn submit(&self, job: BodyJob) -> Result<(), TransportError> {
        let mut workers = self
            .workers
            .lock()
            .map_err(|_| TransportError::Request(io::ErrorKind::Other))?;
        let mut index = 0;
        while index < workers.len() {
            if workers[index].is_finished() {
                let worker = workers.swap_remove(index);
                let _ = worker.join();
            } else {
                index += 1;
            }
        }
        let active = Arc::clone(&self.active);
        let (count, available) = active.as_ref();
        let mut count = count
            .lock()
            .map_err(|_| TransportError::Request(io::ErrorKind::Other))?;
        while *count >= BODY_WORKER_COUNT {
            count = available
                .wait(count)
                .map_err(|_| TransportError::Request(io::ErrorKind::Other))?;
        }
        *count += 1;
        drop(count);
        let worker_active = Arc::clone(&active);
        let worker = match thread::Builder::new()
            .name("zmin-http-body".to_owned())
            .stack_size(256 * 1024)
            .spawn(move || {
                pump_regular_file(job);
                let (count, available) = worker_active.as_ref();
                if let Ok(mut count) = count.lock() {
                    *count = count.saturating_sub(1);
                    available.notify_one();
                }
            }) {
            Ok(worker) => worker,
            Err(error) => {
                let (count, available) = active.as_ref();
                if let Ok(mut count) = count.lock() {
                    *count = count.saturating_sub(1);
                    available.notify_one();
                }
                return Err(TransportError::Request(error.kind()));
            }
        };
        workers.push(worker);
        Ok(())
    }
}

impl Drop for BodyPool {
    fn drop(&mut self) {
        if let Ok(workers) = self.workers.get_mut() {
            for worker in workers.drain(..) {
                let _ = worker.join();
            }
        }
    }
}

fn pump_regular_file(job: BodyJob) {
    let mut offset = 0_u64;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; BODY_BRIDGE_CHUNK_BYTES];
    let mut held_chunk = None;
    while offset < job.length {
        if job.sender.is_closed() || body_job_cancelled(&job) {
            return;
        }
        if job
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            let _ = send_body_message(
                &job,
                UploadMessage::Error(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "HTTP request timed out",
                )),
            );
            return;
        }
        let remaining = (job.length - offset).min(buffer.len() as u64) as usize;
        let amount = match read_file_at(&job.file, &mut buffer[..remaining], offset) {
            Ok(0) => {
                let _ = send_body_message(
                    &job,
                    UploadMessage::Error(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "HTTP request body file was truncated",
                    )),
                );
                return;
            }
            Ok(amount) => amount,
            Err(error) => {
                let _ = send_body_message(&job, UploadMessage::Error(error));
                return;
            }
        };
        if body_job_cancelled(&job) {
            return;
        }
        hasher.update(&buffer[..amount]);
        offset += amount as u64;
        let current = Bytes::copy_from_slice(&buffer[..amount]);
        if let Some(previous) = held_chunk.replace(current) {
            if !send_body_message(&job, UploadMessage::Data(previous)) {
                return;
            }
        }
    }
    if job.sender.is_closed() || body_job_cancelled(&job) {
        return;
    }
    let mut probe = [0_u8; 1];
    let unchanged = read_file_at(&job.file, &mut probe, offset).is_ok_and(|amount| amount == 0)
        && job
            .file
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() == job.length);
    let actual = hasher.finalize();
    if job.sender.is_closed() || body_job_cancelled(&job) {
        return;
    }
    if !unchanged || actual.as_slice() != job.expected_sha256 {
        let _ = send_body_message(
            &job,
            UploadMessage::Error(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP request body file failed integrity verification",
            )),
        );
        return;
    }
    if let Some(final_chunk) = held_chunk {
        if !send_body_message(&job, UploadMessage::Data(final_chunk)) {
            return;
        }
    }
    let _ = send_body_message(&job, UploadMessage::End);
}

fn send_body_message(job: &BodyJob, mut message: UploadMessage) -> bool {
    loop {
        if job.sender.is_closed() || body_job_cancelled(job) {
            return false;
        }
        if job
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return false;
        }
        match job.sender.try_send(message) {
            Ok(()) => return true,
            Err(TrySendError::Closed(_)) => return false,
            Err(TrySendError::Full(returned)) => {
                message = returned;
                thread::park_timeout(Duration::from_millis(1));
            }
        }
    }
}

fn body_job_cancelled(job: &BodyJob) -> bool {
    job.cancellation.is_cancelled()
        || job
            .request_cancellation
            .as_ref()
            .is_some_and(RequestBodyCancellation::is_cancelled)
}

fn verify_empty_regular_file(
    file: &File,
    expected_sha256: [u8; 32],
    body_cancellation: &RequestBodyCancellation,
    request_cancellation: Option<&RequestBodyCancellation>,
) -> Result<(), TransportError> {
    if body_cancellation.is_cancelled()
        || request_cancellation.is_some_and(RequestBodyCancellation::is_cancelled)
    {
        return Err(TransportError::Cancelled);
    }
    let metadata = file
        .metadata()
        .map_err(|error| TransportError::BodyFactory(error.kind()))?;
    let mut probe = [0_u8; 1];
    let is_empty = metadata.is_file()
        && metadata.len() == 0
        && read_file_at(file, &mut probe, 0)
            .map_err(|error| TransportError::BodyFactory(error.kind()))?
            == 0
        && file
            .metadata()
            .is_ok_and(|current| current.is_file() && current.len() == 0);
    let actual: [u8; 32] = Sha256::digest([]).into();
    if !is_empty || expected_sha256 != actual {
        return Err(TransportError::BodyFactory(io::ErrorKind::InvalidData));
    }
    if body_cancellation.is_cancelled()
        || request_cancellation.is_some_and(RequestBodyCancellation::is_cancelled)
    {
        return Err(TransportError::Cancelled);
    }
    Ok(())
}

#[cfg(unix)]
fn read_file_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.read_at(buffer, offset)
}

#[cfg(windows)]
fn read_file_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_read(buffer, offset)
}

fn map_hyper_error(error: HyperClientError) -> TransportError {
    let mut source: Option<&(dyn StdError + 'static)> = Some(&error);
    while let Some(current) = source {
        if let Some(connect) = current.downcast_ref::<ConnectError>() {
            return match connect {
                ConnectError::DialTimeout => TransportError::DialTimeout,
                ConnectError::TlsHandshakeTimeout => TransportError::TlsHandshakeTimeout,
                ConnectError::ActivityTimeout => TransportError::ActivityTimeout,
                ConnectError::InvalidProxy => TransportError::InvalidProxy,
                ConnectError::Connect
                | ConnectError::InvalidTarget
                | ConnectError::ProxyHandshake
                | ConnectError::Tls => TransportError::Connect,
            };
        }
        if let Some(error) = current.downcast_ref::<io::Error>() {
            return if error.kind() == io::ErrorKind::TimedOut {
                TransportError::ActivityTimeout
            } else {
                TransportError::Request(error.kind())
            };
        }
        source = current.source();
    }
    if error.is_connect() {
        TransportError::Connect
    } else {
        TransportError::Request(io::ErrorKind::Other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestFile {
        path: std::path::PathBuf,
        file: File,
    }

    impl Drop for TestFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    fn test_file(bytes: &[u8]) -> TestFile {
        let id = FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("zmin-http-body-{}-{id}", std::process::id()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .expect("temporary body file");
        file.write_all(bytes).expect("temporary body bytes");
        file.sync_all().expect("temporary body sync");
        TestFile { path, file }
    }

    fn sha256(bytes: &[u8]) -> [u8; 32] {
        Sha256::digest(bytes).into()
    }

    #[test]
    fn corrupt_regular_file_never_releases_final_declared_chunk() {
        let body = test_file(b"corrupt");
        let (sender, mut receiver) = mpsc::channel(2);
        pump_regular_file(BodyJob {
            file: body.file.try_clone().expect("clone file"),
            length: 7,
            expected_sha256: sha256(b"correct"),
            cancellation: RequestBodyCancellation::new(),
            request_cancellation: None,
            sender,
            deadline: None,
        });
        assert!(matches!(
            receiver.blocking_recv(),
            Some(UploadMessage::Error(error)) if error.kind() == io::ErrorKind::InvalidData
        ));
        assert!(receiver.blocking_recv().is_none());
    }

    #[test]
    fn cancelled_regular_file_worker_wakes_from_full_queue() {
        let bytes = vec![7_u8; BODY_BRIDGE_CHUNK_BYTES * 4];
        let body = test_file(&bytes);
        let cancellation = RequestBodyCancellation::new();
        let worker_cancellation = cancellation.clone();
        let (sender, receiver) = mpsc::channel(REQUEST_BODY_BRIDGE_QUEUE_DEPTH);
        let worker = thread::spawn(move || {
            pump_regular_file(BodyJob {
                file: body.file.try_clone().expect("clone file"),
                length: bytes.len() as u64,
                expected_sha256: sha256(&bytes),
                cancellation: worker_cancellation,
                request_cancellation: None,
                sender,
                deadline: None,
            });
        });
        let deadline = Instant::now() + Duration::from_secs(1);
        while receiver.len() < REQUEST_BODY_BRIDGE_QUEUE_DEPTH {
            assert!(Instant::now() < deadline, "body queue did not fill");
            thread::yield_now();
        }
        cancellation.cancel();
        worker.join().expect("cancelled body worker");
    }

    #[test]
    fn runtime_memory_bounds_are_exact() {
        assert_eq!(RESPONSE_BODY_BRIDGE_QUEUE_DEPTH, 1);
        assert_eq!(REQUEST_BODY_BRIDGE_QUEUE_DEPTH, 2);
        assert_eq!(HTTP_RUNTIME_THREAD_STACK_BYTES, 512 * 1024);
    }

    #[test]
    fn body_pool_starts_without_eager_workers() {
        let pool = BodyPool::new().expect("body pool");
        assert_eq!(pool.workers.lock().expect("workers").len(), 0);
        assert_eq!(*pool.active.0.lock().expect("active workers"), 0);
    }

    #[test]
    fn premature_download_channel_close_is_not_clean_eof() {
        let runtime = RuntimeBridge::new().expect("runtime");
        let (sender, receiver) = mpsc::channel(1);
        drop(sender);
        let mut body = RuntimeBody {
            receiver,
            current: Bytes::new(),
            ended: false,
            cancellation: None,
            _runtime_guard: runtime,
        };
        let error = body.read(&mut [0_u8; 1]).expect_err("premature close");
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }
}
