//! `JSON.rawJSON(text)` / `JSON.isRawJSON(O)` — ES2026
//! json-parse-with-source (§25.5.1 / §25.5.3).
//!
//! Both route to runtime kernels (torajs-anyvalue `json_raw.rs`):
//! `rawJSON` mints the frozen `[[IsRawJSON]]` carrier the any-lane
//! `JSON.stringify` walk splices verbatim; `isRawJSON` probes the
//! header bit and never throws. The argument is boxed to the any
//! lane (`ToString` / the slot probe both live there), and only the
//! minting side needs a throw check (TypeError from
//! ToString(Symbol), SyntaxError from the §25.5.1 text gates).
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not the
//! `JSON.<method>` Member-Ident shape).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    let is_raw = match m_name.as_str() {
        "rawJSON" => false,
        "isRawJSON" => true,
        _ => return None,
    };
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "JSON" {
        return None;
    }
    // Evaluate the first argument (a missing one is `undefined`,
    // which rawJSON turns into a SyntaxError via ToString); extra
    // arguments still evaluate for their side effects, then drop.
    let arg = match args.first() {
        Some(&a) => {
            let op = ctx.lower_expr(a);
            ctx.box_to_any_from_expr(a, op)
        }
        None => {
            // ANY_UNDEF=5, payload 0 — the missing-argument
            // `undefined` (rawJSON then answers the §25.5.1
            // SyntaxError at runtime).
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            Operand::Value(v)
        }
    };
    for &extra in args.iter().skip(1) {
        let op = ctx.lower_expr(extra);
        let ty = ctx.operand_ty(&op);
        ctx.emit_drop_value(op, ty);
    }
    let kernel = if is_raw {
        ctx.intrinsics.json_is_raw_json
    } else {
        ctx.intrinsics.json_raw_json
    };
    let out = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(kernel, vec![arg]),
        Type::Any,
        None,
    );
    if !is_raw {
        ctx.emit_throw_check(None);
    }
    Some(Operand::Value(out))
}
