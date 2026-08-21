//! Which `s.match(re)` / `re.exec(s)` results nobody ever asks the
//! exec-shape properties of — RFC 20260821 attack B.
//!
//! §22.2.7.8 says a successful match result carries `index`, `input`
//! and `groups` (plus `indices` under `/d`). We attach all of them on
//! every match, into the array's arrprops side table. Measured cost:
//! **16.6–18.5 ns per match**, constant across four regex bench cases
//! and equal to nearly the whole gap on `regex-wireback-minlit`
//! (18.0 ns) — because building that side table means a dynobj alloc,
//! three tagged-value boxes, an rc share of the subject string, and
//! then tearing all of it down again one iteration later.
//!
//! The reason it is pure loss in those programs is that they only ever
//! ask `m[0]` and `m !== null`. Nothing in the typed tier can even read
//! an arrprops entry — every reader (`arrprops_get_tag` /
//! `_get_value` / `_has`) lives in the anyvalue / meta dynamic layer.
//! So when a match result is bound to a local and that local is only
//! ever indexed, measured, or compared against null, the properties
//! provably have no reader and the kernel can skip building them.
//!
//! This is the AOT half of a trade the competition cannot make the same
//! way: a JIT would have to speculate and deoptimize, while we get to
//! *prove* the absence of a reader before emitting the call.
//!
//! ## Shape
//!
//! Sibling of [`super::escape_analyze`], which asks the same question
//! about a different property (can this array literal live on the
//! stack). Same walk, same bias rule, one different safe set:
//!
//!   - `X[i]`      — reads an element, never a property
//!   - `X.length`  — the array's own length, not an arrprops entry
//!   - `X === null` / `X !== null` / `X == null` / `X != null`
//!   - `X` as an `if` / `while` / `!` condition — truthiness only
//!
//! Anything else disqualifies: a bare `X` that flows anywhere (a call
//! argument, a return, another binding, a container element) can reach
//! the dynamic layer, and `X.index` / `X.groups` / `X[k]` with a
//! non-numeric key read the table outright. `console.log(X)` is a bare
//! `X` in a call argument, so the print face — which *does* show the
//! exec shape — is disqualified by the general rule rather than by a
//! special case.
//!
//! **Bias rule (inherited from the sibling, and the reason this is
//! safe): a false negative just keeps the properties, a false positive
//! is a silently missing property. Every uncertain shape answers
//! false.**

use super::{Ast, BinOp, Expr, ExprId, Stmt, UnaryOp};

/// Record the `.match` / `.exec` callee ExprIds whose result provably
/// has no exec-shape reader. `ssa_lower_call_str_regex_methods` and
/// `ssa_lower_call_regex_methods` receive that same callee id and pass
/// `want_exec = 0` to the kernel for the ones listed here.
///
/// Runs at the end of the desugar pipeline, for the sibling's reason:
/// the verifier has to see the final shape of the program, after every
/// rewrite that could introduce a new use of the binding.
pub fn analyze_regex_result_props(ast: &mut Ast) {
    let mut found: std::collections::HashSet<ExprId> = std::collections::HashSet::new();
    let stmts = ast.stmts.clone();
    rrp_walk_stmts(ast, &stmts, &mut found);
    ast.regex_result_props_unread = found;
}

/// The `.match` / `.exec` callee id of `init`, when `init` is such a
/// call. `matchAll` is deliberately absent: its elements are separate
/// arrays produced by the iterator, not this call's result.
fn regex_result_callee(ast: &Ast, init: ExprId) -> Option<ExprId> {
    let Expr::Call { callee, .. } = ast.get_expr(init) else {
        return None;
    };
    let Expr::Member { name, .. } = ast.get_expr(*callee) else {
        return None;
    };
    match name.as_str() {
        "match" | "exec" => Some(*callee),
        _ => None,
    }
}

fn rrp_walk_stmts(ast: &Ast, stmts: &[Stmt], found: &mut std::collections::HashSet<ExprId>) {
    for (i, s) in stmts.iter().enumerate() {
        if let Stmt::LetDecl { name, init, .. } = s
            && let Some(callee) = regex_result_callee(ast, *init)
        {
            // `let` is in scope from its declaration to the end of the
            // block, so the trailing statements are every place the
            // binding can be read — the sibling's window, same reason.
            let trailing = &stmts[i + 1..];
            if trailing.iter().all(|s| rrp_stmt_safe(ast, s, name)) {
                found.insert(callee);
            }
        }
    }
    for s in stmts {
        rrp_recurse_into(ast, s, found);
    }
}

fn rrp_recurse_into(ast: &Ast, s: &Stmt, found: &mut std::collections::HashSet<ExprId>) {
    match s {
        Stmt::Block(inner) | Stmt::Multi(inner) => rrp_walk_stmts(ast, inner, found),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rrp_recurse_into(ast, then_branch, found);
            if let Some(eb) = else_branch {
                rrp_recurse_into(ast, eb, found);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => rrp_recurse_into(ast, body, found),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                rrp_recurse_into(ast, i, found);
            }
            rrp_recurse_into(ast, body, found);
        }
        Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => {
            rrp_recurse_into(ast, body, found)
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                rrp_walk_stmts(ast, &c.body, found);
            }
            if let Some(db) = default {
                rrp_walk_stmts(ast, db, found);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            rrp_walk_stmts(ast, body, found);
            rrp_walk_stmts(ast, catch_body, found);
            if let Some(fb) = finally_body {
                rrp_walk_stmts(ast, fb, found);
            }
        }
        Stmt::FnDecl { body, .. } => rrp_walk_stmts(ast, body, found),
        Stmt::ClassDecl { methods, .. } => {
            for m in methods {
                rrp_walk_stmts(ast, &m.body, found);
            }
        }
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                rrp_recurse_into(ast, inner, found);
            }
        }
        Stmt::Labeled { body, .. } => rrp_recurse_into(ast, body, found),
        _ => {}
    }
}

/// `Null` after peeling the parenthesised / `as` wrappers a comparison
/// against null can carry.
fn is_null_literal(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Null => true,
        Expr::As { expr, .. } => is_null_literal(ast, *expr),
        _ => false,
    }
}

fn is_bare_x(ast: &Ast, eid: ExprId, x_name: &str) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(n) => n == x_name,
        Expr::As { expr, .. } => is_bare_x(ast, *expr, x_name),
        _ => false,
    }
}

/// A condition position reads only truthiness, which for an Array is
/// "is it null/undefined" — no property is consulted, so a bare `X`
/// is safe here and nowhere else outside the null comparisons.
fn rrp_cond_safe(ast: &Ast, eid: ExprId, x_name: &str) -> bool {
    is_bare_x(ast, eid, x_name) || rrp_expr_safe(ast, eid, x_name)
}

fn rrp_stmt_safe(ast: &Ast, s: &Stmt, x_name: &str) -> bool {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) => rrp_expr_safe(ast, *eid, x_name),
        Stmt::Yield(eid) | Stmt::YieldInto { value: eid, .. } => rrp_expr_safe(ast, *eid, x_name),
        Stmt::Return(Some(eid)) => rrp_expr_safe(ast, *eid, x_name),
        Stmt::Return(None) | Stmt::Break(_) | Stmt::Continue(_) => true,
        Stmt::Labeled { body, .. } => rrp_stmt_safe(ast, body, x_name),
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            rrp_expr_safe(ast, *init, x_name)
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            rrp_cond_safe(ast, *cond, x_name)
                && rrp_stmt_safe(ast, then_branch, x_name)
                && else_branch
                    .as_deref()
                    .is_none_or(|e| rrp_stmt_safe(ast, e, x_name))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            rrp_cond_safe(ast, *cond, x_name) && rrp_stmt_safe(ast, body, x_name)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_deref()
                .is_none_or(|i| rrp_stmt_safe(ast, i, x_name))
                && cond.is_none_or(|c| rrp_cond_safe(ast, c, x_name))
                && step.is_none_or(|st| rrp_expr_safe(ast, st, x_name))
                && rrp_stmt_safe(ast, body, x_name)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            rrp_expr_safe(ast, *parent, x_name)
                && rrp_expr_safe(ast, *sep, x_name)
                && rrp_stmt_safe(ast, body, x_name)
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => rrp_expr_safe(ast, *elem_expr, x_name) && rrp_stmt_safe(ast, body, x_name),
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            rrp_expr_safe(ast, *scrutinee, x_name)
                && cases.iter().all(|c| {
                    rrp_expr_safe(ast, c.value, x_name)
                        && c.body.iter().all(|s| rrp_stmt_safe(ast, s, x_name))
                })
                && default
                    .as_ref()
                    .is_none_or(|db| db.iter().all(|s| rrp_stmt_safe(ast, s, x_name)))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().all(|s| rrp_stmt_safe(ast, s, x_name))
                && catch_body.iter().all(|s| rrp_stmt_safe(ast, s, x_name))
                && finally_body
                    .as_ref()
                    .is_none_or(|fb| fb.iter().all(|s| rrp_stmt_safe(ast, s, x_name)))
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().all(|s| rrp_stmt_safe(ast, s, x_name))
        }
        // A nested function body cannot see this block's `let` unless
        // it captured it, and a capture shows up as `Expr::Closure`
        // (handled below) — an un-lifted body here refers to some
        // other binding of the same name.
        Stmt::FnDecl { .. }
        | Stmt::TypeDecl { .. }
        | Stmt::ClassDecl { .. }
        | Stmt::ImportDecl { .. } => true,
        Stmt::ExportDecl { inner, .. } => inner
            .as_deref()
            .is_none_or(|s| rrp_stmt_safe(ast, s, x_name)),
    }
}

fn rrp_expr_safe(ast: &Ast, eid: ExprId, x_name: &str) -> bool {
    match ast.get_expr(eid) {
        Expr::Elision => true,
        // A bare X flows: into a call argument (`console.log(m)` —
        // which is exactly how the print face reads the exec shape),
        // a return, another binding, a container. All of those can
        // reach the dynamic layer, so only the two positions handled
        // by their own arms below get to say otherwise.
        Expr::Ident(n) => n != x_name,
        Expr::Member { obj, name } => {
            if is_bare_x(ast, *obj, x_name) {
                // `.index` / `.input` / `.groups` / `.indices` read the
                // very table in question; `.length` is the array's own
                // header field and every other name would go looking
                // in arrprops too.
                return name == "length";
            }
            rrp_expr_safe(ast, *obj, x_name)
        }
        Expr::Index { obj, index } => {
            if is_bare_x(ast, *obj, x_name) {
                // An element read. A non-numeric key would consult
                // arrprops, but the key's own type is not known here —
                // so admit only what is syntactically a number.
                if !matches!(ast.get_expr(*index), Expr::Number(_)) {
                    return false;
                }
                return rrp_expr_safe(ast, *index, x_name);
            }
            rrp_expr_safe(ast, *obj, x_name) && rrp_expr_safe(ast, *index, x_name)
        }
        Expr::BinOp { op, left, right } => {
            // `X === null` and friends: the comparison reads identity
            // and nullness, never a property.
            let eq = matches!(
                op,
                BinOp::Eq | BinOp::Neq | BinOp::LooseEq | BinOp::LooseNeq
            );
            if eq
                && ((is_bare_x(ast, *left, x_name) && is_null_literal(ast, *right))
                    || (is_null_literal(ast, *left) && is_bare_x(ast, *right, x_name)))
            {
                return true;
            }
            rrp_expr_safe(ast, *left, x_name) && rrp_expr_safe(ast, *right, x_name)
        }
        Expr::Unary { op, expr } => {
            // `!X` is a truthiness test, same as an `if` condition.
            if matches!(op, UnaryOp::Not) && is_bare_x(ast, *expr, x_name) {
                return true;
            }
            rrp_expr_safe(ast, *expr, x_name)
        }
        Expr::Call { callee, args } => {
            rrp_expr_safe(ast, *callee, x_name)
                && args.iter().all(|a| rrp_expr_safe(ast, *a, x_name))
        }
        Expr::Assign { target, value } => {
            rrp_expr_safe(ast, *target, x_name) && rrp_expr_safe(ast, *value, x_name)
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            rrp_cond_safe(ast, *cond, x_name)
                && rrp_expr_safe(ast, *then_branch, x_name)
                && rrp_expr_safe(ast, *else_branch, x_name)
        }
        Expr::Array(els) => els.iter().all(|e| rrp_expr_safe(ast, *e, x_name)),
        Expr::ObjectLit { fields } => fields.iter().all(|(_, e)| rrp_expr_safe(ast, *e, x_name)),
        Expr::Spread { expr } => rrp_expr_safe(ast, *expr, x_name),
        Expr::Nullish { lhs, rhs } => {
            // `X ?? d` yields X itself when it is non-null — an escape.
            rrp_expr_safe(ast, *lhs, x_name) && rrp_expr_safe(ast, *rhs, x_name)
        }
        Expr::OptChain { obj, .. } | Expr::OptIndex { obj, .. } => {
            if is_bare_x(ast, *obj, x_name) {
                return false;
            }
            rrp_expr_safe(ast, *obj, x_name)
        }
        Expr::OptCall { callee, args } => {
            rrp_expr_safe(ast, *callee, x_name)
                && args.iter().all(|a| rrp_expr_safe(ast, *a, x_name))
        }
        Expr::PostIncr { target, .. } => rrp_expr_safe(ast, *target, x_name),
        Expr::TypeOf { expr } | Expr::Delete { expr } => rrp_expr_safe(ast, *expr, x_name),
        Expr::InstanceOf { expr, rhs } => {
            rrp_expr_safe(ast, *expr, x_name) && rrp_expr_safe(ast, *rhs, x_name)
        }
        Expr::As { expr, .. } => rrp_expr_safe(ast, *expr, x_name),
        Expr::Sequence { left, right } => {
            rrp_expr_safe(ast, *left, x_name) && rrp_expr_safe(ast, *right, x_name)
        }
        Expr::Closure { captures, .. } => !captures.iter().any(|n| n == x_name),
        // Un-lifted arrow: its body is not walked here, so any capture
        // is invisible. Bail, exactly as the sibling does.
        Expr::ArrowFn { .. } => false,
        Expr::New { args, .. } => args.iter().all(|a| rrp_expr_safe(ast, *a, x_name)),
        Expr::NewDynamic { callee, args } => {
            rrp_expr_safe(ast, *callee, x_name)
                && args.iter().all(|a| rrp_expr_safe(ast, *a, x_name))
        }
        Expr::Super { args } => args.iter().all(|a| rrp_expr_safe(ast, *a, x_name)),
        Expr::This
        | Expr::NewTarget
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Regex { .. } => true,
    }
}
