// Copyright 2018-2026 the Deno authors. MIT license.

//! `node:module`'s `stripTypeScriptTypes`.
//!
//! Upstream used deno_ast (swc). Simien depends on no deno_* crate (APS 2.1)
//! and parses JavaScript with oxc, so this is oxc's TypeScript transform:
//! - `transform` mode compiles TypeScript to JavaScript, with an inline
//!   source map when asked;
//! - `strip` mode refuses what needs more than removing types (`enum`, a
//!   namespace with values, parameter properties, `import =`, `export =`), as
//!   Node does, then removes the types. Upstream replaced them with spaces so
//!   every position stayed put; here the output is printed again, so columns
//!   can move (simienapp/runtime#46).

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::Declaration;
use oxc_ast::ast::FormalParameter;
use oxc_ast::ast::Statement;
use oxc_ast::ast::TSEnumDeclaration;
use oxc_ast::ast::TSExportAssignment;
use oxc_ast::ast::TSImportEqualsDeclaration;
use oxc_ast::ast::TSNamespaceDeclaration;
use oxc_ast::ast::TSNamespaceDeclarationBody;
use oxc_ast_visit::Visit;
use oxc_ast_visit::walk;
use oxc_codegen::Codegen;
use oxc_codegen::CodegenOptions;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::TransformOptions;
use oxc_transformer::Transformer;
use simien_core::op2;
use simien_error::JsErrorBox;

/// The file name Node reports for this API's input.
const FILE: &str = "stripTypeScriptTypes.ts";

#[op2]
#[string]
pub fn op_node_strip_typescript_types(
  #[string] code: String,
  #[string] mode: &str,
  source_map: bool,
) -> Result<String, JsErrorBox> {
  let allocator = Allocator::default();
  let parsed = Parser::new(&allocator, &code, SourceType::ts()).parse();
  if let Some(error) = parsed.diagnostics.first() {
    return Err(JsErrorBox::new("SyntaxError", error.to_string()));
  }
  let mut program = parsed.program;

  if mode == "strip" {
    let mut check = StripOnly(None);
    check.visit_program(&program);
    if let Some(what) = check.0 {
      return Err(JsErrorBox::new(
        "SyntaxError",
        format!("TypeScript {what} is not supported in strip-only mode"),
      ));
    }
  }

  let scoping = SemanticBuilder::new().build(&program).semantic.into_scoping();
  let transformed = Transformer::new(&allocator, Path::new(FILE), &TransformOptions::default())
    .build_with_scoping(scoping, &mut program);
  if let Some(error) = transformed.diagnostics.first() {
    return Err(JsErrorBox::generic(error.to_string()));
  }

  let printed = Codegen::new()
    .with_options(CodegenOptions {
      source_map_path: (mode != "strip" && source_map).then(|| FILE.into()),
      ..CodegenOptions::default()
    })
    .build(&program);
  Ok(match printed.map {
    Some(map) => format!(
      "{}\n//# sourceMappingURL={}\n",
      printed.code.trim_end(),
      map.to_data_url()
    ),
    None => printed.code,
  })
}

/// The first TypeScript construct that takes more than removing types:
/// what Node's strip-only mode refuses.
struct StripOnly(Option<&'static str>);

impl<'a> Visit<'a> for StripOnly {
  fn visit_ts_enum_declaration(&mut self, it: &TSEnumDeclaration<'a>) {
    if !it.declare {
      self.0.get_or_insert("enum");
    }
  }

  fn visit_ts_namespace_declaration(&mut self, it: &TSNamespaceDeclaration<'a>) {
    if !it.declare && instantiated(&it.body) {
      self.0.get_or_insert("namespace");
    }
  }

  fn visit_formal_parameter(&mut self, it: &FormalParameter<'a>) {
    if it.accessibility.is_some() || it.readonly || it.r#override {
      self.0.get_or_insert("parameter property");
    }
    walk::walk_formal_parameter(self, it);
  }

  fn visit_ts_import_equals_declaration(&mut self, it: &TSImportEqualsDeclaration<'a>) {
    if !it.import_kind.is_type() {
      self.0.get_or_insert("import =");
    }
  }

  fn visit_ts_export_assignment(&mut self, _: &TSExportAssignment<'a>) {
    self.0.get_or_insert("export =");
  }
}

/// Whether a namespace body produces a value at runtime: anything but types,
/// `declare`d declarations and namespaces that don't.
fn instantiated(body: &TSNamespaceDeclarationBody) -> bool {
  match body {
    TSNamespaceDeclarationBody::TSNamespaceDeclaration(inner) => {
      !inner.declare && instantiated(&inner.body)
    }
    TSNamespaceDeclarationBody::TSModuleBlock(block) => {
      block.body.iter().any(statement_instantiates)
    }
  }
}

fn statement_instantiates(statement: &Statement) -> bool {
  match statement {
    Statement::TSTypeAliasDeclaration(_)
    | Statement::TSInterfaceDeclaration(_)
    | Statement::EmptyStatement(_) => false,
    Statement::TSNamespaceDeclaration(inner) => {
      !inner.declare && instantiated(&inner.body)
    }
    Statement::VariableDeclaration(it) => !it.declare,
    Statement::FunctionDeclaration(it) => !it.declare,
    Statement::ClassDeclaration(it) => !it.declare,
    Statement::TSEnumDeclaration(it) => !it.declare,
    Statement::ExportDeclaration(it) => declaration_instantiates(&it.declaration),
    Statement::ExportNamedDeclaration(it) => !it.export_kind.is_type(),
    _ => true,
  }
}

fn declaration_instantiates(declaration: &Declaration) -> bool {
  match declaration {
    Declaration::TSTypeAliasDeclaration(_) | Declaration::TSInterfaceDeclaration(_) => false,
    Declaration::TSNamespaceDeclaration(inner) => {
      !inner.declare && instantiated(&inner.body)
    }
    Declaration::VariableDeclaration(it) => !it.declare,
    Declaration::FunctionDeclaration(it) => !it.declare,
    Declaration::ClassDeclaration(it) => !it.declare,
    Declaration::TSEnumDeclaration(it) => !it.declare,
    _ => true,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn strip(code: &str, mode: &str) -> Result<String, String> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, SourceType::ts()).parse();
    assert!(parsed.diagnostics.is_empty(), "{code}");
    let mut check = StripOnly(None);
    check.visit_program(&parsed.program);
    match (mode, check.0) {
      ("strip", Some(what)) => Err(what.to_string()),
      _ => Ok(String::new()),
    }
  }

  #[test]
  fn strip_mode_refuses_what_node_refuses() {
    for (code, what) in [
      ("enum A { B }", "enum"),
      ("namespace N { export const x = 1 }", "namespace"),
      ("class C { constructor(private x: number) {} }", "parameter property"),
      ("import fs = require('fs')", "import ="),
      ("export = 1", "export ="),
    ] {
      assert_eq!(strip(code, "strip"), Err(what.to_string()), "{code}");
      assert_eq!(strip(code, "transform"), Ok(String::new()), "{code}");
    }
    for code in [
      "declare enum A { B }",
      "namespace N { export type T = string; interface I {} }",
      "declare namespace N { const x: number }",
      "const x: number = 1; function f<T>(a: T): T { return a as T }",
      "import type { A } from 'a'; export type { A }",
      "class C { private x = 1; constructor(x: number) {} }",
    ] {
      assert_eq!(strip(code, "strip"), Ok(String::new()), "{code}");
    }
  }
}
