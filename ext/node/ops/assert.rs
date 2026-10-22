// Copyright 2018-2026 the Deno authors. MIT license.

//! The source text of a failed assertion, for its error message.
//!
//! Upstream lexed the line with deno_ast (swc). Simien depends on no deno_*
//! crate (APS 2.1), and the job needs only a tokenizer, so this file has its
//! own: it gives upstream's answer on every line in the tests below except a
//! regex containing `;` or `)`, where swc's lexer went wrong and this one
//! doesn't.

use simien_core::op2;
use simien_core::v8;
use simien_error::JsErrorBox;

#[op2]
pub fn op_node_get_first_expression<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  arg: v8::Local<v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
  if !arg.is_object() {
    return Err(JsErrorBox::type_error("Argument must be an object"));
  }

  let msg = v8::Exception::create_message(scope, arg);

  let source_line: String;
  if let Some(inner_source_line) = msg.get_source_line(scope) {
    source_line = inner_source_line.to_rust_string_lossy(scope);
  } else {
    return Ok(v8::undefined(scope).into());
  }

  let start_column = msg.get_start_column();
  let result = get_first_expression(&source_line, start_column);

  Ok(v8::String::new(scope, result).unwrap().into())
}

/// What the expression finder needs to tell tokens apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
  /// An identifier that isn't a reserved word.
  Ident,
  /// A reserved word (`if`, `new`, `true`, …): a member name, not a base.
  Keyword,
  Str,
  Num,
  Dot,
  /// `?.`
  QuestionDot,
  LBrack,
  RBrack,
  LParen,
  RParen,
  Semi,
  /// Any other punctuator, a template or a regex.
  Other,
}

type Lexed = (Kind, std::ops::Range<usize>);

const RESERVED: &[&str] = &[
  "await", "break", "case", "catch", "class", "const", "continue", "debugger",
  "default", "delete", "do", "else", "enum", "export", "extends", "false",
  "finally", "for", "function", "if", "import", "in", "instanceof", "new",
  "null", "return", "super", "switch", "this", "throw", "true", "try",
  "typeof", "var", "void", "while", "with", "yield",
];

fn is_ident_start(c: char) -> bool {
  c == '$' || c == '_' || c.is_alphabetic()
}

fn is_ident_part(c: char) -> bool {
  is_ident_start(c) || c.is_alphanumeric() || c == '\u{200c}' || c == '\u{200d}'
}

/// A tokenizer for one line of JavaScript, for finding the expression an
/// assertion failed in. It never fails: an unfinished string, template,
/// comment or regex runs to the end of the line, as a line cut from a longer
/// statement can. Comments are skipped.
fn lex(code: &str) -> Vec<Lexed> {
  let chars: Vec<(usize, char)> = code.char_indices().collect();
  let at = |i: usize| chars.get(i).map(|&(_, c)| c);
  let offset = |i: usize| chars.get(i).map_or(code.len(), |&(o, _)| o);
  let mut tokens: Vec<Lexed> = Vec::new();
  // Whether a `/` here starts a regex rather than dividing: after a value
  // (a name, a number, a string, `)` or `]`) it divides.
  let regex_allowed = |tokens: &[Lexed]| match tokens.last() {
    None => true,
    Some((kind, _)) => !matches!(
      kind,
      Kind::Ident | Kind::Num | Kind::Str | Kind::RParen | Kind::RBrack
    ),
  };
  let mut i = 0;
  while let Some(c) = at(i) {
    let start = i;
    if c.is_whitespace() {
      i += 1;
      continue;
    }
    // Comments.
    if c == '/' && at(i + 1) == Some('/') {
      break;
    }
    if c == '/' && at(i + 1) == Some('*') {
      i += 2;
      while at(i).is_some() && !(at(i) == Some('*') && at(i + 1) == Some('/')) {
        i += 1;
      }
      i = (i + 2).min(chars.len());
      continue;
    }
    let kind = if is_ident_start(c) || c == '\\' {
      while at(i).is_some_and(|c| is_ident_part(c) || c == '\\') {
        i += 1;
      }
      let word = &code[offset(start)..offset(i)];
      if RESERVED.contains(&word) { Kind::Keyword } else { Kind::Ident }
    } else if c.is_ascii_digit() || (c == '.' && at(i + 1).is_some_and(|c| c.is_ascii_digit())) {
      i += 1;
      while at(i).is_some_and(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_') {
        i += 1;
      }
      Kind::Num
    } else if c == '"' || c == '\'' {
      i += 1;
      while let Some(d) = at(i) {
        i += 1;
        if d == '\\' {
          i += 1;
        } else if d == c {
          break;
        }
      }
      Kind::Str
    } else if c == '`' {
      // A template, with `${}` nesting.
      i += 1;
      let mut depth = 0usize;
      while let Some(d) = at(i) {
        i += 1;
        match d {
          '\\' => i += 1,
          '$' if depth == 0 && at(i) == Some('{') => {
            depth = 1;
            i += 1;
          }
          '{' if depth > 0 => depth += 1,
          '}' if depth > 0 => depth -= 1,
          '`' if depth == 0 => break,
          _ => {}
        }
      }
      Kind::Other
    } else if c == '/' && regex_allowed(&tokens) {
      i += 1;
      let mut class = false;
      while let Some(d) = at(i) {
        i += 1;
        match d {
          '\\' => i += 1,
          '[' => class = true,
          ']' => class = false,
          '/' if !class => break,
          _ => {}
        }
      }
      while at(i).is_some_and(|c| c.is_ascii_alphabetic()) {
        i += 1;
      }
      Kind::Other
    } else if c == '?' && at(i + 1) == Some('.') && !at(i + 2).is_some_and(|c| c.is_ascii_digit()) {
      i += 2;
      Kind::QuestionDot
    } else {
      i += 1;
      match c {
        '.' => Kind::Dot,
        '[' => Kind::LBrack,
        ']' => Kind::RBrack,
        '(' => Kind::LParen,
        ')' => Kind::RParen,
        ';' => Kind::Semi,
        _ => Kind::Other,
      }
    };
    let end = i.min(chars.len());
    tokens.push((kind, offset(start)..offset(end)));
    i = end;
  }
  tokens
}

/// Tokens that represent member access operators: `.`, `[`, `]`, and `?.`
/// (one token here; swc read it as `?` then `.`).
fn is_member_access_token(kind: Kind) -> bool {
  matches!(kind, Kind::Dot | Kind::LBrack | Kind::RBrack | Kind::QuestionDot)
}

fn is_member_name_token(kind: Kind) -> bool {
  matches!(kind, Kind::Ident | Kind::Keyword | Kind::Str | Kind::Num)
}

fn is_ident_word(kind: Kind) -> bool {
  kind == Kind::Ident
}

fn utf16_to_utf8_offset(code: &str, offset_utf16: usize) -> Option<usize> {
  let mut current_utf16 = 0;

  for (offset_utf8, c) in code.char_indices() {
    if current_utf16 == offset_utf16 {
      return Some(offset_utf8);
    }

    current_utf16 += c.len_utf16();
    if current_utf16 > offset_utf16 {
      return None;
    }
  }

  (current_utf16 == offset_utf16).then_some(code.len())
}

/// Get the first expression in a code string at the start_column.
///
/// This mirrors Node.js's implementation
/// https://github.com/nodejs/node/blob/70f6b58ac655234435a99d72b857dd7b316d34bf/lib/internal/errors/error_source.js#L61-L142
fn get_first_expression(code: &str, original_start_col_index: usize) -> &str {
  let Some(start_index) = utf16_to_utf8_offset(code, original_start_col_index)
  else {
    return code;
  };

  let tokens = lex(code);

  let mut last_token: Option<&Lexed> = None;
  let mut first_member_access_name_token = None; // start position
  let mut terminating_col = None;
  let mut paren_lvl = 0;

  for lexed in &tokens {
    let (kind, range) = (lexed.0, &lexed.1);
    // Peek before the startColumn.
    if range.start < start_index {
      // There is a semicolon. This is a statement before the startColumn,
      // so reset the memo.
      if kind == Kind::Semi {
        first_member_access_name_token = None;
        last_token = Some(lexed);
        continue;
      }

      // Try to memo the member access expressions before the startColumn,
      // so that the returned source code contains more info:
      //   assert.ok(value)
      //          ^ startColumn
      // The member expression can also be like
      //   assert['ok'](value) or assert?.ok(value)
      //               ^ startColumn      ^ startColumn
      let is_member_access = is_member_access_token(kind);

      if is_member_access
        && first_member_access_name_token.is_none()
        && let Some((last_kind, last_range)) = last_token
        && is_ident_word(*last_kind)
      {
        first_member_access_name_token = Some(last_range.start);
      } else if !is_member_access && !is_member_name_token(kind) {
        // Reset the memo if it is not a simple member access.
        // For example: assert[(() => 'ok')()](value)
        //                                    ^ startColumn
        first_member_access_name_token = None;
      }

      last_token = Some(lexed);
      continue;
    }

    // Now after the startColumn, this must be an expression.
    if kind == Kind::LParen {
      paren_lvl += 1;
      continue;
    }

    if kind == Kind::RParen {
      paren_lvl -= 1;
      if paren_lvl == 0 {
        // A matched closing parenthesis found after the startColumn,
        // terminate here. Include the token.
        //   (assert.ok(false), assert.ok(true))
        //           ^ startColumn
        terminating_col = Some(range.start + 1);
        break;
      }
      continue;
    }

    if kind == Kind::Semi {
      // A semicolon found after the startColumn, terminate here.
      //   assert.ok(false); assert.ok(true));
      //          ^ startColumn
      terminating_col = Some(range.start);
      break;
    }
    // If no semicolon found after the startColumn. The string after the
    // startColumn must be the expression.
    //   assert.ok(false)
    //          ^ startColumn
  }

  let start = first_member_access_name_token.unwrap_or(start_index);
  let end = terminating_col.unwrap_or(code.len());
  code.get(start..end).unwrap_or(code)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_utf16_to_utf8_offset() {
    let text = "a😀汉b";
    assert_eq!(utf16_to_utf8_offset(text, 0), Some(0));
    assert_eq!(utf16_to_utf8_offset(text, 1), Some(1));
    assert_eq!(utf16_to_utf8_offset(text, 2), None);
    assert_eq!(utf16_to_utf8_offset(text, 3), Some(5));
    assert_eq!(utf16_to_utf8_offset(text, 4), Some(8));
    assert_eq!(utf16_to_utf8_offset(text, 5), Some(9));
    assert_eq!(utf16_to_utf8_offset(text, 6), None);
  }

  #[test]
  fn test_get_first_expression_with_multibyte_identifier() {
    assert_eq!(get_first_expression("汉汉.ok(false);", 3), "汉汉.ok(false)");
  }

  #[test]
  fn test_get_first_expression_with_invalid_utf16_offset() {
    let code = "😀; assert.ok(false);";
    assert_eq!(get_first_expression(code, 1), code);
  }

  /// Upstream's answers (deno_ast 0.53.3) for each line and start column,
  /// recorded with its code, except the regex line, which it got wrong
  /// (`assert.ok(/[`).
  #[test]
  fn finds_the_same_expressions_as_upstream() {
    let cases = [
      (7, r#"assert.ok(false);"#, "assert.ok(false)"),
      (7, r#"assert.ok(false)"#, "assert.ok(false)"),
      (9, r#"  assert.ok(value, 'msg');"#, "assert.ok(value, 'msg')"),
      (6, r#"assert['ok'](value)"#, "['ok'](value)"),
      (8, r#"assert?.ok(value)"#, "assert?.ok(value)"),
      (6, r#"a.b.c.ok(x)"#, "a.b.c.ok(x)"),
      (22, r#"assert[(() => 'ok')()](value)"#, "(value)"),
      (8, r#"(assert.ok(false), assert.ok(true))"#, "assert.ok(false)"),
      (14, r#"x = 1; assert.ok(false); assert.ok(true);"#, "assert.ok(false)"),
      (7, r#"assert.strictEqual(a, b)"#, "assert.strictEqual(a, b)"),
      (0, r#"assert(x === 1)"#, "assert(x === 1)"),
      (9, r#"foo(bar).ok(1)"#, "ok(1)"),
      (7, r#"assert.ok(`tpl ${a}`)"#, "assert.ok(`tpl ${a}`)"),
      (7, r#"assert.ok(/re/.test(s))"#, "assert.ok(/re/.test(s))"),
      (7, r#"assert.ok(a /* c */ )"#, "assert.ok(a /* c */ )"),
      (7, r#"assert.deepEqual(obj?.a, 1)"#, "assert.deepEqual(obj?.a, 1)"),
      (7, r#"assert.ok("#, "assert.ok("),
      (3, r#"汉汉.ok(false);"#, "汉汉.ok(false)"),
      (1, r#"😀; assert.ok(false);"#, "😀; assert.ok(false);"),
      (9, r#"t.assert.ok(value)"#, "t.assert.ok(value)"),
      (7, r#"assert.ok("a;b)", x);"#, "assert.ok(\"a;b)\", x)"),
      (7, r#"assert.ok(`a;${b}`); next();"#, "assert.ok(`a;${b}`)"),
      (7, r#"assert.ok(/[;)]/.test(s)); next();"#, "assert.ok(/[;)]/.test(s))"),
      (7, r#"assert.ok(a / b / c); next();"#, "assert.ok(a / b / c)"),
      (7, r#"assert.ok(x /* ); */); next();"#, "assert.ok(x /* ); */)"),
      (9, r#"assert?.[ok](value)"#, "assert?.[ok](value)"),
      (13, r#"a?.b?.assert.ok(value)"#, "a?.b?.assert.ok(value)"),
      (4, r#"obj.if(x)"#, "obj.if(x)"),
      (7, r#"assert.ok(1.5 > a); b"#, "assert.ok(1.5 > a)"),
      (11, r#"x.y; assert.ok(z)"#, ".ok(z)"),
      (0, r#"(a, b); assert.ok(c)"#, "(a, b)"),
      (7, r#"assert.ok(f(g(h)), i); j"#, "assert.ok(f(g(h)), i)"),
    ];
    for (column, line, expected) in cases {
      assert_eq!(get_first_expression(line, column), expected, "{line:?} at {column}");
    }
  }
}
