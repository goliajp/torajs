//! `let o: Pair = Object.fromEntries(es)` fast-path extracted from
//! [`crate::ssa_lower_stmt_let_decl`] (chunk 169 — file-size debt
//! cleanup for chunk 156's sibling).
//!
//! Pre-extract this T-09.c fast-path was 110 LOC inline inside
//! `ssa_lower_stmt_let_decl::lower`. Body verbatim moves here as a
//! `try_lower` free fn returning `bool` (true = handled, caller
//! returns early; false = not this shape, caller continues).
//!
//! T-09.c (v0.4.0) — caller-driven typing for
//! `let o: Pair = Object.fromEntries(es)`:
//! - slot annotation gives the target struct schema
//! - unfold per-field reads from the entries array (assumed in
//!   struct declaration order — matches Object.entries round-trip;
//!   key-matching scan deferred)
//! - each entry's value is untagged from the Any box and stored
//!   into the matching struct field at runtime
//! - heap-typed fields get rc_inc since the new struct takes its
//!   own owning ref (entries array still holds one)

use crate::ast::{Expr, ExprId};
use crate::ssa::{IPred, InstKind, Operand, Type};
use crate::ssa_lower::{LocalInfo, LowerCtx, OBJ_HEADER_SIZE};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
    name: &str,
    type_ann: Option<&String>,
    init: ExprId,
) -> bool {
    let Some(slot_ty) = ctx.try_resolve_type_ann(type_ann.map(|s| s.as_str())) else {
        return false;
    };
    if !ctx.is_fromentries_call(init) {
        return false;
    }
    let Type::Obj(sid) = slot_ty else {
        return false;
    };
    let (entries_eid, trailing): (ExprId, Vec<ExprId>) =
        if let Expr::Call { args, .. } = ctx.ast.get_expr(init).clone() {
            (args[0], args.iter().skip(1).copied().collect())
        } else {
            unreachable!()
        };
    let entries_op = ctx.lower_expr(entries_eid);
    for tid in &trailing {
        // Rotation 325 — owned temps only; an ident-bound borrow
        // keeps its binding's stake (the unconditional drop stole it
        // and the scope-end release dec'd through the freed cell).
        let top = ctx.lower_expr(*tid);
        ctx.release_owned_temp(*tid, &top);
    }
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    let obj_size = OBJ_HEADER_SIZE + (layout.len() as u64) * 8;
    let obj_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.obj_alloc,
            vec![Operand::ConstI64(obj_size as i64)],
        ),
        slot_ty,
        None,
    );
    let obj_op = Operand::Value(obj_ptr);
    ctx.emit_obj_header_init(obj_op.clone());
    // `obj_alloc` is plain malloc — class_tag and vtable_ptr MUST be
    // stored explicitly (a garbage class_tag can alias a real class id
    // and hand the drop/cycle walkers the wrong ClassLayout). The
    // built struct is anonymous; the anon-stamp pool assigns a
    // registered tag so layout-driven walkers release the fields.
    let anon_tag = ctx.anon_stamp_pool.borrow_mut().assign_or_get(sid);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(
            Operand::ConstI64(anon_tag as i64),
            obj_op.clone(),
            crate::ssa_lower::OBJ_CLASS_TAG_OFF,
        ),
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(
            Operand::ConstPtrNull,
            obj_op.clone(),
            crate::ssa_lower::OBJ_VTABLE_OFF,
        ),
    );
    let entries_data = ctx.emit_arr_data_ptr(entries_op.clone());
    for (idx, (_fname, fty)) in layout.iter().enumerate() {
        let inner_off = (idx as u64) * 8;
        let inner_ptr = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::Ptr, entries_data.clone(), inner_off),
            Type::Ptr,
            None,
        );
        let val_tag = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_get_any_tag,
                vec![Operand::Value(inner_ptr), Operand::ConstI64(1)],
            ),
            Type::I64,
            None,
        );
        let val_raw = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_get_any_value,
                vec![Operand::Value(inner_ptr), Operand::ConstI64(1)],
            ),
            Type::I64,
            None,
        );
        let stored: Operand = match *fty {
            Type::I64 | Type::I32 => Operand::Value(val_raw),
            Type::F64 => {
                let f = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::BitCastI64ToF64(Operand::Value(val_raw)),
                    Type::F64,
                    None,
                );
                Operand::Value(f)
            }
            Type::Bool => {
                let b = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::ICmp(IPred::Ne, Operand::Value(val_raw), Operand::ConstI64(0)),
                    Type::Bool,
                    None,
                );
                Operand::Value(b)
            }
            t if t.is_refcounted() => {
                ctx.emit_rc_inc(Operand::Value(val_raw));
                Operand::Value(val_raw)
            }
            other => {
                panic!("not yet supported: Object.fromEntries field type {other:?}")
            }
        };
        let off = OBJ_HEADER_SIZE + (idx as u64) * 8;
        ctx.f
            .append_void(ctx.cur_block, InstKind::Store(stored, obj_op.clone(), off));
        let _ = val_tag;
    }
    // Rotation 325 — same ownership settlement as the trailing args:
    // the per-field walk above only borrows the entries array.
    ctx.release_owned_temp(entries_eid, &entries_op);
    let slot = ctx.binding_slot_alloca(slot_ty, name);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(obj_op, Operand::Value(slot), 0),
    );
    let cur_depth = ctx.scope_stack.len() - 1;
    ctx.locals.insert(
        name.to_string(),
        LocalInfo {
            slot,
            ty: slot_ty,
            moved: false,
            borrowed: false,
            scope_depth: cur_depth,
        },
    );
    let top = ctx.scope_stack.last_mut().expect("scope frame");
    top.push(name.to_string());
    true
}
