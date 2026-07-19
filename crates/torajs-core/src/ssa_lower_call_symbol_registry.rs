//! `Symbol.for(key)` / `Symbol.keyFor(s)` registry helpers pulled out
//! of [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-23 of the `Expr::Call` god-arm decomp (chunks 1-22 = Arr
//! higher-order + Map dispatch + Set dispatch + Arr.push + Number
//! instance methods + bare-name globals + Str regex methods + Number
//! namespace + Array.from + Arr predicate iter + Arr.flatMap +
//! Object.entries + fn-indirect + Number/String/Boolean coercion +
//! universal methods + closure-local + Object.values + Object.keys +
//! Object.getPrototypeOf + Object.assign + Bun runtime cluster +
//! Reflect.get).
//!
//! T-13.b (v0.4.0) — direct delegation to the runtime registry
//! intrinsics. `Symbol.for(key)` → `Type::Symbol` via `symbol_for`;
//! `Symbol.keyFor(s)` → `Type::Any` via the any-lane
//! `symbol_key_for_any` on every arg shape (rotation 155 — §20.4.2.6
//! answers string | undefined, which Nullable<Str> cannot spell).
//!
//! Trailing args past `args[0]` are spec-lowered for side-effect
//! parity per the S272 idiom (check.rs S259 already typecheck-drops
//! them). Mirrors S255/S277/S278/S297 sibling arms.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if m_name != "for" && m_name != "keyFor" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Symbol" {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    let arg_op = ctx.lower_expr(args[0]);
    // S302 — lower-and-drop trailing args[1..] per S272 idiom so
    // step()-style side-effect exprs fire per ES eval-then-discard
    // (check.rs S259 already typecheck-dropped). Mirrors S255/S277/
    // S278/S297 sibling arms.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    // RFC 20260720-symbol-any-call-boundary — an Any arg routes to
    // the any-lane kernels: `symbol_for_any` carries the ToString
    // coercion (and its throw face), `symbol_key_for_any` the
    // §20.4.2.6 brand check (non-Symbol throws) and the
    // unregistered→undefined mapping. The kernel borrows the arg
    // and answers owned NaN-box bits; a throw path returns
    // VALUE_UNDEFINED (immediate, no ledger residue).
    //
    // keyFor takes this lane on TYPED args too (rotation 155):
    // §20.4.2.6 answers string | undefined, which a Nullable<Str>
    // slot cannot spell — NULL printed "null" and static typeof
    // said "string". The box is the pure tag-4 encode (kernel
    // borrows, the brand check trivially passes a Symbol cell).
    let arg_is_any = ctx.operand_ty(&arg_op) == Type::Any;
    if arg_is_any || m_name == "keyFor" {
        let (fid, arg_any) = if m_name == "for" {
            (ctx.intrinsics.symbol_for_any, arg_op.clone())
        } else if arg_is_any {
            (ctx.intrinsics.symbol_key_for_any, arg_op.clone())
        } else {
            let boxed = ctx.box_to_any(arg_op.clone());
            (ctx.intrinsics.symbol_key_for_any, boxed)
        };
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(fid, vec![arg_any]),
            Type::Any,
            None,
        );
        ctx.emit_throw_check(None);
        ctx.release_owned_temp(args[0], &arg_op);
        return Some(Operand::Value(v));
    }
    let (fid, ret_ty) = (ctx.intrinsics.symbol_for, Type::Symbol);
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![arg_op.clone()]),
        ret_ty,
        None,
    );
    // `symbol_for` shares the key on registry miss (delegates to
    // symbol_alloc's internal inc) and borrows on hit; `symbol_key_for`
    // borrows the sym. Either way the caller keeps its stake — the old
    // consume path orphaned an Ident arg's stake (RFC 20260705 ledger
    // #3, 32B/iter probe). Owned temp args release here.
    ctx.release_owned_temp(args[0], &arg_op);
    Some(Operand::Value(v))
}
