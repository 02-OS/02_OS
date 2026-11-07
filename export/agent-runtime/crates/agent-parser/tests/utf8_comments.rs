use agent_core::Language;
use agent_parser::CodeExtractor;

#[test]
fn non_ascii_comments_do_not_panic_the_rust_extractor() {
    let accents = "é".repeat(128);
    let cjk = "字".repeat(80);
    let emoji = "😀".repeat(70);
    let source = format!("//{accents}\n//{cjk}\n//{emoji}\nfn foo() {{}}\n");
    let extracted = CodeExtractor::extract("src/lib.rs", &source, Language::Rust).unwrap();
    assert!(extracted.symbols.iter().any(|symbol| symbol.name == "foo"));
}
