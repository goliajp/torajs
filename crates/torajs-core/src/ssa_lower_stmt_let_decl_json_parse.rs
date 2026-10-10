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
    let declared = slot_ty_for_parse;
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
    let class_depth = type_ann.and_then(|a| class_depth(ctx, a));
    if class_depth.is_some() {
        let text_op = ctx.lower_expr(text_eid);
        let arg = ctx.box_to_any_from_expr(text_eid, text_op);
        let owned = ctx.expr_is_fresh_owned(text_eid) && ctx.operand_ty(&text_op).is_refcounted();
        let drop_text = owned.then_some(text_op);
        let ann = type_ann.map_or("", |a| a.as_str());
        let v = parse_into_class_slot(ctx, arg, drop_text, declared, ann);
        declare(ctx, name, declared, v);
        return true;
    }
    // The typed parse reads `text` as a Str cell; a non-String
    // argument (admitted since the namespace sig widened to Any) must
    // ride the any-lane kernel, whose ToString covers §25.5.1 step 1
    // (a NaN-box read as a Str pointer would fault).
    if let Some(t) = ctx.expr_types.get(&text_eid)
        && !matches!(t, crate::check::Type::String)
    {
        return false;
    }
    let text_op = ctx.lower_expr(text_eid);
    let ann = type_ann.map_or("", |a| a.as_str());
    let result = ctx.lower_json_parse_annotated(text_op, slot_ty_for_parse, ann);
    if ctx.expr_is_fresh_owned(text_eid) && ctx.operand_ty(&text_op).is_refcounted() {
        ctx.emit_drop_value(text_op, ctx.operand_ty(&text_op));
    }
    // The parse's SyntaxError / TypeError surfaces here, before the
    // binding exists: the result built so far is released on the
    // throw edge.
    ctx.emit_throw_check_owned(None, result, slot_ty_for_parse);
    declare(ctx, name, slot_ty_for_parse, result);
    true
}

/// `Some(d)` when `ann` names a class under `d` array levels (`P`,
/// `P[]`, `Array<P>`), either one `| null`.
pub(crate) fn class_depth(ctx: &LowerCtx, ann: &str) -> Option<u32> {
    let mut t = crate::check_type_ann::strip_nullable(ann.trim()).trim();
    let mut depth = 0;
    loop {
        if let Some(inner) = t.strip_suffix("[]") {
            t = inner.trim_end();
        } else if let Some(inner) = t.strip_prefix("Array<").and_then(|r| r.strip_suffix('>')) {
            t = inner.trim();
        } else {
            break;
        }
        depth += 1;
    }
    let base = t.split('<').next().unwrap_or(t).trim_end();
    ctx.ast.class_parents.contains_key(base).then_some(depth)
}

/// `const p: P = JSON.parse(text)` with `P` a class, or an array of
/// one (`ty` the slot's type, `ann` its annotation): the text parses
/// first, on the any lane (its SyntaxError comes before the cast's
/// TypeError, §25.5.1), and the value then meets the annotation as a
/// checked cast of the user's own type (`ssa_lower_checked_cast::obj`):
/// a parsed object whose fields have the class's field types becomes
/// a fresh instance of the class, anything else raises the TypeError.
/// Answers the owned value, NULL under a pending throw.
pub(crate) fn parse_into_class_slot(
    ctx: &mut LowerCtx,
    text_any: Operand,
    drop_text: Option<Operand>,
    ty: Type,
    ann: &str,
) -> Operand {
    let parsed = Operand::Value(ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.json_parse_any, vec![text_any]),
        Type::Any,
        None,
    ));
    if let Some(t) = drop_text {
        ctx.emit_drop_value(t, ctx.operand_ty(&t));
    }
    ctx.emit_throw_check(None);
    let site = format!("JSON.parse result does not have the annotated type `{ann}`");
    let v = ctx.emit_plan_cast_unchecked(parsed, ty, ann, &site);
    ctx.emit_drop_value(parsed, Type::Any);
    ctx.emit_throw_check_owned(None, v, ty);
    v
}

pub(crate) fn declare(ctx: &mut LowerCtx, name: &str, ty: Type, value: Operand) {
    let slot = ctx.binding_slot_alloca(ty, name);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(value, Operand::Value(slot), 0),
    );
    let cur_depth = ctx.scope_stack.len() - 1;
    ctx.locals.insert(
        name.to_string(),
        LocalInfo {
            slot,
            ty,
            moved: false,
            borrowed: false,
            boxed: false,
            scope_depth: cur_depth,
        },
    );
    let top = ctx.scope_stack.last_mut().expect("scope frame");
    top.push(name.to_string());
}
