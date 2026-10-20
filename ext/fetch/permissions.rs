// Copyright 2018-2026 the Deno authors. MIT license.

//! What fetch asks before it reaches the network.
//!
//! Upstream used `deno_permissions::PermissionsContainer`, which brings Deno's
//! whole permission model. Here the embedder supplies the policy through
//! [`FetchPermissions`], and [`PermissionsContainer`] keeps upstream's method
//! names and shapes, so the call sites in this crate are unchanged and upstream
//! patches to them still apply.

use std::net::IpAddr;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use simien_core::url::Url;

/// A denied network access. JavaScript sees it as a `NotCapable` error, as it
/// did upstream.
#[derive(Debug, thiserror::Error, simien_error::JsError)]
#[class("NotCapable")]
#[error("{0}")]
pub struct PermissionCheckError(pub String);

/// The embedder's network policy for fetch.
pub trait FetchPermissions: Send + Sync {
  fn check_net_url(
    &self,
    url: &Url,
    api_name: &str,
  ) -> Result<(), PermissionCheckError>;

  fn check_net(
    &self,
    host: &str,
    port: Option<u16>,
    api_name: &str,
  ) -> Result<(), PermissionCheckError>;

  /// The address a fetch host name resolved to, checked again so a name
  /// can't be used to reach an address the policy denies.
  fn check_net_resolved(
    &self,
    ip: &IpAddr,
    port: u16,
  ) -> Result<(), PermissionCheckError>;

  /// As [`FetchPermissions::check_net_resolved`], for remote module imports.
  fn check_import_resolved(
    &self,
    ip: &IpAddr,
    port: u16,
  ) -> Result<(), PermissionCheckError>;

  /// A Unix socket fetch would connect to (a proxy). Returns the path to
  /// connect to, which the policy may resolve, for example through symlinks.
  fn check_net_unix_socket(
    &self,
    path: &Path,
    api_name: &str,
  ) -> Result<PathBuf, PermissionCheckError>;

  fn check_net_vsock(
    &self,
    cid: u32,
    port: u32,
    api_name: &str,
  ) -> Result<(), PermissionCheckError>;
}

/// The policy in the op state, shared with fetch's connectors.
#[derive(Clone)]
pub struct PermissionsContainer(Arc<dyn FetchPermissions>);

impl std::fmt::Debug for PermissionsContainer {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("PermissionsContainer")
  }
}

impl PermissionsContainer {
  pub fn new(permissions: Arc<dyn FetchPermissions>) -> Self {
    Self(permissions)
  }

  /// Every access allowed: for embedders that apply no policy, and tests.
  pub fn allow_all() -> Self {
    Self(Arc::new(AllowAll))
  }

  pub fn check_net_url(
    &mut self,
    url: &Url,
    api_name: &str,
  ) -> Result<(), PermissionCheckError> {
    self.0.check_net_url(url, api_name)
  }

  pub fn check_net<T: AsRef<str>>(
    &mut self,
    host: &(T, Option<u16>),
    api_name: &str,
  ) -> Result<(), PermissionCheckError> {
    self.0.check_net(host.0.as_ref(), host.1, api_name)
  }

  pub fn check_net_resolved(
    &mut self,
    resolved_ip: &IpAddr,
    port: u16,
    _api_name: &str,
  ) -> Result<(), PermissionCheckError> {
    self.0.check_net_resolved(resolved_ip, port)
  }

  pub fn check_import_resolved(
    &mut self,
    resolved_ip: &IpAddr,
    port: u16,
    _api_name: &str,
  ) -> Result<(), PermissionCheckError> {
    self.0.check_import_resolved(resolved_ip, port)
  }

  pub fn check_net_unix_socket(
    &mut self,
    path: &Path,
    api_name: &str,
  ) -> Result<PathBuf, PermissionCheckError> {
    self.0.check_net_unix_socket(path, api_name)
  }

  pub fn check_net_vsock(
    &mut self,
    cid: u32,
    port: u32,
    api_name: &str,
  ) -> Result<(), PermissionCheckError> {
    self.0.check_net_vsock(cid, port, api_name)
  }
}

struct AllowAll;

impl FetchPermissions for AllowAll {
  fn check_net_url(&self, _: &Url, _: &str) -> Result<(), PermissionCheckError> {
    Ok(())
  }
  fn check_net(
    &self,
    _: &str,
    _: Option<u16>,
    _: &str,
  ) -> Result<(), PermissionCheckError> {
    Ok(())
  }
  fn check_net_resolved(
    &self,
    _: &IpAddr,
    _: u16,
  ) -> Result<(), PermissionCheckError> {
    Ok(())
  }
  fn check_import_resolved(
    &self,
    _: &IpAddr,
    _: u16,
  ) -> Result<(), PermissionCheckError> {
    Ok(())
  }
  fn check_net_unix_socket(
    &self,
    path: &Path,
    _: &str,
  ) -> Result<PathBuf, PermissionCheckError> {
    Ok(path.to_path_buf())
  }
  fn check_net_vsock(
    &self,
    _: u32,
    _: u32,
    _: &str,
  ) -> Result<(), PermissionCheckError> {
    Ok(())
  }
}
