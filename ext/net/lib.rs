// Copyright 2018-2026 the Deno authors. MIT license.

//! The stream layer of Deno's deno_net, forked for Simien: the TCP, TLS and
//! Unix stream types an HTTP server or a WebSocket runs on, and the resources
//! that hold them. The `Deno.listen`/`Deno.connect` APIs, DNS, UDP, QUIC and
//! tunnels are not part of it.

pub mod io;
#[cfg(unix)]
pub mod ops_unix;
pub mod ops_tls;
pub mod raw;
pub mod tcp;

#[cfg(unix)]
pub(crate) fn is_unix_socket_abstract_path(path: &std::path::Path) -> bool {
  #[cfg(any(target_os = "android", target_os = "linux"))]
  {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().first() == Some(&0)
  }

  #[cfg(not(any(target_os = "android", target_os = "linux")))]
  {
    let _ = path;
    false
  }
}
