// Copyright 2018-2026 the Deno authors. MIT license.

// Simien keeps the bootstrap ops something reads: Deno.args and Deno.pid, the
// color and log settings node:util and node:tty use, and the unstable flags
// node:process reports (Simien has none). The rest is cut (APS 2.1).

use simien_core::OpState;
use simien_core::op2;
use simien_terminal::colors::ColorLevel;

use crate::BootstrapOptions;

simien_core::extension!(
  simien_bootstrap,
  ops = [
    op_bootstrap_args,
    op_bootstrap_exec_args,
    op_bootstrap_target,
    op_bootstrap_pid,
    op_bootstrap_log_level,
    op_bootstrap_color_depth,
    op_bootstrap_no_color,
    op_bootstrap_stdout_no_color,
    op_bootstrap_stderr_no_color,
    op_bootstrap_unstable_args,
  ],
  options = {
    bootstrap: BootstrapOptions,
  },
  state = |state, options| {
    state.put(options.bootstrap);
  },
);

/// The target triple `Deno.build` reports, as upstream's snapshot options
/// computed it.
#[op2]
#[string]
pub fn op_bootstrap_target() -> String {
  let arch = std::env::consts::ARCH;
  match std::env::consts::OS {
    "macos" => format!("{arch}-apple-darwin"),
    "linux" => format!("{arch}-unknown-linux-gnu"),
    "windows" => format!("{arch}-pc-windows-msvc"),
    rest => format!("{arch}-{rest}"),
  }
}

#[op2]
pub fn op_bootstrap_args(state: &mut OpState) -> Vec<String> {
  state.borrow::<BootstrapOptions>().args.clone()
}

#[op2]
pub fn op_bootstrap_exec_args(state: &mut OpState) -> Vec<String> {
  state.borrow::<BootstrapOptions>().exec_args.clone()
}

#[op2(fast)]
#[smi]
pub fn op_bootstrap_pid() -> u32 {
  std::process::id()
}

#[op2]
pub fn op_bootstrap_unstable_args(state: &mut OpState) -> Vec<String> {
  // Simien has no --unstable-* flags.
  let _ = state;
  Vec::new()
}

#[op2(fast)]
#[smi]
pub fn op_bootstrap_log_level(state: &mut OpState) -> i32 {
  state.borrow::<BootstrapOptions>().log_level
}

#[op2(fast)]
pub fn op_bootstrap_color_depth(state: &mut OpState) -> i32 {
  let options = state.borrow::<BootstrapOptions>();
  match options.color_level {
    ColorLevel::None => 1,
    ColorLevel::Ansi => 4,
    ColorLevel::Ansi256 => 8,
    ColorLevel::TrueColor => 24,
  }
}

#[op2(fast)]
pub fn op_bootstrap_no_color(_state: &mut OpState) -> bool {
  !simien_terminal::colors::use_color()
}

#[op2(fast)]
pub fn op_bootstrap_stdout_no_color(_state: &mut OpState) -> bool {
  if simien_terminal::colors::force_color() {
    return false;
  }

  !simien_terminal::is_stdout_tty() || !simien_terminal::colors::use_color()
}

#[op2(fast)]
pub fn op_bootstrap_stderr_no_color(_state: &mut OpState) -> bool {
  if simien_terminal::colors::force_color() {
    return false;
  }

  !simien_terminal::is_stderr_tty() || !simien_terminal::colors::use_color()
}

