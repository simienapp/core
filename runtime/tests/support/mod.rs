// Copyright 2018-2026 the Simien authors. MIT license.

//! An isolate as an embedder builds one for code granted dangerouslyUseNode:
//! the web APIs and globals (as Simien's web_host installs them), the policy,
//! then [`simien_node_runtime::extensions`] and its bootstrap. Shared by the
//! tests and the `node` example.

use std::rc::Rc;

use simien_core::JsRuntime;
use simien_core::ModuleLoader;
use simien_core::ModuleSpecifier;
use simien_core::RuntimeOptions;
use simien_node_runtime::BootstrapOptions;
use simien_node_runtime::loader::NodeProgram;
use simien_permissions::PermissionsContainer;

simien_core::extension!(
  host_permissions,
  options = { permissions: PermissionsContainer },
  state = |state, options| {
    state.put(options.permissions);
    state.put(simien_fetch::PermissionsContainer::allow_all());
  },
);

// The web globals an embedder installs (Simien's web_host does the same).
simien_core::extension!(
  host_globals,
  deps = [simien_webidl, simien_web, simien_fetch, simien_crypto],
  esm_entry_point = "ext:host_globals/globals.js",
  esm = [dir "tests", "globals.js"],
);

/// A bootstrapped isolate, ready to run `main_module`.
pub struct Isolate {
  pub runtime: JsRuntime,
  lifecycle: simien_node_runtime::Lifecycle,
  exit_code: simien_os::ExitCode,
  main_module: ModuleSpecifier,
}

/// An isolate whose main module is `main_module`. Modules load through
/// `program` when there is one, else through `loader`.
pub fn isolate(
  permissions: PermissionsContainer,
  program: Option<&NodeProgram>,
  loader: Rc<dyn ModuleLoader>,
  main_module: ModuleSpecifier,
  args: Vec<String>,
) -> Isolate {
  let exit_code = simien_os::ExitCode::default();
  let mut extensions = vec![
    host_permissions::init(permissions),
    simien_webidl::simien_webidl::init(),
    simien_web::simien_web::init(simien_web::BlobStore::default_arc(), None),
    simien_crypto::simien_crypto::init(None),
    simien_fetch::simien_fetch::init(Default::default()),
    host_globals::init(),
  ];
  extensions.extend(simien_node_runtime::extensions(
    program,
    main_module.clone(),
    BootstrapOptions { args, ..Default::default() },
    exit_code.clone(),
  ));
  let mut runtime = JsRuntime::new(RuntimeOptions {
    module_loader: Some(program.map_or(loader, NodeProgram::module_loader)),
    extension_transpiler: Some(Rc::new(
      simien_node_runtime::transpile::transpile_extension,
    )),
    extensions,
    ..Default::default()
  });
  let lifecycle = simien_node_runtime::bootstrap(&mut runtime, "simien", "0.0.0-test")
    .expect("the Node bootstrap runs");
  Isolate { runtime, lifecycle, exit_code, main_module }
}

impl Isolate {
  /// Runs the main module (from `code` when given, else from the loader) as
  /// Node does; its exit code.
  pub async fn run(&mut self, code: Option<String>) -> Result<i32, String> {
    self
      .lifecycle
      .run(&mut self.runtime, &self.exit_code, &self.main_module, code)
      .await
      .map_err(|e| e.to_string())
  }
}
