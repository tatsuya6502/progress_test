use std::{
    fmt,
    future::Future,
    io::SeekFrom,
    mem::ManuallyDrop as ManDrop,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::prelude::*;
use tokio::{
    io::{AsyncSeek, Result},
    sync::{Mutex, MutexGuard},
};

struct ProgressInner<T> {
    size: u64,
    buf: T,
}

impl<T> fmt::Debug for ProgressInner<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProgressInner")
            .field("size", &self.size)
            .finish()
    }
}

type MutexFuture<'a, T> = Box<dyn Future<Output = MutexGuard<'a, T>> + Send + Sync>;

enum LockState<T: 'static> {
    Released,
    Acquiring(Pin<MutexFuture<'static, T>>),
    Locked(MutexGuard<'static, T>),
}

pub struct Progress<T: 'static> {
    inner: Pin<Arc<Mutex<ProgressInner<T>>>>,
    lock_state: ManDrop<LockState<ProgressInner<T>>>,
}

impl<T> Clone for Progress<T> {
    fn clone(&self) -> Self {
        Progress {
            inner: self.inner.clone(),
            lock_state: ManDrop::new(LockState::Released),
        }
    }
}

impl<T> Drop for Progress<T> {
    fn drop(&mut self) {
        unsafe { ManDrop::drop(&mut self.lock_state) };
    }
}

impl<T: Unpin + Send> Progress<T> {
    pub fn new(buf: T) -> Self {
        let inner = Arc::pin(Mutex::new(ProgressInner { size: 0, buf }));
        Progress {
            inner,
            lock_state: ManDrop::new(LockState::Released),
        }
    }

    pub async fn to_size(&self) -> u64 {
        let pg = self.inner.lock().await;
        pg.size
    }

    fn lock_and_then<U, F>(&mut self, cx: &mut Context<'_>, f: F) -> Poll<Result<U>>
    where
        F: FnOnce(Pin<&mut T>, &mut Context<'_>, &mut u64) -> Poll<Result<U>>,
    {
        use LockState::*;
        loop {
            match &mut *self.lock_state {
                Released => {
                    let mutex = &self.inner as &Mutex<_> as *const Mutex<_>;
                    let fut = unsafe { Box::pin((*mutex).lock()) };
                    self.set_lock_state(Acquiring(fut));
                }
                Acquiring(fut) => match fut.as_mut().as_mut().poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(guard) => self.set_lock_state(Locked(guard)),
                },
                Locked(guard) => {
                    let ProgressInner { buf, size } = &mut **guard;
                    let pin = Pin::new(buf);
                    let poll = f(pin, cx, size);
                    if let Poll::Ready(..) = &poll {
                        self.set_lock_state(Released);
                    }
                    return poll;
                }
            };
        }
    }

    fn set_lock_state(&mut self, new: LockState<ProgressInner<T>>) {
        let mut old = std::mem::replace(&mut self.lock_state, ManDrop::new(new));
        unsafe { ManDrop::drop(&mut old) }
    }
}

impl<T: AsyncRead + Unpin + Send> AsyncRead for Progress<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize>> {
        self.lock_and_then(cx, |pin, cx, _| pin.poll_read(cx, buf))
    }
}

impl<T: AsyncWrite + Unpin + Send> AsyncWrite for Progress<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize>> {
        self.lock_and_then(cx, |pin, cx, size| {
            let poll = pin.poll_write(cx, buf);
            if let Poll::Ready(Ok(n)) = poll {
                *size += n as u64;
            }
            poll
        })
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        self.lock_and_then(cx, |pin, cx, _| pin.poll_flush(cx))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        self.lock_and_then(cx, |pin, cx, _| pin.poll_shutdown(cx))
    }
}

impl<T: AsyncSeek + Unpin + Send> AsyncSeek for Progress<T> {
    fn start_seek(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        position: SeekFrom,
    ) -> Poll<Result<()>> {
        self.lock_and_then(cx, |pin, cx, _| pin.start_seek(cx, position))
    }

    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<u64>> {
        self.lock_and_then(cx, |pin, cx, _| pin.poll_complete(cx))
    }
}
