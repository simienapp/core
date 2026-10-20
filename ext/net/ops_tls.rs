// Copyright 2018-2026 the Deno authors. MIT license.

use std::borrow::Cow;
use std::cell::RefCell;
use std::convert::From;
use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::sync::Arc;

use simien_core::AsyncRefCell;
use simien_core::convert::ByteString;
use simien_core::AsyncResult;
use simien_core::CancelHandle;
use simien_core::CancelTryFuture;
use simien_core::RcRef;
use simien_core::Resource;
use simien_core::futures::TryFutureExt;
use simien_error::JsErrorBox;
use simien_tls::ServerConfigProvider;
use simien_tls::rustls::ServerConfig;
pub use rustls_tokio_stream::TlsStream;
pub use rustls_tokio_stream::TlsStreamRead;
pub use rustls_tokio_stream::TlsStreamWrite;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::tcp::TcpListener;

pub(crate) const TLS_BUFFER_SIZE: Option<NonZeroUsize> =
  NonZeroUsize::new(65536);

pub struct TlsListener {
  pub(crate) tcp_listener: TcpListener,
  pub(crate) tls_config: Option<Arc<ServerConfig>>,
  pub(crate) server_config_provider: Option<ServerConfigProvider>,
}

impl TlsListener {
  pub async fn accept(
    &self,
  ) -> std::io::Result<(TlsStream<TcpStream>, SocketAddr)> {
    let (tcp, addr) = self.tcp_listener.accept().await?;
    let tls = if let Some(provider) = &self.server_config_provider {
      TlsStream::new_server_side_acceptor(
        tcp,
        provider.clone(),
        TLS_BUFFER_SIZE,
      )
    } else {
      TlsStream::new_server_side(
        tcp,
        self.tls_config.clone().unwrap(),
        TLS_BUFFER_SIZE,
      )
    };
    Ok((tls, addr))
  }
  pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
    self.tcp_listener.local_addr()
  }
}

#[derive(Debug)]
enum TlsStreamInner {
  Tcp {
    rd: AsyncRefCell<TlsStreamRead<TcpStream>>,
    wr: AsyncRefCell<TlsStreamWrite<TcpStream>>,
  },
}

#[derive(Debug)]
pub struct TlsStreamResource {
  inner: TlsStreamInner,
  // `None` when a TLS handshake hasn't been done.
  handshake_info: RefCell<Option<TlsHandshakeInfo>>,
  // op_cancel_read only cancels read and handshake ops. Closing the resource
  // cancels both halves so blocked writes don't keep the underlying socket
  // alive.
  read_cancel_handle: CancelHandle,
  write_cancel_handle: CancelHandle,
}

impl TlsStreamResource {
  pub fn new_tcp(
    (rd, wr): (TlsStreamRead<TcpStream>, TlsStreamWrite<TcpStream>),
  ) -> Self {
    Self {
      inner: TlsStreamInner::Tcp {
        rd: AsyncRefCell::new(rd),
        wr: AsyncRefCell::new(wr),
      },
      handshake_info: RefCell::new(None),
      read_cancel_handle: Default::default(),
      write_cancel_handle: Default::default(),
    }
  }

  pub fn into_tls_stream(self) -> TlsStream<TcpStream> {
    match self.inner {
      TlsStreamInner::Tcp { rd, wr } => {
        let read_half = rd.into_inner();
        let write_half = wr.into_inner();
        read_half.unsplit(write_half)
      }
    }
  }

  pub fn peer_certificates(
    &self,
  ) -> Option<
    Vec<rustls_tokio_stream::rustls::pki_types::CertificateDer<'static>>,
  > {
    self
      .handshake_info
      .borrow()
      .as_ref()
      .and_then(|info| info.peer_certificates.clone())
  }

  pub async fn read(
    self: Rc<Self>,
    data: &mut [u8],
  ) -> Result<usize, std::io::Error> {
    let mut rd = RcRef::map(&self, |r| match r.inner {
      TlsStreamInner::Tcp { ref rd, .. } => rd,
    })
    .borrow_mut()
    .await;
    let cancel_handle = RcRef::map(&self, |r| &r.read_cancel_handle);
    rd.read(data).try_or_cancel(cancel_handle).await
  }

  pub async fn write(
    self: Rc<Self>,
    data: &[u8],
  ) -> Result<usize, std::io::Error> {
    let mut wr = RcRef::map(&self, |r| match r.inner {
      TlsStreamInner::Tcp { ref wr, .. } => wr,
    })
    .borrow_mut()
    .await;
    let cancel_handle = RcRef::map(&self, |r| &r.write_cancel_handle);
    async {
      let nwritten = wr.write(data).await?;
      wr.flush().await?;
      Ok(nwritten)
    }
    .try_or_cancel(cancel_handle)
    .await
  }

  pub async fn shutdown(self: Rc<Self>) -> Result<(), std::io::Error> {
    let mut wr = RcRef::map(&self, |r| match r.inner {
      TlsStreamInner::Tcp { ref wr, .. } => wr,
    })
    .borrow_mut()
    .await;
    let cancel_handle = RcRef::map(&self, |r| &r.write_cancel_handle);
    wr.shutdown().try_or_cancel(cancel_handle).await?;
    Ok(())
  }

  pub async fn handshake(
    self: &Rc<Self>,
  ) -> Result<TlsHandshakeInfo, std::io::Error> {
    if let Some(tls_info) = &*self.handshake_info.borrow() {
      return Ok(tls_info.clone());
    }

    let mut wr = RcRef::map(self, |r| match r.inner {
      TlsStreamInner::Tcp { ref wr, .. } => wr,
    })
    .borrow_mut()
    .await;
    let cancel_handle = RcRef::map(self, |r| &r.read_cancel_handle);
    let handshake = wr.handshake().try_or_cancel(cancel_handle).await?;

    let alpn_protocol = handshake.alpn.map(|alpn| alpn.into());
    let peer_certificates = handshake.peer_certificates.clone();
    let tls_info = TlsHandshakeInfo {
      alpn_protocol,
      peer_certificates,
    };
    self.handshake_info.replace(Some(tls_info.clone()));
    Ok(tls_info)
  }
}

impl Resource for TlsStreamResource {
  simien_core::impl_readable_byob!();
  simien_core::impl_writable!();

  fn name(&self) -> Cow<'_, str> {
    "tlsStream".into()
  }

  fn shutdown(self: Rc<Self>) -> AsyncResult<()> {
    Box::pin(self.shutdown().map_err(JsErrorBox::from_err))
  }

  fn close(self: Rc<Self>) {
    self.read_cancel_handle.cancel();
    self.write_cancel_handle.cancel();
  }
}

#[derive(Clone, Debug)]
pub struct TlsHandshakeInfo {
  pub alpn_protocol: Option<ByteString>,
  pub peer_certificates:
    Option<Vec<rustls_tokio_stream::rustls::pki_types::CertificateDer<'static>>>,
}
