use agent_core::types::{Language, ReferenceKind, SymbolKind};
use agent_parser::CodeExtractor;

#[test]
fn test_rust_parsing() {
    let code = r#"
        pub struct AuthService;

        impl AuthService {
            pub fn rotate_refresh_token(user: &str) -> bool {
                verify_session(user);
                true
            }
        }

        #[test]
        fn test_rotate() {
            rotate_refresh_token("alice");
        }
    "#;

    let res = CodeExtractor::extract("src/auth.rs", code, Language::Rust).unwrap();
    assert!(res
        .symbols
        .iter()
        .any(|s| s.name == "AuthService" && s.kind == SymbolKind::Struct));
    assert!(res
        .symbols
        .iter()
        .any(|s| s.name == "rotate_refresh_token" && s.kind == SymbolKind::Function));
    assert!(res.symbols.iter().any(|s| s.name == "test_rotate"));

    // Check calls and tests
    assert!(res
        .references
        .iter()
        .any(|r| r.target_name == "verify_session" && r.kind == ReferenceKind::Calls));
    assert!(res.references.iter().any(|r| {
        r.kind == ReferenceKind::Tests
            && r.source_symbol_name.as_deref() == Some("test_rotate")
            && r.target_name == "rotate_refresh_token"
    }));
    assert!(res
        .references
        .iter()
        .all(|r| !(r.kind == ReferenceKind::Tests && r.target_name == "test_rotate")));
}

#[test]
fn test_python_parsing() {
    let code = r#"
import os

class TokenManager:
    def refresh(self, token):
        validate_signature(token)
        return True

def test_refresh_token():
    tm = TokenManager()
    tm.refresh("abc")
    "#;

    let res = CodeExtractor::extract("token_mgr.py", code, Language::Python).unwrap();
    assert!(res
        .symbols
        .iter()
        .any(|s| s.name == "TokenManager" && s.kind == SymbolKind::Class));
    assert!(res.symbols.iter().any(|s| s.name == "refresh"));
    assert!(res.symbols.iter().any(|s| s.name == "test_refresh_token"));
    assert!(res
        .references
        .iter()
        .any(|r| r.target_name == "validate_signature"));
}

#[test]
fn test_bash_parsing() {
    let script = r#"
#!/usr/bin/env bash

function build_iso() {
    echo "Building"
    mkarchiso -v /profile
}
    "#;

    let res = CodeExtractor::extract("build.sh", script, Language::Bash).unwrap();
    assert!(res
        .symbols
        .iter()
        .any(|s| s.name == "build_iso" && s.kind == SymbolKind::Function));
    assert!(res
        .references
        .iter()
        .any(|r| r.target_name == "mkarchiso" && r.kind == ReferenceKind::Calls));
}

#[test]
fn test_go_cpp_typescript_and_dart_parsing() {
    let go = "package main\nfunc helper() {}\nfunc main() { helper() }\n";
    let go_res = CodeExtractor::extract("main.go", go, Language::Go).unwrap();
    assert!(go_res.symbols.iter().any(|s| s.name == "helper"));
    assert!(go_res.symbols.iter().any(|s| s.name == "main"));
    assert!(go_res.references.iter().any(|r| {
        r.kind == ReferenceKind::Calls
            && r.target_name == "helper"
            && r.source_symbol_name.as_deref() == Some("main")
    }));

    let cpp = "namespace demo {\nvoid parse_header() {}\nint main() { parse_header(); }\n}\n";
    let cpp_res = CodeExtractor::extract("parse.cpp", cpp, Language::Cpp).unwrap();
    assert!(cpp_res.symbols.iter().any(|s| s.name == "parse_header"));
    assert!(cpp_res
        .references
        .iter()
        .any(|r| { r.kind == ReferenceKind::Calls && r.target_name == "parse_header" }));

    let ts = "interface User { name: string }\ntype Id = string;\nenum Color { Red }\nfunction greet(id: Id) { return id; }\n";
    let ts_res = CodeExtractor::extract("app.ts", ts, Language::TypeScript).unwrap();
    assert!(ts_res
        .symbols
        .iter()
        .any(|s| s.name == "User" && s.kind == SymbolKind::Interface));
    assert!(ts_res
        .symbols
        .iter()
        .any(|s| s.name == "Id" && s.kind == SymbolKind::TypeAlias));
    assert!(ts_res
        .symbols
        .iter()
        .any(|s| s.name == "Color" && s.kind == SymbolKind::Enum));
    assert!(ts_res
        .symbols
        .iter()
        .any(|s| s.name == "greet" && s.kind == SymbolKind::Function));

    let dart = "void helper() {}\nvoid main() { helper(); }\nvoid runSuite() {\n  group('suite', () {\n    test('adds', () { helper(); });\n  });\n}\n";
    let dart_res = CodeExtractor::extract("app.dart", dart, Language::Dart).unwrap();
    assert!(dart_res.symbols.iter().any(|s| s.name == "helper"));
    assert!(dart_res
        .references
        .iter()
        .any(|r| { r.kind == ReferenceKind::Calls && r.target_name == "helper" }));
    assert!(dart_res
        .references
        .iter()
        .any(|r| r.kind == ReferenceKind::Tests && r.target_name == "test"));
    assert!(dart_res.symbols.iter().any(|s| s.name == "adds"));
}

#[test]
fn same_line_bash_assignments_have_distinct_ids() {
    let res = CodeExtractor::extract("sample.sh", "X=1; X=2", Language::Bash).unwrap();
    let ids: Vec<_> = res
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "X" && symbol.kind == SymbolKind::Variable)
        .map(|symbol| symbol.id.clone())
        .collect();
    assert_eq!(ids.len(), 2, "both assignments should be kept");
    assert_ne!(ids[0], ids[1]);
    assert!(ids.iter().all(|id| id.contains("@")));
    assert_eq!(
        res.symbols
            .iter()
            .filter(|symbol| symbol.name == "X")
            .map(|symbol| symbol.start_line)
            .collect::<Vec<_>>(),
        vec![1, 1]
    );
}

#[test]
fn same_line_rust_modules_keep_distinct_functions() {
    let code = "mod a { fn f() {} } mod b { fn f() {} }";
    let res = CodeExtractor::extract("src/same.rs", code, Language::Rust).unwrap();
    let ids: Vec<_> = res
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "f")
        .map(|symbol| symbol.id.clone())
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    assert!(ids.iter().any(|id| id.contains("::a::f@")));
    assert!(ids.iter().any(|id| id.contains("::b::f@")));
    assert!(res
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "f")
        .all(|symbol| symbol.start_line == 1));
}

#[test]
fn same_line_js_methods_have_distinct_ids() {
    let code = "class A { f() {} } class B { f() {} }";
    let res = CodeExtractor::extract("same.js", code, Language::JavaScript).unwrap();
    let ids: Vec<_> = res
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "f")
        .map(|symbol| symbol.id.clone())
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    assert!(ids.iter().any(|id| id.contains("::A::f@")));
    assert!(ids.iter().any(|id| id.contains("::B::f@")));
}

#[test]
fn deeply_nested_rust_expression_does_not_abort() {
    const DEPTH: usize = 5_000;
    let mut code = String::with_capacity(DEPTH * 2 + 40);
    code.push_str("fn main() { let _x = ");
    code.extend(std::iter::repeat_n('(', DEPTH));
    code.push_str("foo()");
    code.extend(std::iter::repeat_n(')', DEPTH));
    code.push_str("; }\n");
    assert!(
        code.len() < 100_000,
        "fixture must stay under a few hundred KB"
    );

    let res = CodeExtractor::extract("src/deep.rs", &code, Language::Rust).unwrap();
    assert!(res.symbols.iter().any(|symbol| symbol.name == "main"));
    assert!(res.references.iter().any(|reference| {
        reference.kind == ReferenceKind::Calls
            && reference.target_name == "foo"
            && reference.source_symbol_name.as_deref() == Some("main")
    }));
}

#[test]
fn import_only_go_module_keeps_references() {
    let go = "package main\nimport \"fmt\"\n";
    let res = CodeExtractor::extract("main.go", go, Language::Go).unwrap();
    assert!(res.references.iter().any(|reference| {
        reference.kind == ReferenceKind::Imports && reference.target_name.contains("fmt")
    }));
}

#[test]
fn top_level_call_module_keeps_references() {
    let res = CodeExtractor::extract("main.ts", "foo();\n", Language::TypeScript).unwrap();
    assert!(res.references.iter().any(|reference| {
        reference.kind == ReferenceKind::Calls && reference.target_name == "foo"
    }));
}

#[test]
fn rust_method_calls_use_the_method_identifier() {
    let code = r#"
        struct A;
        struct B;
        impl A {
            fn f(&self) {}
            fn call(&self) { self.f(); }
        }
        impl B {
            fn f(&self) {}
        }
        fn use_both() {
            let value = A;
            value.f();
            let other = B;
            other.f();
        }
    "#;
    let res = CodeExtractor::extract("src/methods.rs", code, Language::Rust).unwrap();
    let calls: Vec<_> = res
        .references
        .iter()
        .filter(|reference| reference.kind == ReferenceKind::Calls)
        .collect();
    assert!(calls.len() >= 3);
    assert!(
        calls.iter().all(|reference| reference.target_name == "f"),
        "method calls must not keep the receiver in the target name"
    );
    assert!(calls.iter().any(|reference| {
        reference.target_name == "f" && reference.source_symbol_name.as_deref() == Some("call")
    }));
    assert!(calls.iter().any(|reference| {
        reference.target_name == "f" && reference.source_symbol_name.as_deref() == Some("use_both")
    }));
    let methods: Vec<_> = res
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "f")
        .collect();
    assert_eq!(methods.len(), 2);
    assert_ne!(methods[0].id, methods[1].id);
}

#[test]
fn parser_extracts_docs_scopes_c_declarators_js_arrows_and_non_prefix_tests() {
    let rust_code = r##"
        /// Documented helper.
        pub fn helper_not_a_test() {
            let _s = "#[test] inside string";
        }

        #[test]
        fn verify_token_rotation() {
            helper_not_a_test();
        }
    "##;
    let rs = CodeExtractor::extract("src/lib.rs", rust_code, Language::Rust).unwrap();
    let helper = rs
        .symbols
        .iter()
        .find(|s| s.name == "helper_not_a_test")
        .unwrap();
    assert_eq!(helper.doc_comment.as_deref(), Some("Documented helper."));
    assert!(!rs.references.iter().any(|r| r.kind == ReferenceKind::Tests
        && r.source_symbol_name.as_deref() == Some("helper_not_a_test")));
    assert!(rs.references.iter().any(|r| {
        r.kind == ReferenceKind::Tests
            && r.source_symbol_name.as_deref() == Some("verify_token_rotation")
            && r.target_name == "helper_not_a_test"
    }));

    let c_code = r#"
        /* Return pointer helper */
        int * alloc_buf(void) { return 0; }
        int (*select_op(int x))(int) { obj->dispatch(); return 0; }
    "#;
    let c_res = CodeExtractor::extract("src/mem.c", c_code, Language::C).unwrap();
    let alloc = c_res
        .symbols
        .iter()
        .find(|s| s.name == "alloc_buf")
        .unwrap();
    assert_eq!(alloc.doc_comment.as_deref(), Some("Return pointer helper"));
    assert!(c_res.symbols.iter().any(|s| s.name == "select_op"));
    assert!(c_res
        .references
        .iter()
        .any(|r| r.kind == ReferenceKind::Calls && r.target_name == "dispatch"));

    let js_code = r#"
        class Worker {
            /** Run job */
            run() {}
        }
        /** Build task */
        export const buildTask = async () => { run(); };
    "#;
    let js_res = CodeExtractor::extract("app.js", js_code, Language::JavaScript).unwrap();
    let run_sym = js_res.symbols.iter().find(|s| s.name == "run").unwrap();
    assert_eq!(run_sym.kind, SymbolKind::Method);
    assert_eq!(run_sym.qualified_name, "app.js::Worker::run");
    assert_eq!(run_sym.doc_comment.as_deref(), Some("Run job"));
    let arrow = js_res
        .symbols
        .iter()
        .find(|s| s.name == "buildTask")
        .unwrap();
    assert_eq!(arrow.kind, SymbolKind::Function);
    assert_eq!(arrow.doc_comment.as_deref(), Some("Build task"));

    let py_code = "def greet():\n    \"\"\"Say hello\"\"\"\n    return 1\n";
    let py_res = CodeExtractor::extract("greet.py", py_code, Language::Python).unwrap();
    assert_eq!(
        py_res
            .symbols
            .iter()
            .find(|s| s.name == "greet")
            .unwrap()
            .doc_comment
            .as_deref(),
        Some("Say hello")
    );

    let tsx_res = CodeExtractor::extract(
        "Comp.TSX",
        "export const Badge = () => <span>ok</span>;\n",
        Language::TypeScript,
    )
    .unwrap();
    assert!(tsx_res.symbols.iter().any(|s| s.name == "Badge"));

    let dart_code = r#"
        void suite() {
            group('a', () { test('works', () {}); });
            group('b', () { test('works', () {}); });
        }
    "#;
    let dart_res = CodeExtractor::extract("test.dart", dart_code, Language::Dart).unwrap();
    let works_count = dart_res
        .symbols
        .iter()
        .filter(|s| s.name == "works")
        .count();
    assert_eq!(works_count, 2);
}
