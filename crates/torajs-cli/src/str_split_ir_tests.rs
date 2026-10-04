//! A direct call with an `any` argument for a `string` parameter, read
//! off the lowered SSA: the call site tests the argument inline and
//! calls the typed body or its any-widened clone. The program has no
//! boxed adapter, so the split stands on its own.

use torajs_core::ssa::{InstKind, Module};
use torajs_core::{check, lexer, parser, ssa_lower};

fn lower(src: &str) -> Module {
    let tokens = lexer::tokenize(src).expect("fixture lexes");
    let mut ast = parser::parse(src, &tokens).expect("fixture parses");
    ast.source = src.to_string();
    ast.warm_newline_cache();
    crate::ast_pipeline::run_ast_prelude(&mut ast).expect("prelude");
    crate::ast_pipeline::run_ast_desugar_pipeline(&mut ast).expect("desugar");
    let artifacts = check::check_with_arity(&ast).expect("fixture checks");
    ssa_lower::lower_with_arity(&ast, &artifacts)
}

/// The names of every function `caller` calls directly.
fn callees<'m>(m: &'m Module, caller: &str) -> Vec<&'m str> {
    let f = m
        .funcs
        .iter()
        .find(|f| f.name == caller)
        .expect("caller lowered");
    f.blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter_map(|i| match &i.kind {
            InstKind::Call(fid, _) => Some(m.funcs[fid.0 as usize].name.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn an_any_argument_for_a_string_parameter_reaches_both_bodies() {
    let src = r#"
function size(s: string): number {
  return s.length;
}
const v: any = JSON.parse('"ab"');
console.log(size(v));
"#;
    let m = lower(src);
    let calls = callees(&m, "main");
    assert!(calls.contains(&"size"), "{calls:?}");
    assert!(
        calls.iter().any(|c| c.starts_with("size$$anyw")),
        "{calls:?}"
    );
}

#[test]
fn a_typed_argument_calls_the_typed_body_alone() {
    let src = r#"
function size(s: string): number {
  return s.length;
}
console.log(size("ab"));
"#;
    let m = lower(src);
    let calls = callees(&m, "main");
    assert!(calls.contains(&"size"), "{calls:?}");
    assert!(!calls.iter().any(|c| c.contains("$$anyw")), "{calls:?}");
}
