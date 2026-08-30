//! Push-loop pattern detector — pure AST pre-analysis.
//!
//! Recognises the canonical "fill loop"
//!
//! ```ignore
//!   for (let i = 0; i < N; i++) {   // `i = i + 1` / `i += 1` too
//!     xs.push(_)                  // OR a block/multi of pure xs.push(_) calls
//!   }
//! ```
//!
//! and returns `Some((bound_eid, [xs_name, ...]))` so the caller can
//! emit one `arr_reserve(bound, ...)` per detected array up front and
//! then route each per-iter push through `arr_push_unchecked`. The
//! detector is conservative on the false-positive side: anything not
//! matching the exact shape returns `None` and the regular cap-checked
//! push path runs unchanged. False negatives stay safe, but they are
//! not cheap: not recognising `i++` cost a 10M-append loop 7.7x for
//! as long as nobody noticed.
//!
//! Lives in its own file (not collocated with the other AST visitors
//! in `ssa_lower.rs`) so `ssa_lower.rs` stays on the known-debt
//! "only-shrinks" trajectory; mirrors the placement chosen for the
//! 11-A1 deque-escape visitor and 11-A2-a obj-escape visitor.

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

/// v0.6+1 perf checkpoint — see module-level doc for the pattern this
/// recognises.
pub(crate) fn detect_push_loop_arrays(
    ast: &Ast,
    init: Option<&Stmt>,
    cond: Option<ExprId>,
    step: Option<ExprId>,
    body: &Stmt,
) -> Option<(ExprId, Vec<String>)> {
    /* init: `let i = 0` (literal 0; const 0 is enough — anything
     * else means the loop isn't a simple 0..N walk). */
    let i_name = match init? {
        Stmt::LetDecl {
            name,
            init: init_eid,
            ..
        } => match ast.get_expr(*init_eid) {
            Expr::Number(n) if *n == 0.0 => name.clone(),
            _ => return None,
        },
        _ => return None,
    };
    /* cond: `i < bound`. Capture bound expression. */
    let bound_eid = match ast.get_expr(cond?) {
        Expr::BinOp {
            op: crate::ast::BinOp::Lt,
            left,
            right,
        } => match ast.get_expr(*left) {
            Expr::Ident(n) if n == &i_name => *right,
            _ => return None,
        },
        _ => return None,
    };
    /* step: the `+1` step on `i`, in any of its spellings. */
    if !is_counter_step_expr(ast, step?, &i_name) {
        return None;
    }
    /* body: must be Stmt::Expr(push) or Stmt::Block / Multi of
     * push-only stmts (no conditionals, no other method calls).
     * Single-array OR multi-array both work — we collect every
     * `xs.push(_)` target name. */
    let mut names: Vec<String> = Vec::new();
    if !collect_push_targets_only(ast, body, &mut names) {
        return None;
    }
    if names.is_empty() {
        return None;
    }
    Some((bound_eid, names))
}

/// True when every `xs.push(arg)` in `s` has an argument that cannot
/// look at the array being filled, and cannot leave the loop by any
/// edge but its normal exit.
///
/// This is what buys the deferred length word (see
/// [`crate::ssa_lower::PreReserveState::defer_len`]). The body shape
/// the detectors above accept says the loop contains nothing but
/// pushes; it says nothing about what is being pushed, and an
/// argument is an arbitrary expression. `xs.push(xs.length)` reads
/// the very word the loop is holding back, and `xs.push(f(i))` hands
/// control to a function that may read the array or throw out of the
/// loop entirely.
///
/// An allowlist, because the safe direction is to answer no: a shape
/// this does not recognise gets the length word written on every
/// append instead, which costs a store rather than an answer.
/// Literals, numeric locals, and arithmetic over them are the whole
/// list — none of those can call, read a heap cell, or throw. Two
/// absences are deliberate. A member or index read is out: `xs.length`
/// is the case that started this, and an out-of-range index read on a
/// non-`number[]` throws. And the operand types are checked, not just
/// the operator: `a + b` over `any` runs `valueOf`, which is user code
/// with the array in scope.
pub(crate) fn push_args_all_inert(ctx: &LowerCtx<'_>, s: &Stmt) -> bool {
    match s {
        Stmt::Expr(eid) => match ctx.ast.get_expr(*eid) {
            Expr::Call { args, .. } => args.iter().all(|a| expr_is_inert(ctx, *a)),
            // The `while` lane's body carries its counter step as the
            // last statement. Bumping a local cannot look at the array
            // either, in either spelling.
            Expr::Assign { target, value } => {
                matches!(ctx.ast.get_expr(*target), Expr::Ident(_)) && expr_is_inert(ctx, *value)
            }
            Expr::PostIncr { target, .. } => {
                matches!(ctx.ast.get_expr(*target), Expr::Ident(_))
            }
            _ => false,
        },
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().all(|s| push_args_all_inert(ctx, s))
        }
        _ => false,
    }
}

/// The allowlist itself. See [`push_args_all_inert`].
fn expr_is_inert(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    if !matches!(
        ctx.expr_types.get(&eid),
        Some(crate::check::Type::Number) | Some(crate::check::Type::Boolean)
    ) {
        return false;
    }
    match ctx.ast.get_expr(eid) {
        Expr::Number(_) | Expr::Bool(_) | Expr::Ident(_) => true,
        Expr::BinOp { op, left, right } => {
            is_inert_binop(*op) && expr_is_inert(ctx, *left) && expr_is_inert(ctx, *right)
        }
        _ => false,
    }
}

/// Arithmetic and comparison that cannot call, read a heap cell, or
/// throw. Shared by the push-argument allowlist and the bound's.
fn is_inert_binop(op: crate::ast::BinOp) -> bool {
    use crate::ast::BinOp as B;
    matches!(
        op,
        B::Add
            | B::Sub
            | B::Mul
            | B::Div
            | B::Mod
            | B::Lt
            | B::Gt
            | B::Le
            | B::Ge
            | B::BitAnd
            | B::BitOr
            | B::BitXor
            | B::Shl
            | B::Shr
            | B::UShr
    )
}

/// Lower the loop bound for a pre-reserve install, or answer `None`
/// when the install may not be made at all.
///
/// # The bound must be invariant
///
/// Both installs read `bound` once, above the loop, and then trust
/// that value for the rest of it: the reserved capacity is
/// `len + bound`, and every push in the body becomes an unchecked
/// inline store against it. Both justified that with "the cond reads
/// it on every iter unchanged" — which is true of the *expression*
/// and says nothing about its *value*. Three ways that was wrong,
/// each measured against bun:
///
/// - `i < bnd()` — the read above the loop is a call the program
///   never wrote. A `bnd` that bumps a counter answered 5 where the
///   answer is 4, and it fired even when no array qualified for a
///   reserve at all, because the lowering ran ahead of the filter.
/// - `i < (xs.length >> 1)` — with the length word deferred to a
///   register the cond reads a stale length and the loop stops early:
///   15 where the answer is 19. With it not deferred the trip count
///   is right and the reserve is four elements short, so the
///   unchecked stores run past the end of the buffer.
/// - `i < n / 2` — an f64 bound reaches the I64 `len + bound` add and
///   the backend refuses to materialise it.
///
/// So the bound has to be inert in the same sense a push argument has
/// to be ([`push_args_all_inert`]): literals, numeric locals, and
/// arithmetic over them. A body of nothing but pushes cannot write a
/// local, so inert here really is invariant. The width is checked on
/// the lowered operand rather than the source expression because
/// `number` says nothing about which one it landed in.
///
/// # The reserved arrays must be distinct
///
/// A body may fill several arrays in lockstep, and the install serves
/// each one separately: `reserve(xs, len(xs) + bound)` per name. Two
/// names for one array make that half of what the loop writes —
/// `f(a, a)` with `p.push(i); q.push(i * 10)` reserved eight slots and
/// wrote sixteen, past the end of the buffer, answering length 8 where
/// the answer is 16. So a multi-array install additionally needs each
/// name proved to be this body's alone, which is exactly the question
/// [`PreReserve::owns_alone`] already answers: two fresh literals
/// neither of which ever escaped are two cells. A single name needs no
/// such proof — it is reserved against its own length, whoever else
/// can reach it, and nothing but this loop writes it.
///
/// [`PreReserve::owns_alone`]: crate::ssa_lower_arr_prereserve::PreReserve::owns_alone
pub(crate) fn lower_reserve_bound(
    ctx: &mut LowerCtx<'_>,
    bound: ExprId,
    names: &[String],
) -> Option<Operand> {
    // `xs.length` may be read as a bound only when nothing the body
    // does can move it, which needs each filled array proved to be
    // this body's alone. Ask before the `&self` predicate below —
    // the proof memoises, so it wants `&mut`.
    let all_owned = names.iter().all(|n| {
        ctx.prereserve
            .owns_alone(ctx.ast, &ctx.deque_arrs, n.as_str())
    });
    if names.len() > 1 && !all_owned {
        return None;
    }
    if !bound_is_invariant(ctx, bound, names, all_owned) {
        return None;
    }
    let op = ctx.lower_expr(bound);
    matches!(ctx.operand_ty(&op), Type::I64).then_some(op)
}

/// The bound's allowlist: inert, plus `A.length` for an `A` that the
/// loop provably does not write.
///
/// `A.length` is the shape this whole path exists to serve — filling
/// one array by the length of another is how a copy is written — and
/// it is a genuine loop invariant whenever `A` is not among the
/// arrays being filled and cannot be a second name for one of them.
/// The first half is a name comparison. The second is answered by
/// [`PreReserve::owns_alone`], and it takes *either* side to answer
/// it, which is the whole reason to ask twice:
///
/// - every filled array is this body's alone — then no `A`, whatever
///   it is, can be a second name for one of them; or
/// - `A` itself is this body's alone — then `A` is a cell made here
///   and handed to nobody, so no filled array can be a second name
///   for *it*, and it does not matter where the filled arrays came
///   from.
///
/// Only the first was asked before, so `function fill(dst, src)` —
/// the way a library writes this — was refused whole, for want of a
/// question about `src`. Asking the second admits the half where the
/// bound array is the body's own; `f(a, a)` with both sides handed in
/// still fails both, and needs interprocedural information.
///
/// A string is unconditional: its length cannot move at all, and a
/// body of nothing but pushes cannot rebind the name.
///
/// Reading a length cannot call, throw, or write, so nothing else
/// about the bound changes: the extra read above the loop stays
/// unobservable, and the value still has to land in i64.
///
/// [`PreReserve::owns_alone`]: crate::ssa_lower_arr_prereserve::PreReserve::owns_alone
fn bound_is_invariant(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    names: &[String],
    all_owned: bool,
) -> bool {
    if let Expr::Member { obj, name } = ctx.ast.get_expr(eid)
        && name == "length"
        && let Expr::Ident(a) = ctx.ast.get_expr(*obj)
    {
        if names.iter().any(|n| n == a) {
            return false;
        }
        if matches!(ctx.expr_types.get(obj), Some(crate::check::Type::String)) {
            return true;
        }
        if !matches!(ctx.expr_types.get(obj), Some(crate::check::Type::Array(_))) {
            return false;
        }
        return all_owned || ctx.prereserve.owns_alone(ctx.ast, &ctx.deque_arrs, a);
    }
    if let Expr::BinOp { op, left, right } = ctx.ast.get_expr(eid)
        && is_inert_binop(*op)
    {
        return bound_is_invariant(ctx, *left, names, all_owned)
            && bound_is_invariant(ctx, *right, names, all_owned);
    }
    expr_is_inert(ctx, eid)
}

/// Walk `s` and collect ident names of arrays that are the receiver
/// of a `xs.push(_)` call. Returns `false` if any non-push stmt is
/// found (caller bails). Allows nested Blocks / Multi's so user-
/// formatted bodies parse cleanly.
fn collect_push_targets_only(ast: &Ast, s: &Stmt, out: &mut Vec<String>) -> bool {
    match s {
        Stmt::Expr(eid) => match ast.get_expr(*eid) {
            Expr::Call { callee, args } if args.len() == 1 => {
                let Expr::Member { obj, name } = ast.get_expr(*callee) else {
                    return false;
                };
                if name != "push" {
                    return false;
                }
                let Expr::Ident(xs_name) = ast.get_expr(*obj) else {
                    return false;
                };
                if !out.iter().any(|n| n == xs_name) {
                    out.push(xs_name.clone());
                }
                true
            }
            _ => false,
        },
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().all(|s| collect_push_targets_only(ast, s, out))
        }
        _ => false,
    }
}

/// 12-c-1 — while-shape variant of [`detect_push_loop_arrays`]. Recognises
///
/// ```ignore
///   let i: number = 0;            // immediately-preceding stmt; caller passes counter_name
///   while (i < N) {
///     xs.push(_);                 // any number of xs.push(_) stmts, single- or multi-array
///     // ...
///     i++;                        // MUST be the last stmt; `i = i + 1` / `i += 1` too
///   }
/// ```
///
/// The caller (`Stmt::Block` / `Stmt::Multi` iteration in `ssa_lower`) is
/// responsible for proving `counter_name` is statically 0 at while entry by
/// matching the immediately-preceding stmt's shape via
/// [`let_counter_zero_name`]. Anything else means the loop isn't a simple
/// 0..N walk and the regular cap-checked push path runs unchanged.
///
/// Returns `Some((bound_eid, [xs_name, ...]))` when the shape matches.
/// False negatives stay safe; false positives are guarded against by the
/// caller-side init check + this fn's strict body-shape check.
pub(crate) fn detect_push_loop_arrays_while(
    ast: &Ast,
    counter_name: &str,
    cond: ExprId,
    body: &Stmt,
) -> Option<(ExprId, Vec<String>)> {
    /* cond: `counter < bound`. Capture bound expression. */
    let bound_eid = match ast.get_expr(cond) {
        Expr::BinOp {
            op: crate::ast::BinOp::Lt,
            left,
            right,
        } => match ast.get_expr(*left) {
            Expr::Ident(n) if n == counter_name => *right,
            _ => return None,
        },
        _ => return None,
    };
    /* body: must be Block/Multi where the LAST stmt is the canonical
     * `counter = counter + 1` step and every earlier stmt is a pure
     * xs.push(_) (single- or multi-array; nested Block/Multi allowed
     * via collect_push_targets_only's recursion). */
    let stmts: &[Stmt] = match body {
        Stmt::Block(s) | Stmt::Multi(s) => s.as_slice(),
        _ => return None,
    };
    let (last, init_stmts) = stmts.split_last()?;
    if !is_counter_step_stmt(ast, last, counter_name) {
        return None;
    }
    let mut names: Vec<String> = Vec::new();
    for s in init_stmts {
        if !collect_push_targets_only(ast, s, &mut names) {
            return None;
        }
    }
    if names.is_empty() {
        return None;
    }
    Some((bound_eid, names))
}

/// 12-c-1 — returns Some(counter_name) when `s` is `let counter = 0`
/// (literal `0` init; type-annotation optional). Proves the counter is
/// statically 0 at the immediately-following while entry — caller
/// threads this through [`detect_push_loop_arrays_while`].
pub(crate) fn let_counter_zero_name(ast: &Ast, s: Option<&Stmt>) -> Option<String> {
    let Stmt::LetDecl {
        name,
        init: init_eid,
        ..
    } = s?
    else {
        return None;
    };
    match ast.get_expr(*init_eid) {
        Expr::Number(n) if *n == 0.0 => Some(name.clone()),
        _ => None,
    }
}

fn is_counter_step_stmt(ast: &Ast, s: &Stmt, counter_name: &str) -> bool {
    let Stmt::Expr(eid) = s else {
        return false;
    };
    is_counter_step_expr(ast, *eid, counter_name)
}

/// The `+1` step on `counter`, in either spelling.
///
/// `i = i + 1` is what the module doc has always described, and what
/// `i += 1` parses to. `i++` used to join them — the doc says "parser
/// desugars i++ / i+=1 here" — but postfix increment became its own
/// node (`Expr::PostIncr`, so that `x++` can answer the old value)
/// and this matcher was not told. The whole pre-reserve fast path
/// then quietly stopped firing for the spelling almost every loop
/// uses: the same 10M-append program is 7.7x slower written `i++`
/// than written `i = i + 1`.
///
/// As a for-step or as the last statement of a while body the answer
/// `i++` produces is discarded, so the two spellings are the same
/// step. `i--` is not, hence the `is_inc` test.
fn is_counter_step_expr(ast: &Ast, eid: ExprId, counter_name: &str) -> bool {
    match ast.get_expr(eid) {
        Expr::PostIncr { target, is_inc } => {
            *is_inc && matches!(ast.get_expr(*target), Expr::Ident(n) if n == counter_name)
        }
        Expr::Assign { target, value } => {
            matches!(ast.get_expr(*target), Expr::Ident(n) if n == counter_name)
                && matches!(
                    ast.get_expr(*value),
                    Expr::BinOp { op: crate::ast::BinOp::Add, left, right }
                        if matches!(ast.get_expr(*left), Expr::Ident(n) if n == counter_name)
                            && matches!(ast.get_expr(*right), Expr::Number(v) if *v == 1.0)
                )
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{BinOp as AstBinOp, Expr, Stmt};

    /* Helpers — construct the AST fragments by hand. We bypass the
     * parser so the tests stay sensitive to the detector's exact
     * shape contract. That sensitivity has a blind spot, and it cost
     * a 7.7x: a hand-built `i = i + 1` keeps matching no matter what
     * the parser does with `i++`, and when postfix increment became
     * its own node the whole fast path went off with every test still
     * green. `step_spellings_all_parse_to_a_match` below closes it by
     * going through the parser for each spelling. */

    fn mk_ident(ast: &mut Ast, name: &str) -> ExprId {
        ast.add_expr(Expr::Ident(name.to_string()))
    }
    fn mk_number(ast: &mut Ast, n: f64) -> ExprId {
        ast.add_expr(Expr::Number(n))
    }
    fn mk_lt(ast: &mut Ast, left: ExprId, right: ExprId) -> ExprId {
        ast.add_expr(Expr::BinOp {
            op: AstBinOp::Lt,
            left,
            right,
        })
    }
    fn mk_add(ast: &mut Ast, left: ExprId, right: ExprId) -> ExprId {
        ast.add_expr(Expr::BinOp {
            op: AstBinOp::Add,
            left,
            right,
        })
    }
    fn mk_assign(ast: &mut Ast, target: ExprId, value: ExprId) -> ExprId {
        ast.add_expr(Expr::Assign { target, value })
    }
    fn mk_push_call(ast: &mut Ast, receiver: &str, arg: ExprId) -> ExprId {
        let obj = mk_ident(ast, receiver);
        let callee = ast.add_expr(Expr::Member {
            obj,
            name: "push".to_string(),
        });
        ast.add_expr(Expr::Call {
            callee,
            args: vec![arg],
        })
    }

    fn mk_let_zero(ast: &mut Ast, name: &str) -> Stmt {
        let init = mk_number(ast, 0.0);
        Stmt::LetDecl {
            mutable: true,
            name: name.to_string(),
            type_ann: Some("number".to_string()),
            init,
            is_var: false,
        }
    }

    fn mk_step(ast: &mut Ast, counter: &str) -> Stmt {
        let t = mk_ident(ast, counter);
        let l = mk_ident(ast, counter);
        let r = mk_number(ast, 1.0);
        let plus = mk_add(ast, l, r);
        let assign = mk_assign(ast, t, plus);
        Stmt::Expr(assign)
    }

    fn mk_push_stmt(ast: &mut Ast, receiver: &str, arg_name: &str) -> Stmt {
        let arg = mk_ident(ast, arg_name);
        let call = mk_push_call(ast, receiver, arg);
        Stmt::Expr(call)
    }

    #[test]
    fn let_counter_zero_name_matches_literal_zero_init() {
        let mut ast = Ast::default();
        let s = mk_let_zero(&mut ast, "i");
        assert_eq!(let_counter_zero_name(&ast, Some(&s)), Some("i".to_string()));
    }

    #[test]
    fn let_counter_zero_name_rejects_nonzero_init() {
        let mut ast = Ast::default();
        let init = mk_number(&mut ast, 5.0);
        let s = Stmt::LetDecl {
            mutable: true,
            name: "i".to_string(),
            type_ann: Some("number".to_string()),
            init,
            is_var: false,
        };
        assert_eq!(let_counter_zero_name(&ast, Some(&s)), None);
    }

    #[test]
    fn let_counter_zero_name_rejects_non_letdecl() {
        let mut ast = Ast::default();
        let arg = mk_number(&mut ast, 0.0);
        let s = Stmt::Expr(arg);
        assert_eq!(let_counter_zero_name(&ast, Some(&s)), None);
    }

    #[test]
    fn let_counter_zero_name_rejects_none() {
        let ast = Ast::default();
        assert_eq!(let_counter_zero_name(&ast, None), None);
    }

    #[test]
    fn detect_while_matches_canonical_array_sum_shape() {
        /* let xs: number[] = []; let i: number = 0;
         * while (i < 10000000) { xs.push(i); i = i + 1; }
         * — array-sum-1m bench shape. */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10_000_000.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let push = mk_push_stmt(&mut ast, "xs", "i");
        let step = mk_step(&mut ast, "i");
        let body = Stmt::Block(vec![push, step]);

        let result = detect_push_loop_arrays_while(&ast, "i", cond, &body);
        assert!(result.is_some(), "canonical shape must match");
        let (got_bound, names) = result.unwrap();
        assert_eq!(got_bound, bound);
        assert_eq!(names, vec!["xs".to_string()]);
    }

    #[test]
    fn detect_while_multi_array_push() {
        /* while (i < N) { xs.push(i); ys.push(i); i = i + 1; } */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 100.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let push_xs = mk_push_stmt(&mut ast, "xs", "i");
        let push_ys = mk_push_stmt(&mut ast, "ys", "i");
        let step = mk_step(&mut ast, "i");
        let body = Stmt::Block(vec![push_xs, push_ys, step]);

        let result = detect_push_loop_arrays_while(&ast, "i", cond, &body);
        let (_, names) = result.expect("multi-array shape must match");
        assert_eq!(names, vec!["xs".to_string(), "ys".to_string()]);
    }

    #[test]
    fn detect_while_rejects_missing_step() {
        /* while (i < N) { xs.push(i); }  ← no step; would loop forever
         * in real code, but the parser still produces this AST. */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let push = mk_push_stmt(&mut ast, "xs", "i");
        let body = Stmt::Block(vec![push]);

        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_none());
    }

    #[test]
    fn detect_while_rejects_step_not_last() {
        /* while (i < N) { i = i + 1; xs.push(i); } */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let step = mk_step(&mut ast, "i");
        let push = mk_push_stmt(&mut ast, "xs", "i");
        let body = Stmt::Block(vec![step, push]);

        /* When the supposed step is first, it gets routed through
         * collect_push_targets_only as a non-push stmt and rejected;
         * the actual last stmt is a push, which fails is_counter_step. */
        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_none());
    }

    #[test]
    fn detect_while_rejects_wrong_counter_in_cond() {
        /* while (j < N) { xs.push(i); i = i + 1; }  ← cond uses j, not i */
        let mut ast = Ast::default();
        let j_ref = mk_ident(&mut ast, "j");
        let bound = mk_number(&mut ast, 10.0);
        let cond = mk_lt(&mut ast, j_ref, bound);
        let push = mk_push_stmt(&mut ast, "xs", "i");
        let step = mk_step(&mut ast, "i");
        let body = Stmt::Block(vec![push, step]);

        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_none());
    }

    #[test]
    fn detect_while_rejects_wrong_op_in_cond() {
        /* while (i <= N) { ... }  ← Le not Lt */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10.0);
        let cond = ast.add_expr(Expr::BinOp {
            op: AstBinOp::Le,
            left: i_ref,
            right: bound,
        });
        let push = mk_push_stmt(&mut ast, "xs", "i");
        let step = mk_step(&mut ast, "i");
        let body = Stmt::Block(vec![push, step]);

        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_none());
    }

    #[test]
    fn detect_while_rejects_step_increment_not_one() {
        /* while (i < N) { xs.push(i); i = i + 2; }  ← step+2 */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let push = mk_push_stmt(&mut ast, "xs", "i");
        let t = mk_ident(&mut ast, "i");
        let l = mk_ident(&mut ast, "i");
        let two = mk_number(&mut ast, 2.0);
        let plus = mk_add(&mut ast, l, two);
        let assign = mk_assign(&mut ast, t, plus);
        let bad_step = Stmt::Expr(assign);
        let body = Stmt::Block(vec![push, bad_step]);

        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_none());
    }

    #[test]
    fn detect_while_rejects_non_push_body_stmt() {
        /* while (i < N) { console.log(i); i = i + 1; }  ← non-push */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let console = mk_ident(&mut ast, "console");
        let log_callee = ast.add_expr(Expr::Member {
            obj: console,
            name: "log".to_string(),
        });
        let arg_i = mk_ident(&mut ast, "i");
        let log_call = ast.add_expr(Expr::Call {
            callee: log_callee,
            args: vec![arg_i],
        });
        let log_stmt = Stmt::Expr(log_call);
        let step = mk_step(&mut ast, "i");
        let body = Stmt::Block(vec![log_stmt, step]);

        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_none());
    }

    #[test]
    fn detect_while_rejects_non_block_body() {
        /* while (i < N) i = i + 1;  ← single-stmt while body without block */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let body = mk_step(&mut ast, "i");

        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_none());
    }

    #[test]
    fn detect_while_rejects_push_with_extra_args() {
        /* while (i < N) { xs.push(i, j); i = i + 1; }  ← .push takes 1 arg */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let xs = mk_ident(&mut ast, "xs");
        let callee = ast.add_expr(Expr::Member {
            obj: xs,
            name: "push".to_string(),
        });
        let a1 = mk_ident(&mut ast, "i");
        let a2 = mk_ident(&mut ast, "j");
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![a1, a2],
        });
        let push2 = Stmt::Expr(call);
        let step = mk_step(&mut ast, "i");
        let body = Stmt::Block(vec![push2, step]);

        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_none());
    }

    #[test]
    fn detect_while_accepts_multi_body() {
        /* Body wrapped in Stmt::Multi (synthetic, post-desugar shape). */
        let mut ast = Ast::default();
        let i_ref = mk_ident(&mut ast, "i");
        let bound = mk_number(&mut ast, 10.0);
        let cond = mk_lt(&mut ast, i_ref, bound);
        let push = mk_push_stmt(&mut ast, "xs", "i");
        let step = mk_step(&mut ast, "i");
        let body = Stmt::Multi(vec![push, step]);

        assert!(detect_push_loop_arrays_while(&ast, "i", cond, &body).is_some());
    }

    /// Every spelling of the `+1` step reaches the detector as one.
    ///
    /// The hand-built tests above cannot see a parser change, which is
    /// how `i++` stopped matching without anything going red. This one
    /// starts from source, so a future desugar that moves a spelling
    /// out from under the matcher fails here instead of silently
    /// turning the optimisation off.
    #[test]
    fn step_spellings_all_parse_to_a_match() {
        for step in ["i++", "i = i + 1", "i += 1"] {
            let src = format!(
                "let xs: number[] = [];\nfor (let i = 0; i < 10; {step}) {{ xs.push(i); }}\n"
            );
            let tokens = crate::lexer::tokenize(&src).expect("lex");
            let ast = crate::parser::parse(&src, &tokens).expect("parse");
            let found = ast.stmts.iter().any(|s| match s {
                Stmt::For {
                    init,
                    cond,
                    step,
                    body,
                } => detect_push_loop_arrays(&ast, init.as_deref(), *cond, *step, body)
                    .is_some_and(|(_, names)| names == vec!["xs".to_string()]),
                _ => false,
            });
            assert!(found, "step spelling `{step}` did not reach the detector");
        }
    }
}
