//! The value-class census of pass 0.5 over whole programs: the
//! tokenizer's `Token` is admitted, and every way a program can tell
//! an instance from a copy of its fields keeps the class out.

use torajs_core::ast::Ast;
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

fn classes(src: &str) -> Vec<String> {
    let artifacts = check::check_with_arity(&desugared(src)).expect("fixture checks");
    ssa_lower::value_class_names(&artifacts)
}

const BASE: &str = "class Token {\n\
     kind: number\n\
     value: number\n\
     constructor(kind: number, value: number) {\n\
       this.kind = kind\n\
       this.value = value\n\
     }\n\
   }\n\
   function make(n: number): Token[] {\n\
     const ts: Token[] = []\n\
     for (let i = 0; i < n; i = i + 1) ts.push(new Token(i, i * 2))\n\
     return ts\n\
   }\n\
   function main(): void {\n\
     const ts = make(parseInt(process.argv[2], 10))\n\
     let s = 0\n\
     for (let i = 0; i < ts.length; i = i + 1) {\n\
       s = s + ts[i].kind\n\
     }\n\
     TAIL\n\
     console.log(s)\n\
   }\n\
   main()\n";

fn out_with(tail: &str) {
    let src = BASE.replace("TAIL", tail);
    let got = classes(&src);
    assert!(
        !got.iter().any(|c| c == "Token"),
        "admitted with:\n{tail}\n{got:?}"
    );
}

#[test]
fn the_tokenizer_token_is_a_value_class() {
    let src = include_str!("../../../bench/cases/tokenizer-parser-eval-10k/main.ts");
    assert_eq!(classes(src), vec!["Token".to_string()]);
}

#[test]
fn the_base_program_admits_token() {
    assert_eq!(
        classes(&BASE.replace("TAIL", "")),
        vec!["Token".to_string()]
    );
}

#[test]
fn identity_comparison_keeps_it_out() {
    out_with("const a = ts[0]\nconst b = ts[0]\nconsole.log(a === b)\n");
}

#[test]
fn keyed_collections_keep_it_out() {
    out_with("const m = new Map<Token, number>()\nm.set(ts[0], 1)\nconsole.log(m.get(ts[0]))\n");
    out_with("const st = new Set<Token>(ts)\nconsole.log(st.has(ts[0]))\n");
    out_with("console.log(ts.indexOf(ts[1]))\n");
    out_with("const wm = new WeakMap()\nwm.set(ts[0], 1)\n");
}

#[test]
fn a_field_write_keeps_it_out() {
    out_with("const t = ts[1]\nts[1].kind = 9\nconsole.log(t.kind)\n");
}

#[test]
fn freeze_and_define_keep_it_out() {
    out_with("Object.freeze(ts[0])\n");
    out_with("Object.defineProperty(ts[0], \"kind\", { value: 5 })\n");
}

#[test]
fn a_subclass_keeps_it_out() {
    out_with("class T2 extends Token {\n  x: number = 0\n}\nts.push(new T2(1, 2))\n");
}

#[test]
fn a_wider_literal_keeps_it_out() {
    out_with("const o = { kind: 1, value: 2, extra: 3 }\nts.push(o)\n");
}

#[test]
fn flowing_into_any_keeps_it_out() {
    out_with("console.log(ts[0])\n");
    out_with("function f(x: any) {\n  return x\n}\nf(ts[0])\n");
    out_with("console.log(JSON.stringify(ts))\n");
    out_with("console.log(`${ts[0]}`)\n");
}

#[test]
fn holes_and_shrinking_keep_it_out() {
    out_with("ts[5] = new Token(1, 1)\n");
    out_with("ts.length = 0\n");
    out_with("const holes: Token[] = new Array<Token>(3)\nconsole.log(holes.length)\n");
}

#[test]
fn a_push_between_view_and_read_keeps_it_out() {
    out_with("const t = ts[0]\nts.push(new Token(7, 7))\nconsole.log(t.kind)\n");
}

#[test]
fn other_array_operations_keep_it_out() {
    out_with("for (const t of ts) s = s + t.kind\n");
    out_with("ts.pop()\n");
    out_with("console.log(ts.map((t) => t.kind).length)\n");
}

#[test]
fn reading_the_class_keeps_it_out() {
    out_with("console.log(ts[0] instanceof Token)\n");
    out_with("console.log(ts[0].constructor === Token)\n");
    out_with("console.log(Object.getPrototypeOf(ts[0]) === null)\n");
}

#[test]
fn an_optional_field_or_a_method_keeps_the_class_out() {
    let base = BASE.replace("TAIL", "");
    let opt = base.replace("value: number\n", "value?: number\n");
    assert!(classes(&opt).is_empty(), "{opt}");
    let method = base.replace(
        "constructor(kind",
        "twice(): number {\n return this.kind * 2\n }\n constructor(kind",
    );
    assert!(classes(&method).is_empty(), "{method}");
}

#[test]
fn a_binding_read_as_any_keeps_it_out() {
    // the checker types these top-level reads in a loop body `any`
    let src = "class Token {\n\
         kind: number\n\
         value: number\n\
         constructor(kind: number, value: number) {\n\
           this.kind = kind\n\
           this.value = value\n\
         }\n\
       }\n\
       function make(n: number): Token[] {\n\
         const ts: Token[] = []\n\
         for (let i = 0; i < n; i = i + 1) ts.push(new Token(i, i * 2))\n\
         return ts\n\
       }\n\
       const ts = make(parseInt(process.argv[2], 10))\n\
       let s = 0\n\
       for (let i = 0; i < ts.length; i = i + 1) s = s + ts[i].kind\n\
       console.log(s)\n";
    assert!(classes(src).is_empty());
}
