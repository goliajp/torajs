//! The declaration side of the initializer ownership ledger:
//! given an init expression's SHAPE, does the new binding walk
//! away holding a stake in the value, or is it looking at
//! somebody else's?
//!
//! Two answers, in the order [`crate::ssa_lower_stmt_let_decl::lower`]
//! needs them:
//!
//! - [`init_is_alias`] runs BEFORE the init lowers, off syntax
//!   alone. True means the binding borrows: no inc going in, no
//!   drop at scope close (`borrowed` on its `LocalInfo`).
//! - [`init_shares_source_stake`] runs AFTER, on everything the
//!   lowering learned (the operand's real type, whether a
//!   crossing minted a fresh cell). True means the binding takes
//!   a share of its own — `+1` now, its own drop later — and the
//!   source keeps what it had.
//!
//! Anything that answers neither owns a fresh value outright and
//! transfers it in. That default is why a MISSING row here is
//! never merely conservative: the binding drops at scope close
//! regardless, so a borrow that took no share frees a cell it
//! never owned. Rotation 480 lost the program's only copy of a
//! top-level global exactly that way.
//!
//! The assignment side keeps the same table in
//! [`crate::ssa_lower_assign_ident`]; rows belong in both.

use crate::ast::{Expr, ExprId};
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;

/// Syntactic half — is this init a view of a value that already
/// has an owner? Answered before the init lowers, so it sees
/// shape only; `lower` re-checks the outcome against what the
/// lowering actually produced (a detached member read, a
/// conversion) and can still take the answer back.
pub(crate) fn init_is_alias(ctx: &LowerCtx, init: ExprId, cur_depth: usize) -> bool {
    match ctx.ast.get_expr(init) {
        // L3b #15 residual (chunk 561) — string indexing (`s[i]` on
        // a Str/Substr receiver) emits a FRESH standalone Substr
        // view (str_char_at / substr_slice, rc=1), not a borrow of
        // an element slot: marking it moved skipped the scope drop
        // and leaked 32B per binding. Array/map/any indexing keeps
        // the alias path (element loads borrow the container).
        Expr::Index { obj, .. } => {
            !matches!(ctx.expr_types.get(obj), Some(crate::check::Type::String))
        }
        Expr::Member { .. } => true,
        // Blade 3 — an array pattern's group temp is a compiler-
        // generated, read-only view of its source and never outlives
        // it, so an Ident source is a borrow at ANY scope depth. (The
        // pre-blade-3 desugar expressed the same rule by aliasing onto
        // the source binding and emitting no temp at all; keeping the
        // temp is what gives the iterator lane something to rebind, and
        // it must stay free on the indexable one.) Reaching here at all
        // means the group took the indexable lane — the iterator lane
        // returned above, owning the array it materialized.
        Expr::Ident(src) => {
            ctx.ast.ary_destr_groups.contains_key(&init)
                || ctx
                    .locals
                    .get(src)
                    .map(|info| info.scope_depth < cur_depth)
                    .unwrap_or(false)
        }
        _ => false,
    }
}

/// Value half — the init is not a borrow, so the binding will
/// drop what it holds. Does it need to take a share first?
///
/// `pre_ty` is the lowered operand's type (not the slot's), and
/// `converted` marks a boundary crossing that already minted a
/// fresh cell for this binding — either one turns the answer off.
pub(crate) fn init_shares_source_stake(
    ctx: &LowerCtx,
    init: ExprId,
    pre_ty: Type,
    ty: Type,
    converted: bool,
) -> bool {
    match ctx.ast.get_expr(init) {
        Expr::Ident(src) => {
            // Chunk 698 — a converted init is a fresh cell the
            // binding owns outright; no share with the source.
            //
            // Rotation 480 — a promoted top-level global counts
            // as a binding here, exactly as it does on the
            // assignment side (`ssa_lower_assign_ident`, chunk
            // 558: "Global-slot reads borrow the same way"). The
            // global read is a bare `GlobalRef + Load`
            // (`ssa_lower_ident::try_k3_global_data`) — a BORROW
            // of the slot's stake. Off this arm the binding got
            // neither the share-inc nor the alias mark (the
            // alias classifier consults `ctx.locals` too, and a
            // global is in neither map), so it owned a stake it
            // never took and its scope-end drop freed the
            // program's only copy. The next read of the global
            // then ran use-after-free: in the test262 typed-array
            // driver, `let ctorArgFactories = <global array>` in
            // one call left `.length` reading a reused cell in
            // the next (4342679952 instead of 8), and the push
            // that followed grew a Vec off a garbage capacity —
            // SIGSEGV inside `RawVec::grow_one`.
            !converted
                && (ctx.locals.contains_key(src) || ctx.globals.contains_key(src))
                && pre_ty.is_refcounted()
                && !(ty == Type::Any && pre_ty != Type::Any)
        }
        // A regex literal is the fn-scope LICM singleton
        // (`ssa_lower_lit::lower_regex` hoists one compile into
        // the entry block; fn exit drops its single stake) — a
        // binding taking it is a SHARE, not a transfer. Without
        // the +1 the binding's scope-drop stole the fn's stake
        // and every later occurrence ran use-after-free
        // (`for { const a = /x/; a.test(s) }` answered 1/4).
        Expr::Regex { .. } => {
            !converted && pre_ty.is_refcounted() && !(ty == Type::Any && pre_ty != Type::Any)
        }
        // Rotation 325 — an `as` cast is a value-layer
        // pass-through (lower_as_cast answers the inner operand),
        // so ownership follows the inner expression: `const s =
        // arr as any` binding a local is the same SHARE the bare
        // Ident init takes. Off this arm the binding took the
        // borrow as if it owned it, and its scope-end drop stole
        // the source binding's stake (census underflow on
        // dstr-numeric-key-001 — the destructuring desugar mints
        // exactly this let for a non-Ident source).
        Expr::As { expr, .. } => {
            if let Expr::Ident(src) = ctx.ast.get_expr(*expr) {
                !converted
                    && (ctx.locals.contains_key(src) || ctx.globals.contains_key(src))
                    && pre_ty.is_refcounted()
                    && !(ty == Type::Any && pre_ty != Type::Any)
            } else {
                false
            }
        }
        // Rotation 326 — a Ternary / Nullish / `&&` / `||` join
        // over pure borrows answers a borrow (chunk 722 keeps
        // those joins at zero rc traffic); an owned-unified join
        // recorded itself in owned_member_reads and transfers
        // its fresh stake instead. The binding is a consumer
        // like any other: it takes +1 and the arm sources keep
        // theirs. Off this arm the store took the borrow as if
        // it owned it and the scope-end drop stole an arm
        // source's stake — the destructuring-default desugar
        // mints exactly this shape (`let cls = <in-range> ?
        // src[0] : __ClassExpr_N`), and the class registry's
        // stake went through zero at the exit drain (census:
        // dstr-classname-001).
        Expr::Ternary { .. } | Expr::Nullish { .. } => {
            !converted
                && !ctx.owned_member_reads.contains(&init)
                && pre_ty.is_refcounted()
                && !(ty == Type::Any && pre_ty != Type::Any)
        }
        Expr::BinOp { op, .. }
            if matches!(op, crate::ast::BinOp::LAnd | crate::ast::BinOp::LOr) =>
        {
            !converted
                && !ctx.owned_member_reads.contains(&init)
                && pre_ty.is_refcounted()
                && !(ty == Type::Any && pre_ty != Type::Any)
        }
        _ => false,
    }
}
