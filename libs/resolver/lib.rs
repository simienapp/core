// Copyright 2018-2026 the Deno authors. MIT license.

//! npm resolution for Simien, forked from the npm parts of Deno's
//! deno_resolver: the lockfile (with importers for bun, npm, pnpm and yarn
//! lockfiles), the npm resolvers the module loader and installer use, and
//! `.npmrc` discovery. Deno's specifier resolver, import maps, emit, CJS
//! analysis and deno.json handling are not part of it; `workspace.rs` is
//! Simien's workspace model.

#![deny(clippy::print_stderr)]
#![deny(clippy::print_stdout)]

use simien_cache_dir::npm::NpmCacheDir;

pub mod bun_lockfile_import;
pub mod collections;
pub mod display;
pub mod lockfile;
pub mod npm;
pub mod npm_lockfile_import;
pub mod npmrc;
pub mod pnpm_lockfile_import;
pub mod workspace;
pub mod yarn_lockfile_import;

#[allow(clippy::disallowed_types, reason = "definition")]
pub(crate) type NpmCacheDirRc = simien_maybe_sync::MaybeArc<NpmCacheDir>;
