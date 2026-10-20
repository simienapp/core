# simienapp/core

The parts of [Deno](https://github.com/denoland/deno) that the Simien runtime builds on: the engine glue (`deno_core`, `deno_ops`, `serde_v8`, the `deno_v8` facade) and the web APIs that make up WinterTC's Minimum Common API (`deno_webidl`, `deno_web`, `deno_fetch`, `deno_crypto`, `deno_tls`).

Deno's runtime is winding down after Cloudflare's acquisition (October 2026). Simien keeps these crates maintained, publishes them as `simien_*`, and carries upstream fixes across while upstream still ships them.

## Where it came from

- Forked from `denoland/deno` **v2.9.7** (`0c071246a4`, 2026-09-16), when `deno_core` was 0.412.0. The tag `upstream-v2.9.7` marks it.
- History is upstream's, filtered to these paths with `git filter-repo`, so upstream commits can still be cherry-picked. The `upstream` branch is that filtered history and nothing else.
- Before 2025, `deno_core` lived in `denoland/deno_core`. That earlier history is not included here.

| path | upstream crate | becomes |
|---|---|---|
| `libs/core` | `deno_core` | `simien_core` |
| `libs/ops` | `deno_ops` | `simien_ops` |
| `libs/serde_v8` | `serde_v8` | `simien_serde_v8` |
| `libs/deno_v8` | `deno_v8` (the V8/QuickJS facade) | `simien_v8` |
| `libs/crypto` | `deno_crypto_provider` | `simien_crypto_provider` |
| `libs/deno_error` | `deno_error`, `deno_error_macro` | `simien_error`, `simien_error_macro` |
| `libs/deno_unsync` | `deno_unsync` | `simien_unsync` |
| `libs/deno_path_util` | `deno_path_util` | `simien_path_util` |
| `libs/boxed_error` | `boxed_error` | `simien_boxed_error` |
| `libs/wasm_dep_analyzer` | `wasm_dep_analyzer` | `simien_wasm_dep_analyzer` |
| `libs/deno_native_certs` | `deno_native_certs` | `simien_native_certs` |
| `tests/bench_util` | `deno_bench_util` | `simien_bench_util` (benchmarks only) |
| `ext/webidl` | `deno_webidl` | `simien_webidl` |
| `ext/web` | `deno_web` | `simien_web` |
| `ext/fetch` | `deno_fetch` | `simien_fetch` |
| `ext/crypto` | `deno_crypto` | `simien_crypto` |
| `ext/tls` | `deno_tls` | `simien_tls` |
| `ext/net` | `deno_net` (its stream layer only) | `simien_net` |
| `ext/websocket` | `deno_websocket` | `simien_websocket` |
| `ext/http` | `deno_http` (the `Deno.serve` server) | `simien_http` |
| `libs/http_h1` | `deno_http_h1` | `simien_http_h1` |

Every crate here is published under its `simien_*` name, and the Rust paths, extension names and `ext:` module specifiers are renamed to match. The rename is mechanical: `tools/rename.py` applies `tools/rename-map.txt`, and the same script translates an upstream Deno patch before it's applied, so fixes still port across. `simien_http`'s JavaScript is generated from upstream's TypeScript by `tools/strip-types.mjs`; edit the `.ts`, and CI checks the `.js` matches. The plan and its progress are in [#1](https://github.com/simienapp/core/issues/1).

## Build

```sh
cargo check --workspace
```

## License

MIT, as upstream: Copyright 2018-2026 the Deno authors. See `LICENSE.md`. Changes made here are also MIT.
