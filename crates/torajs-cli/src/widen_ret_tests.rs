//! The return face of a call retargeted to an any-widened clone, read
//! off the checker and the lowered SSA.
//!
//! The clone binds an `any` argument as passed, so a body that hands
//! the parameter back answers whatever was passed, and the call types
//! `any`. TS still types such a call by the declared return, so every
//! program it rejects on that type stays rejected here.

use torajs_core::ast::Ast;
use torajs_core::ssa::{InstKind, Module, Type};
use torajs_core::{check, lexer, parser, ssa_lower};

fn desugared(src: &str) -> Ast {
    let tokens = lexer::tokenize(src).expect("fixture lexes");
    let mut ast = parser::parse(src, &tokens).expect("fixture parses");
    ast.source = src.to_string();
    ast.warm_newline_cache();
    crate::ast_pipeline::run_ast_prelude(&mut ast).expect("prelude");
    crate::ast_pipeline::run_ast_desugar_pipeline(&mut ast).expect("desugar");
    ast
}

fn check_err(src: &str) -> String {
    match check::check_with_arity(&desugared(src)) {
        Ok(_) => panic!("expected a type error"),
        Err(e) => e,
    }
}

fn lower(src: &str) -> Module {
    let ast = desugared(src);
    let artifacts = check::check_with_arity(&ast).expect("fixture checks");
    ssa_lower::lower_with_arity(&ast, &artifacts)
}

/// The return type of every lowered function whose name starts with
/// `prefix`.
fn rets(m: &Module, prefix: &str) -> Vec<Type> {
    m.funcs
        .iter()
        .filter(|f| f.name.starts_with(prefix))
        .map(|f| f.ret)
        .collect()
}

const ECHO: &str = "function echo(s: string) { return s; }\nconst n: any = null;\n";

#[test]
fn a_string_result_into_a_number_binding_is_rejected() {
    let e = check_err(&format!(
        "{ECHO}const k: number = echo(n);\nconsole.log(k);\n"
    ));
    assert!(e.contains("type mismatch on `k`"), "{e}");
}

#[test]
fn a_string_result_into_a_number_param_is_rejected() {
    let e = check_err(&format!(
        "{ECHO}function takesNum(x: number) {{ return x + 1; }}\nconsole.log(takesNum(echo(n)));\n"
    ));
    assert!(e.contains("expected Number, got String"), "{e}");
}

#[test]
fn a_missing_member_of_the_declared_result_is_rejected() {
    let e = check_err(&format!("{ECHO}console.log(echo(n).nope());\n"));
    assert!(e.contains("no member `.nope`"), "{e}");
}

#[test]
fn a_string_result_through_an_unannotated_binding_is_rejected() {
    let e = check_err(&format!(
        "{ECHO}const x = echo(n);\nconst k: number = x;\nconsole.log(k);\n"
    ));
    assert!(e.contains("type mismatch on `k`"), "{e}");
}

#[test]
fn a_returned_widened_param_makes_the_clone_answer_any() {
    let m = lower(&format!("{ECHO}console.log(typeof echo(n));\n"));
    let clones = rets(&m, "echo$$anyw");
    assert!(
        !clones.is_empty() && clones.iter().all(|t| *t == Type::Any),
        "{clones:?}"
    );
    let orig = m
        .funcs
        .iter()
        .find(|f| f.name == "echo")
        .expect("typed body");
    assert_eq!(orig.ret, Type::Str);
}

/// Positions the checker serves for a `string` but not for `any` keep
/// accepting the call: it falls back to the declared return there.
#[test]
fn a_position_that_wants_the_declared_string_still_checks() {
    let refused: Vec<String> = [
        "for (const p of echo(v).split(\",\")) console.log(p);\n",
        "const h: string = \"abc\";\nconsole.log(h.includes(echo(v), 0, 1), h.endsWith(echo(v), 3, 1));\n",
        "const xs: number[] = [1, 2];\nconsole.log(xs.join(echo(v), 0));\n",
    ]
    .iter()
    .filter_map(|tail| {
        let src =
            format!("function echo(s: string) {{ return s; }}\nconst v: any = \"a,b\";\n{tail}");
        check::check_with_arity(&desugared(&src))
            .err()
            .map(|e| format!("{tail}=> {e}"))
    })
    .collect();
    assert!(refused.is_empty(), "{refused:#?}");
}

/// A string argument still takes the typed body, whose answer the call
/// boxes to meet the clone's `any`.
#[test]
fn the_split_call_boxes_the_typed_answer() {
    let src = "function pick(s: string): string | null { return s.length > 3 ? s : null; }\n\
               const v: any = JSON.parse('\"abcdef\"');\nconsole.log(pick(v));\n";
    let m = lower(src);
    let main = m.funcs.iter().find(|f| f.name == "main").expect("main");
    let calls: Vec<&str> = main
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter_map(|i| match &i.kind {
            InstKind::Call(fid, _) => Some(m.funcs[fid.0 as usize].name.as_str()),
            _ => None,
        })
        .collect();
    assert!(calls.contains(&"pick"), "{calls:?}");
    assert!(calls.contains(&"pick$$anyw0s_r"), "{calls:?}");
}

#[test]
fn a_return_that_does_not_read_the_widened_param_keeps_its_type() {
    let src = "function yes(s: string) { return true; }\n\
               const n: any = null;\nconsole.log(yes(n));\n";
    let m = lower(src);
    let clones = rets(&m, "yes$$anyw");
    assert!(
        !clones.is_empty() && clones.iter().all(|t| *t == Type::Bool),
        "{clones:?}"
    );
}
