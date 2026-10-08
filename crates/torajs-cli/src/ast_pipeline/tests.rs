use torajs_core::{lexer, parser};

/// The lifted closures whose return type the context seeded, after
/// the full AST pipeline ran over `src`.
fn contextual_ret_fns(src: &str) -> usize {
    let tokens = lexer::tokenize(src).expect("fixture lexes");
    let mut ast = parser::parse(src, &tokens).expect("fixture parses");
    ast.source = src.to_string();
    ast.warm_newline_cache();
    super::run_ast_prelude(&mut ast).expect("prelude");
    super::run_ast_desugar_pipeline(&mut ast).expect("desugar");
    // a callback's view twin (`$$rview`) carries the seed too
    ast.contextual_ret_fns
        .iter()
        .filter(|n| !n.contains("$$"))
        .count()
}

#[test]
fn a_seeded_callback_return_is_recorded_as_contextual() {
    let src = r#"console.log("abc".replace(/b/, (m) => m + m));"#;
    assert_eq!(contextual_ret_fns(src), 1);
}

#[test]
fn a_user_written_callback_return_is_not_contextual() {
    let src = r#"console.log("abc".replace(/b/, (m: string): string => m + m));"#;
    assert_eq!(contextual_ret_fns(src), 0);
}

mod iface;
