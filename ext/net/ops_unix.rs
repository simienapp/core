// Copyright 2018-2026 the Deno authors. MIT license.

use std::path::PathBuf;

use tokio::net::UnixListener;
use tokio::net::UnixStream;

use crate::is_unix_socket_abstract_path;

/// A wrapper around `UnixListener` that holds the socket path and removes it on drop.
pub struct UnixListenerWithPath {
  listener: UnixListener,
  path: Option<PathBuf>,
}

impl UnixListenerWithPath {
  pub fn new(listener: UnixListener, path: PathBuf) -> Self {
    let path = if is_unix_socket_abstract_path(&path) {
      None
    } else {
      Some(path)
    };
    Self { listener, path }
  }

  pub async fn accept(
    &self,
  ) -> std::io::Result<(UnixStream, tokio::net::unix::SocketAddr)> {
    self.listener.accept().await
  }

  pub fn local_addr(&self) -> std::io::Result<tokio::net::unix::SocketAddr> {
    self.listener.local_addr()
  }
}

impl Drop for UnixListenerWithPath {
  fn drop(&mut self) {
    if let Some(path) = &self.path {
      #[allow(clippy::disallowed_methods, reason = "requires real fs")]
      let _ = std::fs::remove_file(path);
    }
  }
}
