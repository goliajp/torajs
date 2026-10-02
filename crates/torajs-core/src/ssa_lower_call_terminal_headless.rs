//! RFC 20260816-headless-argv-face — the head-less callee's
//! synthetic leading slots, split out of `ssa_lower_call_terminal`
//! when the argv packer pushed that file past the 500-line limit
//! (verbatim move).
//!
//! A head-less body (no `__env` / `__this` head) carries what the
//! env-first and this-first tiers carry behind their head param: the
//! H1 hidden I64 argc, and — since the argv face — the raw argv
//! pointer after it. Neither is visible in the AST arg list, so the
//! terminal shifts every sig-aligned read past them and prepends
//! their operands last. This module owns that whole channel: which
//! slots a callee wants, what count they describe, and how the
//! caller fills the pointer.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// H1 — 1 when the callee is a head-less T-31 body (hidden argc at
/// sig position 0), 0 otherwise. Keyed by name — the mono retarget
/// name when the checker recorded one (clones are mirrored into the
/// side table), the direct Ident otherwise. Non-Ident callees can't
/// reach a head-less body (value escapes ride the `__forward_` relay,
/// itself an env-first closure).
/// `__forward_<f>` is a TRANSPARENT relay minted for a value escape
/// of `f`; when its body's forwarding call reaches a head-less
/// callee, the hidden argc slot must carry the relay's OWN runtime
/// argc (the S1 hidden param every env-first closure receives), not
/// the relay's declared arity — `(f as any)(1, 2, 3)` reaches `f`
/// through the relay and `arguments.length` must answer 3. Any
/// other caller keeps the static count (`args.len()`).
///
/// `packed` — how many argv slots this call packed for an argv-face
/// callee. The relay's forwarding call packs exactly its declared
/// arguments, so a runtime argc above that count names slots the
/// buffer does not hold: the count clamps to `packed`, or the body's
/// materialize would read past the stack slab. (The clamp changes
/// nothing on the routes that matter — an any-world call never
/// reaches the relay body since rotation 625, its adapter enters the
/// target directly with the true list, and a typed direct call's
/// argc is the declared count by construction — but the checker
/// admits `[n][0](12)` on a zero-param closure face, and that site
/// printed a garbage string.) A length-only callee has no buffer and
/// keeps the true count.
pub(crate) fn forwarded_argc(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    packed: Option<usize>,
) -> Option<Operand> {
    let crate::ast::Expr::Ident(callee_name) = ctx.ast.get_expr(callee) else {
        return None;
    };
    // A receiver-first relay (`__fwdrecv_`, rotation 649) forwards
    // its true count the same way; without it a `this`-reading
    // declaration at a thisArg callback position answered
    // `arguments.length` as its declared arity.
    let relay_of = ctx
        .f
        .name
        .strip_prefix("__forward_")
        .or_else(|| ctx.f.name.strip_prefix("__fwdrecv_"))?;
    // A this-reading (or lossy-slot) static's relay forwards to the
    // receiver-polymorphic twin under the mono's `__forward_` name
    // (`forwarder_synth::static_twin_target`), so the callee it
    // reaches is `__smany_<C>__<m>`, not the name the relay is minted
    // for. Matching the mono alone left that call on the static
    // count: `E.two(true, "x", ...arr)` on `static two(a: boolean,
    // b: string)` answered `arguments.length` as the declared 2.
    let forwards_to_callee =
        relay_of == callee_name.as_str() || ctx.ast.smany_twins.get(relay_of) == Some(callee_name);
    if !forwards_to_callee {
        return None;
    }
    let info = ctx.locals.get("__torajs_argc")?;
    let (slot, ty) = (info.slot, info.ty);
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Load(ty, Operand::Value(slot), 0),
        ty,
        None,
    );
    let Some(n) = packed else {
        return Some(Operand::Value(v));
    };
    // The cap guards the callee's materialize against reading past
    // the relay's packed buffer. A length-only this-first body is on
    // the argc tier alone since rotation 650 (`packed` is None for
    // it, the branch above), so a typed HOF site reaching it through
    // `__fwdrecv_` answers the true count; rotation 649 had measured
    // lifting the cap while such a body still took the argv channel,
    // and `[7].map(t, o)` died on the over-read.
    Some(Operand::Value(emit_min_const(ctx, v, n as i64)))
}

/// `min(v, n)` without a select: `n + ((v - n) & ((v - n) >> 63))`
/// — the difference's sign mask keeps it only when negative.
fn emit_min_const(ctx: &mut LowerCtx<'_>, v: crate::ssa::ValueId, n: i64) -> crate::ssa::ValueId {
    use crate::ssa::BinOp;
    let b = ctx.cur_block;
    let d = ctx.f.append_inst(
        b,
        InstKind::BinOp(BinOp::Sub, Operand::Value(v), Operand::ConstI64(n)),
        Type::I64,
        None,
    );
    let mask = ctx.f.append_inst(
        b,
        InstKind::BinOp(BinOp::AShr, Operand::Value(d), Operand::ConstI64(63)),
        Type::I64,
        None,
    );
    let neg = ctx.f.append_inst(
        b,
        InstKind::BinOp(BinOp::And, Operand::Value(d), Operand::Value(mask)),
        Type::I64,
        None,
    );
    ctx.f.append_inst(
        b,
        InstKind::BinOp(BinOp::Add, Operand::ConstI64(n), Operand::Value(neg)),
        Type::I64,
        None,
    )
}

/// The head-less callee's synthetic leading slots: the H1 hidden
/// argc, and — since RFC 20260816-headless-argv-face — the raw argv
/// pointer that follows it. Neither is visible in the AST arg list,
/// so both shift every sig-aligned read and both get prepended at
/// the very end of the emit.
#[derive(Clone, Copy)]
pub(crate) struct HeadlessSlots {
    pub(crate) argc: bool,
    pub(crate) argv: bool,
    /// Where the pair sits in the callee's signature — 0 for a
    /// head-less body, and for a this-first body the width of the
    /// head it declares (`__this` = 1 for a twin or a promoted
    /// declaration, `__this` + `__new_target` = 2 for a constructor;
    /// rotation 616 / 627).
    /// Also the number of leading AST arguments that are NOT user
    /// arguments, which is what `arguments` counts.
    pub(crate) at: usize,
}

impl HeadlessSlots {
    pub(crate) fn width(self) -> usize {
        usize::from(self.argc) + usize::from(self.argv)
    }

    /// Signature index of AST argument `i`.
    pub(crate) fn sig_at(self, i: usize) -> usize {
        if i < self.at { i } else { i + self.width() }
    }
}

pub(crate) fn headless_slots(ctx: &LowerCtx<'_>, eid: ExprId, callee: ExprId) -> HeadlessSlots {
    let name = if let Some(n) = ctx.call_retargets.get(&eid) {
        n.as_str()
    } else if let crate::ast::Expr::Ident(n) = ctx.ast.get_expr(callee) {
        n.as_str()
    } else {
        return HeadlessSlots {
            argc: false,
            argv: false,
            at: 0,
        };
    };
    // The this-first lane carries its slots right after the head
    // its declaration has: the argc on every admitted body, the argv
    // pointer only on one that reads argument values (rotation 650
    // split the tiers as the head-less face has them).
    if let Some(&head) = ctx.ast.this_first_argc_fns.get(name) {
        return HeadlessSlots {
            argc: true,
            argv: ctx.ast.this_first_argv_fns.contains_key(name),
            at: head,
        };
    }
    HeadlessSlots {
        argc: ctx.ast.headless_argc_fns.contains(name),
        argv: ctx.ast.headless_argv_fns.contains(name),
        at: 0,
    }
}

/// The count of arguments the SOURCE wrote at this call, which is
/// what `arguments` describes — `apply_default_args` may have
/// appended slots holding the callee's own declared defaults.
pub(crate) fn user_argc(ctx: &LowerCtx<'_>, eid: ExprId, lowered: usize) -> usize {
    ctx.ast
        .default_padded_argc
        .get(&eid)
        .copied()
        .unwrap_or(lowered)
}

/// RFC 20260816-headless-argv-face — box the call's argument
/// operands into a stack `AnyValue[]` and answer its pointer (the
/// value that rides the callee's synthetic `__torajs_argv` param)
/// with the number of slots it holds.
///
/// The slots are BORROWS, matching the boxed adapter's ledger: the
/// body's `__torajs_arguments_materialize` expansion incs every heap
/// cell it stores into the array, and each operand outlives the call
/// (owned temps release only after it). So nothing is inc'd here and
/// nothing needs releasing.
///
/// The buffer allocas in the entry block (mirroring `pack_any_argv`)
/// so a call inside a loop reuses one slab instead of growing the
/// frame per iteration.
pub(crate) fn pack_headless_argv(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    args: &[ExprId],
    ops: &[Operand],
    skip: usize,
) -> (Operand, usize) {
    // A ctor's leading `__this` / `__new_target` are not arguments
    // the program wrote — the buffer starts after them.
    let n = user_argc(ctx, eid, args.len())
        .min(ops.len())
        .saturating_sub(skip);
    let buf = ctx.f.append_inst(
        crate::ssa::BlockId(0),
        InstKind::AllocaBytes((n.max(1) * 8) as u64),
        Type::Ptr,
        Some("__hl_argv"),
    );
    for i in 0..n {
        let op = ops[skip + i];
        let slot = if ctx.operand_ty(&op) == Type::Any {
            op
        } else {
            ctx.box_to_any_from_expr(args[skip + i], op)
        };
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(slot, Operand::Value(buf), (i * 8) as u64),
        );
    }
    (Operand::Value(buf), n)
}

/// The argc of a call `apply_rest_args` packed: the fixed slots the
/// source filled plus the packed array's runtime length (see
/// `Ast::rest_packed_sites`). `argv` is the user-aligned operand
/// list, whose last operand IS that array. `None` for any other
/// call, which keeps the static count.
pub(crate) fn rest_packed_argc(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    argv: &[Operand],
) -> Option<Operand> {
    let given = *ctx.ast.rest_packed_sites.get(&eid)?;
    let arr = *argv.last()?;
    let b = ctx.cur_block;
    let len = ctx.f.append_inst(
        b,
        InstKind::Load(Type::I64, arr, crate::ssa_lower::ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let n = ctx.f.append_inst(
        b,
        InstKind::BinOp(
            crate::ssa::BinOp::Add,
            Operand::ConstI64(given as i64),
            Operand::Value(len),
        ),
        Type::I64,
        None,
    );
    Some(Operand::Value(n))
}

/// Move the synthetic leading slots into the user-aligned `argv`:
/// the argv pointer (when packed) and then the argc, so the pair
/// sits at `headless.at` in signature order. The count is the call's
/// argument-expression count, minus the trailing slots
/// `apply_default_args` materialized from the callee's declared
/// defaults (those are the callee's own values, not arguments the
/// program passed) — unless a relay's own runtime count or a
/// rest-packed site's runtime length names it instead.
pub(crate) fn insert_headless_slots(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    headless: HeadlessSlots,
    args: &[ExprId],
    argv: &mut Vec<Operand>,
    headless_argv: Option<(Operand, usize)>,
) {
    let packed = headless_argv.as_ref().map(|(_, n)| *n);
    // A rest-packed site's count is a runtime value (the packed
    // array's length) — read it off the user-aligned list before the
    // synthetic slots move in.
    let rest_argc = headless
        .argc
        .then(|| rest_packed_argc(ctx, eid, argv))
        .flatten();
    if let Some((op, _)) = headless_argv {
        argv.insert(headless.at.min(argv.len()), op);
    }
    if headless.argc {
        let argc_op =
            forwarded_argc(ctx, callee, packed)
                .or(rest_argc)
                .unwrap_or(Operand::ConstI64(
                    user_argc(ctx, eid, args.len()).saturating_sub(headless.at) as i64,
                ));
        argv.insert(headless.at.min(argv.len()), argc_op);
    }
}
