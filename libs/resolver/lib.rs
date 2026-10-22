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

use std::borrow::Cow;

/// Whether the React CVE source patches are enabled. Opt in by setting the
/// `DENO_PATCH_REACT_CVE` environment variable to a non-empty value other than
/// `0`.
///
/// The variable is read exactly once (via `sys`) and cached. Module loading
/// happens before any user code runs, so the value is effectively snapshotted
/// at startup and cannot be toggled later via `Deno.env.set`.
pub fn is_react_cve_patch_enabled(sys: &impl simien_sys_traits::EnvVar) -> bool {
  static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
  *ENABLED.get_or_init(|| {
    sys
      .env_var("DENO_PATCH_REACT_CVE")
      .map(|v| !v.is_empty() && v != "0")
      .unwrap_or(false)
  })
}

/// Load time source patches that neutralize two known React Server Components
/// vulnerabilities shipped in affected `react-server-dom-*` builds:
///
/// * CVE-2025-55182 (RCE): deserialized model keys are not filtered, letting a
///   crafted payload reach `constructor` / `prototype` / `_response`.
/// * CVE-2025-55184 (DoS): a cyclic thenable makes chunk fulfillment loop
///   forever.
///
/// Callers must gate this behind [`is_react_cve_patch_enabled`] (the patches
/// are opt-in via the `DENO_PATCH_REACT_CVE` environment variable) and only
/// apply it to JavaScript source.
pub fn patch_react_cves<'a>(
  filename: &str,
  code: Cow<'a, str>,
) -> Cow<'a, str> {
  let mut modified = Cow::Borrowed(&*code);
  let mut offset = 0;

  for (index, _) in code.match_indices("split(") {
    let index = index + offset;

    let len = "split(".len();
    if !modified[index + len..].starts_with("':')")
      && !modified[index + len..].starts_with("\":\")")
    {
      continue;
    }

    let Some((end, _)) = modified[index..].char_indices().nth(100) else {
      break;
    };

    if !modified[index..index + end].contains("resolved_model") {
      continue;
    }

    let insert =
      ".filter(x => !(['constructor', 'prototype', '_response'].includes(x)))";
    let mut s = String::with_capacity(modified.len() + insert.len());
    s.push_str(&modified[0..index + len + 4]);
    s.push_str(insert);
    offset += insert.len();
    s.push_str(&modified[(index + len + 4)..]);
    modified = Cow::Owned(s);
  }

  if offset > 0 {
    log::debug!("Patched React RCE (CVE-2025-55182) in {filename}");
  }

  // Collapse the stage 1 (RCE) result back into a `Cow<'a>` before stage 2
  // borrows it: the original input when nothing was patched, otherwise the
  // owned patched string. Stage 2 must fall back to this (not `code`) so a
  // stage 1 patch is preserved even when stage 2 makes no change.
  let stage1: Cow<'a, str> = match modified {
    Cow::Borrowed(_) => code,
    Cow::Owned(s) => Cow::Owned(s),
  };

  let code2 = &*stage1;
  let mut modified = Cow::Borrowed(&*stage1);
  let mut offset = 0;

  const PROTOTYPE_THEN: &str = ".prototype.then";
  'outer: for (index, _) in code2.match_indices(PROTOTYPE_THEN) {
    let index = index + offset;

    let after_equals =
      if modified[index + PROTOTYPE_THEN.len()..].starts_with(" =") {
        index + PROTOTYPE_THEN.len() + 2
      } else if modified[index + PROTOTYPE_THEN.len()..].starts_with("=") {
        index + PROTOTYPE_THEN.len() + 1
      } else {
        continue;
      };

    let Some((end, _)) = modified[index..].char_indices().nth(300) else {
      break;
    };

    if !modified[index..index + end].contains("resolved_model") {
      continue;
    }

    if !modified[index..index + end].contains("fulfilled") {
      continue;
    }

    // now match { until we end up at the next }
    let mut brace_count = 0;
    let mut insert_index = None;
    for (i, c) in modified[index..].char_indices() {
      if c == '{' {
        brace_count += 1;
      } else if c == '}' {
        if brace_count == 0 {
          continue 'outer;
        }
        brace_count -= 1;
        if brace_count == 0 {
          insert_index = Some(index + i + 1);
          break;
        }
      }
    }
    let Some(insert_index) = insert_index else {
      continue;
    };

    const INSERT_BEFORE: &str = "(()=>{const __old=";
    const INSERT_AFTER: &str = ";const __new=function(res, rej){return __old.call(this,(v)=>{let w=v;let l=0;while(w&&typeof w==='object'&&w.then===__new){l++;if(w===this||l>1000){if(typeof rej==='function')rej(new Error('Cannot have cyclic thenables.'));return}if(w.status==='fulfilled')w=w.value;else break;}res(v)},rej)};return __new})()";

    // after the = insert the before, and after the } insert the after
    let mut s = String::with_capacity(
      modified.len() + INSERT_BEFORE.len() + INSERT_AFTER.len(),
    );
    s.push_str(&modified[0..after_equals]);
    s.push_str(INSERT_BEFORE);
    s.push_str(&modified[after_equals..insert_index]);
    s.push_str(INSERT_AFTER);
    s.push_str(&modified[insert_index..]);
    offset += INSERT_BEFORE.len() + INSERT_AFTER.len();
    modified = Cow::Owned(s);
  }

  if offset > 0 {
    log::debug!("Patched React DoS (CVE-2025-55184) in {filename}");
  }

  match modified {
    Cow::Borrowed(_) => stage1,
    Cow::Owned(s) => Cow::Owned(s),
  }
}
