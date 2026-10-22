// Copyright 2018-2026 the Simien authors. MIT license.

//! Which names a CommonJS module exports, for importing it from ESM.
//!
//! `import { x } from "a-commonjs-package"` works in Node because Node reads
//! the module's source for the names it assigns (cjs-module-lexer).
//! simien_node_resolver's `NodeCodeTranslator` builds the ESM wrapper and asks
//! a `CjsCodeAnalyzer` for those names. Upstream's analyzer is deno_ast's;
//! Simien depends on no deno_* crate (APS 2.1), so this one walks oxc's AST
//! for the shapes cjs-module-lexer recognizes:
//! - `exports.x = …`, `exports["x"] = …`, `module.exports.x = …`;
//! - `Object.defineProperty(exports, "x", …)` (and on `module.exports`);
//! - `module.exports = { a, b: …, c() {}, ...require("y") }`;
//! - `module.exports = require("y")` and the transpilers' star re-exports
//!   (`__exportStar(require("y"), exports)`, `__export(require("y"))`);
//! - `module.exports = require("y").member` (a member re-export).
//!
//! esbuild's `0 && (module.exports = { … })` annotation is an assignment like
//! any other. A file with `import`/`export` syntax, or a `.mjs` file, is ESM.

use std::borrow::Cow;
use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::Argument;
use oxc_ast::ast::AssignmentExpression;
use oxc_ast::ast::AssignmentTarget;
use oxc_ast::ast::CallExpression;
use oxc_ast::ast::Declaration;
use oxc_ast::ast::Expression;
use oxc_ast::ast::ModuleDeclaration;
use oxc_ast::ast::ObjectPropertyKind;
use oxc_ast::ast::Program;
use oxc_ast::ast::PropertyKey;
use oxc_ast_visit::Visit;
use oxc_ast_visit::walk;
use oxc_parser::Parser;
use oxc_span::SourceType;
use simien_error::JsErrorBox;
use simien_node_resolver::analyze::CjsAnalysis;
use simien_node_resolver::analyze::CjsAnalysisExports;
use simien_node_resolver::analyze::CjsCodeAnalyzer;
use simien_node_resolver::analyze::CjsMemberReExport;
use simien_node_resolver::analyze::EsmAnalysisMode;
use url::Url;

/// The names a module exports and the modules it re-exports from.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Exports {
  pub exports: Vec<String>,
  pub reexports: Vec<String>,
  pub member_reexports: Vec<(String, String)>,
}

/// What a source is, and what it exports.
#[derive(Debug, PartialEq, Eq)]
pub enum Analysis {
  Esm(Exports),
  Cjs(Exports),
}

/// Reads `source` (at `path`, for its extension) and finds its exports.
pub fn analyze(path: &Path, source: &str) -> Result<Analysis, JsErrorBox> {
  let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
  let allocator = Allocator::default();
  let source_type = match extension {
    "mjs" => SourceType::mjs(),
    "cjs" => SourceType::cjs(),
    _ => SourceType::unambiguous(),
  };
  let parsed = Parser::new(&allocator, source, source_type).parse();
  if parsed.fatal_error {
    let message = parsed
      .diagnostics
      .first()
      .map(ToString::to_string)
      .unwrap_or_else(|| "could not be parsed".into());
    return Err(JsErrorBox::new(
      "SyntaxError",
      format!("{}: {message}", path.display()),
    ));
  }
  let program = &parsed.program;
  let is_esm = extension == "mjs"
    || (extension != "cjs"
      && program.body.iter().any(|s| s.is_module_declaration()));
  if is_esm {
    return Ok(Analysis::Esm(esm_exports(program)));
  }
  let mut finder = CjsFinder::default();
  finder.visit_program(program);
  let mut exports = finder.exports;
  dedup(&mut exports.exports);
  dedup(&mut exports.reexports);
  Ok(Analysis::Cjs(exports))
}

fn dedup(names: &mut Vec<String>) {
  let mut seen = std::collections::HashSet::new();
  names.retain(|n| seen.insert(n.clone()));
}

/// An ES module's export names and `export * from` sources.
fn esm_exports(program: &Program) -> Exports {
  let mut exports = Exports::default();
  for statement in &program.body {
    let Some(declaration) = statement.as_module_declaration() else {
      continue;
    };
    match declaration {
      ModuleDeclaration::ExportAllDeclaration(all) => match &all.exported {
        Some(name) => exports.exports.push(name.name().to_string()),
        None => exports.reexports.push(all.source.value.to_string()),
      },
      ModuleDeclaration::ExportDefaultDeclaration(_) => {
        exports.exports.push("default".into())
      }
      ModuleDeclaration::ExportNamedDeclaration(named) => {
        for specifier in &named.specifiers {
          exports.exports.push(specifier.exported.name().to_string());
        }
      }
      ModuleDeclaration::ExportFromDeclaration(from) => {
        for specifier in &from.specifiers {
          exports.exports.push(specifier.exported.name().to_string());
        }
      }
      ModuleDeclaration::ExportDeclaration(export) => match &export.declaration {
        Declaration::VariableDeclaration(variables) => {
          for declarator in &variables.declarations {
            for id in declarator.id.get_binding_identifiers() {
              exports.exports.push(id.name.to_string());
            }
          }
        }
        Declaration::FunctionDeclaration(function) => {
          if let Some(id) = &function.id {
            exports.exports.push(id.name.to_string());
          }
        }
        Declaration::ClassDeclaration(class) => {
          if let Some(id) = &class.id {
            exports.exports.push(id.name.to_string());
          }
        }
        _ => {}
      },
      _ => {}
    }
  }
  dedup(&mut exports.exports);
  exports
}

/// The string a `require("…")` call loads.
fn required(expression: &Expression) -> Option<String> {
  let Expression::CallExpression(call) = expression.without_parentheses() else {
    return None;
  };
  required_call(call)
}

fn required_call(call: &CallExpression) -> Option<String> {
  let Expression::Identifier(callee) = &call.callee else {
    return None;
  };
  if callee.name != "require" || call.arguments.len() != 1 {
    return None;
  }
  match &call.arguments[0] {
    Argument::StringLiteral(s) => Some(s.value.to_string()),
    Argument::TemplateLiteral(t) if t.expressions.is_empty() => {
      t.quasis.first().map(|q| q.value.raw.to_string())
    }
    _ => None,
  }
}

/// Whether `expression` is `exports` or `module.exports`.
fn is_exports(expression: &Expression) -> bool {
  match expression.without_parentheses() {
    Expression::Identifier(id) => id.name == "exports",
    Expression::StaticMemberExpression(member) => {
      matches!(&member.object, Expression::Identifier(m) if m.name == "module")
        && member.property.name == "exports"
    }
    _ => false,
  }
}

fn property_name(key: &PropertyKey) -> Option<String> {
  match key {
    PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
    PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
    _ => None,
  }
}

#[derive(Default)]
struct CjsFinder {
  exports: Exports,
}

impl CjsFinder {
  fn module_exports_value(&mut self, value: &Expression) {
    let value = value.without_parentheses();
    if let Some(specifier) = required(value) {
      self.exports.reexports.push(specifier);
      return;
    }
    if let Expression::StaticMemberExpression(member) = value
      && let Some(specifier) = required(&member.object)
    {
      self
        .exports
        .member_reexports
        .push((specifier, member.property.name.to_string()));
      return;
    }
    if let Expression::ObjectExpression(object) = value {
      for property in &object.properties {
        match property {
          ObjectPropertyKind::ObjectProperty(p) => {
            if let Some(name) = property_name(&p.key) {
              self.exports.exports.push(name);
            }
          }
          ObjectPropertyKind::SpreadProperty(spread) => {
            if let Some(specifier) = required(&spread.argument) {
              self.exports.reexports.push(specifier);
            }
          }
        }
      }
    }
  }
}

impl<'a> Visit<'a> for CjsFinder {
  fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
    match &it.left {
      // exports.x = … / module.exports.x = …
      AssignmentTarget::StaticMemberExpression(member)
        if is_exports(&member.object) =>
      {
        self.exports.exports.push(member.property.name.to_string());
      }
      // exports["x"] = …
      AssignmentTarget::ComputedMemberExpression(member)
        if is_exports(&member.object) =>
      {
        if let Expression::StringLiteral(s) = &member.expression {
          self.exports.exports.push(s.value.to_string());
        }
      }
      _ => {}
    }
    // module.exports = …
    if let AssignmentTarget::StaticMemberExpression(member) = &it.left
      && matches!(&member.object, Expression::Identifier(m) if m.name == "module")
      && member.property.name == "exports"
    {
      self.module_exports_value(&it.right);
    }
    walk::walk_assignment_expression(self, it);
  }

  fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
    let callee_name = match &it.callee {
      Expression::Identifier(id) => Some(id.name.as_str()),
      Expression::StaticMemberExpression(m) => Some(m.property.name.as_str()),
      _ => None,
    };
    match callee_name {
      // Object.defineProperty(exports, "x", …)
      Some("defineProperty")
        if matches!(&it.callee, Expression::StaticMemberExpression(m)
          if matches!(&m.object, Expression::Identifier(o) if o.name == "Object"))
          && it.arguments.len() >= 2 =>
      {
        if let Some(target) = it.arguments[0].as_expression()
          && is_exports(target)
          && let Argument::StringLiteral(name) = &it.arguments[1]
        {
          self.exports.exports.push(name.value.to_string());
        }
      }
      // __exportStar(require("y"), exports) / __export(require("y"))
      Some("__exportStar" | "__export") => {
        if let Some(first) = it.arguments.first().and_then(|a| a.as_expression())
          && let Some(specifier) = required(first)
        {
          self.exports.reexports.push(specifier);
        }
      }
      _ => {}
    }
    walk::walk_call_expression(self, it);
  }

}

/// [`CjsCodeAnalyzer`] for simien_node_resolver's translator.
#[derive(Default)]
pub struct OxcCjsCodeAnalyzer;

fn to_analysis_exports(exports: Exports) -> CjsAnalysisExports {
  CjsAnalysisExports {
    exports: exports.exports,
    reexports: exports.reexports,
    member_reexports: exports
      .member_reexports
      .into_iter()
      .map(|(specifier, member)| CjsMemberReExport { specifier, member })
      .collect(),
  }
}

#[async_trait::async_trait(?Send)]
impl CjsCodeAnalyzer for OxcCjsCodeAnalyzer {
  async fn analyze_cjs<'a>(
    &self,
    specifier: &Url,
    maybe_source: Option<Cow<'a, str>>,
    esm_analysis_mode: EsmAnalysisMode,
  ) -> Result<CjsAnalysis<'a>, JsErrorBox> {
    let path = specifier
      .to_file_path()
      .map_err(|_| JsErrorBox::generic(format!("{specifier} is not a file")))?;
    let source = match maybe_source {
      Some(source) => source,
      None => Cow::Owned(
        std::fs::read_to_string(&path)
          .map_err(|e| JsErrorBox::generic(format!("{}: {e}", path.display())))?,
      ),
    };
    Ok(match analyze(&path, &source)? {
      Analysis::Esm(exports) => {
        let exports = matches!(
          esm_analysis_mode,
          EsmAnalysisMode::SourceImportsAndExports
        )
        .then(|| to_analysis_exports(exports));
        CjsAnalysis::Esm(source, exports)
      }
      Analysis::Cjs(exports) => CjsAnalysis::Cjs(to_analysis_exports(exports)),
    })
  }

  /// Upstream narrows a member re-export to the names on that member; this
  /// answers "unknown", so the wrapper advertises none for the member shape
  /// (the default export still carries everything).
  async fn analyze_cjs_member_props<'a>(
    &self,
    _specifier: &Url,
    _maybe_source: Option<Cow<'a, str>>,
    _member: &str,
  ) -> Result<Option<Vec<String>>, JsErrorBox> {
    Ok(None)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn cjs(source: &str) -> Exports {
    match analyze(Path::new("/m.js"), source).unwrap() {
      Analysis::Cjs(exports) => exports,
      Analysis::Esm(_) => panic!("read as ESM: {source}"),
    }
  }

  #[test]
  fn finds_what_cjs_module_lexer_finds() {
    assert_eq!(
      cjs("exports.a = 1; module.exports.b = 2; exports['c'] = 3;").exports,
      ["a", "b", "c"]
    );
    assert_eq!(
      cjs("Object.defineProperty(exports, '__esModule', { value: true }); Object.defineProperty(module.exports, 'd', { get() {} });").exports,
      ["__esModule", "d"]
    );
    let object = cjs("const a = 1; module.exports = { a, b: 2, c() {}, 'd': 4, ...require('./e') };");
    assert_eq!(object.exports, ["a", "b", "c", "d"]);
    assert_eq!(object.reexports, ["./e"]);
    assert_eq!(cjs("module.exports = require('./f');").reexports, ["./f"]);
    assert_eq!(
      cjs("__exportStar(require('./g'), exports); tslib.__exportStar(require('./h'), exports);").reexports,
      ["./g", "./h"]
    );
    // esbuild's annotation for Node's lexer.
    assert_eq!(cjs("0 && (module.exports = { i, j });").exports, ["i", "j"]);
    assert_eq!(
      cjs("module.exports = require('./k').l;").member_reexports,
      [("./k".to_string(), "l".to_string())]
    );
    // Inside a function or a branch, as the lexer does.
    assert_eq!(cjs("if (x) { exports.m = 1 } (function(){ exports.n = 2 })()").exports, ["m", "n"]);
  }

  #[test]
  fn tells_esm_from_cjs() {
    match analyze(Path::new("/m.js"), "export const a = 1; export default 2; export * from './b'; export { c as d } from './e';").unwrap() {
      Analysis::Esm(exports) => {
        assert_eq!(exports.exports, ["a", "default", "d"]);
        assert_eq!(exports.reexports, ["./b"]);
      }
      other => panic!("{other:?}"),
    }
    assert!(matches!(analyze(Path::new("/m.mjs"), "const a = 1;").unwrap(), Analysis::Esm(_)));
    assert!(matches!(analyze(Path::new("/m.cjs"), "exports.a = 1;").unwrap(), Analysis::Cjs(_)));
  }
}
