//! `let v: T = JSON.parse(text)` fast-path extracted from
//! [`crate::ssa_lower_stmt_let_decl`] (chunk 173 — file-size debt
//! cleanup follow-up to chunks 169/172).
//!
//! Pre-extract this T-02 fast-path was 52 LOC inline inside
//! `ssa_lower_stmt_let_decl::lower`. Body verbatim moves here as
//! `try_lower(ctx, name, type_ann, init) -> bool`.
//!
//! T-02 (v0.3.0) — caller-driven typed JSON parse:
//! - slot annotation gives the target type; runtime parser
//!   dispatches per shape via per-shape recursive helpers
//! - `: number` annotation widens to F64 (JS spec Number is f64;
//!   JSON has no integer-vs-float distinction so we follow bun)
//! - `widen_container_ty` agrees with the slot layout (typed cursor
//!   parser MUST agree, else parse_int eats `2` of `2.5` and
//!   deranges every later field)
//! - text Str drop on fresh-owned (literal / call result / concat);
//!   borrow-shaped (Ident / Member / Index) is the source binding's
//!   responsibility

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LocalInfo, LowerCtx};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
    name: &str,
    type_ann: Option<&String>,
    init: ExprId,
) -> bool {
    let Some(mut slot_ty_for_parse) = ctx.try_resolve_type_ann(type_ann.map(|s| s.as_str())) else {
        return false;
    };
    if !ctx.is_json_parse_call(init) {
        return false;
    }
    if matches!(slot_ty_for_parse, Type::I64) && type_ann.map(|s| s.as_str()) == Some("number") {
        slot_ty_for_parse = Type::F64;
    }
    slot_ty_for_parse = crate::ssa_lower_container_width::widen_container_ty(
        slot_ty_for_parse,
        type_ann.map(|s| s.as_str()),
        &ctx.num_width_local_key(name),
        ctx.num_f64_slots,
        ctx.arr_layouts,
        ctx.struct_layouts,
        ctx.fn_sigs,
    );
    let text_eid = if let Expr::Call { args, .. } = ctx.ast.get_expr(init).clone() {
        args[0]
    } else {
        unreachable!()
    };
    // RFC 20260808-json-parse-any blade 3 — the typed parse reads
    // `text` as a Str cell; a non-String argument (admitted since the
    // namespace sig widened to Any) must ride the any-lane kernel,
    // whose ToString covers §25.5.1 step 1 (pre-gate this SIGSEGV'd:
    // json_parse_int dereferenced a NaN-box as a Str pointer).
    if let Some(t) = ctx.expr_types.get(&text_eid)
        && !matches!(t, crate::check::Type::String)
    {
        return false;
    }
    let text_op = ctx.lower_expr(text_eid);
    let cursor = ctx.alloca(Type::I64, Some("__json_pos"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(cursor), 0),
    );
    let result = ctx.lower_json_parse(text_op, Operand::Value(cursor), slot_ty_for_parse);
    if ctx.expr_is_fresh_owned(text_eid) && ctx.operand_ty(&text_op).is_refcounted() {
        ctx.emit_drop_value(text_op, ctx.operand_ty(&text_op));
    }
    let slot = ctx.binding_slot_alloca(slot_ty_for_parse, name);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(result, Operand::Value(slot), 0),
    );
    let cur_depth = ctx.scope_stack.len() - 1;
    ctx.locals.insert(
        name.to_string(),
        LocalInfo {
            slot,
            ty: slot_ty_for_parse,
            moved: false,
            borrowed: false,
            scope_depth: cur_depth,
        },
    );
    let top = ctx.scope_stack.last_mut().expect("scope frame");
    top.push(name.to_string());
    true
}
