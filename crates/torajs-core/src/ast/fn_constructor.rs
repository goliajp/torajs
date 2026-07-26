//! `new F()` where `F` is a function, not a class.
//!
//! RFC 20260726-new-on-function blade 2. `desugar_classes_pass2`
//! rewrites every surviving `Expr::New` into a call to `__new_<name>`
//! without checking that the name belongs to a class, and
//! `desugar_classes_pass3` only ever mints that factory for classes. So
//! `new Con(1)` on a plain function reached the checker as a call to an
//! identifier nobody declared: 1138 cases across 42 directories.
//!
//! This pass runs after `bind_this_param`. It mints the missing factory
//! in the same shape the class one has
//! (`lift_arrow_fns::build_factory_body`) but under its own prefix, and
//! repoints the call:
//!
//! ```text
//! function __fnctor_Con(x: number): any {
//!   let __this: any = {};
//!   Con(__this, x);
//!   return __this;
//! }
//! ```
//!
//! The instance is a dynamic object rather than a nominal struct. A
//! class declares its field set; a function assigns `this.x` from
//! whichever branch it likes, so there is no layout to name — which is
//! also why the receiver parameter blade 1 adds is typed `any`.
//!
//! The prefix is deliberately NOT `__new_`. That name is load-bearing
//! for classes in more places than the factory itself: `class_globals`
//! reconstructs the entire class list by stripping it off FnDecl names
//! ("ClassDecl stmts are gone post-desugar; the factory FnDecl names
//! are the most stable handle"), and `ssa_lower_object_lit` reads it to
//! decide an object literal's class tag, vtable and error flag. Minting
//! `__new_Con` for a function therefore announced a class named `Con`,
//! and every `Ident("Con")` — including the one this factory calls —
//! got rewritten to `__class_Con`, which nothing declares.
//!
//! Whether the callee takes `__this` is read off its parameter list
//! instead of being threaded from blade 1: a function that never
//! mentions `this` (`function Empty() {}`) is still constructible, it
//! just does not receive the instance.

use super::{Ast, Expr, ExprId, Param, Stmt};

/// A function this pass can construct: its name, and the parameters the
/// factory has to forward (blade 1's hidden receiver excluded).
struct Constructible {
    name: String,
    params: Vec<Param>,
    takes_this: bool,
}

fn collect_declared(ast: &Ast) -> (Vec<String>, Vec<Constructible>) {
    let mut declared: Vec<String> = Vec::new();
    let mut candidates: Vec<Constructible> = Vec::new();
    for st in &ast.stmts {
        let Stmt::FnDecl {
            name,
            params,
            is_generator,
            ..
        } = st
        else {
            continue;
        };
        declared.push(name.clone());
        // A generator's `new` shape is its own question (P-J rewrites it
        // into a state machine), so leave those to the existing path.
        if *is_generator {
            continue;
        }
        let takes_this = params.first().is_some_and(|p| p.name == "__this");
        candidates.push(Constructible {
            name: name.clone(),
            params: if takes_this {
                params[1..].to_vec()
            } else {
                params.clone()
            },
            takes_this,
        });
    }
    (declared, candidates)
}

/// Names called as `__new_<X>` for which no `__new_<X>` was declared,
/// plus the callee expressions to repoint at `__fnctor_<X>`.
fn missing_factories(ast: &Ast, declared: &[String]) -> (Vec<String>, Vec<ExprId>) {
    let mut wanted: Vec<String> = Vec::new();
    let mut callees: Vec<ExprId> = Vec::new();
    for e in &ast.exprs {
        let Expr::Call { callee, .. } = e else {
            continue;
        };
        let Expr::Ident(n) = &ast.exprs[callee.0 as usize] else {
            continue;
        };
        let Some(bare) = n.strip_prefix("__new_") else {
            continue;
        };
        if declared.iter().any(|d| d == n) {
            continue; // a real factory exists — class path, untouched
        }
        if !wanted.iter().any(|w| w == bare) {
            wanted.push(bare.to_string());
        }
        callees.push(*callee);
    }
    (wanted, callees)
}

pub fn synthesize_fn_constructors(ast: &mut Ast) {
    let (declared, candidates) = collect_declared(ast);
    let (wanted, callees) = missing_factories(ast, &declared);
    if wanted.is_empty() {
        return;
    }

    // Repoint first, and only for names that turn out to be functions:
    // `new NotAThing()` keeps its `__new_NotAThing` callee so the
    // checker still reports the name the source actually wrote.
    for id in callees {
        let Expr::Ident(n) = &ast.exprs[id.0 as usize] else {
            continue;
        };
        let Some(bare) = n.strip_prefix("__new_") else {
            continue;
        };
        if candidates.iter().any(|c| c.name == bare) {
            let repointed = format!("__fnctor_{bare}");
            ast.exprs[id.0 as usize] = Expr::Ident(repointed);
        }
    }

    for bare in wanted {
        // Not a function either — leave the unknown identifier to the
        // checker. Reporting `new NotAThing()` as a missing name is the
        // honest failure; inventing a factory would turn it silent.
        let Some(c) = candidates.iter().find(|c| c.name == bare) else {
            continue;
        };

        let empty_obj = ast.add_expr(Expr::ObjectLit { fields: Vec::new() });
        let let_this = Stmt::LetDecl {
            mutable: true,
            name: "__this".into(),
            type_ann: Some("any".into()),
            init: empty_obj,
            is_var: false,
        };

        let callee = ast.add_expr(Expr::Ident(c.name.clone()));
        let mut args: Vec<ExprId> = Vec::with_capacity(c.params.len() + 1);
        if c.takes_this {
            args.push(ast.add_expr(Expr::Ident("__this".into())));
        }
        for p in &c.params {
            args.push(ast.add_expr(Expr::Ident(p.name.clone())));
        }
        let call = ast.add_expr(Expr::Call { callee, args });
        let ret = ast.add_expr(Expr::Ident("__this".into()));

        ast.stmts.push(Stmt::FnDecl {
            name: format!("__fnctor_{bare}"),
            type_params: Vec::new(),
            params: c.params.clone(),
            return_type: Some("any".into()),
            body: vec![let_this, Stmt::Expr(call), Stmt::Return(Some(ret))],
            is_generator: false,
            span: crate::lexer::Span { start: 0, end: 0 },
        });
    }
}
