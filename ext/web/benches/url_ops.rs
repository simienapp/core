// Copyright 2018-2026 the Deno authors. MIT license.

use simien_bench_util::bench_js_sync;
use simien_bench_util::bench_or_profile;
use simien_bench_util::bencher::Bencher;
use simien_bench_util::bencher::benchmark_group;
use simien_core::Extension;

fn setup() -> Vec<Extension> {
  simien_core::extension!(
    bench_setup,
    esm_entry_point = "ext:bench_setup/setup",
    esm = ["ext:bench_setup/setup" = {
      source = r#"
        import { URL } from "ext:simien_web/00_url.js";
        globalThis.URL = URL;
      "#
    }]
  );

  vec![
    simien_webidl::simien_webidl::init(),
    simien_web::simien_web::init(
      std::sync::Arc::new(simien_web::BlobStore::default())
        as std::sync::Arc<dyn simien_web::BlobStoreTrait>,
      None,
    ),
    bench_setup::init(),
  ]
}

fn bench_url_parse(b: &mut Bencher) {
  bench_js_sync(b, r#"new URL(`http://www.google.com/`);"#, setup);
}

benchmark_group!(benches, bench_url_parse,);
bench_or_profile!(benches);
