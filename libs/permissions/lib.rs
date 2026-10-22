// Copyright 2018-2026 the Deno authors. MIT license.

//! What Simien's Node layer asks before it touches the outside world.
//!
//! Upstream, `simien_node`, `simien_fs`, `simien_process`, `simien_os` and
//! `simien_napi` used the `PermissionsContainer` of Deno's permissions crate, which brings
//! Deno's whole permission model: flags, prompts, audits. Simien has its own
//! (APS 3). Here the embedder supplies that policy through [`Policy`], and
//! [`PermissionsContainer`] keeps upstream's method names and shapes, so the
//! call sites in those crates are unchanged and upstream patches to them still
//! apply. The data types (`CheckedPath`, `OpenAccessKind`, …) are upstream's.
//!
//! There are no prompts: a check is allowed or denied, never asked.

use std::borrow::Cow;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

pub mod which;

/// The embedder's policy. Every method answers one question; a denial says
/// what was asked, in the words the error shows.
pub trait Policy: Send + Sync {
  /// Reading `path`, an absolute path. `None` asks for every path.
  fn check_read(
    &self,
    path: Option<&Path>,
    api_name: Option<&str>,
  ) -> Result<(), PermissionDeniedError>;

  /// Writing `path`, an absolute path. `None` asks for every path.
  fn check_write(
    &self,
    path: Option<&Path>,
    api_name: Option<&str>,
  ) -> Result<(), PermissionDeniedError>;

  /// Reaching `host` (a name or an address), on `port` when known.
  fn check_net(
    &self,
    host: &str,
    port: Option<u16>,
    api_name: Option<&str>,
  ) -> Result<(), PermissionDeniedError>;

  /// The address a host name resolved to, checked again so a name can't be
  /// used to reach an address the policy denies.
  fn check_net_resolved(
    &self,
    ip: &std::net::IpAddr,
    port: u16,
  ) -> Result<(), PermissionDeniedError>;

  /// Reading environment variable `name`. `None` asks for every variable.
  fn check_env(&self, name: Option<&str>) -> Result<(), PermissionDeniedError>;

  /// Starting `program` (a resolved path or an unresolved name). `None` asks
  /// for every program.
  fn check_run(
    &self,
    program: Option<&RunQueryDescriptor>,
    api_name: Option<&str>,
  ) -> Result<(), PermissionDeniedError>;

  /// System information of `kind` (`hostname`, `osRelease`, …).
  fn check_sys(
    &self,
    kind: &str,
    api_name: Option<&str>,
  ) -> Result<(), PermissionDeniedError>;

  /// Loading the native library at `path` (a Node addon).
  fn check_ffi(&self, path: &Path) -> Result<(), PermissionDeniedError>;
}

/// Quadri-state value for storing permission state
#[derive(Eq, PartialEq, Default, Debug, Clone, Copy, PartialOrd)]
pub enum PermissionState {
  Granted = 0,
  GrantedPartial = 1,
  #[default]
  Prompt = 2,
  Denied = 3,
  DeniedPartial = 4,
  Ignored = 5,
}

/// A denied access. JavaScript sees it as a `NotCapable` error, as it did
/// upstream.
#[derive(Debug, Clone, thiserror::Error, simien_error::JsError)]
#[class("NotCapable")]
#[error(
  "Requires {name} access to {access}{}",
  custom_message.as_deref().map(|m| format!(", {m}")).unwrap_or_default()
)]
pub struct PermissionDeniedError {
  pub access: String,
  pub name: &'static str,
  pub custom_message: Option<String>,
  pub state: PermissionState,
}

impl PermissionDeniedError {
  /// A plain denial of `name` (`read`, `net`, …) to `access`.
  pub fn new(name: &'static str, access: impl Into<String>) -> Self {
    Self {
      access: access.into(),
      name,
      custom_message: None,
      state: PermissionState::Denied,
    }
  }
}

#[derive(Debug, thiserror::Error, simien_error::JsError)]
pub enum PermissionCheckError {
  #[class(inherit)]
  #[error(transparent)]
  PermissionDenied(#[from] PermissionDeniedError),
  #[class(inherit)]
  #[error(transparent)]
  Io(std::io::Error),
}

impl PermissionCheckError {
  pub fn kind(&self) -> std::io::ErrorKind {
    match self {
      PermissionCheckError::PermissionDenied(_) => std::io::ErrorKind::Other,
      PermissionCheckError::Io(e) => e.kind(),
    }
  }

  pub fn into_io_error(self) -> std::io::Error {
    match self {
      Self::PermissionDenied(_) => std::io::Error::new(self.kind(), format!("{}", self)),
      Self::Io(e) => e,
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAccessKind {
  Read,
  ReadNoFollow,
  Write,
  WriteNoFollow,
  ReadWrite,
  ReadWriteNoFollow,
}

impl OpenAccessKind {
  pub fn is_no_follow(&self) -> bool {
    match self {
      OpenAccessKind::ReadNoFollow
      | OpenAccessKind::WriteNoFollow
      | OpenAccessKind::ReadWriteNoFollow => true,
      OpenAccessKind::Read
      | OpenAccessKind::Write
      | OpenAccessKind::ReadWrite => false,
    }
  }

  pub fn is_read(&self) -> bool {
    match self {
      OpenAccessKind::Read
      | OpenAccessKind::ReadNoFollow
      | OpenAccessKind::ReadWrite
      | OpenAccessKind::ReadWriteNoFollow => true,
      OpenAccessKind::Write | OpenAccessKind::WriteNoFollow => false,
    }
  }

  pub fn is_write(&self) -> bool {
    match self {
      OpenAccessKind::Read | OpenAccessKind::ReadNoFollow => false,
      OpenAccessKind::Write
      | OpenAccessKind::WriteNoFollow
      | OpenAccessKind::ReadWrite
      | OpenAccessKind::ReadWriteNoFollow => true,
    }
  }
}

#[derive(Debug)]
pub struct PathWithRequested<'a> {
  pub path: Cow<'a, Path>,
  /// Custom requested display name when differs from resolved.
  pub requested: Option<Cow<'a, str>>,
}

impl<'a> PathWithRequested<'a> {
  pub fn only_path(path: Cow<'a, Path>) -> Self {
    Self {
      path,
      requested: None,
    }
  }

  pub fn display(&self) -> std::path::Display<'_> {
    match &self.requested {
      Some(requested) => Path::new(requested.as_ref()).display(),
      None => self.path.display(),
    }
  }

  pub fn as_owned(&self) -> PathBufWithRequested {
    PathBufWithRequested {
      path: self.path.to_path_buf(),
      requested: self.requested.as_ref().map(|r| r.to_string()),
    }
  }

  pub fn into_owned(self) -> PathBufWithRequested {
    PathBufWithRequested {
      path: self.path.into_owned(),
      requested: self.requested.map(|r| r.into_owned()),
    }
  }
}

impl Deref for PathWithRequested<'_> {
  type Target = Path;

  fn deref(&self) -> &Self::Target {
    &self.path
  }
}

impl AsRef<Path> for PathWithRequested<'_> {
  fn as_ref(&self) -> &Path {
    &self.path
  }
}

impl<'a> AsRef<PathWithRequested<'a>> for PathWithRequested<'a> {
  fn as_ref(&self) -> &PathWithRequested<'a> {
    self
  }
}

#[derive(Debug, Clone)]
pub struct PathBufWithRequested {
  pub path: PathBuf,
  /// Custom requested display name when differs from resolved.
  pub requested: Option<String>,
}

impl PathBufWithRequested {
  pub fn only_path(path: PathBuf) -> Self {
    Self {
      path,
      requested: None,
    }
  }

  pub fn as_path_with_requested(&self) -> PathWithRequested<'_> {
    PathWithRequested {
      path: Cow::Borrowed(self.path.as_path()),
      requested: self.requested.as_deref().map(Cow::Borrowed),
    }
  }
}

impl Deref for PathBufWithRequested {
  type Target = Path;

  fn deref(&self) -> &Self::Target {
    &self.path
  }
}

/// A path the policy allowed, made absolute against the current directory.
#[derive(Debug)]
pub struct CheckedPath<'a> {
  // these are private to prevent someone constructing this outside the crate
  path: PathWithRequested<'a>,
  canonicalized: bool,
}

impl<'a> CheckedPath<'a> {
  pub fn unsafe_new(path: Cow<'a, Path>) -> Self {
    Self {
      path: PathWithRequested {
        path,
        requested: None,
      },
      canonicalized: false,
    }
  }

  pub fn canonicalized(&self) -> bool {
    self.canonicalized
  }

  pub fn display(&self) -> std::path::Display<'_> {
    self.path.display()
  }

  pub fn into_path_with_requested(self) -> PathWithRequested<'a> {
    self.path
  }

  pub fn as_owned(&self) -> CheckedPathBuf {
    CheckedPathBuf {
      path: self.path.as_owned(),
      canonicalized: self.canonicalized,
    }
  }

  pub fn into_owned(self) -> CheckedPathBuf {
    CheckedPathBuf {
      path: self.path.into_owned(),
      canonicalized: self.canonicalized,
    }
  }

  pub fn into_path(self) -> Cow<'a, Path> {
    self.path.path
  }

  pub fn into_owned_path(self) -> PathBuf {
    self.path.path.into_owned()
  }
}

impl<'a> AsRef<PathWithRequested<'a>> for CheckedPath<'a> {
  fn as_ref(&self) -> &PathWithRequested<'a> {
    &self.path
  }
}

impl Deref for CheckedPath<'_> {
  type Target = Path;

  fn deref(&self) -> &Self::Target {
    &self.path.path
  }
}

impl AsRef<Path> for CheckedPath<'_> {
  fn as_ref(&self) -> &Path {
    &self.path.path
  }
}

#[derive(Debug, Clone)]
pub struct CheckedPathBuf {
  path: PathBufWithRequested,
  canonicalized: bool,
}

impl CheckedPathBuf {
  pub fn unsafe_new(path: PathBuf) -> Self {
    Self {
      path: PathBufWithRequested::only_path(path),
      canonicalized: false,
    }
  }

  pub fn as_checked_path(&self) -> CheckedPath<'_> {
    CheckedPath {
      path: self.path.as_path_with_requested(),
      canonicalized: self.canonicalized,
    }
  }

  pub fn into_path_buf(self) -> PathBuf {
    self.path.path
  }
}

impl Deref for CheckedPathBuf {
  type Target = Path;

  fn deref(&self) -> &Self::Target {
    &self.path.path
  }
}

impl AsRef<Path> for CheckedPathBuf {
  fn as_ref(&self) -> &Path {
    &self.path.path
  }
}

/// An executable to check, as upstream described it.
#[derive(Clone, Debug)]
pub struct PathQueryDescriptor<'a> {
  path: Cow<'a, Path>,
  /// The name the program asked for, when it differs from the resolved path.
  requested: Option<String>,
}

impl PartialEq for PathQueryDescriptor<'_> {
  fn eq(&self, other: &Self) -> bool {
    self.path == other.path
  }
}

impl Eq for PathQueryDescriptor<'_> {}

impl<'a> PathQueryDescriptor<'a> {
  pub fn new_known_absolute(path: Cow<'a, Path>) -> Self {
    Self { path, requested: None }
  }

  pub fn with_requested(self, requested: String) -> Self {
    Self { requested: Some(requested), ..self }
  }

  pub fn path(&self) -> &Path {
    &self.path
  }

  pub fn requested(&self) -> Option<&str> {
    self.requested.as_deref()
  }
}

#[derive(Clone, Debug)]
pub enum RunQueryDescriptor<'a> {
  Path(PathQueryDescriptor<'a>),
  /// An executable not resolved to a path.
  Name(String),
}

/// The policy in the op state, shared by the Node layer's crates.
#[derive(Clone)]
pub struct PermissionsContainer(Arc<dyn Policy>);

impl std::fmt::Debug for PermissionsContainer {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("PermissionsContainer")
  }
}

/// `path` made absolute against the current directory, as upstream resolved
/// it before checking.
fn absolute<'a>(path: Cow<'a, Path>) -> Result<Cow<'a, Path>, PermissionCheckError> {
  if path.is_absolute() {
    return Ok(path);
  }
  let cwd = std::env::current_dir().map_err(PermissionCheckError::Io)?;
  Ok(Cow::Owned(cwd.join(path)))
}

impl PermissionsContainer {
  pub fn new(policy: Arc<dyn Policy>) -> Self {
    Self(policy)
  }

  /// Every access allowed: for embedders that apply no policy, and tests.
  pub fn allow_all() -> Self {
    Self(Arc::new(AllowAll))
  }

  pub fn check_open<'a>(
    &self,
    path: Cow<'a, Path>,
    access_kind: OpenAccessKind,
    api_name: Option<&str>,
  ) -> Result<CheckedPath<'a>, PermissionCheckError> {
    let path = absolute(path)?;
    if access_kind.is_read() {
      self.0.check_read(Some(&path), api_name)?;
    }
    if access_kind.is_write() {
      self.0.check_write(Some(&path), api_name)?;
    }
    Ok(CheckedPath::unsafe_new(path))
  }

  /// As [`Self::check_open`]; `display` names the path in a denial upstream,
  /// and here the policy's own message does.
  pub fn check_open_blind<'a>(
    &self,
    path: Cow<'a, Path>,
    access_kind: OpenAccessKind,
    _display: &str,
    api_name: Option<&str>,
  ) -> Result<CheckedPath<'a>, PermissionCheckError> {
    self.check_open(path, access_kind, api_name)
  }

  pub fn check_read_all(&self, api_name: &str) -> Result<(), PermissionCheckError> {
    Ok(self.0.check_read(None, Some(api_name))?)
  }

  pub fn query_read_all(&self) -> bool {
    self.0.check_read(None, None).is_ok()
  }

  pub fn check_write_all(&self, api_name: &str) -> Result<(), PermissionCheckError> {
    Ok(self.0.check_write(None, Some(api_name))?)
  }

  pub fn check_write<'a>(
    &self,
    path: Cow<'a, Path>,
    api_name: &str,
  ) -> Result<CheckedPath<'a>, PermissionCheckError> {
    let path = absolute(path)?;
    self.0.check_write(Some(&path), Some(api_name))?;
    Ok(CheckedPath::unsafe_new(path))
  }

  /// Upstream allows a write that is denied only for part of the tree under
  /// `path`; here the policy answers for `path` itself.
  pub fn check_write_partial<'a>(
    &self,
    path: Cow<'a, Path>,
    api_name: &str,
  ) -> Result<CheckedPath<'a>, PermissionCheckError> {
    self.check_write(path, api_name)
  }

  pub fn check_net<T: AsRef<str>>(
    &self,
    host: &(T, Option<u16>),
    api_name: &str,
  ) -> Result<(), PermissionCheckError> {
    Ok(self.0.check_net(host.0.as_ref(), host.1, Some(api_name))?)
  }

  pub fn check_net_resolved(
    &self,
    resolved_ip: &std::net::IpAddr,
    port: u16,
    _api_name: &str,
  ) -> Result<(), PermissionCheckError> {
    Ok(self.0.check_net_resolved(resolved_ip, port)?)
  }

  pub fn check_env(&self, var: &str) -> Result<(), PermissionCheckError> {
    Ok(self.0.check_env(Some(var))?)
  }

  pub fn check_env_all(&self) -> Result<(), PermissionCheckError> {
    Ok(self.0.check_env(None)?)
  }

  pub fn query_env(&self, var: Option<&str>) -> PermissionState {
    match self.0.check_env(var) {
      Ok(()) => PermissionState::Granted,
      Err(denied) => denied.state,
    }
  }

  pub fn check_run(
    &self,
    cmd: &RunQueryDescriptor,
    api_name: &str,
  ) -> Result<(), PermissionCheckError> {
    Ok(self.0.check_run(Some(cmd), Some(api_name))?)
  }

  pub fn check_run_all(&self, api_name: &str) -> Result<(), PermissionCheckError> {
    Ok(self.0.check_run(None, Some(api_name))?)
  }

  pub fn query_run_all(&self, api_name: &str) -> bool {
    self.0.check_run(None, Some(api_name)).is_ok()
  }

  pub fn check_sys(&self, kind: &str, api_name: &str) -> Result<(), PermissionCheckError> {
    Ok(self.0.check_sys(kind, Some(api_name))?)
  }

  pub fn check_ffi<'a>(
    &self,
    path: Cow<'a, Path>,
  ) -> Result<Cow<'a, Path>, PermissionCheckError> {
    let path = absolute(path)?;
    self.0.check_ffi(&path)?;
    Ok(path)
  }
}

struct AllowAll;

impl Policy for AllowAll {
  fn check_read(&self, _: Option<&Path>, _: Option<&str>) -> Result<(), PermissionDeniedError> {
    Ok(())
  }
  fn check_write(&self, _: Option<&Path>, _: Option<&str>) -> Result<(), PermissionDeniedError> {
    Ok(())
  }
  fn check_net(&self, _: &str, _: Option<u16>, _: Option<&str>) -> Result<(), PermissionDeniedError> {
    Ok(())
  }
  fn check_net_resolved(&self, _: &std::net::IpAddr, _: u16) -> Result<(), PermissionDeniedError> {
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

/// Upstream serialized terminal input between prompts. Simien never prompts,
/// so there is nothing to serialize; the lock is kept for the call sites.
pub mod prompter {
  use std::sync::Mutex;
  use std::sync::MutexGuard;
  use std::sync::OnceLock;

  static TERMINAL_INPUT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

  pub fn lock_terminal_input() -> MutexGuard<'static, ()> {
    TERMINAL_INPUT_LOCK
      .get_or_init(Default::default)
      .lock()
      .unwrap_or_else(|err| err.into_inner())
  }
}
