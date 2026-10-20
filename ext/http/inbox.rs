// Copyright 2018-2026 the Simien authors. MIT license.

//! Connections accepted elsewhere, served by one isolate.
//!
//! Upstream's server accepts from a listener it owns. In Simien the embedder
//! accepts (one place, any thread) and decides which isolate gets each
//! connection; that isolate's server reads them from its [`InboxListener`].
//! A connection crosses threads as a `std` stream, which no reactor owns yet,
//! and is registered with the isolate's own runtime when it's accepted.
//!
//! ```text
//! embedder thread                    isolate thread
//! accept() ─▶ ConnectionSender ──▶ InboxListener ─▶ simien_http (Deno.serve)
//! ```

use std::borrow::Cow;
use std::cell::RefCell;
use std::future::poll_fn;
use std::net::SocketAddr;
use std::rc::Rc;

use simien_core::Resource;
use simien_core::ResourceId;
use simien_core::ResourceTable;
use simien_error::JsErrorBox;
use simien_net::raw::NetworkStream;
use simien_net::raw::NetworkStreamAddress;
use simien_net::raw::NetworkStreamType;
use tokio::sync::mpsc;

/// A connection for an isolate to serve, before any runtime owns it.
pub enum InboxStream {
  Tcp(std::net::TcpStream),
  #[cfg(unix)]
  Unix(std::os::unix::net::UnixStream),
}

impl InboxStream {
  /// Registers the stream with the current (isolate's) runtime.
  fn register(self) -> std::io::Result<NetworkStream> {
    match self {
      Self::Tcp(stream) => {
        stream.set_nonblocking(true)?;
        Ok(NetworkStream::Tcp(tokio::net::TcpStream::from_std(stream)?))
      }
      #[cfg(unix)]
      Self::Unix(stream) => {
        stream.set_nonblocking(true)?;
        Ok(NetworkStream::Unix(tokio::net::UnixStream::from_std(stream)?))
      }
    }
  }
}

/// The embedder's end: hands connections to one isolate. Cheap to clone and
/// `Send`, so the accept loop can live on any thread.
#[derive(Clone)]
pub struct ConnectionSender(mpsc::Sender<InboxStream>);

impl ConnectionSender {
  /// Queues a connection. When the inbox is full or its isolate is gone the
  /// stream comes back, so the embedder can give it to another isolate.
  pub fn try_send(&self, stream: InboxStream) -> Result<(), InboxStream> {
    self.0.try_send(stream).map_err(|err| match err {
      mpsc::error::TrySendError::Full(stream)
      | mpsc::error::TrySendError::Closed(stream) => stream,
    })
  }

  /// Connections queued and not yet taken by the isolate.
  pub fn queued(&self) -> usize {
    self.0.max_capacity() - self.0.capacity()
  }
}

/// The isolate's end, as the listener `op_http_serve` takes by resource id.
pub struct InboxListener {
  receiver: RefCell<mpsc::Receiver<InboxStream>>,
  local_address: SocketAddr,
}

impl InboxListener {
  pub(crate) async fn accept(&self) -> Result<NetworkStream, JsErrorBox> {
    // The borrow lasts one poll, never across an await.
    let stream = poll_fn(|cx| self.receiver.borrow_mut().poll_recv(cx)).await;
    let stream = stream.ok_or_else(|| {
      JsErrorBox::new("Interrupted", "the connection inbox was closed")
    })?;
    stream.register().map_err(JsErrorBox::from_err)
  }

  pub(crate) fn stream_type(&self) -> NetworkStreamType {
    NetworkStreamType::Tcp
  }

  pub(crate) fn local_address(&self) -> NetworkStreamAddress {
    NetworkStreamAddress::Ip(self.local_address)
  }

  pub(crate) fn take(
    resource_table: &mut ResourceTable,
    rid: ResourceId,
  ) -> Result<Self, JsErrorBox> {
    let resource = resource_table
      .take::<InboxResource>(rid)
      .map_err(JsErrorBox::from_err)?;
    let resource = Rc::try_unwrap(resource)
      .map_err(|_| JsErrorBox::new("Busy", "the connection inbox is in use"))?;
    Ok(resource.0)
  }
}

struct InboxResource(InboxListener);

impl Resource for InboxResource {
  fn name(&self) -> Cow<'_, str> {
    "connectionInbox".into()
  }
}

/// An inbox holding up to `capacity` waiting connections, for a server whose
/// public address is `local_address` (what request URLs fall back to when a
/// request has no Host). The listener goes into `resource_table`; pass the id
/// it returns to `serveHttpOnListener` in the isolate.
pub fn connection_inbox(
  resource_table: &mut ResourceTable,
  local_address: SocketAddr,
  capacity: usize,
) -> (ConnectionSender, ResourceId) {
  let (sender, receiver) = mpsc::channel(capacity);
  let listener = InboxListener {
    receiver: RefCell::new(receiver),
    local_address,
  };
  let rid = resource_table.add(InboxResource(listener));
  (ConnectionSender(sender), rid)
}
