use crate::extractor::{
    declaration_id, extract_doc_comment, qualified_name, walk_tree, ExtractionResult, WalkAction,
};
use agent_core::types::{ReferenceKind, Symbol, SymbolKind, SymbolReference};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use tree_sitter::{Node, Parser};

pub enum Kind {
    Cpp,
    Go,
    TypeScript,
    Tsx,
    Dart,
}

impl Kind {
    fn language(&self) -> tree_sitter::Language {
        match self {
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::Dart => tree_sitter_dart::LANGUAGE.into(),
        }
    }
}

/// Parse with the grammar for `kind`.
///
/// Returns `None` only when tree-sitter cannot produce a tree. A valid module
/// with references and no declarations is still a successful parse.
pub fn extract(file_path: &str, content: &str, kind: Kind) -> Option<ExtractionResult> {
    let mut parser = Parser::new();
    let _ = parser.set_language(&kind.language());
    let tree = parser.parse(content, None)?;
    let mut symbols = Vec::new();
    let mut references = Vec::new();
    walk(
        file_path,
        content,
        tree.root_node(),
        &mut symbols,
        &mut references,
    );
    Some(ExtractionResult {
        symbols,
        references,
    })
}

fn walk(
    file_path: &str,
    content: &str,
    root: Node,
    symbols: &mut Vec<Symbol>,
    references: &mut Vec<SymbolReference>,
) {
    let mut seen_tests: HashSet<(Vec<String>, String, usize)> = HashSet::new();

    walk_tree(root, |node, scope, cur, cur_id| {
        let mut action = WalkAction::Same;
        let mut active_name = cur;
        let mut active_id = cur_id;
        let mut declared_name: Option<String> = None;
        let mut declared_id: Option<String> = None;

        if node.kind() == "variable_declarator" {
            if let (Some(name_node), Some(val_node)) = (
                node.child_by_field_name("name"),
                node.child_by_field_name("value"),
            ) {
                if matches!(
                    val_node.kind(),
                    "arrow_function" | "function_expression" | "function" | "generator_function"
                ) {
                    if let Some(name) = identifier_text(content, name_node) {
                        if !is_noise(name) {
                            let line = node.start_position().row + 1;
                            let owned = name.to_string();
                            let body = node_text(content, node);
                            let id = declaration_id(file_path, scope, &owned, node.start_byte());
                            symbols.push(Symbol {
                                id: id.clone(),
                                name: owned.clone(),
                                qualified_name: qualified_name(file_path, scope, &owned),
                                kind: SymbolKind::Function,
                                file_path: file_path.to_string(),
                                start_line: line,
                                end_line: node.end_position().row + 1,
                                signature: Some(first_line(body).to_string()),
                                doc_comment: extract_doc_comment(content, node, false),
                                fingerprint: fingerprint(body),
                            });
                            declared_name = Some(owned.clone());
                            declared_id = Some(id.clone());
                            action = WalkAction::EnterSymbol { name: owned, id };
                        }
                    }
                }
            }
        } else if let Some(kind) = symbol_kind(node.kind()) {
            if let Some(name) = symbol_name(content, node) {
                if !is_noise(name) {
                    let line = node.start_position().row + 1;
                    let owned = name.to_string();
                    let body = node_text(content, node);
                    let id = declaration_id(file_path, scope, &owned, node.start_byte());
                    symbols.push(Symbol {
                        id: id.clone(),
                        name: owned.clone(),
                        qualified_name: qualified_name(file_path, scope, &owned),
                        kind,
                        file_path: file_path.to_string(),
                        start_line: line,
                        end_line: node.end_position().row + 1,
                        signature: Some(first_line(body).to_string()),
                        doc_comment: extract_doc_comment(content, node, false),
                        fingerprint: fingerprint(body),
                    });
                    declared_name = Some(owned.clone());
                    declared_id = Some(id.clone());
                    action = WalkAction::EnterSymbol { name: owned, id };
                }
            }
        }

        if let Some(name) = declared_name.as_deref() {
            active_name = Some(name);
            active_id = declared_id.as_deref();
        }

        if node.kind() == "call_expression" {
            if let Some(name) = call_name(content, node) {
                if !is_noise(name) {
                    let line = node.start_position().row + 1;
                    let kind = if name == "test" || name == "group" {
                        ReferenceKind::Tests
                    } else {
                        ReferenceKind::Calls
                    };
                    references.push(SymbolReference {
                        source_file: file_path.to_string(),
                        source_symbol_name: active_name.map(str::to_string),
                        source_symbol_id: active_id.map(str::to_string),
                        target_name: name.to_string(),
                        target_symbol_id: None,
                        kind,
                        line_number: line,
                    });
                    if name == "test" || name == "group" {
                        if let Some(label) = first_quoted(node_text(content, node)) {
                            let key = (scope.to_vec(), label.to_string(), line);
                            if seen_tests.insert(key) {
                                symbols.push(Symbol {
                                    id: declaration_id(file_path, scope, label, node.start_byte()),
                                    name: label.to_string(),
                                    qualified_name: qualified_name(file_path, scope, label),
                                    kind: SymbolKind::Function,
                                    file_path: file_path.to_string(),
                                    start_line: line,
                                    end_line: line,
                                    signature: Some(format!("{name}('{label}')")),
                                    doc_comment: extract_doc_comment(content, node, false),
                                    fingerprint: fingerprint(label),
                                });
                            }
                        }
                    }
                }
            }
        }

        if is_import(node.kind()) {
            let text = node_text(content, node).trim();
            if !text.is_empty() {
                references.push(SymbolReference {
                    source_file: file_path.to_string(),
                    source_symbol_name: active_name.map(str::to_string),
                    source_symbol_id: active_id.map(str::to_string),
                    target_name: text.to_string(),
                    target_symbol_id: None,
                    kind: ReferenceKind::Imports,
                    line_number: node.start_position().row + 1,
                });
            }
        }

        action
    });
}

fn symbol_kind(kind: &str) -> Option<SymbolKind> {
    match kind {
        "function_definition"
        | "function_declaration"
        | "local_function_declaration"
        | "external_function_declaration" => Some(SymbolKind::Function),
        "method_declaration" | "method_definition" => Some(SymbolKind::Method),
        "class_specifier" | "class_declaration" | "class_definition" | "mixin_declaration" => {
            Some(SymbolKind::Class)
        }
        "struct_specifier" | "struct_declaration" => Some(SymbolKind::Struct),
        "enum_specifier" | "enum_declaration" => Some(SymbolKind::Enum),
        "interface_declaration" => Some(SymbolKind::Interface),
        "type_alias_declaration" | "type_spec" | "type_alias" => Some(SymbolKind::TypeAlias),
        "namespace_definition" => Some(SymbolKind::Module),
        _ => None,
    }
}

fn symbol_name<'a>(content: &'a str, node: Node) -> Option<&'a str> {
    for field in ["name", "declarator", "signature"] {
        if let Some(child) = node.child_by_field_name(field) {
            if let Some(found) = symbol_name(content, child) {
                return Some(found);
            }
        }
    }
    if matches!(
        node.kind(),
        "function_signature"
            | "method_signature"
            | "function_declarator"
            | "qualified_identifier"
            | "scoped_identifier"
            | "template_function"
            | "selector_expression"
            | "field_expression"
            | "namespace_identifier"
    ) {
        let mut cursor = node.walk();
        let mut last = None;
        for child in node.named_children(&mut cursor) {
            if let Some(found) = symbol_name(content, child) {
                last = Some(found);
            }
        }
        if last.is_some() {
            return last;
        }
    }
    identifier_text(content, node)
}

fn call_name<'a>(content: &'a str, node: Node) -> Option<&'a str> {
    let func = node.child_by_field_name("function")?;
    if let Some(field) = func.child_by_field_name("field") {
        if let Some(text) = identifier_text(content, field) {
            return Some(text);
        }
    }
    symbol_name(content, func)
}

fn identifier_text<'a>(content: &'a str, node: Node) -> Option<&'a str> {
    match node.kind() {
        "identifier" | "field_identifier" | "type_identifier" | "property_identifier" => {
            let text = node_text(content, node).trim();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        _ => None,
    }
}

fn node_text<'a>(content: &'a str, node: Node) -> &'a str {
    content.get(node.byte_range()).unwrap_or("")
}

fn is_import(kind: &str) -> bool {
    matches!(
        kind,
        "import_spec" | "import_statement" | "import_specification" | "preproc_include"
    )
}

fn is_noise(name: &str) -> bool {
    matches!(
        name,
        "if" | "for"
            | "while"
            | "switch"
            | "catch"
            | "sizeof"
            | "return"
            | "case"
            | "do"
            | "new"
            | "delete"
    )
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text).trim()
}

fn first_quoted(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'"' || bytes[index] == b'\'' {
            let quote = bytes[index];
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != quote {
                end += 1;
            }
            if end < bytes.len() {
                return text.get(start..end).filter(|value| !value.is_empty());
            }
        }
        index += 1;
    }
    None
}

fn fingerprint(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.trim().as_bytes());
    hex::encode(hasher.finalize())
}
