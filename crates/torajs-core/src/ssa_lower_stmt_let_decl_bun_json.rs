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
    let cursor = ctx.alloca(Type::I64, Some("__json_pos"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(cursor), 0),
    );
    let result = ctx.lower_json_parse(
        Operand::Value(str_v),
        Operand::Value(cursor),
        slot_ty_for_parse,
    );
    ctx.emit_drop_value(Operand::Value(str_v), Type::Str);
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
    ctx.scope_stack.last_mut().unwrap().push(name.to_string());
    true
}
