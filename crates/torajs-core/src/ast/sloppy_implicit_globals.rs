//! Sloppy-goal implicit globals — the third member of the goal-triage
//! family (`delete <bare name>` / readonly-global writes next door).
//!
//! §9.1.1.4.6 SetMutableBinding + §6.2.5.6 PutValue: in sloppy code an
//! assignment to an unresolvable name CREATES a global binding at run
//! time (`__x = 1` then `typeof __x` is `"number"`); before the write
//! runs, the name simply does not resolve (`typeof __x` is
//! `"undefined"`). The checker hard-rejects such writes for
//! `__`-prefixed names (its compiler-synthesized carve-out) and would
//! give the strict runtime-ReferenceError posture to the rest — both
//! wrong under the sloppy goal.
//!
//! Statically decidable here, same as the siblings: collect every
//! assignment whose target is an identifier the program never
//! declares, and synthesize one hoisted `var <name>;` per name at the
//! top level. A hoisted uninitialized var IS the observable shape of
//! the not-yet-written implicit global (`undefined` on read, `typeof`
//! answers `"undefined"`), and after the write both agree. Recorded
//! boundary (accepted, not built): `delete <name>` on a var answers
//! false where a true implicit global's property is configurable —
//! none of the measured cases delete their implicit binding.
//!
//! Known builtin globals stay out (`Object = 12` keeps the recorded
//! reject next to `check_assign_ident`'s carve-out), and the §19.1
//! readonly names never reach this pass — the sibling already folded
//! their writes. Runs right after it, before the checker.

use super::{Ast, Expr, Stmt};

pub fn synthesize_sloppy_implicit_globals(ast: &mut Ast) {
    if !ast.sloppy_script_goal {
        return;
    }
    let mut declared = std::collections::HashSet::new();
    super::delete_bare_name::collect_declared_names(&ast.stmts, &mut declared);
    let mut names: Vec<String> = Vec::new();
    for e in &ast.exprs {
        let Expr::Assign { target, .. } = e else {
            continue;
        };
        let Expr::Ident(n) = ast.get_expr(*target) else {
            continue;
        };
        if declared.contains(n)
            || names.iter().any(|seen| seen == n)
            || crate::check::is_known_builtin_global(n)
            || matches!(
                n.as_str(),
                "undefined" | "NaN" | "Infinity" | "eval" | "arguments"
            )
        {
            continue;
        }
        names.push(n.clone());
    }
    let decls: Vec<Stmt> = names
        .into_iter()
        .map(|name| {
            let init = ast.add_expr(Expr::Uninit);
            ast.sloppy_implicit_global_names.insert(name.clone());
            Stmt::LetDecl {
                mutable: true,
                name,
                type_ann: None,
                init,
                is_var: true,
            }
        })
        .collect();
    ast.stmts.splice(0..0, decls);
}
