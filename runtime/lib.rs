// Copyright 2018-2026 the Deno authors. MIT license.

//! The Deno namespace simien_node reads, for isolates granted
//! `dangerouslyUseNode` (Simien APS 2 and APS 3, option A).
//!
//! An isolate gets these extensions after the web APIs, simien_napi (whose
//! `op_napi_open` `node:module` imports; addons load only past `check_ffi`)
//! and the Node layer:
//! - [`ops::bootstrap::simien_bootstrap`] with [`BootstrapOptions`], and
//!   [`ops::runtime::simien_runtime`] with the main module;
//! - [`ops::tty::simien_tty`] and [`ops::fs_events::simien_fs_events`];
//! - [`runtime`], the JavaScript: it builds the namespace and leaves
//!   `globalThis.__simienBootstrapNode(argv0, versions)` for the embedder to
//!   call once, which fills simien_node's private Deno namespace, removes
//!   core's `Deno` from the global (programs never see one) and stashes
//!   Node's bootstrap so it starts on first `node:*` use.
//!
//! The runtime must pass [`transpile::transpile_extension`] as
//! `RuntimeOptions::extension_transpiler`: simien_node's files are TypeScript.
//!
//! Everything here asks Simien's policy through simien_permissions; nothing
//! is granted by being here.

pub use simien_node_resolver::cjs;
pub mod loader;
pub mod ops;
pub mod transpile;

use simien_core::Extension;
use simien_core::JsRuntime;
use simien_core::ModuleSpecifier;
use simien_core::error::CoreError;
use simien_core::error::JsError;
use simien_core::v8;
use simien_terminal::colors::ColorLevel;

use crate::loader::InNpmPackageChecker;
use crate::loader::NodeProgram;
use crate::loader::NpmFolderResolver;
use crate::loader::Sys;

/// What the bootstrap ops report.
#[derive(Clone, Debug)]
pub struct BootstrapOptions {
  /// `Deno.args` and Node's `process.argv` after the script.
  pub args: Vec<String>,
  /// How this program starts another one like it: when it spawns
  /// `process.execPath` (`child_process.fork()`, `spawn(process.execPath,
  /// [script])`), these arguments go before Node's own, so the embedder runs
  /// the child its way (Simien: `__node-program --project … <rules>`). Empty:
  /// upstream's translation to Deno's command line.
  pub exec_args: Vec<String>,
  /// A `cluster` worker's id and the primary's scheduling policy, as the
  /// primary passed them (`NODE_UNIQUE_ID`, `NODE_CLUSTER_SCHED_POLICY`).
  /// `None` for a program that isn't a worker.
  pub cluster_unique_id: Option<String>,
  pub cluster_sched_policy: Option<String>,
  /// `0` off to `5` trace, as upstream's `WorkerLogLevel`.
  pub log_level: i32,
  pub color_level: ColorLevel,
}

impl Default for BootstrapOptions {
  fn default() -> Self {
    Self {
      args: Vec::new(),
      exec_args: Vec::new(),
      cluster_unique_id: None,
      cluster_sched_policy: None,
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
    simien_napi,
    simien_node_crypto,
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

/// Every extension the Node layer adds after an embedder's web APIs
/// (simien_webidl, simien_web, simien_crypto, simien_fetch) and its
/// `PermissionsContainer` in op state, in the order they must load. Without
/// a `program`, `require()` of files and packages is unavailable.
pub fn extensions(
  program: Option<&NodeProgram>,
  main_module: ModuleSpecifier,
  bootstrap: BootstrapOptions,
  exit_code: simien_os::ExitCode,
) -> Vec<Extension> {
  let fs: simien_fs::FileSystemRc = simien_maybe_sync::new_rc(simien_fs::RealFs);
  vec![
    simien_net::simien_net::init(None, None),
    simien_io::simien_io::init(Some(Default::default())),
    simien_fs::simien_fs::init(fs.clone()),
    simien_os::simien_os::init(Some(exit_code)),
    simien_process::simien_process::init(None),
    simien_napi::simien_napi::init(None),
    simien_node_crypto::simien_node_crypto::init(),
    simien_node::simien_node::init::<InNpmPackageChecker, NpmFolderResolver, Sys>(
      program.map(NodeProgram::init_services),
      fs,
    ),
    ops::bootstrap::simien_bootstrap::init(bootstrap),
    ops::runtime::simien_runtime::init(main_module),
    ops::tty::simien_tty::init(),
    ops::fs_events::simien_fs_events::init(),
    ops::node_gaps::simien_node_gaps::init(),
    runtime::init(),
  ]
}

/// Fills simien_node's private Deno namespace, installs Node's globals and
/// the global events, and stashes Node's bootstrap, once, before any app code
/// runs. `argv0` is `process.argv0`; `simien_version` is the embedder's
/// version, which `process.versions.simien` reports. The [`Lifecycle`] runs
/// the program.
pub fn bootstrap(
  runtime: &mut JsRuntime,
  argv0: &str,
  simien_version: &str,
) -> Result<Lifecycle, Box<JsError>> {
  let argv0 = serde_json::to_string(argv0).expect("a string serializes");
  let versions = serde_json::json!({
    "simien": simien_version,
    "v8": v8::V8::get_version(),
  });
  let hooks = runtime.execute_script(
    "ext:runtime/bootstrap",
    format!("globalThis.__simienBootstrapNode({argv0}, {versions});"),
  )?;
  simien_core::scope!(scope, runtime);
  let hooks = v8::Local::new(scope, hooks);
  let hooks: v8::Local<v8::Object> =
    hooks.try_into().expect("the bootstrap returns its hooks");
  let hook = |name: &str| {
    let key = v8::String::new(scope, name).expect("a short string");
    let value = hooks.get(scope, key.into()).expect("the hook is there");
    let function: v8::Local<v8::Function> =
      value.try_into().expect("the hook is a function");
    v8::Global::new(scope, function)
  };
  Ok(Lifecycle {
    load: hook("load"),
    before_unload: hook("beforeUnload"),
    unload: hook("unload"),
    process_before_exit: hook("processBeforeExit"),
    process_exit: hook("processExit"),
  })
}

/// The events a Node program sees around its run, in Node's order.
pub struct Lifecycle {
  load: v8::Global<v8::Function>,
  before_unload: v8::Global<v8::Function>,
  unload: v8::Global<v8::Function>,
  process_before_exit: v8::Global<v8::Function>,
  process_exit: v8::Global<v8::Function>,
}

impl Lifecycle {
  /// Runs `main` (from `code` when given, else through the module loader)
  /// as Node runs a program: evaluate it, run the event loop dry, emit
  /// "beforeExit" (whose listeners may schedule more work, which runs too),
  /// then "exit". Returns the exit code (`process.exitCode`).
  pub async fn run(
    &self,
    runtime: &mut JsRuntime,
    exit_code: &simien_os::ExitCode,
    main: &ModuleSpecifier,
    code: Option<String>,
  ) -> Result<i32, CoreError> {
    let id = match code {
      Some(code) => runtime.load_main_es_module_from_code(main, code).await?,
      None => runtime.load_main_es_module(main).await?,
    };
    self.call(runtime, &self.load).await?;
    let evaluated = runtime.mod_evaluate(id);
    runtime.run_event_loop(Default::default()).await?;
    evaluated.await?;
    loop {
      let web_continue = self.call(runtime, &self.before_unload).await?;
      let more = if web_continue {
        self.call(runtime, &self.process_before_exit).await?
      } else {
        true
      };
      if !more {
        break;
      }
      runtime.run_event_loop(Default::default()).await?;
    }
    self.call(runtime, &self.unload).await?;
    self.call(runtime, &self.process_exit).await?;
    Ok(exit_code.get())
  }

  /// Calls a hook; true when it returned true.
  async fn call(
    &self,
    runtime: &mut JsRuntime,
    hook: &v8::Global<v8::Function>,
  ) -> Result<bool, CoreError> {
    let returned = Box::pin(runtime.call(hook));
    let value = runtime
      .with_event_loop_future(returned, Default::default())
      .await?;
    simien_core::scope!(scope, runtime);
    Ok(v8::Local::new(scope, value).is_true())
  }
}
