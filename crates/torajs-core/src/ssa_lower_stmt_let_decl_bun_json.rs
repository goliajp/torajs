//! `let X: T = await Bun.file(p).json()` fast-path extracted from
//! [`crate::ssa_lower_stmt_let_decl`] (chunk 171b — file-size debt
//! cleanup follow-up to chunk 169 + 170).
//!
//! Pre-extract this T-19.d fast-path was 45 LOC inline inside
//! `ssa_lower_stmt_let_decl::lower`. Body verbatim moves here as
//! `try_lower(ctx, name, type_ann, init) -> bool` (true = handled,
//! caller returns early; false = not this shape, caller continues).
//!
//! T-19.d (v0.5.0) — `let X: T = await Bun.file(p).json()`:
//! - same caller-driven typed JSON parse machinery as JSON.parse(text)
//! - but with the file read inlined via `__torajs_fs_read_file_sync`
//! - `: number` annotation widens to F64 (per T-02 number→F64 rule)

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
    name: &str,
    type_ann: Option<&String>,
    init: ExprId,
) -> bool {
    let Some(mut slot_ty_for_parse) = ctx.try_resolve_type_ann(type_ann.map(|s| s.as_str())) else {
        return false;
    };
    let Some(path_eid) = ctx.is_bun_file_json_await(init) else {
        return false;
    };
    if matches!(slot_ty_for_parse, Type::I64) && type_ann.map(|s| s.as_str()) == Some("number") {
        slot_ty_for_parse = Type::F64;
    }
    let path_op = ctx.lower_expr(path_eid);
    let str_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.fs_read_file_sync, vec![path_op]),
        Type::Str,
        None,
    );
    let ann = type_ann.map_or("", |a| a.as_str());
    let class_depth = crate::ssa_lower_stmt_let_decl_json_parse::class_depth(ctx, ann);
    if class_depth.is_some() {
        use crate::ssa_lower_stmt_let_decl_json_parse::{declare, parse_into_class_slot};
        let text_any = ctx.box_to_any(Operand::Value(str_v));
        let drop_text = Some(Operand::Value(str_v));
        let v = parse_into_class_slot(ctx, text_any, drop_text, slot_ty_for_parse, ann);
        declare(ctx, name, slot_ty_for_parse, v);
        return true;
    }
    let result = ctx.lower_json_parse_annotated(Operand::Value(str_v), slot_ty_for_parse, ann);
    ctx.emit_drop_value(Operand::Value(str_v), Type::Str);
    ctx.emit_throw_check_owned(None, result, slot_ty_for_parse);
    crate::ssa_lower_stmt_let_decl_json_parse::declare(ctx, name, slot_ty_for_parse, result);
    true
}
