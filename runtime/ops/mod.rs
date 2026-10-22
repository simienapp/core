// Copyright 2018-2026 the Deno authors. MIT license.

// Simien keeps the ops behind the Deno namespace simien_node reads. Workers,
// desktop, permission prompts and Deno's HTTP ops are cut (APS 2.1).

pub mod bootstrap;
pub mod fs_events;
pub mod node_gaps;
pub mod runtime;
pub mod tty;
