// Copyright 2018-2026 the Simien authors. MIT license.

//! Connections accepted on another thread reach the isolate through a
//! connection inbox and are served by the Deno.serve machinery: plain
//! requests, a keep-alive connection carrying several, a request body, and
//! several connections at once.

use std::io::Read;
use std::io::Write;
use std::net::Shutdown;
use std::net::TcpListener;
use std::net::TcpStream;
use std::thread;

use simien_core::JsRuntime;
use simien_core::RuntimeOptions;
use simien_http::InboxStream;

// The globals an embedder installs (the Simien runtime: web_host), which
// upstream's server code also uses.
const APP: &str = r#"
globalThis.Response = Deno.core.loadExtScript("ext:simien_fetch/23_response.js").Response;
globalThis.URL = Deno.core.loadExtScript("ext:simien_web/00_url.js").URL;
const { serveHttpOnListener } = Deno.core.loadExtScript("ext:simien_http/00_serve.js");
const listener = { [Deno.core.internalRidSymbol]: RID };
serveHttpOnListener(
  listener,
  undefined,
  async (request) => {
    const url = new URL(request.url);
    if (request.method === "POST") {
      return new Response(`got ${await request.text()}`);
    }
    return new Response(`hello ${url.pathname}`, { headers: { "x-simien": "1" } });
  },
  (error) => new Response(String(error), { status: 500 }),
  () => {},
);
"#;

/// Sends raw HTTP/1.1 and reads until the server closes.
fn exchange(addr: std::net::SocketAddr, request: &str) -> String {
  let mut stream = TcpStream::connect(addr).expect("connect");
  stream.write_all(request.as_bytes()).expect("write");
  stream.shutdown(Shutdown::Write).expect("half close");
  let mut response = String::new();
  stream.read_to_string(&mut response).expect("read");
  response
}

fn body_count(response: &str, needle: &str) -> usize {
  response.matches(needle).count()
}

#[tokio::test(flavor = "current_thread")]
async fn connections_from_another_thread_are_served() {
  let accept = TcpListener::bind("127.0.0.1:0").expect("bind");
  let addr = accept.local_addr().expect("addr");

  let mut runtime = JsRuntime::new(RuntimeOptions {
    extensions: vec![
      simien_webidl::simien_webidl::init(),
      simien_web::simien_web::init(simien_web::BlobStore::default_arc(), None),
      simien_fetch::simien_fetch::init(Default::default()),
      simien_websocket::simien_websocket::init(),
      simien_http::simien_http::init(Default::default()),
    ],
    ..Default::default()
  });
  let (sender, rid) = simien_http::connection_inbox(
    &mut runtime.op_state().borrow_mut().resource_table,
    addr,
    64,
  );
  runtime
    .execute_script("app.js", APP.replace("RID", &rid.to_string()))
    .expect("start the server");

  // The embedder: accepts on its own thread, hands each connection over.
  thread::spawn(move || {
    for stream in accept.incoming() {
      let stream = stream.expect("accept");
      if sender.try_send(InboxStream::Tcp(stream)).is_err() {
        return;
      }
    }
  });

  let client = tokio::task::spawn_blocking(move || {
    let one = exchange(
      addr,
      "GET /one HTTP/1.1\r\nHost: simien.test\r\nConnection: close\r\n\r\n",
    );
    let keep_alive = exchange(
      addr,
      "GET /a HTTP/1.1\r\nHost: simien.test\r\n\r\n\
       GET /b HTTP/1.1\r\nHost: simien.test\r\nConnection: close\r\n\r\n",
    );
    let post = exchange(
      addr,
      "POST /echo HTTP/1.1\r\nHost: simien.test\r\nContent-Length: 6\r\n\
       Connection: close\r\n\r\nsimien",
    );
    let parallel: Vec<String> = (0..8)
      .map(|i| {
        thread::spawn(move || {
          exchange(
            addr,
            &format!(
              "GET /p{i} HTTP/1.1\r\nHost: simien.test\r\nConnection: close\r\n\r\n"
            ),
          )
        })
      })
      .collect::<Vec<_>>()
      .into_iter()
      .map(|handle| handle.join().expect("client thread"))
      .collect();
    (one, keep_alive, post, parallel)
  });

  let (one, keep_alive, post, parallel) = tokio::select! {
    result = client => result.expect("client"),
    result = runtime.run_event_loop(Default::default()) => {
      panic!("event loop ended before the clients: {result:?}")
    }
  };

  assert!(one.starts_with("HTTP/1.1 200 OK\r\n"), "{one}");
  assert!(one.contains("x-simien: 1\r\n"), "{one}");
  assert!(one.ends_with("hello /one"), "{one}");

  assert_eq!(body_count(&keep_alive, "HTTP/1.1 200 OK"), 2, "{keep_alive}");
  assert!(keep_alive.contains("hello /a"), "{keep_alive}");
  assert!(keep_alive.ends_with("hello /b"), "{keep_alive}");

  assert!(post.ends_with("got simien"), "{post}");

  for (i, response) in parallel.iter().enumerate() {
    assert!(response.ends_with(&format!("hello /p{i}")), "{response}");
  }
}
