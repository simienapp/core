// Copyright 2018-2026 the Simien authors. MIT license.

//! The Node layer in a real isolate: every extension an embedder registers
//! for code granted dangerouslyUseNode, the oxc transpiler for simien_node's
//! TypeScript, and the bootstrap. Node's modules work, `Deno` is there but
//! hidden, and every file access still asks the embedder's policy.

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use simien_core::ModuleLoadResponse;
use simien_core::ModuleLoader;
use simien_core::ModuleSpecifier;
use simien_core::ResolutionKind;
use simien_error::JsErrorBox;
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

mod support;

/// Runs `code` as the main module; a thrown error fails the run.
fn run(permissions: PermissionsContainer, code: String) -> Result<(), String> {
  run_program(permissions, None, "file:///main.mjs", code)
}

fn run_program(
  permissions: PermissionsContainer,
  program: Option<simien_node_runtime::loader::NodeProgram>,
  main: &str,
  code: String,
) -> Result<(), String> {
  let tokio = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
  tokio.block_on(async move {
    let specifier = ModuleSpecifier::parse(main).unwrap();
    let mut isolate = support::isolate(
      permissions,
      program.as_ref(),
      Rc::new(NodeOnlyLoader),
      specifier,
      Vec::new(),
    );
    match isolate.run(Some(code)).await? {
      0 => Ok(()),
      code => Err(format!("exit code {code}")),
    }
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
    import nodeConsole from "node:console";
    function check(what, got, want) {{
      if (got !== want) throw new Error(`${{what}}: got ${{got}}, want ${{want}}`);
    }}
    check("path.join", path.join("a", "b", "..", "c"), "a/c");
    check("Buffer", Buffer.from("hi").toString("base64"), "aGk=");
    check("process.platform", typeof process.platform, "string");
    check("process.argv0", process.argv0, "simien");
    fs.writeFileSync({file:?}, "from node:fs");
    check("fs round trip", fs.readFileSync({file:?}, "utf8"), "from node:fs");
    check("global", global, globalThis);
    check("process global", globalThis.process, process);
    check("Buffer global", globalThis.Buffer, Buffer);
    check("setImmediate", typeof setImmediate, "function");
    check("console is node:console's", console, nodeConsole);
    check("console.info", typeof console.info, "function");
    let missing;
    try {{ fs.statSync({file:?} + ".missing"); }} catch (e) {{ missing = e; }}
    check("a missing file is ENOENT", missing?.code, "ENOENT");
    let notFound;
    try {{ Deno.lstatSync({file:?} + ".missing"); }} catch (e) {{ notFound = e; }}
    check("op errors keep their class", notFound instanceof Deno.errors.NotFound, true);
    await new Promise((resolve) => setImmediate(resolve));
    check("setTimeout returns a Timeout", typeof setTimeout(() => {{}}, 0).unref, "function");
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

/// A node_modules tree like an installed project's.
fn project() -> tempfile::TempDir {
  let dir = tempfile::tempdir().unwrap();
  let write = |path: &str, text: &str| {
    let path = dir.path().join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
  };
  write("node_modules/cjs-pkg/package.json", r#"{ "name": "cjs-pkg", "main": "lib/index.js" }"#);
  write(
    "node_modules/cjs-pkg/lib/index.js",
    "const helper = require('./helper');\nmodule.exports = { greet: (n) => helper.prefix + n };\nexports.version = '1.0';\nmodule.exports.version = '1.0';\n",
  );
  write("node_modules/cjs-pkg/lib/helper.js", "exports.prefix = 'hello ';\n");
  write(
    "node_modules/esm-pkg/package.json",
    r#"{ "name": "esm-pkg", "type": "module", "exports": { ".": { "import": "./dist/index.js" } } }"#,
  );
  write("node_modules/esm-pkg/dist/index.js", "export const shout = (s) => s.toUpperCase();\n");
  write("data.json", r#"{ "answer": 42 }"#);
  dir
}

#[test]
fn a_program_loads_esm_and_commonjs_from_node_modules() {
  let dir = project();
  let main = dir.path().join("main.mjs");
  let code = r#"
    import { greet, version } from "cjs-pkg";
    import cjsDefault from "cjs-pkg";
    import { shout } from "esm-pkg";
    import { createRequire } from "node:module";
    import fs from "fs";
    function check(what, got, want) {
      if (got !== want) throw new Error(`${what}: got ${got}, want ${want}`);
    }
    check("named import from CommonJS", greet("simien"), "hello simien");
    check("export from CommonJS", version, "1.0");
    check("default import from CommonJS", cjsDefault.greet("x"), "hello x");
    check("ESM through exports", shout("node"), "NODE");
    const require = createRequire(import.meta.url);
    check("require() of a package", require("cjs-pkg").greet("y"), "hello y");
    check("require() of JSON", require("./data.json").answer, 42);
    check("bare builtin", typeof fs.readFileSync, "function");
  "#;
  std::fs::write(&main, code).unwrap();
  let program = simien_node_runtime::loader::NodeProgram::new(dir.path());
  let main_url = ModuleSpecifier::from_file_path(&main).unwrap().to_string();
  run_program(PermissionsContainer::allow_all(), Some(program), &main_url, code.to_string())
    .expect("the program runs");
}

#[test]
fn require_reads_only_what_the_policy_allows() {
  let dir = project();
  let main = dir.path().join("main.mjs");
  let code = r#"
    import { createRequire } from "node:module";
    const require = createRequire(import.meta.url);
    let error;
    try { require("./data.json"); } catch (e) { error = e; }
    if (!error) throw new Error("require() read a file the policy denies");
    if (!String(error).includes("Requires read access")) throw error;
  "#;
  std::fs::write(&main, code).unwrap();
  let program = simien_node_runtime::loader::NodeProgram::new(dir.path());
  let main_url = ModuleSpecifier::from_file_path(&main).unwrap().to_string();
  run_program(PermissionsContainer::new(Arc::new(NoFiles)), Some(program), &main_url, code.to_string())
    .expect("require()'s denial is an error in JavaScript");
}
