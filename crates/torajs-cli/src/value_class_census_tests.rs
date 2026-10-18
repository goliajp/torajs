//! The value-class census of pass 0.5 over whole programs: the
//! tokenizer's `Token` is admitted, and every way a program can tell
//! an instance from a copy of its fields keeps the class out.

use torajs_core::ast::Ast;
use torajs_core::{check, lexer, parser, ssa_lower};

pub(crate) fn desugared(src: &str) -> Ast {
    let tokens = lexer::tokenize(src).expect("fixture lexes");
    let mut ast = parser::parse(src, &tokens).expect("fixture parses");
    ast.source = src.to_string();
    ast.warm_newline_cache();
    crate::ast_pipeline::run_ast_prelude(&mut ast).expect("prelude");
    crate::ast_pipeline::run_ast_desugar_pipeline(&mut ast).expect("desugar");
    ast
}

pub(crate) fn classes(src: &str) -> Vec<String> {
    let artifacts = check::check_with_arity(&desugared(src)).expect("fixture checks");
    ssa_lower::value_class_names(&artifacts)
}

/// The reason the census keeps `Token` out of `src`.
fn token_reason(src: &str) -> String {
    let artifacts = check::check_with_arity(&desugared(src)).expect("fixture checks");
    let verdicts = ssa_lower::value_class_verdicts(&artifacts);
    let reason = verdicts.iter().find(|(c, _)| c == "Token");
    match reason {
        Some((_, Some(r))) => r.clone(),
        None => "not a candidate".into(),
        Some((_, None)) => panic!("Token not ruled out: {verdicts:?}\n{src}"),
    }
}

pub(crate) const BASE: &str = "class Token {\n\
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

/// `Token` is kept out of the base program with `tail` added, for a
/// reason that says `why`.
fn out_with(tail: &str, why: &str) {
    let src = BASE.replace("TAIL", tail);
    let got = classes(&src);
    assert!(
        !got.iter().any(|c| c == "Token"),
        "admitted with:\n{tail}\n{got:?}"
    );
    let reason = token_reason(&src);
    assert!(reason.contains(why), "with:\n{tail}\nreason: {reason}");
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
    out_with(
        "const a = ts[0]\nconst b = ts[0]\nconsole.log(a === b)\n",
        "Other under BinOp): Ident(\"a\")",
    );
}

#[test]
fn keyed_collections_keep_it_out() {
    out_with(
        "const m = new Map<Token, number>()\nm.set(ts[0], 1)\nconsole.log(m.get(ts[0]))\n",
        "typed Some(Map(ClassRef(\"Token\")",
    );
    out_with(
        "const st = new Set<Token>(ts)\nconsole.log(st.has(ts[0]))\n",
        "under New): Ident(\"ts\")",
    );
    out_with(
        "console.log(ts.indexOf(ts[1]))\n",
        "name: \"indexOf\", role: Call",
    );
    out_with(
        "const wm = new WeakMap()\nwm.set(ts[0], 1)\n",
        "Arg { want: None } under Call): Index",
    );
}

#[test]
fn a_field_write_keeps_it_out() {
    out_with(
        "const t = ts[1]\nts[1].kind = 9\nconsole.log(t.kind)\n",
        "name: \"kind\", role: Write",
    );
}

#[test]
fn freeze_and_define_keep_it_out() {
    out_with("Object.freeze(ts[0])\n", "the program may freeze or delete");
    out_with(
        "Object.defineProperty(ts[0], \"kind\", { value: 5 })\n",
        "not a candidate",
    );
}

#[test]
fn a_getter_on_the_array_prototype_keeps_it_out() {
    // `ts[5]` past the end reads the getter, not an element
    out_with(
        "Object.defineProperty(Array.prototype, \"5\", { get() { return 1 } })\n",
        "an array read may run a prototype getter",
    );
}

#[test]
fn a_subclass_keeps_it_out() {
    out_with(
        "class T2 extends Token {\n  x: number = 0\n}\nts.push(new T2(1, 2))\n",
        "not a candidate",
    );
}

#[test]
fn a_wider_literal_keeps_it_out() {
    out_with(
        "const o = { kind: 1, value: 2, extra: 3 }\nts.push(o)\n",
        "Other under Call): Ident(\"ts\")",
    );
}

#[test]
fn flowing_into_any_keeps_it_out() {
    out_with(
        "console.log(ts[0])\n",
        "Arg { want: None } under Call): Index",
    );
    out_with(
        "function f(x: any) {\n  return x\n}\nf(ts[0])\n",
        "Arg { want: None } under Call): Index",
    );
    out_with(
        "console.log(JSON.stringify(ts))\n",
        "Arg { want: None } under Call): Ident(\"ts\")",
    );
    out_with(
        "console.log(`${ts[0]}`)\n",
        "Arg { want: None } under Call): Index",
    );
}

#[test]
fn holes_and_shrinking_keep_it_out() {
    out_with(
        "ts[5] = new Token(1, 1)\n",
        "Discard under a statement): Assign",
    );
    out_with("ts.length = 0\n", "name: \"length\", role: Write");
    out_with(
        "const holes: Token[] = new Array<Token>(3)\nconsole.log(holes.length)\n",
        "is declared holding it but bound to another value",
    );
}

#[test]
fn an_index_that_is_not_a_number_keeps_it_out() {
    // a key that is not a number names a property, which the keyed read
    // can find on the prototype chain; an inline element has none
    out_with(
        "console.log(typeof ts[Symbol.iterator])\n",
        "(IndexObj { index",
    );
    // a string or `any` key may spell `shift`, so the program is out
    // before its index is read
    for tail in [
        "console.log(ts[\"0\"].kind)\n",
        "const k: string = \"1\"\nconst t = ts[k]\nconsole.log(t.kind)\n",
        "const k: any = 1\nconsole.log(ts[k].kind)\n",
    ] {
        out_with(tail, "an array head may move");
    }
}

#[test]
fn a_push_between_view_and_read_keeps_it_out() {
    out_with(
        "const t = ts[0]\nts.push(new Token(7, 7))\nconsole.log(t.kind)\n",
        "can come between the view `t` and its read",
    );
}

#[test]
fn other_array_operations_keep_it_out() {
    out_with(
        "for (const t of ts) s = s + t.kind\n",
        "Other under a statement): Index",
    );
    out_with("ts.pop()\n", "Discard under a statement): Call");
    out_with(
        "console.log(ts.map((t) => t.kind).length)\n",
        "a parameter of `__closure_0` holds it in another shape",
    );
}

#[test]
fn reading_the_class_keeps_it_out() {
    out_with(
        "console.log(ts[0] instanceof Token)\n",
        "under InstanceOf): Index",
    );
    // `constructor` and `getPrototypeOf` are spellings that can reach a
    // builtin prototype, so the program-wide check keeps these out
    // before the walk meets the read
    out_with(
        "console.log(ts[0].constructor === Token)\n",
        "an array read may run a prototype getter",
    );
    out_with(
        "console.log(Object.getPrototypeOf(ts[0]) === null)\n",
        "an array read may run a prototype getter",
    );
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

pub(crate) const BOXED: &str = "class Token {\n\
     kind: number\n\
     value: number\n\
     constructor(kind: number, value: number) {\n\
       this.kind = kind\n\
       this.value = value\n\
     }\n\
   }\n\
   class Holder {\n\
     toks: Token[]\n\
     constructor(toks: Token[]) {\n\
       this.toks = toks\n\
     }\n\
   }\n\
   function make(n: number): Token[] {\n\
     const ts: Token[] = []\n\
     for (let i = 0; i < n; i = i + 1) ts.push(new Token(i, i * 2))\n\
     return ts\n\
   }\n\
   function main(): void {\n\
     let s = 0\n\
     BODY\n\
     console.log(s)\n\
   }\n\
   main()\n";

#[test]
fn a_view_off_a_rooted_container_field_is_admitted() {
    let src = BOXED.replace(
        "BODY",
        "const h = new Holder(make(3))\nconst t = h.toks[1]\ns = s + t.kind + h.toks[2].value\n",
    );
    assert_eq!(classes(&src), vec!["Token".to_string()]);
}

#[test]
fn an_element_of_a_temporary_array_keeps_it_out() {
    // the temporary is released right after the element is read
    for body in [
        "const t = make(3)[1]\ns = s + t.kind\n",
        "s = s + make(3)[1].kind\n",
        "const t = new Holder(make(3)).toks[0]\ns = s + t.kind\n",
    ] {
        let reason = token_reason(&BOXED.replace("BODY", body));
        assert!(
            reason.contains("temporary array"),
            "{body}\nreason: {reason}"
        );
    }
}

#[test]
fn a_view_shadowed_in_a_block_is_admitted() {
    let tail = "const t = ts[0]\n{\n  const t = ts[1]\n  s = s + t.kind\n}\ns = s + t.kind\n";
    assert_eq!(
        classes(&BASE.replace("TAIL", tail)),
        vec!["Token".to_string()]
    );
}

#[test]
fn a_closure_reading_a_shadowed_view_keeps_it_out() {
    // the block's read of its own `t` must not stand in for the
    // closure's read of the outer one
    out_with(
        "const t = ts[0]\n{\n  const t = ts[1]\n  s = s + t.kind\n}\nconst g = () => t.kind\nts.push(new Token(7, 7))\ns = s + g()\n",
        "a read of `t` that no view in scope binds",
    );
}

#[test]
fn a_push_after_a_shadowed_view_keeps_it_out() {
    out_with(
        "const t = ts[0]\n{\n  const t = ts[1]\n  s = s + t.kind\n}\nts.push(new Token(7, 7))\ns = s + t.kind\n",
        "can come between the view `t` and its read",
    );
}

#[test]
fn an_admitted_class_records_what_each_field_starts_with() {
    use torajs_core::ssa_lower::ValueFieldInit as Init;
    let src = "class P {\n\
         x: number\n\
         y: number\n\
         on: boolean\n\
         constructor(y: number) {\n\
           this.y = y\n\
           this.on = true\n\
         }\n\
       }\n\
       function main(): void {\n\
         const ps: P[] = []\n\
         ps.push(new P(parseInt(process.argv[2], 10)))\n\
         console.log(ps[0].x + ps[0].y)\n\
       }\n\
       main()\n";
    let artifacts = check::check_with_arity(&desugared(src)).expect("fixture checks");
    let fields = |v: &[(&str, Init)]| v.iter().map(|(f, i)| (f.to_string(), *i)).collect();
    assert_eq!(
        ssa_lower::value_class_inits(&artifacts),
        vec![(
            "P".to_string(),
            fields(&[
                ("x", Init::Number(0.0)),
                ("y", Init::Arg(0)),
                ("on", Init::Bool(true)),
            ])
        )]
    );
}
