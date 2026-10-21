// Copyright 2018-2026 the Simien authors. MIT license.

//! The workspace a Simien project lives in, as the npm crates see it.
//!
//! Upstream reads Deno's workspace model (deno.json, import maps, JSR). Here
//! a workspace is a root `package.json`, the member packages its
//! `workspaces` globs name, and the packages `simien.json` links in place of
//! registry versions. Only what npm resolution, the lockfile and `.npmrc`
//! discovery need is here; the link-package conversion below is upstream's.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use indexmap::IndexMap;
use simien_npm::registry::NpmPackageVersionInfo;
use simien_package_json::PackageJson;
use simien_package_json::PackageJsonLoadError;
use simien_package_json::PackageJsonRc;
use simien_semver::SmallStackString;
use simien_semver::StackString;
use simien_semver::Version;
use simien_semver::package::PackageName;
use simien_sys_traits::FsDirEntry;
use simien_sys_traits::FsRead;
use simien_sys_traits::FsReadDir;
use simien_terminal::colors;
use thiserror::Error;
use url::Url;

/// The lockfile Simien writes next to the root package.json.
pub const LOCKFILE_NAME: &str = "simien.lock";
/// Simien's own project configuration, next to the root package.json.
pub const CONFIG_NAME: &str = "simien.json";

#[derive(Debug, Error)]
pub enum WorkspaceDiscoverError {
  #[error(transparent)]
  PackageJson(#[from] PackageJsonLoadError),
  #[error("failed to read {}: {source}", path.display())]
  Io {
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },
  #[error("invalid {}: {source}", path.display())]
  Config {
    path: PathBuf,
    #[source]
    source: serde_json::Error,
  },
  #[error("{} is not an absolute path", .0.display())]
  NotAbsolute(PathBuf),
}

/// The parts of `simien.json` the npm crates read.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SimienJson {
  /// Directories of packages to use in place of their registry versions.
  links: Vec<String>,
}

#[derive(Debug)]
pub struct Workspace {
  root_dir: PathBuf,
  root_dir_url: Url,
  root_pkg_json: Option<PackageJsonRc>,
  /// Member packages: their directory relative to the root (no trailing
  /// slash) and their package.json.
  members: Vec<(String, PackageJsonRc)>,
  links: Vec<PackageJsonRc>,
  catalogs: IndexMap<String, IndexMap<String, String>>,
}

impl Workspace {
  /// The workspace rooted at `root_dir`: its package.json (optional), the
  /// members its `workspaces` globs name, and `simien.json`'s links.
  pub fn discover(
    sys: &(impl FsRead + FsReadDir),
    root_dir: &Path,
  ) -> Result<Self, WorkspaceDiscoverError> {
    if !root_dir.is_absolute() {
      return Err(WorkspaceDiscoverError::NotAbsolute(root_dir.to_path_buf()));
    }
    let root_dir_url = simien_path_util::url_from_directory_path(root_dir)
      .map_err(|_| WorkspaceDiscoverError::NotAbsolute(root_dir.to_path_buf()))?;
    let root_pkg_json =
      PackageJson::load_from_path(sys, None, &root_dir.join("package.json"))?;

    let mut members = Vec::new();
    if let Some(globs) = root_pkg_json.as_ref().and_then(|p| p.workspaces.as_ref()) {
      for dir in expand_workspace_globs(sys, root_dir, globs)? {
        if let Some(pkg_json) =
          PackageJson::load_from_path(sys, None, &root_dir.join(&dir).join("package.json"))?
        {
          members.push((dir, pkg_json));
        }
      }
    }

    let config_path = root_dir.join(CONFIG_NAME);
    let config: SimienJson = match sys.fs_read_to_string(&config_path) {
      Ok(text) => serde_json::from_str(&text).map_err(|source| {
        WorkspaceDiscoverError::Config { path: config_path.clone(), source }
      })?,
      Err(err) if err.kind() == std::io::ErrorKind::NotFound => SimienJson::default(),
      Err(source) => {
        return Err(WorkspaceDiscoverError::Io { path: config_path, source });
      }
    };
    let mut links = Vec::new();
    for link in &config.links {
      let path = root_dir.join(link).join("package.json");
      if let Some(pkg_json) = PackageJson::load_from_path(sys, None, &path)? {
        links.push(pkg_json);
      }
    }

    let mut catalogs = IndexMap::new();
    if let Some(root) = &root_pkg_json {
      if let Some(catalog) = &root.catalog {
        catalogs.insert(String::new(), catalog.clone());
      }
      if let Some(named) = &root.catalogs {
        for (name, catalog) in named {
          catalogs.insert(name.clone(), catalog.clone());
        }
      }
    }

    Ok(Self {
      root_dir: root_dir.to_path_buf(),
      root_dir_url,
      root_pkg_json,
      members,
      links,
      catalogs,
    })
  }

  pub fn root_dir(&self) -> &Path {
    &self.root_dir
  }

  pub fn root_dir_url(&self) -> &Url {
    &self.root_dir_url
  }

  pub fn root_pkg_json(&self) -> Option<&PackageJsonRc> {
    self.root_pkg_json.as_ref()
  }

  pub fn members(&self) -> impl Iterator<Item = (&str, &PackageJsonRc)> {
    self.members.iter().map(|(dir, pkg_json)| (dir.as_str(), pkg_json))
  }

  pub fn link_pkg_jsons(&self) -> impl Iterator<Item = &PackageJsonRc> {
    self.links.iter()
  }

  /// `catalog` (the default, named "") and `catalogs` from the root
  /// package.json, as bun and pnpm write them.
  pub fn catalogs(&self) -> &IndexMap<String, IndexMap<String, String>> {
    &self.catalogs
  }

  /// The root package.json's `overrides`.
  pub fn npm_overrides(
    &self,
  ) -> Option<&serde_json::Map<String, serde_json::Value>> {
    self.root_pkg_json.as_ref()?.overrides.as_ref()
  }

  pub fn lockfile_path(&self) -> PathBuf {
    self.root_dir.join(LOCKFILE_NAME)
  }
}

/// How node_modules is laid out: isolated (pnpm-style, each package sees
/// only its own dependencies) or hoisted (npm-style, flattened). From
/// deno_config's NodeModulesLinkerMode.
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum NodeModulesLinkerMode {
  #[default]
  Isolated,
  Hoisted,
}

/// Whether `version` satisfies `version_req`, counting a prerelease that
/// falls inside one of its ranges. From deno_config.
pub fn version_req_matches_including_pre(
  version_req: &simien_semver::VersionReq,
  version: &Version,
) -> bool {
  match version_req.inner() {
    simien_semver::RangeSetOrTag::RangeSet(set) => {
      set.satisfies(version)
        || (!version.pre.is_empty()
          && set.0.iter().any(|range| range.intersects_version(version)))
    }
    simien_semver::RangeSetOrTag::Tag(_) => false,
  }
}

/// Expands `workspaces` entries: a directory, or a directory ending in `/*`
/// for each of its subdirectories. Order is stable (sorted).
fn expand_workspace_globs(
  sys: &(impl FsRead + FsReadDir),
  root_dir: &Path,
  globs: &[String],
) -> Result<Vec<String>, WorkspaceDiscoverError> {
  let mut dirs = Vec::new();
  for glob in globs {
    let glob = glob.trim_start_matches("./").trim_end_matches('/');
    if let Some(parent) = glob.strip_suffix("/*") {
      let path = root_dir.join(parent);
      let entries = match sys.fs_read_dir(&path) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
        Err(source) => return Err(WorkspaceDiscoverError::Io { path, source }),
      };
      let mut found = Vec::new();
      for entry in entries {
        let entry = entry.map_err(|source| WorkspaceDiscoverError::Io {
          path: path.clone(),
          source,
        })?;
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
          found.push(format!("{parent}/{}", entry.file_name().to_string_lossy()));
        }
      }
      found.sort();
      dirs.extend(found);
    } else {
      dirs.push(glob.to_string());
    }
  }
  dirs.dedup();
  Ok(dirs)
}

#[allow(clippy::disallowed_types, reason = "wraps Arc directly as the Rc type")]
#[derive(Debug, Default, Clone)]
pub struct WorkspaceNpmLinkPackagesRc(
  pub std::sync::Arc<HashMap<PackageName, Vec<NpmPackageVersionInfo>>>,
);

impl WorkspaceNpmLinkPackagesRc {
  pub fn from_workspace(workspace: &Workspace) -> Self {
    let mut entries: HashMap<PackageName, Vec<NpmPackageVersionInfo>> =
      HashMap::new();
    for pkg_json in workspace.link_pkg_jsons() {
      let Some(name) = pkg_json.name.as_ref() else {
        log::warn!(
          "{} Link package ignored because package.json was missing name field.\n    at {}",
          colors::yellow("Warning"),
          pkg_json.path.display(),
        );
        continue;
      };
      match pkg_json_to_version_info(pkg_json) {
        Ok(version_info) => {
          let entry = entries.entry(PackageName::from_str(name)).or_default();
          entry.push(version_info);
        }
        Err(err) => {
          log::warn!(
            "{} {}\n    at {}",
            colors::yellow("Warning"),
            err,
            pkg_json.path.display(),
          );
        }
      }
    }
    Self(simien_maybe_sync::new_arc(entries))
  }
}

#[derive(Debug, Error)]
enum PkgJsonToVersionInfoError {
  #[error(
    "Linked package ignored because package.json was missing version field."
  )]
  VersionMissing,
  #[error(
    "Linked package ignored because package.json version field could not be parsed."
  )]
  VersionInvalid {
    #[source]
    source: simien_semver::npm::NpmVersionParseError,
  },
}

fn pkg_json_to_version_info(
  pkg_json: &simien_package_json::PackageJson,
) -> Result<NpmPackageVersionInfo, PkgJsonToVersionInfoError> {
  fn parse_deps(
    deps: Option<&IndexMap<String, String>>,
  ) -> HashMap<StackString, StackString> {
    deps
      .map(|d| {
        d.into_iter()
          .map(|(k, v)| (StackString::from_str(k), StackString::from_str(v)))
          .collect()
      })
      .unwrap_or_default()
  }

  fn parse_small_stack_string_array(v: &[String]) -> Vec<SmallStackString> {
    v.iter().map(|s| SmallStackString::from_str(s)).collect()
  }

  fn parse_stack_string_array(v: &[String]) -> Vec<StackString> {
    v.iter().map(|s| StackString::from_str(s)).collect()
  }

  let Some(version) = &pkg_json.version else {
    return Err(PkgJsonToVersionInfoError::VersionMissing);
  };

  let version = Version::parse_from_npm(version)
    .map_err(|source| PkgJsonToVersionInfoError::VersionInvalid { source })?;
  Ok(NpmPackageVersionInfo {
    version,
    exports: pkg_json
      .exports
      .as_ref()
      .map(simien_npm::registry::NpmPackageExportKeys::from_exports_object),
    dist: None,
    bin: pkg_json
      .bin
      .as_ref()
      .and_then(|v| serde_json::from_value(v.clone()).ok()),
    dependencies: parse_deps(pkg_json.dependencies.as_ref()),
    bundle_dependencies: pkg_json
      .bundle_dependencies
      .as_ref()
      .map(|d| parse_stack_string_array(d))
      .unwrap_or_default(),
    bundled_dependencies: Vec::new(),
    optional_dependencies: parse_deps(pkg_json.optional_dependencies.as_ref()),
    peer_dependencies: parse_deps(pkg_json.peer_dependencies.as_ref()),
    peer_dependencies_meta: pkg_json
      .peer_dependencies_meta
      .clone()
      .and_then(|m| serde_json::from_value(m).ok())
      .unwrap_or_default(),
    os: pkg_json
      .os
      .as_deref()
      .map(parse_small_stack_string_array)
      .unwrap_or_default(),
    cpu: pkg_json
      .cpu
      .as_deref()
      .map(parse_small_stack_string_array)
      .unwrap_or_default(),
    scripts: pkg_json
      .scripts
      .as_ref()
      .map(|scripts| {
        scripts
          .iter()
          .map(|(k, v)| (SmallStackString::from_str(k), v.clone()))
          .collect()
      })
      .unwrap_or_default(),
    has_install_script: None,
    // not worth increasing memory for showing a deprecated
    // message for linked packages
    deprecated: None,
    // linked/workspace packages from a package.json carry no registry
    // trust signals
    npm_user: None,
  })
}

#[cfg(test)]
mod test {
  use simien_sys_traits::impls::InMemorySys;

  use super::*;

  #[test]
  fn discovers_members_links_and_catalogs() {
    let sys = InMemorySys::default();
    sys.fs_insert_json(
      "/app/package.json",
      serde_json::json!({
        "name": "app",
        "workspaces": ["packages/*", "tools/cli"],
        "catalog": { "react": "^19.0.0" },
        "catalogs": { "test": { "vitest": "^3.0.0" } },
        "overrides": { "left-pad": "1.3.0" },
        "dependencies": { "react": "catalog:" },
      }),
    );
    sys.fs_insert_json(
      "/app/packages/b/package.json",
      serde_json::json!({ "name": "b", "dependencies": { "zod": "^4" } }),
    );
    sys.fs_insert_json(
      "/app/packages/a/package.json",
      serde_json::json!({ "name": "a" }),
    );
    sys.fs_insert_json(
      "/app/tools/cli/package.json",
      serde_json::json!({ "name": "cli" }),
    );
    sys.fs_insert_json(
      "/app/simien.json",
      serde_json::json!({ "links": ["../lib"] }),
    );
    sys.fs_insert_json(
      "/lib/package.json",
      serde_json::json!({ "name": "lib", "version": "1.0.0" }),
    );

    let workspace = Workspace::discover(&sys, Path::new("/app")).unwrap();
    assert_eq!(workspace.root_dir_url().as_str(), "file:///app/");
    assert_eq!(
      workspace.members().map(|(dir, _)| dir).collect::<Vec<_>>(),
      ["packages/a", "packages/b", "tools/cli"],
    );
    assert_eq!(
      workspace
        .link_pkg_jsons()
        .map(|p| p.name.clone().unwrap())
        .collect::<Vec<_>>(),
      ["lib"],
    );
    assert_eq!(workspace.catalogs()[""]["react"], "^19.0.0");
    assert_eq!(workspace.catalogs()["test"]["vitest"], "^3.0.0");
    assert!(workspace.npm_overrides().unwrap().contains_key("left-pad"));
    assert_eq!(workspace.lockfile_path(), Path::new("/app/simien.lock"));

    let links = WorkspaceNpmLinkPackagesRc::from_workspace(&workspace);
    assert_eq!(links.0.len(), 1);
  }

  #[test]
  fn a_directory_without_package_json_is_an_empty_workspace() {
    let sys = InMemorySys::default();
    let workspace = Workspace::discover(&sys, Path::new("/empty")).unwrap();
    assert!(workspace.root_pkg_json().is_none());
    assert_eq!(workspace.members().count(), 0);
  }

  #[test]
  fn an_invalid_simien_json_is_an_error() {
    let sys = InMemorySys::default();
    sys.fs_insert("/app/simien.json", "{ not json");
    assert!(matches!(
      Workspace::discover(&sys, Path::new("/app")),
      Err(WorkspaceDiscoverError::Config { .. })
    ));
  }
}
