// Copyright 2018-2026 the Simien authors. MIT license.

//! TypeScript in extensions, compiled to JavaScript as it loads.
//!
//! simien_node ships 201 `.ts` files (and this crate one) that import each
//! other by their `.ts` names. Upstream compiled them with deno_ast; Simien
//! depends on no deno_* crate (APS 2.1), so this is oxc's TypeScript
//! transform, handed to simien_core as its `extension_transpiler`. The files
//! stay identical to upstream.

use std::borrow::Cow;
use std::path::Path;

use oxc_allocator::Allocator;
use oxc_codegen::Codegen;
use oxc_codegen::CodegenOptions;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::TransformOptions;
use oxc_transformer::Transformer;
use simien_core::ModuleCodeString;
use simien_core::ModuleName;
use simien_core::SourceMapData;
use simien_error::JsErrorBox;

/// The transpiler for `RuntimeOptions::extension_transpiler`, as upstream's
/// `maybe_transpile_source`: `node:*` modules (TypeScript whatever their
/// file) and `.ts`/`.mts` files become JavaScript; everything else passes
/// through. Source maps are kept in debug builds only, as upstream does.
pub fn transpile_extension(
  name: ModuleName,
  code: ModuleCodeString,
) -> Result<(ModuleCodeString, Option<SourceMapData>), JsErrorBox> {
  let specifier = name.as_str();
  let code = substitute_node_version(specifier, code);
  let typescript = specifier.starts_with("node:")
    || specifier.ends_with(".ts")
    || specifier.ends_with(".mts");
  if !typescript {
    return Ok((code, None));
  }
  let (javascript, map) = transpile_typescript(specifier, code.as_str())?;
  let map = cfg!(debug_assertions).then(|| Cow::Owned(map.into_bytes()));
  Ok((javascript.into(), map))
}

/// `node:process` reports the Node version simien_node implements: its
/// source holds a `__NODE_VERSION__` placeholder for `simien_node::NODE_VERSION`
/// (as upstream substitutes `deno_node::NODE_VERSION`).
fn substitute_node_version(name: &str, source: ModuleCodeString) -> ModuleCodeString {
  const TOKEN: &str = "__NODE_VERSION__";
  if !name.ends_with("simien_node/_process/process.ts") {
    return source;
  }
  source.as_str().replace(TOKEN, simien_node::NODE_VERSION).into()
}

/// `source`, TypeScript, as JavaScript and its source map (JSON).
pub fn transpile_typescript(
  specifier: &str,
  source: &str,
) -> Result<(String, String), JsErrorBox> {
  let allocator = Allocator::default();
  let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
  if let Some(error) = parsed.diagnostics.first() {
    return Err(JsErrorBox::new(
      "SyntaxError",
      format!("{specifier}: {error}"),
    ));
  }
  let mut program = parsed.program;
  let scoping = SemanticBuilder::new()
    .with_enum_eval(true)
    .build(&program)
    .semantic
    .into_scoping();
  let path = Path::new(specifier);
  let transformed = Transformer::new(&allocator, path, &TransformOptions::default())
    .build_with_scoping(scoping, &mut program);
  if let Some(error) = transformed.diagnostics.first() {
    return Err(JsErrorBox::generic(format!("{specifier}: {error}")));
  }
  let printed = Codegen::new()
    .with_options(CodegenOptions {
      source_map_path: Some(path.to_path_buf()),
      ..CodegenOptions::default()
    })
    .build(&program);
  let map = printed
    .map
    .map(|map| map.to_json_string())
    .unwrap_or_default();
  Ok((printed.code, map))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn typescript_becomes_javascript_and_the_rest_passes_through() {
    let (js, map) = transpile_typescript(
      "ext:x/a.ts",
      "import type { T } from './t.ts';\nenum E { A }\nexport const f = (x: number): T => x as T;\n",
    )
    .unwrap();
    assert!(!js.contains(": number") && !js.contains("import type"), "{js}");
    assert!(js.contains("export const f"), "{js}");
    assert!(js.contains("E[\"A\"]") || js.contains("E.A"), "enums are compiled: {js}");
    assert!(map.contains("\"mappings\""), "{map}");

    let (out, map) = transpile_extension(
      String::from("ext:x/b.js").into(),
      String::from("const a = 1;").into(),
    )
    .unwrap();
    assert_eq!(out.as_str(), "const a = 1;");
    assert!(map.is_none());

    let error = transpile_typescript("ext:x/c.ts", "const = ;").unwrap_err();
    assert!(error.to_string().contains("ext:x/c.ts"), "{error}");
  }
}
