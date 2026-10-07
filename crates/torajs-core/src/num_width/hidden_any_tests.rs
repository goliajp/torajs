use super::super::analyze;
use crate::ast::{Ast, Expr, Stmt};
use crate::{lexer, parser};
use std::collections::HashMap;

/// Each declaration `name = <number literal>` in `stmts`, at any depth.
fn decls(ast: &Ast, stmts: &[Stmt], out: &mut Vec<(String, String, crate::ast::ExprId)>) {
    for s in stmts {
        match s {
            Stmt::LetDecl { name, init, .. } => {
                let v = match ast.get_expr(*init) {
                    Expr::Number(n) => n.to_string(),
                    _ => String::new(),
                };
                out.push((name.clone(), v, *init));
            }
            Stmt::Block(v) | Stmt::Multi(v) => decls(ast, v, out),
            Stmt::FnDecl { body, .. } => decls(ast, body, out),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                decls(ast, std::slice::from_ref(&**then_branch), out);
                if let Some(e) = else_branch {
                    decls(ast, std::slice::from_ref(&**e), out);
                }
            }
            Stmt::For { body, .. } | Stmt::ForOf { body, .. } | Stmt::While { body, .. } => {
                decls(ast, std::slice::from_ref(&**body), out)
            }
            _ => {}
        }
    }
}

/// The declarations the analysis would retype, as `name=init`.
fn retyped_in(src: &str, lift: bool) -> Vec<String> {
    let tokens = lexer::tokenize(src).expect("lex");
    let mut ast = parser::parse(src, &tokens).expect("parse");
    if lift {
        crate::ast::lift_arrow_fns(&mut ast);
    }
    let (_, types) = crate::check::check_with_types(&ast).expect("check");
    let t = analyze(&ast, &HashMap::new(), &HashMap::new(), &types);
    let mut all = Vec::new();
    decls(&ast, &ast.stmts, &mut all);
    let mut out: Vec<String> = (all.into_iter())
        .filter(|(_, _, init)| t.hidden_any_inits.contains(init))
        .map(|(n, v, _)| if v.is_empty() { n } else { format!("{n}={v}") })
        .collect();
    out.sort();
    out
}

fn retyped(src: &str) -> Vec<String> {
    retyped_in(src, false)
}

const ESCAPED: &str = "const d: number[] = [1, 2];\nconst u: any = d;\n";

#[test]
fn plus_and_copies_carry_an_escaped_element() {
    let src = format!(
        "{ESCAPED}let t: number = 0;\nt = t + d[1];\nlet c = 0;\nc = t;\nlet m: number = 1;\nm = m * d[0];\nconst k = d.length > 0 ? d[0] : 0;"
    );
    assert_eq!(retyped(&src), ["c=0", "k", "t=0"]);
}

#[test]
fn for_of_binding_carries_an_escaped_element() {
    let src = format!("{ESCAPED}let acc = 0;\nfor (const v of d) {{ acc = acc + v; }}");
    assert_eq!(retyped(&src), ["acc=0"]);
}

#[test]
fn string_concatenation_and_unescaped_arrays_keep_their_type() {
    let src = format!(
        "{ESCAPED}let s: string = \"\";\ns = s + \"x\" + d[0];\nconst b: number[] = [1];\nlet n: number = 0;\nn = n + b[0];"
    );
    assert!(retyped(&src).is_empty());
}

#[test]
fn sibling_blocks_retype_only_the_written_binding() {
    let src = format!(
        "{ESCAPED}function f(): void {{\n  {{\n    let sum = 10;\n    sum = sum + d[0];\n    console.log(sum);\n  }}\n  {{\n    let sum = 20;\n    sum = sum + 1;\n    console.log(sum);\n  }}\n}}\nf();"
    );
    assert_eq!(retyped(&src), ["sum=10"]);
}

#[test]
fn a_shadowing_binding_keeps_the_outer_one_apart() {
    let src = format!(
        "{ESCAPED}let x = 10;\n{{\n  let x = 20;\n  x = x + d[0];\n  console.log(x);\n}}\nx = x + 1;\nlet y = 30;\n{{\n  let y = 40;\n  y = y + 1;\n}}\ny = y + d[1];\nconsole.log(x, y);"
    );
    assert_eq!(retyped(&src), ["x=20", "y=30"]);
}

#[test]
fn a_callback_writes_the_binding_its_block_names() {
    let src = format!(
        "{ESCAPED}function f(): void {{\n  {{\n    let tot = 10;\n    d.forEach((v: number) => {{ tot = tot + v; }});\n    console.log(tot);\n  }}\n  {{\n    let tot = 20;\n    [1].forEach((v: number) => {{ tot = tot + v; }});\n    console.log(tot);\n  }}\n}}\nf();"
    );
    assert_eq!(retyped_in(&src, true), ["tot=10"]);
}

#[test]
fn an_any_array_read_is_a_source() {
    let src = "const xs: any[] = [1, \"s\"];\nlet u: number = 0;\nfor (let i = 0; i < xs.length; i++) u = u + xs[i];\nlet acc = 0;\nfor (const v of xs) acc = acc + v;\nlet n: number = 0;\nn = n * xs[0];";
    assert_eq!(retyped(src), ["acc=0", "u=0"]);
}
