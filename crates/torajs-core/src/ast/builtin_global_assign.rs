//! Assigning to a built-in global name.
//!
//! §10.2.4 puts the built-in constructors and namespace objects on the
//! global object as ORDINARY writable properties, so a program may
//! replace one — test262 leans on it constantly, saving the original
//! and installing a throwing stand-in to prove some internal read does
//! not go through the public name:
//!
//! ```text
//! var oldArray = Array;
//! Array = function () { throw new Error("invoke customer defined Array!") };
//! Object.getOwnPropertyNames({})        // must still answer an Array
//! ```
//!
//! tr resolves those names at COMPILE time — `Array` is an intrinsic,
//! not a slot — so the write had nowhere to land and the checker
//! answered `assignment to undeclared \`Array\``. Reading one as a
//! value already worked (`let A: any = Array` binds the constructor
//! and `new A(3)` / `A.isArray([])` / `x instanceof A` all dispatch),
//! which is the whole shape this pass needs.
//!
//! So: when a program assigns to a built-in global name, that name
//! stops being an intrinsic reference for the rest of the program and
//! becomes an ordinary `any` binding seeded with the intrinsic. Every
//! existing mention is renamed onto the binding; the seed keeps the
//! original spelling and is the one site still resolving to the
//! built-in. A `new <name>(…)` mention becomes the dynamic-callee
//! form, since after the write there is no compile-time class to bind
//! a factory to.
//!
//! Nothing happens to a program that only READS these names, which is
//! all but a handful — the hot paths keep their intrinsic resolution.
//!
//! Out of scope, deliberately: the non-writable globals (`undefined`,
//! `NaN`, `Infinity`, `globalThis`) — §10.2.4 makes those
//! `[[Writable]]: false`, so a sloppy assignment to one is a silent
//! no-op and a strict one is a TypeError, neither of which is a
//! binding. They keep today's refusal.

use super::{Ast, Expr, Stmt};
use std::collections::HashSet;

/// The prefix a demoted name takes. Chosen to be unspellable in
/// source so the rename cannot collide with a user binding.
const SHADOW_PREFIX: &str = "__gshadow_";

pub fn demote_assigned_builtin_globals(ast: &mut Ast) {
    let mut names: HashSet<String> = HashSet::new();
    for e in &ast.exprs {
        // An update expression writes its target too (`eval--`, the
        // t262 postfix-decrement/eval-nostrict shape) — rotation 628.
        let target = match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => *target,
            _ => continue,
        };
        let Expr::Ident(n) = ast.get_expr(target) else {
            continue;
        };
        if is_demotable(n) {
            names.insert(n.clone());
        }
    }
    if names.is_empty() {
        return;
    }
    // A name the program itself declares is not the global at all —
    // the existing shadow machinery owns that case.
    names.retain(|n| !declares_name(&ast.stmts, n));
    if names.is_empty() {
        return;
    }
    // name order, not hash order: the seeds run in this order
    let mut ordered: Vec<&String> = names.iter().collect();
    ordered.sort();
    let mut seeds: Vec<Stmt> = Vec::new();
    for name in ordered {
        let init = ast.add_expr(Expr::Ident(name.clone()));
        seeds.push(Stmt::LetDecl {
            mutable: true,
            name: format!("{SHADOW_PREFIX}{name}"),
            type_ann: Some("any".into()),
            init,
            is_var: false,
        });
    }
    // The seed initializers were minted LAST, so renaming everything
    // before them leaves each seed reading the intrinsic it stands
    // for. Walk by index and stop at the first seed expression.
    let first_seed = ast.exprs.len() - names.len();
    for i in 0..first_seed {
        match &mut ast.exprs[i] {
            Expr::Ident(n) if names.contains(n) => {
                *n = format!("{SHADOW_PREFIX}{n}");
            }
            _ => {}
        }
    }
    // `new Array(3)` names its target as a string, so the rename above
    // cannot see it. After the write there is no compile-time class
    // to bind a factory to anyway, so it becomes the dynamic form.
    let mut news: Vec<(usize, String, Vec<super::ExprId>)> = Vec::new();
    for (i, e) in ast.exprs[..first_seed].iter().enumerate() {
        if let Expr::New {
            class_name, args, ..
        } = e
            && names.contains(class_name)
        {
            news.push((i, class_name.clone(), args.clone()));
        }
    }
    for (i, class_name, args) in news {
        let callee = ast.add_expr(Expr::Ident(format!("{SHADOW_PREFIX}{class_name}")));
        ast.exprs[i] = Expr::NewDynamic { callee, args };
    }
    seeds.append(&mut ast.stmts);
    ast.stmts = seeds;
}

/// A built-in global whose property is writable — the constructors and
/// namespace objects, and `eval` (§19.2.1 is an ordinary writable
/// function property; a sloppy `eval = 1` is a plain global write,
/// and the direct-call desugar has already run by the time this pass
/// walks, so what is left of the name is its value). The value
/// globals §10.2.4 marks non-writable are excluded by name;
/// `arguments` is never a global.
fn is_demotable(name: &str) -> bool {
    if matches!(
        name,
        "undefined" | "NaN" | "Infinity" | "globalThis" | "arguments"
    ) {
        return false;
    }
    !name.starts_with("__") && super::free_vars_globals::is_global_name(name)
}

/// Whether the program declares the name itself anywhere — a
/// declaration, parameter, catch or loop variable.
fn declares_name(stmts: &[Stmt], name: &str) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::LetDecl { name: n, .. } => n == name,
        Stmt::FnDecl {
            name: n,
            params,
            body,
            ..
        } => n == name || params.iter().any(|p| p.name == name) || declares_name(body, name),
        Stmt::ClassDecl { name: n, .. } => n == name,
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            catch_param.as_deref() == Some(name)
                || declares_name(body, name)
                || declares_name(catch_body, name)
                || finally_body
                    .as_ref()
                    .is_some_and(|f| declares_name(f, name))
        }
        Stmt::ForOf { var_name, body, .. } | Stmt::ForOfSplitIter { var_name, body, .. } => {
            var_name == name || declares_name(std::slice::from_ref(body), name)
        }
        Stmt::For { init, body, .. } => {
            init.as_ref()
                .is_some_and(|i| declares_name(std::slice::from_ref(i), name))
                || declares_name(std::slice::from_ref(body), name)
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
            declares_name(std::slice::from_ref(body), name)
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            declares_name(std::slice::from_ref(then_branch), name)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| declares_name(std::slice::from_ref(e), name))
        }
        Stmt::Switch { cases, default, .. } => {
            cases.iter().any(|c| declares_name(&c.body, name))
                || default.as_ref().is_some_and(|d| declares_name(d, name))
        }
        Stmt::Block(inner) | Stmt::Multi(inner) => declares_name(inner, name),
        _ => false,
    })
}
