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

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::{HashMap, HashSet};

pub use crate::ast_throw_info_fixpoint::compute_may_throw_fns;

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
        Stmt::LetDecl { name, init, .. } | Stmt::UsingDecl { name, init, .. } => {
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
        // A `for..of` iterates its source under the same §7.4.2
        // protocol a spread does and has the same blind spot
        // (`scan_spread`'s doc). The source is the receiver of the
        // parser-minted element read (`src[i]`, see the variant doc).
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            if let Expr::Index { obj, .. } = ast.get_expr(*elem_expr) {
                crate::ast_throw_info_call::scan_spread(
                    ast, *obj, called, direct, fn_values, expr_types,
                );
            }
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

/// A property STORE target (`o.k = v`, `o.k++`, `xs[i] = v`, `xs[i]++`)
/// — the shape whose store can raise, whatever the receiver is.
///
/// This used to be an inclusion whitelist of receiver types, grown
/// one entry at a time: `xs.length` (rotation 270, the §10.4.2.5
/// resize helper), a `ClassRef`/`Struct` field (rotation 373, the
/// §10.1.9 writable gate), and `Any` by way of the `Expr::Member`
/// read arm below. Three arms, one reason — and the whitelist shape
/// was structurally wrong, because the store ladder it is meant to
/// mirror ends in a lane that takes EVERYTHING ELSE: a receiver with
/// no arm of its own is boxed and routed through
/// `__torajs_any_member_set`, which raises. So the receivers nobody
/// had thought to name were exactly the ones whose throw went
/// missing, and the one that surfaced it is a receiver the ladder
/// never even had to reach for: a specialized body's parameter.
///
/// `function f(n) { n.nope = 1 }` called as `f(1)` types `n` as
/// `Number` — not `Any`, because specialization already resolved it —
/// so no arm matched, the caller pruned its check under M4.3.b, and
/// the strict-mode TypeError §10.1.8.1 owes a primitive receiver was
/// dropped on the floor. The callee did stop (its own throw check
/// fired); the caller then printed the ret sentinel, so
/// `console.log(f(1))` answered `null` and a `try`/`catch` around it
/// caught nothing. The SAME program with `n: any`, or routed through
/// a `const m: any = n`, threw correctly all along.
///
/// Nothing statically rules a store out: a frozen instance, a
/// `defineProperty`-demoted field, an accessor-only property and a
/// primitive receiver all raise, and none of them is visible in the
/// receiver's type. So the predicate is the position, not the shape.
/// The KEYED spelling is on the same rule for the same reason, and it
/// is not an exotic shape either: `function w(xs) { xs[0] = 99 }` over
/// a FROZEN array raised in the callee, which correctly left the
/// element alone — and then the caller, having pruned its check,
/// printed the ret sentinel. `console.log(w(a))` answered `null` where
/// every engine throws.
fn is_property_store(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::Member { .. } | Expr::Index { .. })
}

/// The two marks that make a bare identifier read RAISE instead of
/// answering: an unresolvable Reference (RFC 20260730-undeclared-ident,
/// §6.2.5.5) and a binding still in its temporal dead zone
/// (`ast::mark_tdz_reads`, §8.1.1.1). Either way the enclosing
/// fn must enter `may_throw`, or callers prune the check and the throw
/// is silently swallowed across the fn boundary.
pub(crate) fn ident_read_raises(ast: &Ast, eid: ExprId) -> bool {
    ast.undeclared_reads.contains_key(&eid) || ast.tdz_reads.contains(&eid)
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
            // Band B — the checker decided this pair can only throw
            // (`Ast::spec_throws`), and the lowering emits that raise
            // unconditionally. Both operands are typed primitives
            // (BigInt against Number is the shape), so the any-lane
            // test does NOT catch it: without this line a named fn
            // whose only raise is `1n & 1` is M4.3.b-skipped at its
            // caller and the throw is swallowed crossing the fn
            // boundary. Same reason `ident_read_raises` exists.
            if ast.spec_throws.contains_key(&eid) {
                *direct = true;
            }
            if crate::ast_throw_info_binop::binop_raises(
                *op,
                expr_types.get(left),
                expr_types.get(right),
            ) {
                *direct = true;
            }
            scan_expr(ast, *left, out, direct, fn_values, expr_types);
            scan_expr(ast, *right, out, direct, fn_values, expr_types);
        }
        Expr::Unary { expr, .. } => {
            // Band B, same reason as the BinOp arm above.
            if ast.spec_throws.contains_key(&eid) {
                *direct = true;
            }
            scan_expr(ast, *expr, out, direct, fn_values, expr_types)
        }
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
            // Every member store can raise — see `is_member_store`
            // for why the receiver's type cannot tell us otherwise,
            // and for the three whitelist arms this one replaces.
            if is_property_store(ast, *target) {
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
        Expr::Spread { expr } => {
            crate::ast_throw_info_call::scan_spread(ast, *expr, out, direct, fn_values, expr_types)
        }
        Expr::TypeOf { expr } | Expr::Delete { expr } | Expr::As { expr, .. } => {
            scan_expr(ast, *expr, out, direct, fn_values, expr_types)
        }
        Expr::InstanceOf { expr, rhs } => {
            scan_expr(ast, *expr, out, direct, fn_values, expr_types);
            scan_expr(ast, *rhs, out, direct, fn_values, expr_types);
        }
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
            // `o.k++` reads, adds and STORES, so it is on the same
            // rule as the Assign arm above (this arm only ever named
            // `xs.length--`).
            //
            // Measured, so it is not claimed as a fix: `o.hit++` on a
            // FROZEN class instance still answers "after" with this
            // marking in place, while the plain `o.hit = 5` spelling
            // of it throws. The `++` store lane does not consult the
            // §10.1.9 writable gate at all, so there is no pending
            // throw for a caller check to find — the gap is in the
            // lowering, one layer below this file, and is registered
            // separately. Marking is still right here: when that lane
            // does raise, the caller must not have pruned its check.
            if is_property_store(ast, *target) {
                *direct = true;
            }
            scan_expr(ast, *target, out, direct, fn_values, expr_types)
        }
        Expr::This | Expr::NewTarget => {}
        Expr::Ident(_) if ident_read_raises(ast, eid) => {
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
