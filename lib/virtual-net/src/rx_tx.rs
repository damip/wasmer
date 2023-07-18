use std::{
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use crate::Result;
use futures_util::{future::BoxFuture, Future, Sink, SinkExt, Stream};
use serde::Serialize;
use tokio::{
    io::AsyncWrite,
    sync::{
        mpsc::{self, error::TrySendError},
        oneshot,
    },
};
use wasmer_virtual_io::InlineWaker;

use crate::{io_err_into_net_error, NetworkError};

#[derive(Debug, Clone, Default)]
pub(crate) struct RemoteTxWakers {
    wakers: Arc<Mutex<Vec<Waker>>>,
}
impl RemoteTxWakers {
    pub fn add(&self, waker: &Waker) {
        let mut guard = self.wakers.lock().unwrap();
        if guard.iter().any(|w| w.will_wake(waker)) == false {
            guard.push(waker.clone());
        }
    }
    pub fn wake(&self) {
        let mut guard = self.wakers.lock().unwrap();
        guard.drain(..).for_each(|w| w.wake());
    }
}

#[derive(Debug, Default)]
struct FailOnWrite {}
impl AsyncWrite for FailOnWrite {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::ErrorKind::ConnectionAborted.into()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::ErrorKind::ConnectionAborted.into()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::ErrorKind::ConnectionAborted.into()))
    }
}

pub(crate) enum RemoteTx<T>
where
    T: Serialize,
{
    Mpsc {
        tx: mpsc::Sender<T>,
        work: mpsc::UnboundedSender<BoxFuture<'static, ()>>,
        wakers: RemoteTxWakers,
    },
    Stream {
        tx: Arc<tokio::sync::Mutex<Pin<Box<dyn Sink<T, Error = std::io::Error> + Send + 'static>>>>,
        work: mpsc::UnboundedSender<BoxFuture<'static, ()>>,
        wakers: RemoteTxWakers,
    },
}
impl<T> RemoteTx<T>
where
    T: Serialize + Send + Sync + 'static,
{
    pub(crate) async fn send(&self, req: T) -> Result<()> {
        match self {
            RemoteTx::Mpsc { tx, .. } => tx
                .send(req)
                .await
                .map_err(|_| NetworkError::ConnectionAborted),
            RemoteTx::Stream { tx, work, .. } => {
                let (tx_done, rx_done) = oneshot::channel();
                let tx = tx.clone();
                work.send(Box::pin(async move {
                    let job = async {
                        let mut tx_guard = tx.lock_owned().await;
                        tx_guard.send(req).await.map_err(io_err_into_net_error)
                    };
                    tx_done.send(job.await).ok();
                }))
                .map_err(|_| NetworkError::ConnectionAborted)?;

                rx_done
                    .await
                    .unwrap_or_else(|_| Err(NetworkError::ConnectionAborted))
            }
        }
    }

    pub(crate) fn poll_send(&self, cx: &mut Context<'_>, req: T) -> Poll<Result<()>> {
        match self {
            RemoteTx::Mpsc { tx, wakers, .. } => match tx.try_send(req) {
                Ok(()) => Poll::Ready(Ok(())),
                Err(TrySendError::Closed(_)) => Poll::Ready(Err(NetworkError::ConnectionAborted)),
                Err(TrySendError::Full(_)) => {
                    wakers.add(cx.waker());
                    Poll::Pending
                }
            },
            RemoteTx::Stream { tx, work, wakers } => {
                let mut tx_guard = match tx.clone().try_lock_owned() {
                    Ok(lock) => lock,
                    Err(_) => {
                        wakers.add(cx.waker());
                        return Poll::Pending;
                    }
                };
                match tx_guard.poll_ready_unpin(cx) {
                    Poll::Ready(Ok(())) => {}
                    Poll::Ready(Err(err)) => return Poll::Ready(Err(io_err_into_net_error(err))),
                    Poll::Pending => return Poll::Pending,
                }
                let mut job = Box::pin(async move {
                    if let Err(err) = tx_guard.send(req).await.map_err(io_err_into_net_error) {
                        tracing::error!("failed to send remaining bytes for request - {}", err);
                    }
                });

                // First we try to finish it synchronously
                if job.as_mut().poll(cx).is_ready() {
                    return Poll::Ready(Ok(()));
                }

                // Otherwise we push it to the driver which will block all future send
                // operations until it finishes
                work.send(job).map_err(|err| {
                    tracing::error!("failed to send remaining bytes for request - {}", err);
                    NetworkError::ConnectionAborted
                })?;
                Poll::Ready(Ok(()))
            }
        }
    }

    pub(crate) fn send_with_driver(&self, req: T) -> Result<()> {
        match self {
            RemoteTx::Mpsc { tx, work, .. } => match tx.try_send(req) {
                Ok(()) => Ok(()),
                Err(TrySendError::Closed(_)) => Err(NetworkError::ConnectionAborted),
                Err(TrySendError::Full(req)) => {
                    let tx = tx.clone();
                    work.send(Box::pin(async move {
                        tx.send(req).await.ok();
                    }))
                    .ok();
                    Ok(())
                }
            },
            RemoteTx::Stream { tx, work, .. } => {
                let mut tx_guard = match tx.clone().try_lock_owned() {
                    Ok(lock) => lock,
                    Err(_) => {
                        let tx = tx.clone();
                        work.send(Box::pin(async move {
                            let mut tx_guard = tx.lock().await;
                            tx_guard.send(req).await.ok();
                        }))
                        .ok();
                        return Ok(());
                    }
                };

                let inline_waker = InlineWaker::new();
                let waker = inline_waker.as_waker();
                let mut cx = Context::from_waker(&waker);

                let mut job = Box::pin(async move {
                    if let Err(err) = tx_guard.send(req).await.map_err(io_err_into_net_error) {
                        tracing::error!("failed to send remaining bytes for request - {}", err);
                    }
                });

                // First we try to finish it synchronously
                if job.as_mut().poll(&mut cx).is_ready() {
                    return Ok(());
                }

                // Otherwise we push it to the driver which will block all future send
                // operations until it finishes
                work.send(job).map_err(|err| {
                    tracing::error!("failed to send remaining bytes for request - {}", err);
                    NetworkError::ConnectionAborted
                })?;
                Ok(())
            }
        }
    }
}

pub(crate) enum RemoteRx<T>
where
    T: serde::de::DeserializeOwned,
{
    Mpsc {
        rx: mpsc::Receiver<T>,
        wakers: RemoteTxWakers,
    },
    Stream {
        rx: Pin<Box<dyn Stream<Item = std::io::Result<T>> + Send + 'static>>,
    },
}
impl<T> RemoteRx<T>
where
    T: serde::de::DeserializeOwned,
{
    pub(crate) fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Option<T>> {
        loop {
            return match self {
                RemoteRx::Mpsc { rx, wakers } => {
                    let ret = Pin::new(rx).poll_recv(cx);
                    if ret.is_ready() {
                        wakers.wake();
                    }
                    ret
                }
                RemoteRx::Stream { rx } => match rx.as_mut().poll_next(cx) {
                    Poll::Ready(Some(Ok(msg))) => Poll::Ready(Some(msg)),
                    Poll::Ready(Some(Err(err))) => {
                        tracing::warn!("failed to read from channel - {}", err);
                        Poll::Ready(None)
                    }
                    Poll::Ready(None) => Poll::Ready(None),
                    Poll::Pending => Poll::Pending,
                },
            };
        }
    }
}