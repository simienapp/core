// Copyright 2018-2026 the Simien authors. MIT license.

//! Loading a Node program's modules from its `node_modules` (APS 2: Simien
//! runs Node programs itself, for code granted dangerouslyUseNode).
//!
//! - [`NodeProgram::module_loader`] is the isolate's module loader: `node:*`
//!   and bare built-in names resolve to `node:*` (simien_node's modules, which
//!   simien_core serves itself); everything else resolves as Node does
//!   (package `exports` and conditions, `node_modules` up the tree), through
//!   simien_node_resolver with the bring-your-own-node_modules resolver.
//! - A module that may be CommonJS goes through simien_node_resolver's
//!   translator with oxc's export analysis ([`crate::cjs`]); ESM comes back
//!   unchanged, so `import { x } from "a-cjs-package"` works as in Node.
//! - [`NodeProgram::init_services`] gives simien_node its `require()`: files
//!   it reads are checked against the policy first.

use std::borrow::Cow;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;

use futures::FutureExt;
use simien_core::FastString;
use simien_core::ModuleLoadOptions;
use simien_core::ModuleLoadReferrer;
use simien_core::ModuleLoadResponse;
use simien_core::ModuleLoader;
use simien_core::ModuleSource;
use simien_core::ModuleSourceCode;
use simien_core::ModuleSpecifier;
use simien_core::ModuleType;
use simien_core::ResolutionKind;
use simien_core::error::ModuleLoaderError;
use simien_error::JsErrorBox;
use simien_node::NodeExtInitServices;
use simien_node::NodeRequireLoader;
use simien_node_resolver::DenoIsBuiltInNodeModuleChecker;
use simien_node_resolver::IsBuiltInNodeModuleChecker;
use simien_node_resolver::NodeResolutionKind;
use simien_node_resolver::NodeResolver;
use simien_node_resolver::NodeResolverOptions;
use simien_node_resolver::PackageJsonResolver;
use simien_node_resolver::PackageJsonResolverRc;
use simien_node_resolver::ResolutionMode;
use simien_node_resolver::analyze::CjsModuleExportAnalyzer;
use simien_node_resolver::analyze::NodeCodeTranslator;
use simien_node_resolver::analyze::NodeCodeTranslatorMode;
use simien_node_resolver::cache::NodeResolutionSys;
use simien_node_resolver::errors::PackageJsonLoadError;
use simien_permissions::OpenAccessKind;
use simien_permissions::PermissionsContainer;
use simien_resolver::npm::ByonmInNpmPackageChecker;
use simien_resolver::npm::ByonmNpmResolver;
use simien_resolver::npm::ByonmNpmResolverCreateOptions;
use url::Url;

use crate::cjs::OxcCjsCodeAnalyzer;

/// The file system Node programs run on.
pub type Sys = simien_sys_traits::impls::RealSys;
pub type InNpmPackageChecker = ByonmInNpmPackageChecker;
pub type NpmFolderResolver = ByonmNpmResolver<Sys>;
type Resolver = NodeResolver<
  InNpmPackageChecker,
  DenoIsBuiltInNodeModuleChecker,
  NpmFolderResolver,
  Sys,
>;
type Translator = NodeCodeTranslator<
  OxcCjsCodeAnalyzer,
  InNpmPackageChecker,
  DenoIsBuiltInNodeModuleChecker,
  NpmFolderResolver,
  Sys,
>;

/// One Node program: its resolver, its CommonJS translator, and the
/// services simien_node's `require()` needs.
#[derive(Clone)]
pub struct NodeProgram {
  resolver: simien_maybe_sync::MaybeArc<Resolver>,
  pkg_json_resolver: PackageJsonResolverRc<Sys>,
  translator: Rc<Translator>,
}

impl NodeProgram {
  /// A program whose packages are in `root`'s `node_modules` (and its
  /// parents', as Node looks).
  pub fn new(root: &Path) -> Self {
    let sys = Sys::default();
    let pkg_json_resolver: PackageJsonResolverRc<Sys> =
      simien_maybe_sync::new_rc(PackageJsonResolver::new(sys.clone(), None));
    let npm = ByonmNpmResolver::new(ByonmNpmResolverCreateOptions {
      root_node_modules_dir: Some(root.join("node_modules")),
      search_stop_dir: None,
      sys: NodeResolutionSys::new(sys.clone(), None),
      pkg_json_resolver: pkg_json_resolver.clone(),
    });
    let resolver = simien_maybe_sync::new_rc(NodeResolver::new(
      ByonmInNpmPackageChecker,
      DenoIsBuiltInNodeModuleChecker,
      npm.clone(),
      pkg_json_resolver.clone(),
      NodeResolutionSys::new(sys.clone(), None),
      NodeResolverOptions::default(),
    ));
    let analyzer = simien_maybe_sync::new_rc(CjsModuleExportAnalyzer::new(
      OxcCjsCodeAnalyzer,
      ByonmInNpmPackageChecker,
      resolver.clone(),
      npm,
      pkg_json_resolver.clone(),
      sys,
    ));
    let translator = Rc::new(NodeCodeTranslator::new(
      analyzer,
      NodeCodeTranslatorMode::ModuleLoader,
    ));
    Self {
      resolver,
      pkg_json_resolver,
      translator,
    }
  }

  /// What simien_node's extension takes, for `require()`.
  pub fn init_services(
    &self,
  ) -> NodeExtInitServices<InNpmPackageChecker, NpmFolderResolver, Sys> {
    NodeExtInitServices {
      node_require_loader: Rc::new(RequireLoader {
        pkg_json_resolver: self.pkg_json_resolver.clone(),
      }),
      simien_node_resolver: self.resolver.clone(),
      pkg_json_resolver: self.pkg_json_resolver.clone(),
      sys: Sys::default(),
    }
  }

  /// The isolate's module loader.
  pub fn module_loader(&self) -> Rc<dyn ModuleLoader> {
    Rc::new(self.clone())
  }

  fn may_be_cjs(&self, path: &Path) -> Result<bool, PackageJsonLoadError> {
    may_be_cjs(&self.pkg_json_resolver, path)
  }
}

/// Whether Node would load `path` as CommonJS (or decide by its syntax):
/// `.cjs`, or a `.js` whose closest package.json isn't `"type": "module"`.
fn may_be_cjs(
  pkg_json_resolver: &PackageJsonResolverRc<Sys>,
  path: &Path,
) -> Result<bool, PackageJsonLoadError> {
  Ok(match path.extension().and_then(|e| e.to_str()) {
    Some("cjs") => true,
    Some("js") => pkg_json_resolver
      .get_closest_package_json(path)?
      .is_none_or(|pkg| pkg.typ != "module"),
    _ => false,
  })
}

fn file_path(specifier: &Url) -> Result<PathBuf, JsErrorBox> {
  specifier
    .to_file_path()
    .map_err(|_| JsErrorBox::generic(format!("{specifier} is not a file")))
}

impl ModuleLoader for NodeProgram {
  fn resolve(
    &self,
    specifier: &str,
    referrer: &str,
    kind: ResolutionKind,
  ) -> Result<ModuleSpecifier, ModuleLoaderError> {
    if specifier.starts_with("node:") {
      return Url::parse(specifier).map_err(JsErrorBox::from_err);
    }
    if DenoIsBuiltInNodeModuleChecker.is_builtin_node_module(specifier) {
      return Url::parse(&format!("node:{specifier}")).map_err(JsErrorBox::from_err);
    }
    if matches!(kind, ResolutionKind::MainModule) {
      return simien_core::resolve_url_or_path(specifier, Path::new("/"))
        .map_err(JsErrorBox::from_err);
    }
    let referrer = Url::parse(referrer).map_err(JsErrorBox::from_err)?;
    let resolved = self
      .resolver
      .resolve(
        specifier,
        &referrer,
        ResolutionMode::Import,
        NodeResolutionKind::Execution,
      )
      .map_err(JsErrorBox::from_err)?;
    resolved.into_url().map_err(JsErrorBox::from_err)
  }

  fn load(
    &self,
    specifier: &ModuleSpecifier,
    _referrer: Option<&ModuleLoadReferrer>,
    _options: ModuleLoadOptions,
  ) -> ModuleLoadResponse {
    let this = self.clone();
    let specifier = specifier.clone();
    ModuleLoadResponse::Async(
      async move {
        let path = file_path(&specifier)?;
        let code = std::fs::read_to_string(&path)
          .map_err(|e| JsErrorBox::generic(format!("{}: {e}", path.display())))?;
        if path.extension().is_some_and(|e| e == "json") {
          return Ok(ModuleSource::new(
            ModuleType::Json,
            ModuleSourceCode::String(code.into()),
            &specifier,
            None,
          ));
        }
        let code = if this.may_be_cjs(&path).map_err(JsErrorBox::from_err)? {
          this
            .translator
            .translate_cjs_to_esm(&specifier, Some(Cow::Owned(code)))
            .await
            .map_err(JsErrorBox::from_err)?
            .into_owned()
        } else {
          code
        };
        Ok(ModuleSource::new(
          ModuleType::JavaScript,
          ModuleSourceCode::String(code.into()),
          &specifier,
          None,
        ))
      }
      .boxed_local(),
    )
  }
}

/// simien_node's `require()`: reads files after the policy allows them.
struct RequireLoader {
  pkg_json_resolver: PackageJsonResolverRc<Sys>,
}

impl NodeRequireLoader for RequireLoader {
  fn ensure_read_permission<'a>(
    &self,
    permissions: &mut PermissionsContainer,
    path: Cow<'a, Path>,
  ) -> Result<Cow<'a, Path>, JsErrorBox> {
    let checked = permissions
      .check_open(path, OpenAccessKind::Read, Some("require()"))
      .map_err(JsErrorBox::from_err)?;
    Ok(checked.into_path())
  }

  fn load_text_file_lossy(&self, path: &Path) -> Result<FastString, JsErrorBox> {
    let bytes = std::fs::read(path)
      .map_err(|e| JsErrorBox::generic(format!("{}: {e}", path.display())))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned().into())
  }

  fn is_maybe_cjs(&self, specifier: &Url) -> Result<bool, PackageJsonLoadError> {
    match specifier.to_file_path() {
      Ok(path) => may_be_cjs(&self.pkg_json_resolver, &path),
      Err(()) => Ok(false),
    }
  }

  fn is_maybe_cjs_from_require(
    &self,
    specifier: &Url,
  ) -> Result<bool, PackageJsonLoadError> {
    self.is_maybe_cjs(specifier)
  }
}
