// Copyright 2018-2026 the Deno authors. MIT license.

//! Every lazily loaded module of simien_webidl, simien_web and simien_fetch
//! evaluates in a real isolate. Nothing else here runs this JavaScript, so
//! without this an import of an extension that isn't part of simienapp/core
//! (deno_net, deno_telemetry) only fails in an embedder, as it did in 0.1.0.

use simien_core::JsRuntime;
use simien_core::RuntimeOptions;

const MODULES: &[&str] = &[
  "ext:simien_webidl/00_webidl.js",
  "ext:simien_web/00_infra.js",
  "ext:simien_web/00_url.js",
  "ext:simien_web/01_console.js",
  "ext:simien_web/01_dom_exception.js",
  "ext:simien_web/01_mimesniff.js",
  "ext:simien_web/01_urlpattern.js",
  "ext:simien_web/02_event.js",
  "ext:simien_web/02_structured_clone.js",
  "ext:simien_web/02_timers.js",
  "ext:simien_web/03_abort_signal.js",
  "ext:simien_web/04_global_interfaces.js",
  "ext:simien_web/05_base64.js",
  "ext:simien_web/06_streams.js",
  "ext:simien_web/08_text_encoding.js",
  "ext:simien_web/09_file.js",
  "ext:simien_web/10_filereader.js",
  "ext:simien_web/12_location.js",
  "ext:simien_web/13_message_port.js",
  "ext:simien_web/14_compression.js",
  "ext:simien_web/15_performance.js",
  "ext:simien_fetch/20_headers.js",
  "ext:simien_fetch/21_formdata.js",
  "ext:simien_fetch/22_body.js",
  "ext:simien_fetch/22_http_client.js",
  "ext:simien_fetch/23_request.js",
  "ext:simien_fetch/23_response.js",
  "ext:simien_fetch/26_fetch.js",
  "ext:simien_fetch/27_eventsource.js",
];

#[tokio::test]
async fn every_lazy_module_loads() {
  let mut runtime = JsRuntime::new(RuntimeOptions {
    extensions: vec![
      simien_webidl::simien_webidl::init(),
      simien_web::simien_web::init(simien_web::BlobStore::default_arc(), None),
      simien_fetch::simien_fetch::init(Default::default()),
    ],
    ..Default::default()
  });
  for module in MODULES {
    if let Err(err) = runtime.execute_script(
      "lazy_modules.js",
      format!("Deno.core.loadExtScript({module:?});"),
    ) {
      panic!("{module} did not load: {err}");
    }
  }
}
