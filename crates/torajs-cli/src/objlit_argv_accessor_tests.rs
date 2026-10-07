//! Object-literal accessors whose body reads `arguments`: the setter's
//! declared param still types an assignment, and only a slot that can
//! hold such a closure is called through the boxed entry.

use torajs_core::ast::Ast;
use torajs_core::ssa::{InstKind, Module};
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

/// How many accessor calls `main` makes through the boxed entry.
fn boxed_calls(src: &str) -> usize {
    let ast = desugared(src);
    let artifacts = check::check_with_arity(&ast).expect("fixture checks");
    let m: Module = ssa_lower::lower_with_arity(&ast, &artifacts);
    let main = m.funcs.iter().find(|f| f.name == "main").expect("main");
    main.blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter(|i| {
            matches!(&i.kind, InstKind::Call(fid, _)
            if m.funcs[fid.0 as usize].name == "__torajs_closure_call_with_this")
        })
        .count()
}

const SETTER: &str = "set s(v: number) { console.log(arguments.length, v); }";

#[test]
fn an_argv_setter_takes_its_declared_param() {
    let e = check_err(&format!("const o = {{ {SETTER} }};\no.s = \"x\";\n"));
    assert!(e.contains("setter expects Number, value is String"), "{e}");
    let e = check_err(&format!(
        "function f() {{ const o = {{ {SETTER} }}; o.s = \"x\"; }}\nf();\n"
    ));
    assert!(e.contains("setter expects Number, value is String"), "{e}");
    let e = check_err(&format!(
        "function mk() {{ return {{ {SETTER} }}; }}\nconst o = mk();\no.s = \"x\";\n"
    ));
    assert!(e.contains("setter expects Number, value is String"), "{e}");
}

#[test]
fn object_assign_into_an_argv_setter_takes_its_declared_param() {
    let e = check_err(&format!(
        "const o = {{ {SETTER} }};\nObject.assign(o, {{ s: \"x\" }});\n"
    ));
    assert!(e.contains("but the target takes Number"), "{e}");
}

#[test]
fn a_plain_getter_sharing_the_names_keeps_its_direct_call() {
    let src = "const c = { get g() { return 3; } };\n\
               const d = { get g() { return arguments.length + 20; } };\n\
               console.log(c.g);\n";
    assert_eq!(boxed_calls(src), 0);
    let src = format!("{src}console.log(d.g);\n");
    assert_eq!(boxed_calls(&src), 1);
}
