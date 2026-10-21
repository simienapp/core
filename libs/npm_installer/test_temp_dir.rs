// Copyright 2018-2026 the Simien authors. MIT license.

//! The part of Deno's test_util::TempDir these tests use: a temporary
//! directory shared by clones and removed when the last one drops.

use std::path::Path;
use std::sync::Arc;

#[derive(Clone)]
pub struct TempDir(Arc<tempfile::TempDir>);

#[allow(
  clippy::disallowed_methods,
  reason = "a test helper on the real file system"
)]
impl TempDir {
  pub fn new() -> Self {
    Self(Arc::new(tempfile::tempdir().expect("create a temp dir")))
  }

  pub fn path(&self) -> &Path {
    self.0.path()
  }

  pub fn write(&self, path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
    std::fs::write(self.path().join(path), contents).expect("write");
  }

  pub fn read_to_string(&self, path: impl AsRef<Path>) -> String {
    std::fs::read_to_string(self.path().join(path)).expect("read")
  }
}
