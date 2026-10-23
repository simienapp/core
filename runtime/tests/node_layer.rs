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
  run_with(permissions, program, main, code, BootstrapOptions::default())
}

fn run_with(
  permissions: PermissionsContainer,
  program: Option<simien_node_runtime::loader::NodeProgram>,
  main: &str,
  code: String,
  bootstrap: BootstrapOptions,
) -> Result<(), String> {
  let tokio = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
  tokio.block_on(async move {
    let specifier = ModuleSpecifier::parse(main).unwrap();
    let mut isolate = support::isolate(
      permissions,
      program.as_ref(),
      Rc::new(NodeOnlyLoader),
      specifier,
      bootstrap,
    );
    match isolate.run(Some(code)).await? {
      0 => Ok(()),
      code => Err(format!("exit code {code}")),
    }
  })
}

#[test]
fn node_modules_work_and_there_is_no_deno() {
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
    // node:fs maps op errors by class (`instanceof Deno.errors.NotFound`).
    let inaccessible;
    try {{ fs.accessSync({file:?} + ".missing"); }} catch (e) {{ inaccessible = e; }}
    check("op errors keep their class", inaccessible?.message.startsWith("ENOENT: no such file or directory, access"), true);
    await new Promise((resolve) => setImmediate(resolve));
    check("setTimeout returns a Timeout", typeof setTimeout(() => {{}}, 0).unref, "function");
    check("no Deno", typeof Deno, "undefined");
    check("no Deno property", "Deno" in globalThis, false);
    check("process.versions.simien", process.versions.simien, "0.0.0-test");
    check("process.versions.v8", /^\d+\.\d+/.test(process.versions.v8), true);
    check("no process.versions.deno", "deno" in process.versions, false);
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

/// A program that starts itself through a shell gets the embedder's
/// arguments (`BootstrapOptions::exec_args`), as `spawn(process.execPath)`
/// does, not Deno's `run -A`. `/bin/echo` as the shell shows the command it
/// was handed.
#[cfg(unix)]
#[test]
fn starting_itself_through_a_shell_uses_the_embedders_arguments() {
  let code = r#"
    import { execSync } from "node:child_process";
    import process from "node:process";
    const handed = String(
      execSync(`"${process.execPath}" child.js one`, { shell: "/bin/echo" }),
    ).trim();
    const want = `-c "${process.execPath}" __node-program --project /app child.js one`;
    if (handed !== want) throw new Error(`the shell got ${handed}, want ${want}`);
    "#;
  let bootstrap = BootstrapOptions {
    exec_args: ["__node-program", "--project", "/app"].map(String::from).to_vec(),
    ..Default::default()
  };
  run_with(PermissionsContainer::allow_all(), None, "file:///main.mjs", code.to_string(), bootstrap)
    .expect("the embedder's arguments");
}

/// A runtime started without a snapshot or an inspector (Simien's) still
/// makes `vm` contexts, and an inspector session is an error, not an abort.
#[test]
fn vm_contexts_work_and_a_missing_inspector_is_an_error() {
  let code = r#"
    import vm from "node:vm";
    import process from "node:process";
    import inspector from "node:inspector";
    function check(what, got, want) {
      if (got !== want) throw new Error(`${what}: got ${got}, want ${want}`);
    }
    const context = vm.createContext({ x: 2 });
    check("runs in the context", vm.runInContext("x * 21", context), 42);
    check("its own globals", vm.runInContext("typeof Array", context), "function");
    check("no inspector", process.features.inspector, false);
    let error;
    try { new inspector.Session().connect(); } catch (e) { error = e; }
    check("connect", error?.code, "ERR_INSPECTOR_NOT_AVAILABLE");
    "#;
  run(PermissionsContainer::allow_all(), code.to_string()).expect("vm and inspector");
}

/// Output piped to another program has no color codes: a parent reading a
/// child's `console.log(pid)` gets the number. The test runs itself again with
/// stdout piped, so it doesn't depend on where `cargo test` writes.
#[test]
fn console_output_into_a_pipe_has_no_colors() {
  const CHILD: &str = "SIMIEN_TEST_CONSOLE_CHILD";
  if std::env::var_os(CHILD).is_some() {
    let code = "console.log(12345); console.log({ n: 1 });".to_string();
    run(PermissionsContainer::allow_all(), code).expect("console.log");
    return;
  }
  let output = std::process::Command::new(std::env::current_exe().unwrap())
    .args(["--exact", "console_output_into_a_pipe_has_no_colors", "--nocapture", "--test-threads=1"])
    .env(CHILD, "1")
    .env_remove("NO_COLOR")
    .env_remove("FORCE_COLOR")
    .stdout(std::process::Stdio::piped())
    .output()
    .unwrap();
  let stdout = String::from_utf8_lossy(&output.stdout);
  assert!(output.status.success(), "{stdout}");
  assert!(stdout.contains("12345\n") && stdout.contains("{ n: 1 }"), "{stdout:?}");
  assert!(!stdout.contains('\x1b'), "color codes in a pipe: {stdout:?}");
}

/// `os.availableParallelism()` is the machine's, with no `navigator` global.
#[test]
fn available_parallelism_is_a_count() {
  let want = std::thread::available_parallelism().unwrap().get();
  let code = format!(
    r#"
    import os from "node:os";
    const got = os.availableParallelism();
    if (got !== {want}) throw new Error(`availableParallelism: got ${{got}}, want {want}`);
    if (+os.availableParallelism !== {want}) throw new Error("as a primitive");
    "#
  );
  run(PermissionsContainer::allow_all(), code).expect("availableParallelism");
}

/// A program started as a `cluster` worker (the primary's NODE_UNIQUE_ID,
/// handed over in `BootstrapOptions`) is one; any other program is the
/// primary, as upstream's bootstrap makes it.
#[test]
fn a_cluster_worker_knows_it_is_one() {
  let worker = r#"
    import cluster from "node:cluster";
    if (!cluster.isWorker || cluster.isPrimary) throw new Error(`isWorker ${cluster.isWorker}`);
    if (cluster.schedulingPolicy !== cluster.SCHED_NONE) throw new Error(`policy ${cluster.schedulingPolicy}`);
    "#;
  let bootstrap = BootstrapOptions {
    cluster_unique_id: Some("1".to_string()),
    cluster_sched_policy: Some("none".to_string()),
    ..Default::default()
  };
  run_with(PermissionsContainer::allow_all(), None, "file:///main.mjs", worker.to_string(), bootstrap)
    .expect("a worker");
  let primary = r#"
    import cluster from "node:cluster";
    if (!cluster.isPrimary || cluster.isWorker) throw new Error(`isPrimary ${cluster.isPrimary}`);
    "#;
  run(PermissionsContainer::allow_all(), primary.to_string()).expect("the primary");
}

/// `path.win32.resolve` of a relative path starts from the working folder,
/// as everywhere else: it read the cwd from a global `Deno`, which Simien
/// doesn't have, and threw.
#[test]
fn win32_resolve_uses_the_working_folder() {
  let code = r#"
    import path from "node:path";
    import process from "node:process";
    const got = path.win32.resolve("a", "b");
    const want = path.win32.join(process.cwd(), "a", "b");
    if (got !== want) throw new Error(`win32.resolve: got ${got}, want ${want}`);
    "#;
  run(PermissionsContainer::allow_all(), code.to_string()).expect("win32.resolve");
}

/// node:test's tests run once the program's code has, in order, subtests
/// included, as in Node; a failing one makes the exit code 1. (They used to
/// register with a `Deno.test` that did nothing, so nothing ran.)
#[test]
fn node_test_runs_the_tests_and_a_failure_fails_the_program() {
  let dir = tempfile::tempdir().unwrap();
  let log = dir.path().join("ran.txt");
  let code = format!(
    r#"
    import test from "node:test";
    import fs from "node:fs";
    const ran = [];
    test("a", async (t) => {{
      ran.push("a");
      await t.test("b", () => {{ ran.push("b"); }});
    }});
    test.skip("skipped", () => {{ ran.push("skipped"); }});
    test("c", () => {{ ran.push("c"); fs.writeFileSync({log:?}, ran.join(",")); }});
    "#,
    log = log.display().to_string(),
  );
  run(PermissionsContainer::allow_all(), code).expect("passing tests");
  assert_eq!(std::fs::read_to_string(&log).expect("the tests ran"), "a,b,c");

  let failing = r#"
    import test from "node:test";
    test("fails", () => { throw new Error("no"); });
    "#;
  assert_eq!(run(PermissionsContainer::allow_all(), failing.to_string()), Err("exit code 1".to_string()));
}
