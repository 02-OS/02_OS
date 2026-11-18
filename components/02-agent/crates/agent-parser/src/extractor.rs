use agent_core::{
    types::{Language, ReferenceKind, Symbol, SymbolKind, SymbolReference},
    Result,
};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;
use tree_sitter::{Node, Parser};

pub struct ExtractionResult {
    pub symbols: Vec<Symbol>,
    pub references: Vec<SymbolReference>,
}

pub struct CodeExtractor;

/// `{file_path}::{scope}::{name}@{start_byte}`.
/// Scope is enclosing symbol names joined by `::`; an empty scope leaves that slot blank.
pub(crate) fn declaration_id(
    file_path: &str,
    scope: &[String],
    name: &str,
    start_byte: usize,
) -> String {
    format!("{file_path}::{}::{name}@{start_byte}", scope.join("::"))
}

pub(crate) fn qualified_name(file_path: &str, scope: &[String], name: &str) -> String {
    if scope.is_empty() {
        format!("{file_path}::{name}")
    } else {
        format!("{file_path}::{}::{name}", scope.join("::"))
    }
}

pub(crate) enum WalkAction {
    Same,
    PushScope(String),
    EnterSymbol { name: String, id: String },
}

/// Iterative `TreeCursor` traversal with an in-place scope/symbol stack.
/// Nested expressions never recurse on the native stack or clone per-node vectors.
pub(crate) fn walk_tree<'a>(
    root: Node<'a>,
    mut visit: impl FnMut(Node<'a>, &[String], Option<&str>, Option<&str>) -> WalkAction,
) {
    let mut cursor = root.walk();
    let mut scope: Vec<String> = Vec::new();
    let mut current: Vec<(String, String)> = Vec::new();
    let mut frames: Vec<(usize, usize)> = Vec::new();

    loop {
        let node = cursor.node();
        let prev_scope_len = scope.len();
        let prev_cur_len = current.len();
        let (cur_name, cur_id) = match current.last() {
            Some((name, id)) => (Some(name.as_str()), Some(id.as_str())),
            None => (None, None),
        };
        match visit(node, &scope, cur_name, cur_id) {
            WalkAction::Same => {}
            WalkAction::PushScope(name) => scope.push(name),
            WalkAction::EnterSymbol { name, id } => {
                scope.push(name.clone());
                current.push((name, id));
            }
        }

        if cursor.goto_first_child() {
            frames.push((prev_scope_len, prev_cur_len));
            continue;
        }
        scope.truncate(prev_scope_len);
        current.truncate(prev_cur_len);

        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if cursor.goto_parent() {
                if let Some((s_len, c_len)) = frames.pop() {
                    scope.truncate(s_len);
                    current.truncate(c_len);
                }
                continue;
            }
            return;
        }
    }
}

fn clean_comment_block(raw: &str, out: &mut Vec<String>) {
    let trimmed = raw.trim();
    if trimmed.starts_with("/*") {
        let inner = trimmed
            .strip_prefix("/**")
            .or_else(|| trimmed.strip_prefix("/*!"))
            .or_else(|| trimmed.strip_prefix("/*"))
            .unwrap_or(trimmed);
        let inner = inner.strip_suffix("*/").unwrap_or(inner);
        for line in inner.lines() {
            let line = line.trim();
            let line = line.strip_prefix('*').map(str::trim_start).unwrap_or(line);
            if !line.is_empty() {
                out.push(line.to_string());
            }
        }
        return;
    }
    for line in trimmed.lines() {
        let line = line.trim();
        let cleaned = if let Some(rest) = line.strip_prefix("///") {
            rest
        } else if let Some(rest) = line.strip_prefix("//!") {
            rest
        } else if let Some(rest) = line.strip_prefix("//") {
            rest
        } else if let Some(rest) = line.strip_prefix('#') {
            rest
        } else {
            line
        };
        let cleaned = cleaned.strip_prefix(' ').unwrap_or(cleaned).trim_end();
        if !cleaned.is_empty() {
            out.push(cleaned.to_string());
        }
    }
}

fn unquote_python_docstring(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let trimmed = trimmed
        .strip_prefix('r')
        .or_else(|| trimmed.strip_prefix('u'))
        .or_else(|| trimmed.strip_prefix('f'))
        .unwrap_or(trimmed);
    let inner = trimmed
        .strip_prefix("\"\"\"")
        .and_then(|s| s.strip_suffix("\"\"\""))
        .or_else(|| {
            trimmed
                .strip_prefix("'''")
                .and_then(|s| s.strip_suffix("'''"))
        })
        .or_else(|| trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
        })?;
    let lines: Vec<&str> = inner
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn extract_python_docstring(content: &str, node: Node<'_>) -> Option<String> {
    let body = node.child_by_field_name("body")?;
    let mut cursor = body.walk();
    let first = body.named_children(&mut cursor).next()?;
    if first.kind() != "expression_statement" {
        return None;
    }
    let mut expr_cursor = first.walk();
    let str_node = first.named_children(&mut expr_cursor).next()?;
    if str_node.kind() != "string" {
        return None;
    }
    let text = content.get(str_node.byte_range())?;
    unquote_python_docstring(text)
}

fn has_adjacent_comment(node: Node<'_>) -> bool {
    let mut next_start_row = node.start_position().row;
    let mut cursor = node;
    while let Some(prev) = cursor.prev_sibling() {
        if matches!(
            prev.kind(),
            "attribute_item" | "decorator" | "annotation" | "modifiers"
        ) && next_start_row.saturating_sub(prev.end_position().row) <= 1
        {
            next_start_row = prev.start_position().row;
            cursor = prev;
            continue;
        }
        return prev.kind().contains("comment")
            && next_start_row.saturating_sub(prev.end_position().row) <= 1;
    }
    false
}

pub(crate) fn extract_doc_comment(
    content: &str,
    node: Node<'_>,
    check_python_body: bool,
) -> Option<String> {
    if check_python_body {
        if let Some(doc) = extract_python_docstring(content, node) {
            return Some(doc);
        }
    }

    let mut anchor = node;
    while !has_adjacent_comment(anchor) {
        let Some(parent) = anchor.parent() else {
            break;
        };
        if matches!(
            parent.kind(),
            "decorated_definition"
                | "export_statement"
                | "lexical_declaration"
                | "variable_declaration"
        ) {
            anchor = parent;
        } else {
            break;
        }
    }

    let mut raw_blocks: Vec<&str> = Vec::new();
    let mut next_start_row = anchor.start_position().row;
    let mut cursor = anchor;
    while let Some(prev) = cursor.prev_sibling() {
        if matches!(
            prev.kind(),
            "attribute_item" | "decorator" | "annotation" | "modifiers"
        ) && next_start_row.saturating_sub(prev.end_position().row) <= 1
        {
            next_start_row = prev.start_position().row;
            cursor = prev;
            continue;
        }
        if prev.kind().contains("comment") {
            if next_start_row.saturating_sub(prev.end_position().row) > 1 {
                break;
            }
            if let Some(text) = content.get(prev.byte_range()) {
                raw_blocks.push(text);
            }
            next_start_row = prev.start_position().row;
            cursor = prev;
        } else {
            break;
        }
    }

    if raw_blocks.is_empty() {
        return None;
    }
    raw_blocks.reverse();
    let mut lines = Vec::new();
    for block in raw_blocks {
        clean_comment_block(block, &mut lines);
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn has_rust_test_attr(content: &str, node: Node<'_>) -> bool {
    let mut cursor = node;
    while let Some(prev) = cursor.prev_sibling() {
        if prev.kind().contains("comment") {
            cursor = prev;
            continue;
        }
        if prev.kind() != "attribute_item" {
            break;
        }
        let attr_text = content.get(prev.byte_range()).unwrap_or("");
        let stripped: String = attr_text
            .trim()
            .strip_prefix("#[")
            .and_then(|s| s.strip_suffix(']'))
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let attr_head = stripped.split('(').next().unwrap_or("");
        if attr_head == "test" || attr_head.ends_with("::test") {
            return true;
        }
        cursor = prev;
    }
    false
}

fn c_declarator_name<'a>(content: &'a str, mut node: Node<'a>) -> Option<&'a str> {
    for _ in 0..16 {
        match node.kind() {
            "identifier" | "field_identifier" => {
                let text = content.get(node.byte_range()).unwrap_or("").trim();
                return if text.is_empty() { None } else { Some(text) };
            }
            "parenthesized_declarator" => {
                let mut cursor = node.walk();
                node = node.named_children(&mut cursor).next()?;
            }
            _ => {
                if let Some(inner) = node.child_by_field_name("declarator") {
                    node = inner;
                } else {
                    break;
                }
            }
        }
    }
    None
}

fn c_call_target<'a>(content: &'a str, func_node: Node<'a>) -> &'a str {
    if func_node.kind() == "field_expression" {
        if let Some(field) = func_node.child_by_field_name("field") {
            let text = content.get(field.byte_range()).unwrap_or("").trim();
            if !text.is_empty() {
                return text;
            }
        }
    }
    let raw = content.get(func_node.byte_range()).unwrap_or("");
    let after_arrow = raw.rsplit("->").next().unwrap_or(raw);
    after_arrow.rsplit('.').next().unwrap_or(after_arrow).trim()
}

fn rust_scope_type_name<'a>(content: &'a str, mut node: Node<'a>) -> Option<&'a str> {
    for _ in 0..8 {
        if matches!(node.kind(), "type_identifier" | "identifier") {
            let text = content.get(node.byte_range()).unwrap_or("").trim();
            return if text.is_empty() { None } else { Some(text) };
        }
        let next = node
            .child_by_field_name("name")
            .or_else(|| node.child_by_field_name("type"));
        match next {
            Some(inner) if inner.id() != node.id() => node = inner,
            _ => break,
        }
    }
    None
}

/// Method and path calls store the identifier only. `SymbolReference` has no receiver field.
fn rust_call_target<'a>(content: &'a str, func_node: Node<'a>) -> &'a str {
    let node = if func_node.kind() == "generic_function" {
        func_node
            .child_by_field_name("function")
            .unwrap_or(func_node)
    } else {
        func_node
    };
    let selected = match node.kind() {
        "field_expression" => node
            .child_by_field_name("field")
            .and_then(|field| content.get(field.byte_range())),
        "scoped_identifier" | "scoped_type_identifier" => node
            .child_by_field_name("name")
            .and_then(|name| content.get(name.byte_range())),
        _ => None,
    };
    if let Some(text) = selected.map(str::trim).filter(|text| !text.is_empty()) {
        return text;
    }
    let raw = content.get(node.byte_range()).unwrap_or("");
    let after_path = raw.rsplit("::").next().unwrap_or(raw);
    after_path.rsplit('.').next().unwrap_or(after_path).trim()
}

impl CodeExtractor {
    pub fn extract(file_path: &str, content: &str, language: Language) -> Result<ExtractionResult> {
        let mut result = match language {
            Language::Rust => Self::extract_rust(file_path, content)?,
            Language::Python => Self::extract_python(file_path, content)?,
            Language::C => Self::extract_c_cpp(file_path, content)?,
            Language::Cpp => Self::extract_grammar(file_path, content, crate::grammar::Kind::Cpp)?,
            Language::Bash => Self::extract_bash(file_path, content)?,
            Language::JavaScript => Self::extract_js_ts(file_path, content)?,
            Language::TypeScript => {
                let kind = if Path::new(file_path)
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("tsx"))
                {
                    crate::grammar::Kind::Tsx
                } else {
                    crate::grammar::Kind::TypeScript
                };
                Self::extract_grammar(file_path, content, kind)?
            }
            Language::Dart => {
                Self::extract_grammar(file_path, content, crate::grammar::Kind::Dart)?
            }
            Language::Go => Self::extract_grammar(file_path, content, crate::grammar::Kind::Go)?,
            _ => Self::extract_fallback(file_path, content)?,
        };
        Self::retarget_test_references(&mut result.references);
        Ok(result)
    }

    fn extract_grammar(
        file_path: &str,
        content: &str,
        kind: crate::grammar::Kind,
    ) -> Result<ExtractionResult> {
        match crate::grammar::extract(file_path, content, kind) {
            Some(result) => Ok(result),
            // None means the parse itself failed, not that the module declared nothing.
            None => Self::extract_fallback(file_path, content),
        }
    }

    fn symbol_fingerprint(text: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(text.trim().as_bytes());
        hex::encode(hasher.finalize())
    }

    // -------------------------------------------------------------
    // RUST
    // -------------------------------------------------------------
    fn extract_rust(file_path: &str, content: &str) -> Result<ExtractionResult> {
        let mut parser = Parser::new();
        let _ = parser.set_language(&tree_sitter_rust::LANGUAGE.into());
        let tree = match parser.parse(content, None) {
            Some(t) => t,
            None => return Self::extract_fallback(file_path, content),
        };

        let mut symbols = Vec::new();
        let mut references = Vec::new();
        walk_tree(tree.root_node(), |node, scope, cur, cur_id| {
            Self::visit_rust_node(
                file_path,
                content,
                node,
                scope,
                cur,
                cur_id,
                &mut symbols,
                &mut references,
            )
        });

        Ok(ExtractionResult {
            symbols,
            references,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn visit_rust_node<'a>(
        file_path: &str,
        content: &'a str,
        node: Node<'a>,
        scope: &[String],
        current_symbol: Option<&str>,
        current_symbol_id: Option<&str>,
        symbols: &mut Vec<Symbol>,
        references: &mut Vec<SymbolReference>,
    ) -> WalkAction {
        let kind = node.kind();

        match kind {
            "function_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];
                    let sig = full_text.lines().next().unwrap_or(name).to_string();
                    let is_test = name.starts_with("test_") || has_rust_test_attr(content, node);
                    let id = declaration_id(file_path, scope, name, node.start_byte());

                    symbols.push(Symbol {
                        id: id.clone(),
                        name: name.to_string(),
                        qualified_name: qualified_name(file_path, scope, name),
                        kind: SymbolKind::Function,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: Some(sig),
                        doc_comment: extract_doc_comment(content, node, false),
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });

                    if is_test {
                        references.push(SymbolReference {
                            source_file: file_path.to_string(),
                            source_symbol_name: Some(name.to_string()),
                            source_symbol_id: Some(id.clone()),
                            target_name: name.to_string(),
                            target_symbol_id: None,
                            kind: ReferenceKind::Tests,
                            line_number: start_line,
                        });
                    }
                    return WalkAction::EnterSymbol {
                        name: name.to_string(),
                        id,
                    };
                }
            }
            "struct_item" | "enum_item" | "trait_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];
                    let sym_kind = match kind {
                        "struct_item" => SymbolKind::Struct,
                        "enum_item" => SymbolKind::Enum,
                        _ => SymbolKind::Trait,
                    };
                    let id = declaration_id(file_path, scope, name, node.start_byte());

                    symbols.push(Symbol {
                        id: id.clone(),
                        name: name.to_string(),
                        qualified_name: qualified_name(file_path, scope, name),
                        kind: sym_kind,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: full_text.lines().next().map(|s| s.to_string()),
                        doc_comment: extract_doc_comment(content, node, false),
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });
                    return WalkAction::EnterSymbol {
                        name: name.to_string(),
                        id,
                    };
                }
            }
            "mod_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    if !name.is_empty() {
                        return WalkAction::PushScope(name.to_string());
                    }
                }
            }
            "impl_item" => {
                if let Some(type_node) = node.child_by_field_name("type") {
                    if let Some(name) = rust_scope_type_name(content, type_node) {
                        return WalkAction::PushScope(name.to_string());
                    }
                }
            }
            "call_expression" => {
                if let Some(func_node) = node.child_by_field_name("function") {
                    let clean_target = rust_call_target(content, func_node);
                    let line = node.start_position().row + 1;

                    references.push(SymbolReference {
                        source_file: file_path.to_string(),
                        source_symbol_name: current_symbol.map(|s| s.to_string()),
                        source_symbol_id: current_symbol_id.map(str::to_string),
                        target_name: clean_target.to_string(),
                        target_symbol_id: None,
                        kind: ReferenceKind::Calls,
                        line_number: line,
                    });
                }
            }
            "use_declaration" => {
                let use_text = &content[node.byte_range()];
                let line = node.start_position().row + 1;
                let target = use_text
                    .trim_start_matches("use ")
                    .trim_end_matches(';')
                    .trim();
                references.push(SymbolReference {
                    source_file: file_path.to_string(),
                    source_symbol_name: None,
                    source_symbol_id: None,
                    target_name: target.to_string(),
                    target_symbol_id: None,
                    kind: ReferenceKind::Imports,
                    line_number: line,
                });
            }
            _ => {}
        }

        WalkAction::Same
    }

    // -------------------------------------------------------------
    // PYTHON
    // -------------------------------------------------------------
    fn extract_python(file_path: &str, content: &str) -> Result<ExtractionResult> {
        let mut parser = Parser::new();
        let _ = parser.set_language(&tree_sitter_python::LANGUAGE.into());
        let tree = match parser.parse(content, None) {
            Some(t) => t,
            None => return Self::extract_fallback(file_path, content),
        };

        let mut symbols = Vec::new();
        let mut references = Vec::new();
        walk_tree(tree.root_node(), |node, scope, cur, cur_id| {
            Self::visit_python_node(
                file_path,
                content,
                node,
                scope,
                cur,
                cur_id,
                &mut symbols,
                &mut references,
            )
        });

        Ok(ExtractionResult {
            symbols,
            references,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn visit_python_node<'a>(
        file_path: &str,
        content: &'a str,
        node: Node<'a>,
        scope: &[String],
        current_symbol: Option<&str>,
        current_symbol_id: Option<&str>,
        symbols: &mut Vec<Symbol>,
        references: &mut Vec<SymbolReference>,
    ) -> WalkAction {
        let kind = node.kind();

        match kind {
            "function_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];
                    let sig = full_text.lines().next().unwrap_or(name).to_string();
                    let id = declaration_id(file_path, scope, name, node.start_byte());

                    symbols.push(Symbol {
                        id: id.clone(),
                        name: name.to_string(),
                        qualified_name: qualified_name(file_path, scope, name),
                        kind: SymbolKind::Function,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: Some(sig),
                        doc_comment: extract_doc_comment(content, node, true),
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });

                    if name.starts_with("test_") {
                        references.push(SymbolReference {
                            source_file: file_path.to_string(),
                            source_symbol_name: Some(name.to_string()),
                            source_symbol_id: Some(id.clone()),
                            target_name: name.to_string(),
                            target_symbol_id: None,
                            kind: ReferenceKind::Tests,
                            line_number: start_line,
                        });
                    }
                    return WalkAction::EnterSymbol {
                        name: name.to_string(),
                        id,
                    };
                }
            }
            "class_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];
                    let id = declaration_id(file_path, scope, name, node.start_byte());

                    symbols.push(Symbol {
                        id: id.clone(),
                        name: name.to_string(),
                        qualified_name: qualified_name(file_path, scope, name),
                        kind: SymbolKind::Class,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: full_text.lines().next().map(|s| s.to_string()),
                        doc_comment: extract_doc_comment(content, node, true),
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });
                    return WalkAction::EnterSymbol {
                        name: name.to_string(),
                        id,
                    };
                }
            }
            "call" => {
                if let Some(func_node) = node.child_by_field_name("function") {
                    let call_name = &content[func_node.byte_range()];
                    let clean = call_name.split('.').next_back().unwrap_or(call_name).trim();
                    let line = node.start_position().row + 1;

                    references.push(SymbolReference {
                        source_file: file_path.to_string(),
                        source_symbol_name: current_symbol.map(|s| s.to_string()),
                        source_symbol_id: current_symbol_id.map(str::to_string),
                        target_name: clean.to_string(),
                        target_symbol_id: None,
                        kind: ReferenceKind::Calls,
                        line_number: line,
                    });
                }
            }
            "import_statement" | "import_from_statement" => {
                let text = &content[node.byte_range()];
                let line = node.start_position().row + 1;
                references.push(SymbolReference {
                    source_file: file_path.to_string(),
                    source_symbol_name: None,
                    source_symbol_id: None,
                    target_name: text.trim().to_string(),
                    target_symbol_id: None,
                    kind: ReferenceKind::Imports,
                    line_number: line,
                });
            }
            _ => {}
        }

        WalkAction::Same
    }

    // -------------------------------------------------------------
    // C / C++
    // -------------------------------------------------------------
    fn extract_c_cpp(file_path: &str, content: &str) -> Result<ExtractionResult> {
        let mut parser = Parser::new();
        let _ = parser.set_language(&tree_sitter_c::LANGUAGE.into());
        let tree = match parser.parse(content, None) {
            Some(t) => t,
            None => return Self::extract_fallback(file_path, content),
        };

        let mut symbols = Vec::new();
        let mut references = Vec::new();
        walk_tree(tree.root_node(), |node, scope, cur, cur_id| {
            Self::visit_c_node(
                file_path,
                content,
                node,
                scope,
                cur,
                cur_id,
                &mut symbols,
                &mut references,
            )
        });

        Ok(ExtractionResult {
            symbols,
            references,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn visit_c_node<'a>(
        file_path: &str,
        content: &'a str,
        node: Node<'a>,
        scope: &[String],
        current_symbol: Option<&str>,
        current_symbol_id: Option<&str>,
        symbols: &mut Vec<Symbol>,
        references: &mut Vec<SymbolReference>,
    ) -> WalkAction {
        let kind = node.kind();

        match kind {
            "function_definition" => {
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    if let Some(func_name) = c_declarator_name(content, declarator) {
                        let start_line = node.start_position().row + 1;
                        let end_line = node.end_position().row + 1;
                        let full_text = &content[node.byte_range()];
                        let id = declaration_id(file_path, scope, func_name, node.start_byte());

                        symbols.push(Symbol {
                            id: id.clone(),
                            name: func_name.to_string(),
                            qualified_name: qualified_name(file_path, scope, func_name),
                            kind: SymbolKind::Function,
                            file_path: file_path.to_string(),
                            start_line,
                            end_line,
                            signature: full_text.lines().next().map(|s| s.to_string()),
                            doc_comment: extract_doc_comment(content, node, false),
                            fingerprint: Self::symbol_fingerprint(full_text),
                        });
                        return WalkAction::EnterSymbol {
                            name: func_name.to_string(),
                            id,
                        };
                    }
                }
            }
            "struct_specifier" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];

                    symbols.push(Symbol {
                        id: declaration_id(file_path, scope, name, node.start_byte()),
                        name: name.to_string(),
                        qualified_name: qualified_name(file_path, scope, name),
                        kind: SymbolKind::Struct,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: full_text.lines().next().map(|s| s.to_string()),
                        doc_comment: extract_doc_comment(content, node, false),
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });
                    return WalkAction::PushScope(name.to_string());
                }
            }
            "call_expression" => {
                if let Some(func_node) = node.child_by_field_name("function") {
                    let target = c_call_target(content, func_node);
                    let line = node.start_position().row + 1;
                    if !target.is_empty() {
                        references.push(SymbolReference {
                            source_file: file_path.to_string(),
                            source_symbol_name: current_symbol.map(|s| s.to_string()),
                            source_symbol_id: current_symbol_id.map(str::to_string),
                            target_name: target.to_string(),
                            target_symbol_id: None,
                            kind: ReferenceKind::Calls,
                            line_number: line,
                        });
                    }
                }
            }
            "preproc_include" => {
                let text = &content[node.byte_range()];
                let line = node.start_position().row + 1;
                references.push(SymbolReference {
                    source_file: file_path.to_string(),
                    source_symbol_name: None,
                    source_symbol_id: None,
                    target_name: text.trim().to_string(),
                    target_symbol_id: None,
                    kind: ReferenceKind::Imports,
                    line_number: line,
                });
            }
            _ => {}
        }

        WalkAction::Same
    }

    // -------------------------------------------------------------
    // BASH
    // -------------------------------------------------------------
    fn extract_bash(file_path: &str, content: &str) -> Result<ExtractionResult> {
        let mut parser = Parser::new();
        let _ = parser.set_language(&tree_sitter_bash::LANGUAGE.into());
        let tree = match parser.parse(content, None) {
            Some(t) => t,
            None => return Self::extract_fallback(file_path, content),
        };

        let mut symbols = Vec::new();
        let mut references = Vec::new();
        walk_tree(tree.root_node(), |node, scope, cur, cur_id| {
            Self::visit_bash_node(
                file_path,
                content,
                node,
                scope,
                cur,
                cur_id,
                &mut symbols,
                &mut references,
            )
        });

        Ok(ExtractionResult {
            symbols,
            references,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn visit_bash_node<'a>(
        file_path: &str,
        content: &'a str,
        node: Node<'a>,
        scope: &[String],
        current_symbol: Option<&str>,
        current_symbol_id: Option<&str>,
        symbols: &mut Vec<Symbol>,
        references: &mut Vec<SymbolReference>,
    ) -> WalkAction {
        let kind = node.kind();

        match kind {
            "function_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];
                    let id = declaration_id(file_path, scope, name, node.start_byte());

                    symbols.push(Symbol {
                        id: id.clone(),
                        name: name.to_string(),
                        qualified_name: qualified_name(file_path, scope, name),
                        kind: SymbolKind::Function,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: Some(format!("function {}()", name)),
                        doc_comment: extract_doc_comment(content, node, false),
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });
                    return WalkAction::EnterSymbol {
                        name: name.to_string(),
                        id,
                    };
                }
            }
            "variable_assignment" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];

                    symbols.push(Symbol {
                        id: declaration_id(file_path, scope, name, node.start_byte()),
                        name: name.to_string(),
                        qualified_name: qualified_name(file_path, scope, name),
                        kind: SymbolKind::Variable,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: Some(full_text.lines().next().unwrap_or(name).to_string()),
                        doc_comment: extract_doc_comment(content, node, false),
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });
                    return WalkAction::PushScope(name.to_string());
                }
            }
            "command" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let cmd_name = &content[name_node.byte_range()];
                    let line = node.start_position().row + 1;

                    references.push(SymbolReference {
                        source_file: file_path.to_string(),
                        source_symbol_name: current_symbol.map(|s| s.to_string()),
                        source_symbol_id: current_symbol_id.map(str::to_string),
                        target_name: cmd_name.to_string(),
                        target_symbol_id: None,
                        kind: ReferenceKind::Calls,
                        line_number: line,
                    });
                }
            }
            _ => {}
        }

        WalkAction::Same
    }

    // -------------------------------------------------------------
    // JAVASCRIPT / TYPESCRIPT
    // -------------------------------------------------------------
    fn extract_js_ts(file_path: &str, content: &str) -> Result<ExtractionResult> {
        let mut parser = Parser::new();
        let _ = parser.set_language(&tree_sitter_javascript::LANGUAGE.into());
        let tree = match parser.parse(content, None) {
            Some(t) => t,
            None => return Self::extract_fallback(file_path, content),
        };

        let mut symbols = Vec::new();
        let mut references = Vec::new();
        walk_tree(tree.root_node(), |node, scope, cur, cur_id| {
            Self::visit_js_node(
                file_path,
                content,
                node,
                scope,
                cur,
                cur_id,
                &mut symbols,
                &mut references,
            )
        });

        Ok(ExtractionResult {
            symbols,
            references,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn visit_js_node<'a>(
        file_path: &str,
        content: &'a str,
        node: Node<'a>,
        scope: &[String],
        current_symbol: Option<&str>,
        current_symbol_id: Option<&str>,
        symbols: &mut Vec<Symbol>,
        references: &mut Vec<SymbolReference>,
    ) -> WalkAction {
        let kind = node.kind();

        match kind {
            "function_declaration" | "method_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];
                    let sym_kind = if kind == "method_definition" {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    };
                    let id = declaration_id(file_path, scope, name, node.start_byte());

                    symbols.push(Symbol {
                        id: id.clone(),
                        name: name.to_string(),
                        qualified_name: qualified_name(file_path, scope, name),
                        kind: sym_kind,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: full_text.lines().next().map(|s| s.to_string()),
                        doc_comment: extract_doc_comment(content, node, false),
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });
                    return WalkAction::EnterSymbol {
                        name: name.to_string(),
                        id,
                    };
                }
            }
            "variable_declarator" => {
                if let (Some(name_node), Some(val_node)) = (
                    node.child_by_field_name("name"),
                    node.child_by_field_name("value"),
                ) {
                    if name_node.kind() == "identifier"
                        && matches!(
                            val_node.kind(),
                            "arrow_function"
                                | "function_expression"
                                | "function"
                                | "generator_function"
                        )
                    {
                        let name = &content[name_node.byte_range()];
                        let start_line = node.start_position().row + 1;
                        let end_line = node.end_position().row + 1;
                        let full_text = &content[node.byte_range()];
                        let id = declaration_id(file_path, scope, name, node.start_byte());
                        symbols.push(Symbol {
                            id: id.clone(),
                            name: name.to_string(),
                            qualified_name: qualified_name(file_path, scope, name),
                            kind: SymbolKind::Function,
                            file_path: file_path.to_string(),
                            start_line,
                            end_line,
                            signature: full_text.lines().next().map(|s| s.to_string()),
                            doc_comment: extract_doc_comment(content, node, false),
                            fingerprint: Self::symbol_fingerprint(full_text),
                        });
                        return WalkAction::EnterSymbol {
                            name: name.to_string(),
                            id,
                        };
                    }
                }
            }
            "class_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];
                    let id = declaration_id(file_path, scope, name, node.start_byte());

                    symbols.push(Symbol {
                        id: id.clone(),
                        name: name.to_string(),
                        qualified_name: qualified_name(file_path, scope, name),
                        kind: SymbolKind::Class,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: full_text.lines().next().map(|s| s.to_string()),
                        doc_comment: extract_doc_comment(content, node, false),
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });
                    return WalkAction::EnterSymbol {
                        name: name.to_string(),
                        id,
                    };
                }
            }
            "call_expression" => {
                if let Some(func_node) = node.child_by_field_name("function") {
                    let target = &content[func_node.byte_range()];
                    let clean = target.split('.').next_back().unwrap_or(target).trim();
                    let line = node.start_position().row + 1;

                    let is_test_runner = clean == "test" || clean == "it" || clean == "describe";

                    references.push(SymbolReference {
                        source_file: file_path.to_string(),
                        source_symbol_name: current_symbol.map(|s| s.to_string()),
                        source_symbol_id: current_symbol_id.map(str::to_string),
                        target_name: clean.to_string(),
                        target_symbol_id: None,
                        kind: if is_test_runner {
                            ReferenceKind::Tests
                        } else {
                            ReferenceKind::Calls
                        },
                        line_number: line,
                    });
                }
            }
            "import_statement" => {
                let text = &content[node.byte_range()];
                let line = node.start_position().row + 1;
                references.push(SymbolReference {
                    source_file: file_path.to_string(),
                    source_symbol_name: None,
                    source_symbol_id: None,
                    target_name: text.trim().to_string(),
                    target_symbol_id: None,
                    kind: ReferenceKind::Imports,
                    line_number: line,
                });
            }
            _ => {}
        }

        WalkAction::Same
    }

    fn extract_fallback(file_path: &str, content: &str) -> Result<ExtractionResult> {
        static FALLBACK_RE: OnceLock<Regex> = OnceLock::new();
        let func_re = FALLBACK_RE.get_or_init(|| {
            Regex::new(r"^(?:pub\s+)?(?:async\s+)?def\s+([A-Za-z0-9_]+)|^fn\s+([A-Za-z0-9_]+)|function\s+([A-Za-z0-9_]+)")
                .expect("fallback function regex")
        });

        let mut symbols = Vec::new();
        let references = Vec::new();
        let mut byte = 0usize;
        for (idx, line) in content.split('\n').enumerate() {
            let line_num = idx + 1;
            if let Some(caps) = func_re.captures(line.trim()) {
                let name = caps.get(1).or_else(|| caps.get(2)).or_else(|| caps.get(3));
                if let Some(m) = name {
                    let sym_name = m.as_str();
                    let leading = line.len() - line.trim_start().len();
                    symbols.push(Symbol {
                        id: declaration_id(file_path, &[], sym_name, byte + leading),
                        name: sym_name.to_string(),
                        qualified_name: qualified_name(file_path, &[], sym_name),
                        kind: SymbolKind::Function,
                        file_path: file_path.to_string(),
                        start_line: line_num,
                        end_line: line_num,
                        signature: Some(line.trim().to_string()),
                        doc_comment: None,
                        fingerprint: Self::symbol_fingerprint(line),
                    });
                }
            }
            byte += line.len() + 1;
        }

        Ok(ExtractionResult {
            symbols,
            references,
        })
    }

    /// Point test references at the code under test instead of the test function itself.
    fn retarget_test_references(references: &mut [SymbolReference]) {
        let mut test_ids: HashSet<(&str, &str)> = HashSet::new();
        let mut test_names: HashSet<(&str, &str)> = HashSet::new();
        for reference in references.iter() {
            if reference.kind != ReferenceKind::Tests {
                continue;
            }
            if let Some(id) = reference.source_symbol_id.as_deref() {
                test_ids.insert((reference.source_file.as_str(), id));
            }
            if let Some(name) = reference.source_symbol_name.as_deref() {
                test_names.insert((reference.source_file.as_str(), name));
            }
        }

        let calls: Vec<SymbolReference> = references
            .iter()
            .filter(|r| {
                if r.kind != ReferenceKind::Calls || is_test_helper(&r.target_name) {
                    return false;
                }
                let by_id = r
                    .source_symbol_id
                    .as_deref()
                    .is_some_and(|id| test_ids.contains(&(r.source_file.as_str(), id)));
                let by_name = r.source_symbol_name.as_deref().is_some_and(|name| {
                    name.starts_with("test_")
                        || test_names.contains(&(r.source_file.as_str(), name))
                });
                by_id || by_name
            })
            .cloned()
            .collect();

        for reference in references.iter_mut() {
            if reference.kind != ReferenceKind::Tests {
                continue;
            }
            let Some(source) = reference.source_symbol_name.clone() else {
                continue;
            };
            if reference.target_name != source {
                continue;
            }
            let own_calls: Vec<&SymbolReference> = calls
                .iter()
                .filter(|call| {
                    call.source_file == reference.source_file
                        && call.source_symbol_name.as_deref() == Some(source.as_str())
                        && call.source_symbol_id == reference.source_symbol_id
                })
                .collect();
            if let Some(target) = preferred_test_target(&source, &own_calls) {
                if let Some(call) = own_calls.iter().find(|call| call.target_name == target) {
                    reference.line_number = call.line_number;
                }
                reference.target_name = target;
            }
        }
    }
}

fn preferred_test_target(test_name: &str, calls: &[&SymbolReference]) -> Option<String> {
    let stripped = test_name
        .strip_prefix("test_")
        .filter(|name| !name.is_empty())
        .unwrap_or(test_name);
    let targets: Vec<&str> = calls
        .iter()
        .map(|call| call.target_name.as_str())
        .filter(|target| !is_test_helper(target))
        .collect();

    if let Some(exact) = targets.iter().copied().find(|target| *target == stripped) {
        return Some(exact.to_string());
    }
    if let Some(related) = targets.iter().copied().find(|target| {
        target.starts_with(stripped) || (stripped.starts_with(*target) && target.len() >= 4)
    }) {
        return Some(related.to_string());
    }
    if !stripped.is_empty() && stripped != test_name {
        return Some(stripped.to_string());
    }
    targets.first().map(|target| (*target).to_string())
}

fn is_test_helper(name: &str) -> bool {
    matches!(
        name,
        "assert"
            | "assert_eq"
            | "assert_ne"
            | "assert_matches"
            | "debug_assert"
            | "debug_assert_eq"
            | "unwrap"
            | "expect"
            | "panic"
            | "todo"
            | "unimplemented"
            | "dbg"
            | "println"
            | "print"
            | "eprintln"
            | "format"
            | "write"
            | "writeln"
            | "vec"
            | "Some"
            | "None"
            | "Ok"
            | "Err"
            | "clone"
            | "to_string"
            | "to_owned"
            | "new"
            | "default"
            | "String"
            | "matches"
            | "pytest"
            | "raises"
    ) || name.starts_with("assert")
}
