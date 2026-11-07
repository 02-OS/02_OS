use agent_core::{
    types::{Language, ReferenceKind, Symbol, SymbolKind, SymbolReference},
    Result,
};
use regex::Regex;
use sha2::{Digest, Sha256};
use tree_sitter::{Node, Parser};

pub struct ExtractionResult {
    pub symbols: Vec<Symbol>,
    pub references: Vec<SymbolReference>,
}

pub struct CodeExtractor;

fn floor_char_boundary(content: &str, mut index: usize) -> usize {
    if index > content.len() {
        index = content.len();
    }
    while index > 0 && !content.is_char_boundary(index) {
        index -= 1;
    }
    index
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
                let kind = if file_path.ends_with(".tsx") {
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
        let root = tree.root_node();

        Self::walk_rust_node(
            file_path,
            content,
            root,
            &mut symbols,
            &mut references,
            None,
        );

        Ok(ExtractionResult {
            symbols,
            references,
        })
    }

    fn walk_rust_node(
        file_path: &str,
        content: &str,
        node: Node,
        symbols: &mut Vec<Symbol>,
        references: &mut Vec<SymbolReference>,
        current_symbol: Option<&str>,
    ) {
        let kind = node.kind();
        let mut active_symbol = current_symbol;

        match kind {
            "function_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];
                    let sig = full_text.lines().next().unwrap_or(name).to_string();

                    // Check for #[test] attribute or test_ prefix without heap allocations
                    let mut has_test_attr = false;
                    let mut prev_sibling = node.prev_sibling();
                    while let Some(sibling) = prev_sibling {
                        if sibling.kind() == "attribute_item" {
                            let attr_text = &content[sibling.byte_range()];
                            if attr_text.contains("test") {
                                has_test_attr = true;
                                break;
                            }
                            prev_sibling = sibling.prev_sibling();
                        } else {
                            break;
                        }
                    }
                    let pre_start =
                        floor_char_boundary(content, node.start_byte().saturating_sub(256));
                    let pre_slice = &content[pre_start..node.start_byte()];
                    let is_test = name.starts_with("test_")
                        || full_text.contains("#[test]")
                        || has_test_attr
                        || pre_slice.contains("#[test]");

                    let sym = Symbol {
                        id: format!("{}::{}::{}", file_path, name, start_line),
                        name: name.to_string(),
                        qualified_name: format!("{}::{}", file_path, name),
                        kind: SymbolKind::Function,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: Some(sig),
                        doc_comment: None,
                        fingerprint: Self::symbol_fingerprint(full_text),
                    };
                    symbols.push(sym);
                    active_symbol = Some(name);

                    if is_test {
                        // Mark test reference
                        references.push(SymbolReference {
                            source_file: file_path.to_string(),
                            source_symbol_name: Some(name.to_string()),
                            target_name: name.to_string(),
                            target_symbol_id: None,
                            kind: ReferenceKind::Tests,
                            line_number: start_line,
                        });
                    }
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

                    symbols.push(Symbol {
                        id: format!("{}::{}::{}", file_path, name, start_line),
                        name: name.to_string(),
                        qualified_name: format!("{}::{}", file_path, name),
                        kind: sym_kind,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: full_text.lines().next().map(|s| s.to_string()),
                        doc_comment: None,
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });
                    active_symbol = Some(name);
                }
            }
            "call_expression" => {
                if let Some(func_node) = node.child_by_field_name("function") {
                    let target_name = &content[func_node.byte_range()];
                    let line = node.start_position().row + 1;
                    let clean_target = target_name
                        .rsplit("::")
                        .next()
                        .unwrap_or(target_name)
                        .trim();

                    references.push(SymbolReference {
                        source_file: file_path.to_string(),
                        source_symbol_name: current_symbol.map(|s| s.to_string()),
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
                    target_name: target.to_string(),
                    target_symbol_id: None,
                    kind: ReferenceKind::Imports,
                    line_number: line,
                });
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::walk_rust_node(
                file_path,
                content,
                child,
                symbols,
                references,
                active_symbol,
            );
        }
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
        let root = tree.root_node();

        Self::walk_python_node(
            file_path,
            content,
            root,
            &mut symbols,
            &mut references,
            None,
        );

        Ok(ExtractionResult {
            symbols,
            references,
        })
    }

    fn walk_python_node(
        file_path: &str,
        content: &str,
        node: Node,
        symbols: &mut Vec<Symbol>,
        references: &mut Vec<SymbolReference>,
        current_symbol: Option<&str>,
    ) {
        let kind = node.kind();
        let mut active_symbol = current_symbol;

        match kind {
            "function_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];
                    let sig = full_text.lines().next().unwrap_or(name).to_string();

                    symbols.push(Symbol {
                        id: format!("{}::{}::{}", file_path, name, start_line),
                        name: name.to_string(),
                        qualified_name: format!("{}::{}", file_path, name),
                        kind: SymbolKind::Function,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: Some(sig),
                        doc_comment: None,
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });
                    active_symbol = Some(name);

                    if name.starts_with("test_") {
                        references.push(SymbolReference {
                            source_file: file_path.to_string(),
                            source_symbol_name: Some(name.to_string()),
                            target_name: name.to_string(),
                            target_symbol_id: None,
                            kind: ReferenceKind::Tests,
                            line_number: start_line,
                        });
                    }
                }
            }
            "class_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];

                    symbols.push(Symbol {
                        id: format!("{}::{}::{}", file_path, name, start_line),
                        name: name.to_string(),
                        qualified_name: format!("{}::{}", file_path, name),
                        kind: SymbolKind::Class,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: full_text.lines().next().map(|s| s.to_string()),
                        doc_comment: None,
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });
                    active_symbol = Some(name);
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
                    target_name: text.trim().to_string(),
                    target_symbol_id: None,
                    kind: ReferenceKind::Imports,
                    line_number: line,
                });
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::walk_python_node(
                file_path,
                content,
                child,
                symbols,
                references,
                active_symbol,
            );
        }
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
        let root = tree.root_node();

        Self::walk_c_node(
            file_path,
            content,
            root,
            &mut symbols,
            &mut references,
            None,
        );

        Ok(ExtractionResult {
            symbols,
            references,
        })
    }

    fn walk_c_node(
        file_path: &str,
        content: &str,
        node: Node,
        symbols: &mut Vec<Symbol>,
        references: &mut Vec<SymbolReference>,
        current_symbol: Option<&str>,
    ) {
        let kind = node.kind();
        let mut active_symbol = current_symbol;

        match kind {
            "function_definition" => {
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    let decl_text = &content[declarator.byte_range()];
                    let func_name = decl_text
                        .split('(')
                        .next()
                        .unwrap_or(decl_text)
                        .trim()
                        .trim_start_matches('*');
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];

                    symbols.push(Symbol {
                        id: format!("{}::{}::{}", file_path, func_name, start_line),
                        name: func_name.to_string(),
                        qualified_name: format!("{}::{}", file_path, func_name),
                        kind: SymbolKind::Function,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: full_text.lines().next().map(|s| s.to_string()),
                        doc_comment: None,
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });
                    active_symbol = Some(func_name);
                }
            }
            "struct_specifier" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];

                    symbols.push(Symbol {
                        id: format!("{}::{}::{}", file_path, name, start_line),
                        name: name.to_string(),
                        qualified_name: format!("{}::{}", file_path, name),
                        kind: SymbolKind::Struct,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: full_text.lines().next().map(|s| s.to_string()),
                        doc_comment: None,
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });
                }
            }
            "call_expression" => {
                if let Some(func_node) = node.child_by_field_name("function") {
                    let target = &content[func_node.byte_range()];
                    let line = node.start_position().row + 1;
                    references.push(SymbolReference {
                        source_file: file_path.to_string(),
                        source_symbol_name: current_symbol.map(|s| s.to_string()),
                        target_name: target.to_string(),
                        target_symbol_id: None,
                        kind: ReferenceKind::Calls,
                        line_number: line,
                    });
                }
            }
            "preproc_include" => {
                let text = &content[node.byte_range()];
                let line = node.start_position().row + 1;
                references.push(SymbolReference {
                    source_file: file_path.to_string(),
                    source_symbol_name: None,
                    target_name: text.trim().to_string(),
                    target_symbol_id: None,
                    kind: ReferenceKind::Imports,
                    line_number: line,
                });
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::walk_c_node(
                file_path,
                content,
                child,
                symbols,
                references,
                active_symbol,
            );
        }
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
        let root = tree.root_node();

        Self::walk_bash_node(
            file_path,
            content,
            root,
            &mut symbols,
            &mut references,
            None,
        );

        Ok(ExtractionResult {
            symbols,
            references,
        })
    }

    fn walk_bash_node(
        file_path: &str,
        content: &str,
        node: Node,
        symbols: &mut Vec<Symbol>,
        references: &mut Vec<SymbolReference>,
        current_symbol: Option<&str>,
    ) {
        let kind = node.kind();
        let mut active_symbol = current_symbol;

        match kind {
            "function_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];

                    symbols.push(Symbol {
                        id: format!("{}::{}::{}", file_path, name, start_line),
                        name: name.to_string(),
                        qualified_name: format!("{}::{}", file_path, name),
                        kind: SymbolKind::Function,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: Some(format!("function {}()", name)),
                        doc_comment: None,
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });
                    active_symbol = Some(name);
                }
            }
            "variable_assignment" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];

                    symbols.push(Symbol {
                        id: format!("{}::{}::{}", file_path, name, start_line),
                        name: name.to_string(),
                        qualified_name: format!("{}::{}", file_path, name),
                        kind: SymbolKind::Variable,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: Some(full_text.lines().next().unwrap_or(name).to_string()),
                        doc_comment: None,
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });
                }
            }
            "command" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let cmd_name = &content[name_node.byte_range()];
                    let line = node.start_position().row + 1;

                    references.push(SymbolReference {
                        source_file: file_path.to_string(),
                        source_symbol_name: current_symbol.map(|s| s.to_string()),
                        target_name: cmd_name.to_string(),
                        target_symbol_id: None,
                        kind: ReferenceKind::Calls,
                        line_number: line,
                    });
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::walk_bash_node(
                file_path,
                content,
                child,
                symbols,
                references,
                active_symbol,
            );
        }
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
        let root = tree.root_node();

        Self::walk_js_node(
            file_path,
            content,
            root,
            &mut symbols,
            &mut references,
            None,
        );

        Ok(ExtractionResult {
            symbols,
            references,
        })
    }

    fn walk_js_node(
        file_path: &str,
        content: &str,
        node: Node,
        symbols: &mut Vec<Symbol>,
        references: &mut Vec<SymbolReference>,
        current_symbol: Option<&str>,
    ) {
        let kind = node.kind();
        let mut active_symbol = current_symbol;

        match kind {
            "function_declaration" | "method_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];

                    symbols.push(Symbol {
                        id: format!("{}::{}::{}", file_path, name, start_line),
                        name: name.to_string(),
                        qualified_name: format!("{}::{}", file_path, name),
                        kind: SymbolKind::Function,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: full_text.lines().next().map(|s| s.to_string()),
                        doc_comment: None,
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });
                    active_symbol = Some(name);
                }
            }
            "class_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &content[name_node.byte_range()];
                    let start_line = node.start_position().row + 1;
                    let end_line = node.end_position().row + 1;
                    let full_text = &content[node.byte_range()];

                    symbols.push(Symbol {
                        id: format!("{}::{}::{}", file_path, name, start_line),
                        name: name.to_string(),
                        qualified_name: format!("{}::{}", file_path, name),
                        kind: SymbolKind::Class,
                        file_path: file_path.to_string(),
                        start_line,
                        end_line,
                        signature: full_text.lines().next().map(|s| s.to_string()),
                        doc_comment: None,
                        fingerprint: Self::symbol_fingerprint(full_text),
                    });
                    active_symbol = Some(name);
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
                    target_name: text.trim().to_string(),
                    target_symbol_id: None,
                    kind: ReferenceKind::Imports,
                    line_number: line,
                });
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::walk_js_node(
                file_path,
                content,
                child,
                symbols,
                references,
                active_symbol,
            );
        }
    }

    fn extract_fallback(file_path: &str, content: &str) -> Result<ExtractionResult> {
        let mut symbols = Vec::new();
        let references = Vec::new();

        let func_re = Regex::new(r"^(?:pub\s+)?(?:async\s+)?def\s+([A-Za-z0-9_]+)|^fn\s+([A-Za-z0-9_]+)|function\s+([A-Za-z0-9_]+)").unwrap();

        for (idx, line) in content.lines().enumerate() {
            let line_num = idx + 1;
            if let Some(caps) = func_re.captures(line.trim()) {
                let name = caps.get(1).or_else(|| caps.get(2)).or_else(|| caps.get(3));
                if let Some(m) = name {
                    let sym_name = m.as_str();
                    symbols.push(Symbol {
                        id: format!("{}::{}::{}", file_path, sym_name, line_num),
                        name: sym_name.to_string(),
                        qualified_name: format!("{}::{}", file_path, sym_name),
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
        }

        Ok(ExtractionResult {
            symbols,
            references,
        })
    }

    /// Point test references at the code under test instead of the test function itself.
    fn retarget_test_references(references: &mut [SymbolReference]) {
        let calls: Vec<SymbolReference> = references
            .iter()
            .filter(|r| {
                r.kind == ReferenceKind::Calls
                    && r.source_symbol_name
                        .as_deref()
                        .is_some_and(|s| s.starts_with("test_"))
                    && !is_test_helper(&r.target_name)
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
