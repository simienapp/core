// Copyright 2018-2026 the Simien authors. MIT license.

//! The Node layer in a real isolate: every extension an embedder registers
//! for code granted dangerouslyUseNode, the oxc transpiler for simien_node's
//! TypeScript, and the bootstrap. Node's modules work, `Deno` is there but
//! hidden, and every file access still asks the embedder's policy.

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use simien_core::JsRuntime;
use simien_core::ModuleLoadResponse;
use simien_core::ModuleLoader;
use simien_core::ModuleSpecifier;
use simien_core::ResolutionKind;
use simien_core::RuntimeOptions;
use simien_error::JsErrorBox;
use simien_node_runtime::BootstrapOptions;
use simien_permissions::PermissionDeniedError;
use simien_permissions::PermissionsContainer;
use simien_permissions::Policy;
use simien_permissions::RunQueryDescriptor;

/// Resolves only the main module: `node:*` imports fall back to simien_node's
/// registered modules, which is what an embedder's loader leaves them to.
struct NodeOnlyLoader;

impl ModuleLoader for NodeOnlyLoader {
  fn resolve(
    &self,
    specifier: &str,
    _referrer: &str,
    _kind: ResolutionKind,
  ) -> Result<ModuleSpecifier, simien_core::error::ModuleLoaderError> {
    if specifier == "file:///main.mjs" {
      return Ok(ModuleSpecifier::parse(specifier).unwrap());
    }
    Err(JsErrorBox::generic(format!("no module {specifier} here")))
  }

  fn load(
    &self,
    specifier: &ModuleSpecifier,
    _referrer: Option<&simien_core::ModuleLoadReferrer>,
    _options: simien_core::ModuleLoadOptions,
  ) -> ModuleLoadResponse {
    ModuleLoadResponse::Sync(Err(JsErrorBox::generic(format!(
      "no module {specifier} here"
    ))))
  }
}

/// Denies every file read and write; allows the rest.
struct NoFiles;

impl Policy for NoFiles {
  fn check_read(&self, path: Option<&Path>, _: Option<&str>) -> Result<(), PermissionDeniedError> {
    Err(PermissionDeniedError::new("read", path.map_or("everything".into(), |p| p.display().to_string())))
  }
  fn check_write(&self, path: Option<&Path>, _: Option<&str>) -> Result<(), PermissionDeniedError> {
    Err(PermissionDeniedError::new("write", path.map_or("everything".into(), |p| p.display().to_string())))
  }
  fn check_net(&self, _: &str, _: Option<u16>, _: Option<&str>) -> Result<(), PermissionDeniedError> {
    Ok(())
  }
  fn check_net_resolved(&self, _: &std::net::IpAddr, _: u16) -> Result<(), PermissionDeniedError> {
    Ok(())
  }
  fn check_net_unix_socket(&self, _: &Path, _: Option<&str>) -> Result<(), PermissionDeniedError> {
    Ok(())
  }
  fn check_net_vsock(&self, _: u32, _: u32, _: Option<&str>) -> Result<(), PermissionDeniedError> {
    Ok(())
  }
  fn check_env(&self, _: Option<&str>) -> Result<(), PermissionDeniedError> {
    Ok(())
  }
  fn check_run(&self, _: Option<&RunQueryDescriptor>, _: Option<&str>) -> Result<(), PermissionDeniedError> {
    Ok(())
  }
  fn check_sys(&self, _: &str, _: Option<&str>) -> Result<(), PermissionDeniedError> {
    Ok(())
  }
  fn check_ffi(&self, _: &Path) -> Result<(), PermissionDeniedError> {
    Ok(())
  }
}

simien_core::extension!(
  test_permissions,
  options = { permissions: PermissionsContainer },
  state = |state, options| {
    state.put(options.permissions);
    state.put(simien_fetch::PermissionsContainer::allow_all());
  },
);

// The web globals an embedder installs (Simien's web_host does the same).
simien_core::extension!(
  test_globals,
  deps = [simien_webidl, simien_web, simien_fetch, simien_crypto],
  esm_entry_point = "ext:test_globals/globals.js",
  esm = [dir "tests", "globals.js"],
);

type Sys = simien_sys_traits::impls::RealSys;

fn runtime(permissions: PermissionsContainer) -> JsRuntime {
  let fs: simien_fs::FileSystemRc = simien_maybe_sync::new_rc(simien_fs::RealFs);
  let main_module = ModuleSpecifier::parse("file:///main.mjs").unwrap();
  let mut runtime = JsRuntime::new(RuntimeOptions {
    module_loader: Some(Rc::new(NodeOnlyLoader)),
    extension_transpiler: Some(Rc::new(simien_node_runtime::transpile::transpile_extension)),
    extensions: vec![
      test_permissions::init(permissions),
      simien_webidl::simien_webidl::init(),
      simien_web::simien_web::init(simien_web::BlobStore::default_arc(), None),
      simien_crypto::simien_crypto::init(None),
      simien_fetch::simien_fetch::init(Default::default()),
      test_globals::init(),
      simien_net::simien_net::init(None, None),
      simien_io::simien_io::init(Some(Default::default())),
      simien_fs::simien_fs::init(fs.clone()),
      simien_os::simien_os::init(None),
      simien_process::simien_process::init(None),
      simien_node::simien_node::init::<
        simien_resolver::npm::DenoInNpmPackageChecker,
        simien_resolver::npm::NpmResolver<Sys>,
        Sys,
      >(None, fs),
      simien_node_runtime::ops::bootstrap::simien_bootstrap::init(BootstrapOptions::default()),
      simien_node_runtime::ops::runtime::simien_runtime::init(main_module),
      simien_node_runtime::ops::tty::simien_tty::init(),
      simien_node_runtime::ops::fs_events::simien_fs_events::init(),
      simien_node_runtime::ops::node_gaps::simien_node_gaps::init(),
      simien_node_runtime::runtime::init(),
    ],
    ..Default::default()
  });
  runtime
    .execute_script("bootstrap", "globalThis.__simienBootstrapNode('simien');")
    .expect("the Node bootstrap runs");
  runtime
}

/// Runs `code` as the main module; a thrown error fails the run.
fn run(permissions: PermissionsContainer, code: String) -> Result<(), String> {
  let tokio = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
  tokio.block_on(async move {
    let mut runtime = runtime(permissions);
    let specifier = ModuleSpecifier::parse("file:///main.mjs").unwrap();
    let id = runtime
      .load_main_es_module_from_code(&specifier, code)
      .await
      .map_err(|e| e.to_string())?;
    let evaluated = runtime.mod_evaluate(id);
    runtime.run_event_loop(Default::default()).await.map_err(|e| e.to_string())?;
    evaluated.await.map_err(|e| e.to_string())
  })
}

#[test]
fn node_modules_work_and_deno_is_hidden() {
  let dir = tempfile::tempdir().unwrap();
  let file = dir.path().join("hello.txt");
  let code = format!(
    r#"
    import path from "node:path";
    import {{ Buffer }} from "node:buffer";
    import process from "node:process";
    import fs from "node:fs";
    function check(what, got, want) {{
      if (got !== want) throw new Error(`${{what}}: got ${{got}}, want ${{want}}`);
    }}
    check("path.join", path.join("a", "b", "..", "c"), "a/c");
    check("Buffer", Buffer.from("hi").toString("base64"), "aGk=");
    check("process.platform", typeof process.platform, "string");
    check("process.argv0", process.argv0, "simien");
    fs.writeFileSync({file:?}, "from node:fs");
    check("fs round trip", fs.readFileSync({file:?}, "utf8"), "from node:fs");
    check("Deno is usable", typeof Deno.readTextFile, "function");
    check("Deno is hidden", Object.keys(globalThis).includes("Deno"), false);
    check("Deno is fixed", Object.getOwnPropertyDescriptor(globalThis, "Deno").writable, false);
    "#,
    file = file.display().to_string(),
  );
  run(PermissionsContainer::allow_all(), code).expect("Node's modules run");
  assert_eq!(std::fs::read_to_string(&file).unwrap(), "from node:fs");
}

#[test]
fn the_policy_still_decides_every_file_access() {
  let dir = tempfile::tempdir().unwrap();
  let file = dir.path().join("secret.txt");
  std::fs::write(&file, "secret").unwrap();
  let code = format!(
    r#"
    import fs from "node:fs";
    let error;
    try {{ fs.readFileSync({file:?}, "utf8"); }} catch (e) {{ error = e; }}
    if (!error) throw new Error("read a file the policy denies");
    if (!String(error).includes("Requires read access")) throw error;
    let wrote = true;
    try {{ fs.writeFileSync({file:?}, "changed"); }} catch {{ wrote = false; }}
    if (wrote) throw new Error("wrote a file the policy denies");
    "#,
    file = file.display().to_string(),
  );
  run(PermissionsContainer::new(Arc::new(NoFiles)), code).expect("denials are errors in JavaScript");
  assert_eq!(std::fs::read_to_string(&file).unwrap(), "secret");
}
