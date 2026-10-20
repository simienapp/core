// Copyright 2018-2026 the Deno authors. MIT license.

use simien_bench_util::bench_js_async;
use simien_bench_util::bench_or_profile;
use simien_bench_util::bencher::Bencher;
use simien_bench_util::bencher::benchmark_group;
use simien_core::Extension;

#[derive(Clone)]
struct Permissions;

fn setup() -> Vec<Extension> {
  simien_core::extension!(
    bench_setup,
    esm_entry_point = "ext:bench_setup/setup",
    esm = ["ext:bench_setup/setup" = {
      source = r#"
        import { setTimeout } from "ext:simien_web/02_timers.js";
        globalThis.setTimeout = setTimeout;
      "#
    }],
    state = |state| {
      state.put(Permissions {});
    },
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

fn bench_set_timeout(b: &mut Bencher) {
  bench_js_async(b, r#"setTimeout(() => {}, 0);"#, setup);
}

benchmark_group!(benches, bench_set_timeout,);
bench_or_profile!(benches);
