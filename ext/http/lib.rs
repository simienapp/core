// Copyright 2018-2026 the Deno authors. MIT license.

//! The HTTP server behind `Deno.serve`, forked from Deno's simien_http for the
//! Simien runtime. The embedder decides where connections come from through
//! [`HttpPropertyExtractor`]; the default one serves connections a Rust
//! embedder accepts and hands over through a [`connection_inbox`].

use std::borrow::Cow;
use std::future::Future;

use bytes::Bytes;
use simien_net::raw::NetworkStream;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::net::TcpStream;

use crate::network_buffered_stream::NetworkBufferedStream;
use hyper_util::rt::TokioIo;

use hyper::server::conn::http2;

mod fly_accept_encoding;
mod http_next;
mod inbox;
mod network_buffered_stream;
mod otel;
mod request_body;
mod request_properties;
mod response_body;
mod service;
mod v8_util;
pub mod compressible;

use fly_accept_encoding::Encoding;
pub use http_next::HttpNextError;
pub use inbox::ConnectionSender;
pub use inbox::InboxStream;
pub use inbox::connection_inbox;
pub use request_properties::DefaultHttpPropertyExtractor;
pub use request_properties::HttpConnectionProperties;
pub use request_properties::HttpListenProperties;
pub use request_properties::HttpPropertyExtractor;
pub use request_properties::HttpRequestProperties;
pub use service::UpgradeUnavailableError;

fn preferred_supported_encoding(
  encodings: impl Iterator<
    Item = Result<(Option<Encoding>, f32), fly_accept_encoding::EncodingError>,
  >,
) -> Encoding {
  let mut best_brotli_qval = 0.0;
  let mut best_gzip_qval = 0.0;
  let mut best_identity_qval = 0.0;

  for encoding in encodings {
    let Ok((encoding, qval)) = encoding else {
      continue;
    };
    match encoding {
      Some(Encoding::Brotli) if qval > best_brotli_qval => {
        best_brotli_qval = qval;
      }
      Some(Encoding::Gzip) if qval > best_gzip_qval => {
        best_gzip_qval = qval;
      }
      Some(Encoding::Identity) if qval > best_identity_qval => {
        best_identity_qval = qval;
      }
      _ => {}
    }
  }

  if best_brotli_qval >= best_gzip_qval
    && best_brotli_qval >= best_identity_qval
    && best_brotli_qval > 0.0
  {
    Encoding::Brotli
  } else if best_gzip_qval >= best_identity_qval && best_gzip_qval > 0.0 {
    Encoding::Gzip
  } else {
    Encoding::Identity
  }
}

fn cache_control_has_no_transform(value: &str) -> Option<bool> {
  let mut no_transform = false;
  for token in value.split(',') {
    let (key, val) = {
      let mut split = token.split('=').map(|s| s.trim());
      (split.next().unwrap(), split.next())
    };

    match key {
      "max-age" | "max-stale" | "min-fresh" => {
        val.and_then(|v| v.parse::<u64>().ok())?;
      }
      "no-transform" => no_transform = true,
      _ => (),
    }
  }
  Some(no_transform)
}

/// Per-request HTTP metrics. Never constructed in simien_http: it holds an
/// [`otel::OtelGlobals`], which has no values (see `otel.rs`).
struct OtelInfo {
  globals: &'static otel::OtelGlobals,
  attributes: OtelInfoAttributes,
  response_size: Option<u64>,
}

impl OtelInfo {
  fn new(
    otel: &otel::OtelGlobals,
    _instant: std::time::Instant,
    _request_size: u64,
    _attributes: OtelInfoAttributes,
  ) -> Self {
    match *otel {}
  }

  fn handle_duration_and_request_size(&mut self) {
    match *self.globals {}
  }
}

#[allow(
  dead_code,
  reason = "read only by the OpenTelemetry recorder, which simien_http leaves out"
)]
struct OtelInfoAttributes {
  http_request_method: Cow<'static, str>,
  network_protocol_version: &'static str,
  url_scheme: Cow<'static, str>,
  server_address: Option<String>,
  server_port: Option<i64>,
  error_type: Option<&'static str>,
  http_route: Option<String>,
  http_response_status_code: Option<i64>,
}

impl OtelInfoAttributes {
  fn method(method: &http::method::Method) -> Cow<'static, str> {
    use http::method::Method;

    match *method {
      Method::GET => Cow::Borrowed("GET"),
      Method::POST => Cow::Borrowed("POST"),
      Method::PUT => Cow::Borrowed("PUT"),
      Method::DELETE => Cow::Borrowed("DELETE"),
      Method::HEAD => Cow::Borrowed("HEAD"),
      Method::OPTIONS => Cow::Borrowed("OPTIONS"),
      Method::CONNECT => Cow::Borrowed("CONNECT"),
      Method::PATCH => Cow::Borrowed("PATCH"),
      Method::TRACE => Cow::Borrowed("TRACE"),
      _ => Cow::Owned(method.to_string()),
    }
  }

  fn version(version: http::Version) -> &'static str {
    use http::Version;

    match version {
      Version::HTTP_09 => "0.9",
      Version::HTTP_10 => "1.0",
      Version::HTTP_11 => "1.1",
      Version::HTTP_2 => "2",
      Version::HTTP_3 => "3",
      _ => unreachable!(),
    }
  }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Options {
  /// By passing a hook function, the caller can customize various configuration
  /// options for the HTTP/2 server.
  /// See [`http2::Builder`] for what parameters can be customized.
  ///
  /// If `None`, the default configuration provided by hyper will be used. Note
  /// that the default configuration is subject to change in future versions.
  pub http2_builder_hook:
    Option<fn(http2::Builder<LocalExecutor>) -> http2::Builder<LocalExecutor>>,

  /// If `false`, the server will abort the request when the response is dropped.
  pub no_legacy_abort: bool,

  /// If `true`, responses may be compressed based on request and response headers.
  pub automatic_compression: bool,
}

#[cfg(not(feature = "default_property_extractor"))]
simien_core::extension!(
  simien_http,
  deps = [simien_web, simien_fetch, simien_websocket],
  parameters = [ HTTP: HttpPropertyExtractor ],
  ops = [
    http_next::op_http_close_after_finish,
    http_next::op_http_get_request_header,
    http_next::op_http_get_request_headers,
    http_next::op_http_request_on_cancel,
    http_next::op_http_get_request_method<HTTP>,
    http_next::op_http_get_request_url<HTTP>,
    http_next::op_http_get_request_method_and_url<HTTP>,
    http_next::op_http_get_request_remote_addr<HTTP>,
    http_next::op_http_get_request_cancelled,
    http_next::op_http_is_raw_request,
    http_next::op_http_read_request_body,
    http_next::op_http_try_take_full_request_body,
    http_next::op_http_try_take_full_request_body_text,
    http_next::op_http_serve_on<HTTP>,
    http_next::op_http_serve<HTTP>,
    http_next::op_http_set_promise_complete,
    http_next::op_http_drop_response_native,
    http_next::op_http_new_response_native_headers,
    http_next::op_http_new_response_native_static,
    http_next::op_http_set_response_native,
    http_next::op_http_set_response_body_bytes,
    http_next::op_http_set_response_body_bytes_with_headers,
    http_next::op_http_set_response_body_resource,
    http_next::op_http_set_response_body_static_with_content_type,
    http_next::op_http_set_response_body_static_with_default_header,
    http_next::op_http_set_response_body_static_with_header,
    http_next::op_http_set_response_body_text,
    http_next::op_http_set_response_body_text_with_headers,
    http_next::op_http_set_response_header,
    http_next::op_http_set_response_headers,
    http_next::op_http_set_response_trailers,
    http_next::op_http_upgrade_websocket_next,
    http_next::op_http_ws_create_from_stream_resource,
    http_next::op_http_wait,
    http_next::op_http_close,
    http_next::op_http_cancel,
    http_next::op_http_metric_handle_otel_error,
  ],
  lazy_loaded_js = ["00_serve.js", "02_websocket.js"],
  options = {
    options: Options,
  },
  state = |state, options| {
    state.put::<Options>(options.options);
  }
);

#[cfg(feature = "default_property_extractor")]
simien_core::extension!(
  simien_http,
  deps = [simien_web, simien_fetch, simien_websocket],
  ops = [
    http_next::op_http_close_after_finish,
    http_next::op_http_get_request_header,
    http_next::op_http_get_request_headers,
    http_next::op_http_request_on_cancel,
    http_next::op_http_get_request_method<DefaultHttpPropertyExtractor>,
    http_next::op_http_get_request_url<DefaultHttpPropertyExtractor>,
    http_next::op_http_get_request_method_and_url<DefaultHttpPropertyExtractor>,
    http_next::op_http_get_request_remote_addr<DefaultHttpPropertyExtractor>,
    http_next::op_http_get_request_cancelled,
    http_next::op_http_is_raw_request,
    http_next::op_http_read_request_body,
    http_next::op_http_try_take_full_request_body,
    http_next::op_http_try_take_full_request_body_text,
    http_next::op_http_serve_on<DefaultHttpPropertyExtractor>,
    http_next::op_http_serve<DefaultHttpPropertyExtractor>,
    http_next::op_http_set_promise_complete,
    http_next::op_http_drop_response_native,
    http_next::op_http_new_response_native_headers,
    http_next::op_http_new_response_native_static,
    http_next::op_http_set_response_native,
    http_next::op_http_set_response_body_bytes,
    http_next::op_http_set_response_body_bytes_with_headers,
    http_next::op_http_set_response_body_resource,
    http_next::op_http_set_response_body_static_with_content_type,
    http_next::op_http_set_response_body_static_with_default_header,
    http_next::op_http_set_response_body_static_with_header,
    http_next::op_http_set_response_body_text,
    http_next::op_http_set_response_body_text_with_headers,
    http_next::op_http_set_response_header,
    http_next::op_http_set_response_headers,
    http_next::op_http_set_response_trailers,
    http_next::op_http_upgrade_websocket_next,
    http_next::op_http_ws_create_from_stream_resource,
    http_next::op_http_wait,
    http_next::op_http_close,
    http_next::op_http_cancel,
    http_next::op_http_metric_handle_otel_error,
  ],
  lazy_loaded_js = ["00_serve.js", "02_websocket.js"],
  options = {
    options: Options,
  },
  state = |state, options| {
    state.put::<Options>(options.options);
  }
);

// Needed so hyper can use non Send futures
#[derive(Clone)]
pub struct LocalExecutor;

impl<Fut> hyper::rt::Executor<Fut> for LocalExecutor
where
  Fut: Future + 'static,
  Fut::Output: 'static,
{
  fn execute(&self, fut: Fut) {
    simien_core::unsync::spawn(fut);
  }
}

trait CanDowncastUpgrade: Sized {
  fn downcast<T: AsyncRead + AsyncWrite + Unpin + 'static>(
    self,
  ) -> Result<(T, Bytes), Self>;
}

impl CanDowncastUpgrade for hyper::upgrade::Upgraded {
  fn downcast<T: AsyncRead + AsyncWrite + Unpin + 'static>(
    self,
  ) -> Result<(T, Bytes), Self> {
    let hyper::upgrade::Parts { io, read_buf, .. } =
      self.downcast::<TokioIo<T>>()?;
    Ok((io.into_inner(), read_buf))
  }
}

fn maybe_extract_network_stream<
  T: Into<NetworkStream> + AsyncRead + AsyncWrite + Unpin + 'static,
  U: CanDowncastUpgrade,
>(
  upgraded: U,
) -> Result<(NetworkStream, Bytes), U> {
  let upgraded = match upgraded.downcast::<T>() {
    Ok((stream, bytes)) => return Ok((stream.into(), bytes)),
    Err(x) => x,
  };

  match upgraded.downcast::<NetworkBufferedStream<T>>() {
    Ok((stream, upgraded_bytes)) => {
      // Both the upgrade and the stream might have unread bytes
      let (io, stream_bytes) = stream.into_inner();
      let bytes = match (stream_bytes.is_empty(), upgraded_bytes.is_empty()) {
        (false, false) => Bytes::default(),
        (true, false) => upgraded_bytes,
        (false, true) => stream_bytes,
        (true, true) => {
          // The upgraded bytes come first as they have already been read
          let mut v = upgraded_bytes.to_vec();
          v.append(&mut stream_bytes.to_vec());
          Bytes::from(v)
        }
      };
      Ok((io.into(), bytes))
    }
    Err(x) => Err(x),
  }
}

fn extract_network_stream<U: CanDowncastUpgrade>(
  upgraded: U,
) -> (NetworkStream, Bytes) {
  let upgraded =
    match maybe_extract_network_stream::<tokio::net::TcpStream, _>(upgraded) {
      Ok(res) => return res,
      Err(x) => x,
    };
  let upgraded = match maybe_extract_network_stream::<
    simien_net::ops_tls::TlsStream<TcpStream>,
    _,
  >(upgraded)
  {
    Ok(res) => return res,
    Err(x) => x,
  };
  #[cfg(unix)]
  let upgraded =
    match maybe_extract_network_stream::<tokio::net::UnixStream, _>(upgraded) {
      Ok(res) => return res,
      Err(x) => x,
    };
  let upgraded =
    match maybe_extract_network_stream::<NetworkStream, _>(upgraded) {
      Ok(res) => return res,
      Err(x) => x,
    };

  // TODO(mmastrac): HTTP/2 websockets may yield an un-downgradable type
  drop(upgraded);
  unreachable!("unexpected stream type");
}
