// Copyright 2018-2026 the Deno authors. MIT license.

//! The Deno namespace simien_node reads, for isolates granted
//! `dangerouslyUseNode` (Simien APS 2 and APS 3, option A).
//!
//! An isolate gets these extensions after the web APIs and the Node layer:
//! - [`ops::bootstrap::simien_bootstrap`] with [`BootstrapOptions`], and
//!   [`ops::runtime::simien_runtime`] with the main module;
//! - [`ops::tty::simien_tty`] and [`ops::fs_events::simien_fs_events`];
//! - [`runtime`], the JavaScript: it builds the namespace and leaves
//!   `globalThis.__simienBootstrapNode(argv0)` for the embedder to call once,
//!   which installs `Deno` as a hidden global (not enumerable, not writable)
//!   and stashes Node's bootstrap so it starts on first `node:*` use.
//!
//! The runtime must pass [`transpile::transpile_extension`] as
//! `RuntimeOptions::extension_transpiler`: simien_node's files are TypeScript.
//!
//! Everything here asks Simien's policy through simien_permissions; nothing
//! is granted by being here.

pub mod ops;
pub mod transpile;

use simien_terminal::colors::ColorLevel;

/// What the bootstrap ops report.
#[derive(Clone, Debug)]
pub struct BootstrapOptions {
  /// `Deno.args` and Node's `process.argv` after the script.
  pub args: Vec<String>,
  /// `0` off to `5` trace, as upstream's `WorkerLogLevel`.
  pub log_level: i32,
  pub color_level: ColorLevel,
}

impl Default for BootstrapOptions {
  fn default() -> Self {
    Self {
      args: Vec::new(),
      log_level: 1,
      color_level: simien_terminal::colors::get_color_level(),
    }
  }
}

simien_core::extension!(
  runtime,
  deps = [
    simien_webidl,
    simien_web,
    simien_fetch,
    simien_net,
    simien_io,
    simien_fs,
    simien_os,
    simien_process,
    simien_node
  ],
  esm_entry_point = "ext:runtime/99_simien_node.js",
  esm = [
    dir "js",
    "90_deno_ns.js",
    "99_simien_node.js"
  ],
  // Imported by node:console (console_esm.ts) when it first loads.
  lazy_loaded_esm = [
    dir "js",
    "98_global_scope_shared.js"
  ],
  lazy_loaded_js = [
    dir "js",
    "01_errors.js",
    "01_version.ts",
    "06_util.js",
    "40_fs_events.js",
    "40_tty.js"
  ],
);
