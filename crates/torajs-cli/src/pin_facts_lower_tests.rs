//! The array fields the finished module records as holding still while
//! a cluster of a class's methods runs (`Module::pins`), read off the
//! lowered SSA of the parser fixtures and the tokenizer bench source.

use torajs_core::ast::Ast;
use torajs_core::ssa::{FuncId, Module};
use torajs_core::{check, lexer, parser, ssa_lower};

/// An object cell's header, ahead of its 8-byte field slots.
const OBJ_HEADER_SIZE: u64 = 32;

fn lower(src: &str) -> Module {
    let tokens = lexer::tokenize(src).expect("fixture lexes");
    let mut ast: Ast = parser::parse(src, &tokens).expect("fixture parses");
    ast.source = src.to_string();
    ast.warm_newline_cache();
    crate::ast_pipeline::run_ast_prelude(&mut ast).expect("prelude");
    crate::ast_pipeline::run_ast_desugar_pipeline(&mut ast).expect("desugar");
    let artifacts = check::check_with_arity(&ast).expect("fixture checks");
    ssa_lower::lower_with_arity(&ast, &artifacts)
}

/// A pin as its class, its field, its cluster's method names, its
/// entries' and whether only direct calls enter it.
type PinRow = (String, String, Vec<String>, Vec<String>, bool);

fn pins(src: &str) -> Vec<PinRow> {
    let m = lower(src);
    let method = |f: &FuncId| {
        let name = &m.funcs[f.0 as usize].name;
        let (class, method) = (name.strip_prefix("__cm_"))
            .and_then(|r| r.split_once("__"))
            .expect("a class method");
        (class.to_string(), method.to_string())
    };
    (m.pins.iter())
        .map(|p| {
            let idx = ((p.field_off - OBJ_HEADER_SIZE) / 8) as usize;
            let field = m.struct_layouts[p.sid.0 as usize][idx]
                .0
                .to_string_lossy_owned();
            let class = method(&p.cluster[0]).0;
            let names = |fs: &[FuncId]| fs.iter().map(|f| method(f).1).collect();
            (
                class,
                field,
                names(&p.cluster),
                names(&p.entries),
                p.direct_only,
            )
        })
        .collect()
}

fn parser_pin() -> Vec<PinRow> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    vec![(
        "Parser".into(),
        "toks".into(),
        s(&["expr", "term", "factor"]),
        s(&["expr"]),
        true,
    )]
}

#[test]
fn a_parser_whose_constructor_alone_stores_its_tokens_pins_them() {
    let src = include_str!("../../../conformance/cases/tok-pin-001.ts");
    assert_eq!(pins(src), parser_pin());
}

#[test]
fn a_method_that_stores_new_tokens_into_the_field_pins_nothing() {
    let src = include_str!("../../../conformance/cases/tok-pin-004.ts");
    assert_eq!(pins(src), []);
}

#[test]
fn methods_called_on_another_parser_leave_the_rest_pinned() {
    let src = include_str!("../../../conformance/cases/tok-pin-006.ts");
    assert_eq!(pins(src), parser_pin());
}

#[test]
fn a_method_that_pushes_onto_the_tokens_pins_nothing() {
    let src = include_str!("../../../conformance/cases/tok-pin-008.ts");
    assert_eq!(pins(src), []);
}

#[test]
fn a_subclass_pins_nothing_in_either_class() {
    let src = include_str!("../../../conformance/cases/tok-pin-009.ts");
    assert_eq!(pins(src), []);
}

#[test]
fn a_parser_without_a_release_fn_test_pins_its_tokens() {
    let src = include_str!("../../../conformance/cases/tok-pin-014.ts");
    assert_eq!(pins(src), parser_pin());
}

#[test]
fn an_error_thrown_outside_the_methods_leaves_the_tokens_pinned() {
    let src = include_str!("../../../conformance/cases/tok-pin-015.ts");
    assert_eq!(pins(src), parser_pin());
}

#[test]
fn a_string_thrown_by_a_method_leaves_the_tokens_pinned() {
    let src = include_str!("../../../conformance/cases/tok-pin-016.ts");
    assert_eq!(pins(src), parser_pin());
}

#[test]
fn a_pop_an_any_receiver_or_an_error_constructor_pins_nothing() {
    for src in [
        include_str!("../../../conformance/cases/tok-pin-003.ts"),
        include_str!("../../../conformance/cases/tok-pin-005.ts"),
        include_str!("../../../conformance/cases/tok-pin-007.ts"),
    ] {
        assert_eq!(pins(src), []);
    }
}

#[test]
fn the_tokenizer_bench_source_has_exactly_one_cluster() {
    let src = include_str!("../../../bench/cases/tokenizer-parser-eval-10k/main.ts");
    assert_eq!(pins(src).len(), 1, "{:?}", pins(src));
}
