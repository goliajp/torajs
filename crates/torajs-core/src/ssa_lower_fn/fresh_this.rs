//! Which `this.f = v` writes at the head of a constructor body reach
//! an instance nothing else can see yet — seeded once per body, into
//! [`crate::ssa_lower::proofs::Proofs`].
//!
//! `__cm_<C>__ctor` is entered only from the factory `__new_<C>` and
//! from a derived constructor's `super(...)`, and both hand it a cell
//! the factory just minted: its header carries no frozen bit, no
//! field was redefined, and every field holds its type's default.
//! Nothing changes that until the pointer escapes — a call that takes
//! it, a closure that captures it, a read that stores it somewhere.
//! Along the leading run of statements that only write `this` fields
//! from values not involving `this`, or do not mention `this` at all,
//! the writable guard a field store carries asks a question whose
//! answer is already known, and the first write to a field whose
//! default is `null` / `undefined` has no old value to release. Both
//! are what the egraph's fresh-cell forwarding would recover after
//! inlining the constructor into its factory — but a constructor
//! carrying four guards and two inline drop expansions is too large
//! to inline, so the fact has to be spent here, where the body is
//! still small enough to splice.
//!
//! The run stops at the first statement of any other shape: a
//! `super(...)` call (the parent may freeze `this`), a branch, a
//! loop, anything that mentions `this` outside the receiver slot of
//! a field write. A default parameter mentioning `this` disqualifies
//! the whole body.

use std::collections::HashSet;

use crate::ast::{self, Expr, ExprId, Stmt};
use crate::ssa_lower::LowerCtx;

const THIS: &str = "__this";

pub(crate) fn seed_fresh_this(
    ctx: &mut LowerCtx<'_>,
    name: &str,
    params: &[ast::Param],
    body: &[Stmt],
) {
    let Some(cname) = ctor_class_name(name) else {
        return;
    };
    if params
        .iter()
        .any(|p| p.default.is_some_and(|d| mentions_this(ctx.ast, d)))
    {
        return;
    }
    let nullish_fields = nullish_default_fields(ctx.ast, cname);
    let mut written: HashSet<&str> = HashSet::new();
    for s in body {
        match s {
            Stmt::Expr(e) => {
                if let Some((recv, field, value)) = this_field_write(ctx.ast, *e) {
                    if mentions_this(ctx.ast, value) {
                        return;
                    }
                    ctx.proofs.fresh_this_recv.insert(recv);
                    if written.insert(field) && nullish_fields.contains(field) {
                        ctx.proofs.fresh_this_first_write.insert(recv);
                    }
                } else if mentions_this(ctx.ast, *e) {
                    return;
                }
            }
            Stmt::LetDecl { init, .. } => {
                if mentions_this(ctx.ast, *init) {
                    return;
                }
            }
            _ => return,
        }
    }
}

/// `__cm_<C>__ctor`, or a specialised twin of it (`$$` suffix).
fn ctor_class_name(name: &str) -> Option<&str> {
    let (cname, tail) = name.strip_prefix("__cm_")?.split_once("__ctor")?;
    (tail.is_empty() || tail.starts_with("$$")).then_some(cname)
}

/// `__this.<field> = <value>` as (receiver expr, field, value).
fn this_field_write(ast: &ast::Ast, e: ExprId) -> Option<(ExprId, &str, ExprId)> {
    let Expr::Assign { target, value } = ast.get_expr(e) else {
        return None;
    };
    let Expr::Member { obj, name } = ast.get_expr(*target) else {
        return None;
    };
    matches!(ast.get_expr(*obj), Expr::Ident(n) if n == THIS).then_some((*obj, name, *value))
}

fn mentions_this(ast: &ast::Ast, e: ExprId) -> bool {
    match ast.get_expr(e) {
        Expr::Ident(n) => n == THIS,
        Expr::This | Expr::ArrowFn { .. } => true,
        Expr::Closure { captures, .. } => captures.iter().any(|c| c == THIS),
        _ => {
            let mut found = false;
            ast::desugar_async_sm_walk::visit_expr_children(ast, e, &mut |c| {
                found = found || mentions_this(ast, c);
            });
            found
        }
    }
}

/// The fields whose factory default is `null` or `undefined` in `cname`
/// and in every class derived from it — a derived class may redeclare
/// a field with another type, and its factory then seeds a cell the
/// base constructor's first write would otherwise leak.
fn nullish_default_fields(ast: &ast::Ast, cname: &str) -> HashSet<String> {
    let mut nullish: HashSet<String> = HashSet::new();
    let mut other: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        let Stmt::TypeDecl { name, fields, .. } = s else {
            continue;
        };
        if name != cname && !derives_from(ast, name, cname) {
            continue;
        }
        for (f, ann) in fields {
            let f = f.lossy().into_owned();
            if ann.starts_with("__nullable(")
                || matches!(ann.as_str(), "any" | "undefined" | "void")
            {
                nullish.insert(f);
            } else {
                other.insert(f);
            }
        }
    }
    nullish.retain(|f| !other.contains(f));
    nullish
}

fn derives_from(ast: &ast::Ast, class: &str, ancestor: &str) -> bool {
    let mut cur = class;
    while let Some(Some(parent)) = ast.class_parents.get(cur) {
        if parent == ancestor {
            return true;
        }
        cur = parent;
    }
    false
}
