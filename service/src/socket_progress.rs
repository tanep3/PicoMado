//! Connection-local positive AsyncWrite acceptance, never TCP ACK/arrival.
use std::io::{self, IoSlice};
use std::pin::Pin;
use std::sync::{atomic::{AtomicU64, Ordering}, Arc};
use std::task::{Context, Poll};
use axum::extract::connect_info::Connected;
use axum::serve::{IncomingStream, Listener};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[derive(Clone, Default, Debug)]
pub struct WriteProgress(Arc<AtomicU64>);
impl WriteProgress {
    pub fn bytes(&self) -> u64 { self.0.load(Ordering::Relaxed) }
    fn accepted(&self, n: usize) { self.0.fetch_add(n as u64, Ordering::Relaxed); }
    #[cfg(test)]
    pub(crate) fn test_accept(&self, n: usize) { self.accepted(n); }
}

pub struct TrackedIo<I> { inner: I, progress: WriteProgress }
impl<I: AsyncRead + Unpin> AsyncRead for TrackedIo<I> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}
impl<I: AsyncWrite + Unpin> AsyncWrite for TrackedIo<I> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this=self.get_mut();
        let result=Pin::new(&mut this.inner).poll_write(cx,buf);
        if let Poll::Ready(Ok(n))=result { this.progress.accepted(n); }
        result
    }
    fn poll_write_vectored(self: Pin<&mut Self>, cx: &mut Context<'_>, bufs: &[IoSlice<'_>]) -> Poll<io::Result<usize>> {
        let this=self.get_mut();
        let result=Pin::new(&mut this.inner).poll_write_vectored(cx,bufs);
        if let Poll::Ready(Ok(n))=result { this.progress.accepted(n); }
        result
    }
    fn is_write_vectored(&self)->bool { self.inner.is_write_vectored() }
    fn poll_flush(self:Pin<&mut Self>,cx:&mut Context<'_>)->Poll<io::Result<()>> { Pin::new(&mut self.get_mut().inner).poll_flush(cx) }
    fn poll_shutdown(self:Pin<&mut Self>,cx:&mut Context<'_>)->Poll<io::Result<()>> { Pin::new(&mut self.get_mut().inner).poll_shutdown(cx) }
}

pub struct TrackedListener<L>(pub L);
impl<L:Listener> Listener for TrackedListener<L> {
    type Io=TrackedIo<L::Io>;
    type Addr=L::Addr;
    async fn accept(&mut self)->(Self::Io,Self::Addr) {
        let (inner,addr)=self.0.accept().await;
        (TrackedIo{inner,progress:WriteProgress::default()},addr)
    }
    fn local_addr(&self)->io::Result<Self::Addr> { self.0.local_addr() }
}
impl<L:Listener> Connected<IncomingStream<'_,TrackedListener<L>>> for WriteProgress {
    fn connect_info(stream:IncomingStream<'_,TrackedListener<L>>)->Self { stream.io().progress.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::task::noop_waker;
    struct Partial;
    impl AsyncWrite for Partial {
        fn poll_write(self:Pin<&mut Self>,_:&mut Context<'_>,b:&[u8])->Poll<io::Result<usize>> {
            match b.first(){Some(0)=>Poll::Pending,Some(1)=>Poll::Ready(Ok(0)),_=>Poll::Ready(Ok(2.min(b.len())))}
        }
        fn poll_flush(self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<io::Result<()>> {Poll::Ready(Ok(()))}
        fn poll_shutdown(self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<io::Result<()>> {Poll::Ready(Ok(()))}
    }
    #[test]
    fn only_positive_kernel_write_acceptance_is_progress() {
        let p=WriteProgress::default();let mut io=TrackedIo{inner:Partial,progress:p.clone()};
        let w=noop_waker();let mut cx=Context::from_waker(&w);
        assert!(Pin::new(&mut io).poll_write(&mut cx,&[0]).is_pending());
        assert!(matches!(Pin::new(&mut io).poll_write(&mut cx,&[1]),Poll::Ready(Ok(0))));
        assert_eq!(p.bytes(),0);
        assert!(matches!(Pin::new(&mut io).poll_write(&mut cx,&[2,3,4]),Poll::Ready(Ok(2))));
        assert_eq!(p.bytes(),2);
        assert!(Pin::new(&mut io).poll_flush(&mut cx).is_ready());assert_eq!(p.bytes(),2);
    }

    #[tokio::test]
    async fn connect_info_tracks_the_actual_upgraded_socket() {
        use axum::{Router, routing::get, extract::{ConnectInfo, ws::{WebSocketUpgrade, Message}}};
        use futures_util::StreamExt;
        let (tx,mut rx)=tokio::sync::mpsc::channel(1);
        let router=Router::new().route("/probe",get(move |ConnectInfo(p):ConnectInfo<WriteProgress>,ws:WebSocketUpgrade| {
            let tx=tx.clone();async move { ws.on_upgrade(move |mut socket|async move {
                let before=p.bytes();socket.send(Message::Binary(vec![7;100].into())).await.unwrap();
                tx.send((before,p.bytes())).await.unwrap();
            })}
        }));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {axum::serve(TrackedListener(listener),router.into_make_service_with_connect_info::<WriteProgress>()).await.unwrap()});
        let (mut client,_)=tokio_tungstenite::connect_async(format!("ws://{address}/probe")).await.unwrap();
        assert!(client.next().await.unwrap().unwrap().is_binary());
        let (before,after)=rx.recv().await.unwrap();assert!(before>0&&after>=before+102);
        drop(client);server.abort();let _=server.await;
    }
}
