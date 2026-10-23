// Copyright 2018-2026 the Simien authors. MIT license.

//! Runs a Node program on Simien, for trying real tools against the Node
//! layer: `cargo run -p simien_node_runtime --example node -- <script> [args]`.
//! Packages resolve from the current directory's node_modules; the policy
//! allows everything, as for a program granted dangerouslyUseNode.

#[path = "../tests/support/mod.rs"]
mod support;

use std::rc::Rc;

use simien_core::ModuleSpecifier;
use simien_core::NoopModuleLoader;
use simien_node_runtime::BootstrapOptions;
use simien_node_runtime::loader::NodeProgram;
use simien_permissions::PermissionsContainer;

fn main() {
  let mut args = std::env::args().skip(1);
  let Some(script) = args.next() else {
    eprintln!("usage: node <script> [args]");
    std::process::exit(2);
  };
  let cwd = std::env::current_dir().expect("a current directory");
  let main_module = ModuleSpecifier::from_file_path(cwd.join(&script).canonicalize().unwrap_or_else(|e| {
    eprintln!("{script}: {e}");
    std::process::exit(2);
  }))
  .expect("an absolute path");
  let program = NodeProgram::new(&cwd);
  let tokio = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .expect("a tokio runtime");
  let result = tokio.block_on(async {
    let mut isolate = support::isolate(
      PermissionsContainer::allow_all(),
      Some(&program),
      Rc::new(NoopModuleLoader),
      main_module,
      BootstrapOptions { args: args.collect(), ..Default::default() },
    );
    isolate.run(None).await
  });
  match result {
    Ok(code) => std::process::exit(code),
    Err(error) => {
      eprintln!("{error}");
      std::process::exit(1);
    }
  }
}
