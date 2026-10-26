//! Every await the lowering meets sits in a body that suspends: a
//! function the gate moved (`ast::desugar_async_ssa_gate`), or the
//! module body when it awaits at its top level. An async generator's
//! step function carries an await only when the generator rewrite could
//! not turn it into a yield (`desugar_generators_async::prepare`). The
//! check runs before anything is lowered and names every such
//! function, where the lowering itself would stop at the first.

use std::collections::HashSet;

use crate::ast::{Ast, Stmt, body_awaits};
use crate::check_monomorph::MonoOutput;

/// The functions of `ast` (generic templates in `skip` aside, their
/// copies are checked) that hold an await or a `for await` and do not
/// suspend, sorted; the module body as `<module body>`.
pub(crate) fn stray_awaits(ast: &Ast, skip: &HashSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut module = false;
    for s in &ast.stmts {
        match s {
            Stmt::FnDecl { name, body, .. } => {
                if !skip.contains(name)
                    && !ast.async_ssa.fns.contains(name)
                    && body_awaits(ast, body)
                {
                    out.push(name.clone());
                }
            }
            other => module |= body_awaits(ast, std::slice::from_ref(other)),
        }
    }
    if module && !ast.async_ssa.main {
        out.push("<module body>".to_string());
    }
    out.sort();
    out
}

pub(crate) fn assert_every_await_suspends(mono: &MonoOutput) {
    let stray = stray_awaits(&mono.mono_ast, &mono.generic_fn_names);
    assert!(
        stray.is_empty(),
        "internal error: an await in {}, which does not suspend",
        stray
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

#[cfg(test)]
mod tests {
    use super::stray_awaits;
    use std::collections::HashSet;

    fn parse(src: &str) -> crate::ast::Ast {
        let tokens = crate::lexer::tokenize(src).expect("tokenize");
        crate::parser::parse(src, &tokens).expect("parse")
    }

    #[test]
    fn names_each_unmoved_function_that_awaits() {
        let mut ast = parse(
            "async function a() { await 0 }\n\
             async function b() { for await (const x of []) {} }\n\
             async function c() { return 1 }\n\
             function d() { return 2 }",
        );
        assert_eq!(stray_awaits(&ast, &HashSet::new()), ["a", "b"]);
        crate::ast::mark_async_ssa_fns(&mut ast);
        assert!(stray_awaits(&ast, &HashSet::new()).is_empty());
    }

    #[test]
    fn an_async_generator_left_with_an_await() {
        let mut ast = parse("async function* g() { await 0; yield 1 }");
        crate::ast::mark_async_ssa_fns(&mut ast);
        assert_eq!(stray_awaits(&ast, &HashSet::new()), ["g"]);
    }

    #[test]
    fn a_skipped_template_and_the_module_body() {
        let mut ast = parse("async function g() { await 0 }\nawait 1;");
        let skip: HashSet<String> = ["g".to_string()].into();
        assert_eq!(stray_awaits(&ast, &skip), ["<module body>"]);
        crate::ast::mark_async_ssa_fns(&mut ast);
        assert!(stray_awaits(&ast, &skip).is_empty());
    }
}
