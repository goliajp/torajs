//! M4.3.b — per-fn throw-shape analysis, split out of `ast.rs`
//! (file-size known-debt: ast.rs only shrinks).
//!
//! Describe a fn's throw shape: `direct_throw` is true if any `throw`
//! statement appears anywhere in the body; `called_fns` is the
//! deduplicated list of identifier names referenced as direct call
//! callees (`Expr::Call { callee: Ident(name) }`). The lowerer combines
//! this with a fixed-point closure to compute may_throw transitively.
//!
//! P7.4-a-b — `direct_throw` is ALSO set for IMPLICIT runtime throws the
//! AST has no `throw` node for: a bigint `Div/Mod/Pow/Shl/Shr` (the
//! runtime helper calls `__torajs_throw_range_error` on divide-by-zero
//! / negative-exponent / shift-too-large) and a `BigInt(x)` conversion
//! (RangeError on a non-integer / non-finite argument). Without this a
//! named fn whose only throw is one of these wouldn't be in `may_throw`,
//! so the caller's `emit_throw_check` would be skipped and the real
//! RangeError silently swallowed. The bigint-op test mirrors ssa_lower's
//! runtime dispatch condition exactly — `op ∈ {Div,Mod,Pow,Shl,Shr}`
//! AND BOTH operands type `check::Type::BigInt` per `expr_types` — so it
//! is precise: number `/ % ** << >>` is never marked (no bench-tr
//! hot-path throw-check regression), and every bigint throwing op is
//! (no silent swallow).
//!
//! bug-327 C2.5 — `direct_throw` is ALSO set when the fn calls through
//! a fn-valued binding (`thunk()` where thunk is a param, or `h()`
//! where h is a body-local `let`). The callee is statically unknown, so
//! the fixed-point can't resolve the name against any FnDecl — before
//! this rule the indirect call simply fell out of the analysis and a
//! throwing callback was silently swallowed at the caller's caller.
//! Param / let names are exact here: the typechecker only admits a
//! call through an ident whose binding is fn-typed, and builtin ctor
//! names (parseInt, Number, ...) are neither params nor body lets, so
//! hot conversion calls stay unmarked.
//!
//! Chunk 701 — `direct_throw` is ALSO set for method calls
//! (`Expr::Call { callee: Member { .. } }`, console.* exempt): the
//! method lowerings bottom out in runtime helpers that record pending
//! throws (kind-mismatch mutators, arr_any_to_typed, JSON.parse, OOB
//! index-assign, any-receiver dispatch, ...). Before this rule a
//! runtime TypeError raised inside a named fn unwound the fn body but
//! the caller's throw-check was M4.3.b-skipped: try/catch around the
//! call printed "no throw", a non-void callee answered the ret
//! sentinel 0, and top-level execution continued with exit code 0.
//! Method names are NOT mirrored per-shape (68 in-body throw-check
//! sites across the lowerers; a missed mirror is a silent swallow) —
//! the default is conservative and the exempt list carries the
//! never-throwing hot surfaces instead, because the two directions
//! fail differently: an over-wide exempt entry swallows, an
//! over-narrow default costs one cold TLS-load check.

use crate::ast::{Ast, BinOp, Expr, ExprId, Param, Stmt};
use std::collections::{HashMap, HashSet};

/// M4.3.b — the transitive may-throw set: collect each FnDecl's
/// `(direct_throw, called_names)` then iterate to fixed-point — a fn
/// is may_throw if it throws directly OR calls any may_throw fn.
pub fn compute_may_throw_fns(
    ast: &Ast,
    expr_types: &HashMap<ExprId, crate::check::Type>,
) -> HashSet<String> {
    let mut may_throw: HashSet<String> = HashSet::new();
    let mut decl_throw_info: Vec<(String, bool, Vec<String>)> = Vec::new();
    for stmt in &ast.stmts {
        if let Stmt::FnDecl {
            name, params, body, ..
        } = stmt
        {
            let (direct, called) = fn_throw_info(ast, params, body, expr_types);
            if direct {
                may_throw.insert(name.clone());
            }
            decl_throw_info.push((name.clone(), direct, called));
        }
    }
    loop {
        let mut grew = false;
        for (name, _direct, called) in &decl_throw_info {
            if may_throw.contains(name) {
                continue;
            }
            for c in called {
                if may_throw.contains(c) {
                    may_throw.insert(name.clone());
                    grew = true;
                    break;
                }
            }
        }
        if !grew {
            break;
        }
    }
    may_throw
}

pub fn fn_throw_info(
    ast: &Ast,
    params: &[Param],
    body: &[Stmt],
    expr_types: &HashMap<ExprId, crate::check::Type>,
) -> (bool, Vec<String>) {
    let mut direct = false;
    let mut called: Vec<String> = Vec::new();
    // Fn-valued bindings visible to calls in this body: every param
    // name (a call through one is necessarily fn-typed — the checker
    // rejects calling a number), plus body `let`s as the stmt scan
    // encounters them (declaration-before-use holds post-var-hoist).
    let mut fn_values: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
    for s in body {
        scan_stmt(ast, s, &mut direct, &mut called, &mut fn_values, expr_types);
    }
    (direct, called)
}

fn scan_stmt(
    ast: &Ast,
    s: &Stmt,
    direct: &mut bool,
    called: &mut Vec<String>,
    fn_values: &mut HashSet<String>,
    expr_types: &HashMap<ExprId, crate::check::Type>,
) {
    match s {
        Stmt::Throw(eid) => {
            *direct = true;
            scan_expr(ast, *eid, called, direct, fn_values, expr_types);
        }
        Stmt::Expr(eid) | Stmt::Return(Some(eid)) | Stmt::Yield(eid) => {
            scan_expr(ast, *eid, called, direct, fn_values, expr_types)
        }
        Stmt::YieldInto { value, .. } => {
            scan_expr(ast, *value, called, direct, fn_values, expr_types)
        }
        Stmt::Return(None) | Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::Labeled { body, .. } => {
            scan_stmt(ast, body, direct, called, fn_values, expr_types);
        }
        Stmt::LetDecl { name, init, .. } => {
            scan_expr(ast, *init, called, direct, fn_values, expr_types);
            fn_values.insert(name.clone());
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            scan_expr(ast, *cond, called, direct, fn_values, expr_types);
            scan_stmt(ast, then_branch, direct, called, fn_values, expr_types);
            if let Some(eb) = else_branch {
                scan_stmt(ast, eb, direct, called, fn_values, expr_types);
            }
        }
        Stmt::While { cond, body } => {
            scan_expr(ast, *cond, called, direct, fn_values, expr_types);
            scan_stmt(ast, body, direct, called, fn_values, expr_types);
        }
        Stmt::DoWhile { body, cond } => {
            scan_stmt(ast, body, direct, called, fn_values, expr_types);
            scan_expr(ast, *cond, called, direct, fn_values, expr_types);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            scan_expr(ast, *scrutinee, called, direct, fn_values, expr_types);
            for c in cases {
                scan_expr(ast, c.value, called, direct, fn_values, expr_types);
                for s in &c.body {
                    scan_stmt(ast, s, direct, called, fn_values, expr_types);
                }
            }
            if let Some(db) = default {
                for s in db {
                    scan_stmt(ast, s, direct, called, fn_values, expr_types);
                }
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                scan_stmt(ast, i, direct, called, fn_values, expr_types);
            }
            if let Some(c) = cond {
                scan_expr(ast, *c, called, direct, fn_values, expr_types);
            }
            if let Some(st) = step {
                scan_expr(ast, *st, called, direct, fn_values, expr_types);
            }
            scan_stmt(ast, body, direct, called, fn_values, expr_types);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for st in stmts {
                scan_stmt(ast, st, direct, called, fn_values, expr_types);
            }
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            scan_expr(ast, *parent, called, direct, fn_values, expr_types);
            scan_expr(ast, *sep, called, direct, fn_values, expr_types);
            scan_stmt(ast, body, direct, called, fn_values, expr_types);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            scan_expr(ast, *elem_expr, called, direct, fn_values, expr_types);
            scan_stmt(ast, body, direct, called, fn_values, expr_types);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for st in body {
                scan_stmt(ast, st, direct, called, fn_values, expr_types);
            }
            for st in catch_body {
                scan_stmt(ast, st, direct, called, fn_values, expr_types);
            }
            if let Some(fb) = finally_body {
                for st in fb {
                    scan_stmt(ast, st, direct, called, fn_values, expr_types);
                }
            }
        }
        Stmt::FnDecl { .. } | Stmt::TypeDecl { .. } => {}
        Stmt::ClassDecl { .. } => {
            // desugar_classes runs before throw analysis; classes split
            // into FnDecls each get their own throw-info pass.
        }
        Stmt::ImportDecl { .. } => {}
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                scan_stmt(ast, inner, direct, called, fn_values, expr_types);
            }
        }
    }
}

/// `Member { obj: <Arr-or-Any receiver>, name: "length" }` in a
/// WRITE position — the shape whose store rides the throwing
/// §10.4.2.5 resize helper (see the Assign / PostIncr arms).
fn is_arr_length_member(
    ast: &Ast,
    eid: ExprId,
    expr_types: &HashMap<ExprId, crate::check::Type>,
) -> bool {
    matches!(ast.get_expr(eid), Expr::Member { obj, name }
    if name == "length"
        && matches!(
            expr_types.get(obj),
            Some(crate::check::Type::Array(_)) | Some(crate::check::Type::Any)
        ))
}

pub(crate) fn scan_expr(
    ast: &Ast,
    eid: ExprId,
    out: &mut Vec<String>,
    direct: &mut bool,
    fn_values: &HashSet<String>,
    expr_types: &HashMap<ExprId, crate::check::Type>,
) {
    match ast.get_expr(eid) {
        Expr::Elision => {}
        Expr::Call { callee, args } => {
            crate::ast_throw_info_call::scan_call(
                ast, *callee, args, out, direct, fn_values, expr_types,
            );
        }
        Expr::BinOp { op, left, right } => {
            // P7.4-a-b — a bigint Div/Mod/Pow/Shl/Shr lowers to a
            // runtime helper that calls __torajs_throw_range_error
            // (divide-by-zero / negative exponent / shift too large).
            // This mirrors ssa_lower's runtime dispatch condition
            // EXACTLY (op in the throwing set AND both operands typed
            // BigInt) so it is precise: number `/ % ** << >>` is never
            // flagged (no bench-tr hot-path throw-check regression),
            // and every bigint throwing op is (no silent swallow
            // across a named-fn boundary).
            if matches!(
                op,
                BinOp::Div | BinOp::Mod | BinOp::Pow | BinOp::Shl | BinOp::Shr
            ) && matches!(expr_types.get(left), Some(crate::check::Type::BigInt))
                && matches!(expr_types.get(right), Some(crate::check::Type::BigInt))
            {
                *direct = true;
            }
            scan_expr(ast, *left, out, direct, fn_values, expr_types);
            scan_expr(ast, *right, out, direct, fn_values, expr_types);
        }
        Expr::Unary { expr, .. } => scan_expr(ast, *expr, out, direct, fn_values, expr_types),
        Expr::Member { obj, .. } => {
            // RFC 20260713 blade 6 — an any-receiver member READ lowers
            // to __torajs_any_member_get_tag, which records a pending
            // TypeError on a null/undefined receiver. Before this rule
            // a named fn whose only throw source was `p.x` (p: any) was
            // M4.3.b-skipped at the caller: the pending throw was
            // silently dropped, the read answered garbage NaN-box bits
            // (`[unknown-any-tag]`), and a later drop walk could
            // SIGSEGV. Mirrors the chunk-701 method-call rule; typed
            // receivers stay unmarked (no hot-path throw-check cost).
            if matches!(expr_types.get(obj), Some(crate::check::Type::Any)) {
                *direct = true;
            }
            scan_expr(ast, *obj, out, direct, fn_values, expr_types)
        }
        Expr::Assign { target, value } => {
            // Rotation 270 — `xs.length = v` routes through the
            // §10.4.2.5 resize helper, which throws (RangeError on
            // an invalid length value, TypeError on a locked one).
            // Before this rule a named fn whose ONLY throw source
            // was a length assign stayed out of may_throw, the
            // caller pruned its check, and the pending throw was
            // silently dropped — the callee answered the ret
            // sentinel 0 under a try/catch that printed "no throw";
            // the arguments LiveLength face turned that swallow into
            // a SIGSEGV (test262 S10.6_A5_T4).
            if is_arr_length_member(ast, *target, expr_types) {
                *direct = true;
            }
            scan_expr(ast, *target, out, direct, fn_values, expr_types);
            scan_expr(ast, *value, out, direct, fn_values, expr_types);
        }
        Expr::Index { obj, index } => {
            // Same as the Member arm: any-receiver index reads throw on
            // null/undefined via __torajs_any_index.
            if matches!(expr_types.get(obj), Some(crate::check::Type::Any)) {
                *direct = true;
            }
            scan_expr(ast, *obj, out, direct, fn_values, expr_types);
            scan_expr(ast, *index, out, direct, fn_values, expr_types);
        }
        Expr::Array(elems) => {
            for e in elems {
                scan_expr(ast, *e, out, direct, fn_values, expr_types);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                scan_expr(ast, *e, out, direct, fn_values, expr_types);
            }
        }
        // ArrowFn / Closure bodies are walked separately (their own
        // FnDecls — lifted by lift_arrow_fns); from this fn's
        // perspective the closure body's calls don't propagate to the
        // outer fn's may_throw bit until the outer fn actually invokes
        // the closure (which is itself a Call → tracked above). Same
        // reasoning applies to the bigint-throw bit: a throwing bigint
        // op INSIDE the closure belongs to the closure's own lifted
        // FnDecl, not the enclosing fn.
        Expr::ArrowFn { .. } | Expr::Closure { .. } => {}
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                scan_expr(ast, *a, out, direct, fn_values, expr_types);
            }
        }
        Expr::NewDynamic { callee, args } => {
            scan_expr(ast, *callee, out, direct, fn_values, expr_types);
            for a in args {
                scan_expr(ast, *a, out, direct, fn_values, expr_types);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            scan_expr(ast, *cond, out, direct, fn_values, expr_types);
            scan_expr(ast, *then_branch, out, direct, fn_values, expr_types);
            scan_expr(ast, *else_branch, out, direct, fn_values, expr_types);
        }
        Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. }
        | Expr::As { expr, .. } => scan_expr(ast, *expr, out, direct, fn_values, expr_types),
        Expr::Sequence { left, right } => {
            scan_expr(ast, *left, out, direct, fn_values, expr_types);
            scan_expr(ast, *right, out, direct, fn_values, expr_types);
        }
        Expr::Nullish { lhs, rhs } => {
            scan_expr(ast, *lhs, out, direct, fn_values, expr_types);
            scan_expr(ast, *rhs, out, direct, fn_values, expr_types);
        }
        Expr::OptChain { obj, .. } => scan_expr(ast, *obj, out, direct, fn_values, expr_types),
        Expr::OptIndex { obj, index } => {
            scan_expr(ast, *obj, out, direct, fn_values, expr_types);
            scan_expr(ast, *index, out, direct, fn_values, expr_types);
        }
        // `f?.(…)` — the callee is a dynamic value (any_call raises a
        // catchable TypeError on non-callables; a statically-plain
        // callee delegates to the Call lowering): conservative
        // may-throw, same as the Index/OptChain-callee rule above.
        Expr::OptCall { callee, args } => {
            *direct = true;
            scan_expr(ast, *callee, out, direct, fn_values, expr_types);
            for a in args {
                if let Expr::Closure { fn_name, .. } = ast.get_expr(*a)
                    && !out.contains(fn_name)
                {
                    out.push(fn_name.clone());
                }
                scan_expr(ast, *a, out, direct, fn_values, expr_types);
            }
        }
        Expr::PostIncr { target, .. } => {
            // `xs.length--` — same §10.4.2.5 resize-helper throw
            // face as the Assign arm above.
            if is_arr_length_member(ast, *target, expr_types) {
                *direct = true;
            }
            scan_expr(ast, *target, out, direct, fn_values, expr_types)
        }
        Expr::This | Expr::NewTarget => {}
        // RFC 20260730-undeclared-ident — a marked unresolvable read
        // raises ReferenceError at evaluation, so the enclosing fn
        // must enter `may_throw` or callers prune the check and the
        // throw is silently swallowed across the fn boundary.
        Expr::Ident(_) if ast.undeclared_reads.contains_key(&eid) => {
            *direct = true;
        }
        Expr::Ident(_)
        | Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Regex { .. } => {}
    }
}
