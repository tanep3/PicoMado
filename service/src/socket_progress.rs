//! Connection-local positive AsyncWrite acceptance, never TCP ACK/arrival.
use std::io::{self, IoSlice};
use std::num::NonZeroU64;
use std::pin::Pin;
use std::sync::{atomic::{AtomicU64, Ordering}, Arc};
use std::task::{Context, Poll};
use axum::extract::connect_info::Connected;
use axum::serve::{IncomingStream, Listener};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

// Process-local order shared by listeners; it is never persisted. Allocation
// happens immediately after TCP accept, before HTTP/WebSocket/Hello handling.
pub(crate) fn next_accept_order() -> NonZeroU64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let value = NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .expect("connection acceptance order exhausted");
    NonZeroU64::new(value).expect("acceptance order is nonzero")
}

#[derive(Clone, Debug)]
pub struct WriteProgress {
    bytes: Arc<AtomicU64>,
    accept_order: NonZeroU64,
}
impl Default for WriteProgress {
    fn default() -> Self {
        Self { bytes: Arc::default(), accept_order: next_accept_order() }
    }
}
impl WriteProgress {
    pub fn bytes(&self) -> u64 { self.bytes.load(Ordering::Relaxed) }
    pub(crate) fn accept_order(&self) -> NonZeroU64 { self.accept_order }
    fn accepted(&self, n: usize) { self.bytes.fetch_add(n as u64, Ordering::Relaxed); }
    #[cfg(test)]
    pub(crate) fn test_accept(&self, n: usize) { self.accepted(n); }
}

pub struct TrackedIo<I> { inner: I, progress: WriteProgress }
impl<I: AsyncRead + Unpin> AsyncRead for TrackedIo<I> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        #[cfg(not(feature="ack-path-probe"))]
        {Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)}
        #[cfg(feature="ack-path-probe")]
        {let this=self.get_mut();let connection=this.progress.accept_order().get();let before=buf.filled().len();
         crate::ack_probe::record(crate::ack_probe::Event::RawPoll,connection,0,0,0,0,0,0);
         let result=Pin::new(&mut this.inner).poll_read(cx,buf);
         let event=match &result{Poll::Pending=>crate::ack_probe::Event::RawPending,Poll::Ready(Ok(()))=>crate::ack_probe::Event::RawReady,Poll::Ready(Err(_))=>crate::ack_probe::Event::RawError};
         crate::ack_probe::record(event,connection,0,0,0,0,(buf.filled().len()-before)as u64,0);result}
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

    struct MemoryListener(std::collections::VecDeque<tokio::io::DuplexStream>);
    impl Listener for MemoryListener {
        type Io = tokio::io::DuplexStream;
        type Addr = std::net::SocketAddr;
        async fn accept(&mut self) -> (Self::Io, Self::Addr) {
            (self.0.pop_front().expect("configured in-memory accepts"), self.local_addr().unwrap())
        }
        fn local_addr(&self) -> io::Result<Self::Addr> { Ok("127.0.0.1:0".parse().unwrap()) }
    }

    #[tokio::test]
    async fn accept_order_precedes_handshake_and_survives_listener_restart() {
        let (a, _peer_a) = tokio::io::duplex(64);
        let (b, _peer_b) = tokio::io::duplex(64);
        let mut listener = TrackedListener(MemoryListener([a, b].into()));
        let (old, _) = listener.accept().await;
        let (new, _) = listener.accept().await;
        // No HTTP/WS/Hello has been polled: metadata already has the order.
        assert!(old.progress.accept_order() < new.progress.accept_order());
        assert_eq!(old.progress.clone().accept_order(), old.progress.accept_order());
        let previous = new.progress.accept_order();
        drop(listener);
        let (next, _peer) = tokio::io::duplex(64);
        let mut restarted = TrackedListener(MemoryListener([next].into()));
        let (next, _) = restarted.accept().await;
        assert!(next.progress.accept_order() > previous);
    }
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
