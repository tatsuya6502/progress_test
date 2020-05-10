use std::{
    fmt,
    io::SeekFrom,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    task::{Context, Poll},
};
use tokio::io::{AsyncSeek, Result};
use tokio::prelude::*;

// 非同期IOの進捗を表す型。複数の非同期タスクから共有されるのでAtomic系の型を使う
pub struct Progress {
    size: AtomicU64,
}

impl fmt::Debug for Progress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Progress")
            .field("size", &self.size)
            .finish()
    }
}

impl Progress {
    pub fn to_size(&self) -> u64 {
        // Acquire (Release-Acquire) は今回の用途には十分
        // これに対応するマシン命令はx86系など多くのプロセッサに標準装備されており
        // 効率よく実行できる
        self.size.load(Ordering::Acquire)
    }
}

// AsyncWriteなどを提供する型。一つの非同期タスクのみから使われることを前提にしている
pub struct ProgressDecorator<T> {
    buf: T,
    progress: Arc<Progress>,
}

impl<T> ProgressDecorator<T> {
    pub fn new(buf: T) -> Self {
        Self {
            progress: Arc::new(Progress {
                size: AtomicU64::default(),
            }),
            buf,
        }
    }

    pub fn progress(&self) -> Arc<Progress> {
        Arc::clone(&self.progress)
    }
}

impl<T: AsyncRead + Unpin + Send> AsyncRead for ProgressDecorator<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize>> {
        Pin::new(&mut self.buf).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Unpin + Send> AsyncWrite for ProgressDecorator<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize>> {
        let poll = Pin::new(&mut self.buf).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = poll {
            self.progress.size.fetch_add(n as u64, Ordering::Acquire);
        }
        poll
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        Pin::new(&mut self.buf).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        Pin::new(&mut self.buf).poll_shutdown(cx)
    }
}

impl<T: AsyncSeek + Unpin + Send> AsyncSeek for ProgressDecorator<T> {
    fn start_seek(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        position: SeekFrom,
    ) -> Poll<Result<()>> {
        Pin::new(&mut self.buf).start_seek(cx, position)
    }

    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<u64>> {
        Pin::new(&mut self.buf).poll_complete(cx)
    }
}
