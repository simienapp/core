// Copyright 2018-2026 the Simien authors. MIT license.

// Native addons call Node-API's functions in the binary that loads them:
// the examples and tests export them, as an embedder's binary must
// (simien_napi::print_linker_flags in its build script).
fn main() {
  simien_napi::print_linker_flags_for(simien_napi::LinkTarget::Examples);
  simien_napi::print_linker_flags_for(simien_napi::LinkTarget::Tests);
}
