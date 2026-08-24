use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::{Instant, Sleep};

/// Async equivalent of Git LFS' `deadlineConn`: one deadline is shared by
/// reads and writes and is replaced whenever a new logical socket operation
/// starts. Re-polling the same pending operation never extends its deadline.
pub(crate) struct ActivityIo<T> {
    inner: T,
    timeout: Option<Duration>,
    timer: Option<Pin<Box<Sleep>>>,
    read_pending: bool,
    write_pending: bool,
    timed_out: bool,
}

impl<T> ActivityIo<T> {
    pub(crate) fn new(inner: T, timeout: Option<Duration>) -> Self {
        Self {
            inner,
            timeout,
            timer: None,
            read_pending: false,
            write_pending: false,
            timed_out: false,
        }
    }

    fn start_operation(&mut self) {
        let Some(timeout) = self.timeout else {
            return;
        };
        let deadline = Instant::now() + timeout;
        match self.timer.as_mut() {
            Some(timer) => timer.as_mut().reset(deadline),
            None => self.timer = Some(Box::pin(tokio::time::sleep_until(deadline))),
        }
    }

    fn timeout_error(&mut self) -> io::Error {
        self.timed_out = true;
        io::Error::new(io::ErrorKind::TimedOut, "HTTP TCP activity timed out")
    }

    fn poll_timeout(&mut self, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.timed_out {
            return Poll::Ready(Err(self.timeout_error()));
        }
        let Some(timer) = self.timer.as_mut() else {
            return Poll::Pending;
        };
        if timer.as_mut().poll(context).is_ready() {
            Poll::Ready(Err(self.timeout_error()))
        } else {
            Poll::Pending
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for ActivityIo<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.timed_out {
            return Poll::Ready(Err(self.timeout_error()));
        }
        if !self.read_pending {
            self.start_operation();
            self.read_pending = true;
        }
        match Pin::new(&mut self.inner).poll_read(context, buffer) {
            Poll::Ready(result) => {
                self.read_pending = false;
                Poll::Ready(result)
            }
            Poll::Pending => self.poll_timeout(context),
        }
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for ActivityIo<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        if self.timed_out {
            return Poll::Ready(Err(self.timeout_error()));
        }
        if !self.write_pending {
            self.start_operation();
            self.write_pending = true;
        }
        match Pin::new(&mut self.inner).poll_write(context, buffer) {
            Poll::Ready(result) => {
                self.write_pending = false;
                Poll::Ready(result)
            }
            Poll::Pending => match self.poll_timeout(context) {
                Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                Poll::Ready(Ok(())) | Poll::Pending => Poll::Pending,
            },
        }
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffers: &[io::IoSlice<'_>],
    ) -> Poll<Result<usize, io::Error>> {
        if self.timed_out {
            return Poll::Ready(Err(self.timeout_error()));
        }
        if !self.write_pending {
            self.start_operation();
            self.write_pending = true;
        }
        match Pin::new(&mut self.inner).poll_write_vectored(context, buffers) {
            Poll::Ready(result) => {
                self.write_pending = false;
                Poll::Ready(result)
            }
            Poll::Pending => match self.poll_timeout(context) {
                Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                Poll::Ready(Ok(())) | Poll::Pending => Poll::Pending,
            },
        }
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.inner).poll_flush(context)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
    }

    #[test]
    fn periodic_read_progress_outlives_one_activity_window() {
        runtime().block_on(async {
            let (client, mut peer) = tokio::io::duplex(8);
            let mut client = ActivityIo::new(client, Some(Duration::from_millis(35)));
            tokio::spawn(async move {
                for byte in 0_u8..6 {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    peer.write_all(&[byte]).await.expect("peer write");
                }
            });
            let started = Instant::now();
            let mut received = [0_u8; 6];
            client
                .read_exact(&mut received)
                .await
                .expect("rolling progress");
            assert!(started.elapsed() > Duration::from_millis(100));
            assert_eq!(received, [0, 1, 2, 3, 4, 5]);
        });
    }

    #[test]
    fn stalled_read_fails_at_activity_window() {
        runtime().block_on(async {
            let (client, _peer) = tokio::io::duplex(8);
            let mut client = ActivityIo::new(client, Some(Duration::from_millis(30)));
            let started = Instant::now();
            let error = client.read(&mut [0_u8; 1]).await.expect_err("stalled read");
            assert_eq!(error.kind(), io::ErrorKind::TimedOut);
            assert!(started.elapsed() >= Duration::from_millis(25));
        });
    }

    #[test]
    fn periodic_write_progress_outlives_one_activity_window() {
        runtime().block_on(async {
            let (client, mut peer) = tokio::io::duplex(1);
            let mut client = ActivityIo::new(client, Some(Duration::from_millis(35)));
            tokio::spawn(async move {
                let mut buffer = [0_u8; 1];
                for _ in 0..6 {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    peer.read_exact(&mut buffer).await.expect("peer read");
                }
            });
            let started = Instant::now();
            client
                .write_all(b"abcdef")
                .await
                .expect("rolling write progress");
            assert!(started.elapsed() > Duration::from_millis(80));
        });
    }
}
