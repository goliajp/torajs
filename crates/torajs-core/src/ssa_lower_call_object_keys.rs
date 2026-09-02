//! `Object.keys(obj)` / `Object.getOwnPropertyNames(obj)` /
//! `Reflect.ownKeys(obj)` namespace static methods pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as chunk-18
//! of the `Expr::Call` god-arm decomp (chunks 1-17 = Arr higher-order +
//! Map dispatch + Set dispatch + Arr.push + Number instance methods +
//! bare-name globals + Str regex methods + Number namespace + Array.from +
//! Arr predicate iter + Arr.flatMap + Object.entries + fn-indirect +
//! Number/String/Boolean coercion + universal methods + closure-local +
//! Object.values).
//!
//! Three surfaces share this arm. tr has no prototype chain + no symbol
//! keys, so own-string-keys == all-own-keys for structs; the only
//! difference is whether Array/String `length` is included:
//! - `Object.keys`: spec §22.1.3.16 — enumerable-own only, so Array/String
//!   `length` is non-enumerable (§10.4.2.4 step 4 / §22.1.5.1) → omit.
//! - `Object.getOwnPropertyNames`: all own keys → include `length`.
//! - `Reflect.ownKeys`: aliases `getOwnPropertyNames` here (no symbol
//!   keys → identical result for these receivers).
//!
//! Surface picked by callee `m_name` (`"keys"` vs others).
//!
//! Four receiver routes:
//! - `Type::Arr(_)`: Load `arr.len` at `ARR_LEN_OFF=8` + route through
//!   per-surface helper (`__torajs_arr_keys_only` / `__torajs_arr_index_strs`).
//!   Length is runtime-dynamic so the helper builds the str array
//!   (`"0".."<len-1>"` + optional trailing `"length"`).
//! - `Type::Str`: per-surface helper (`__torajs_str_keys_only` /
//!   `__torajs_str_index_strs`) reads u32 length at `STR_LEN_OFF=8`
//!   internally and delegates to its Arr counterpart.
//! - `Type::Any` (W-J Phase C1): struct identity is only known at
//!   runtime. `__torajs_anyv_struct_keys` reads `class_tag@+8`, looks
//!   the layout up, and walks the field names. `keys` and
//!   `getOwnPropertyNames` coincide for a struct (no prototype chain,
//!   no array `length`), so both surfaces share this arm. Non-struct
//!   cells throw loudly — propagated via `emit_throw_check`.
//! - `Type::Obj(struct)`: compile-time literal name array. Zero-cost
//!   reflection — emit `arr_alloc(N)` + N direct stores of interned str
//!   ptrs, identical to writing `["x", "y", ...]` by hand.
//!
//! S255 + S297 — ES §20.1.2.{17,22} / §28.1.11 trailing-arg ignore:
//! widens the `args.len() == 1` gate to `>= 1` and lower-and-drops
//! `args[1..]` for spec L-to-R side-effect order (check.rs already
//! typecheck-drops them).
//!
//! Returns `Some(result)` when callee matches one of the three surfaces
//! with a non-empty args list; non-Arr/Str/Any/Obj receivers panic at
//! SSA lower-time (preserving the original block's behavior). `None`
//! lets the caller fall through to the next arm.

use crate::ast::PropKey;
use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, intern_arr_layout};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    let matched = (ns == "Object"
        && (m_name == "keys"
            // chunk B2 — parser-synthesized for-in keys source; same
            // enumerable-own emit as `keys` except the Any arm
            // tolerates null / undefined (enumerates nothing).
            || m_name == "__forinKeys"
            || m_name == "getOwnPropertyNames"
            || m_name == "getOwnPropertySymbols"))
        // Reflect.ownKeys aliases the same emit — tr has no symbol keys
        // + no prototype chain so own-string-keys == all-own-keys.
        || (ns == "Reflect" && m_name == "ownKeys");
    if !matched {
        return None;
    }
    // S255 — widen `== 1` → `>= 1` per ES §20.1.2.{17,22} / §28.1.11
    // trailing-arg ignore. Lower only args[0] (the target obj);
    // trailing args dropped at lower-time (check.rs already type_of'd
    // them).
    if args.is_empty() {
        return None;
    }
    let arg_op = ctx.lower_expr(args[0]);
    // S297 — lower-and-drop trailing args past the 1 useful obj slot
    // per S255 (S272 idiom). check.rs already type_of'd them.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    // An owned receiver temp (member-read chain like `D.prototype`,
    // call result, as-cast) has no other release site — every kernel
    // below reads the receiver without consuming it. Rotation 325
    // census: the stranded +1 sat on the class prototype through the
    // at-exit cycle drain and either leaked its group or cut it in
    // two (same defect family as the gOPD receiver / rotation 324
    // member-write receiver). Snapshot the pre-box operand: the
    // Closure box below is rc-neutral, and the release wants the
    // receiver's own type.
    let arg_raw = arg_op.clone();
    // Cluster #4 follow-up (rotation 235) — a typed Closure receiver
    // boxes to any at this boundary and rides the runtime own-keys
    // walk: `anyv_own_keys`' TAG_CLOSURE_CELL arm answers the
    // §20.2.4 virtual length/name/prototype face plus expando props
    // (borrow-shaped box, RC-NEUTRAL — no release).
    let arg_op = if matches!(ctx.operand_ty(&arg_op), Type::Closure(_)) {
        ctx.box_to_any(arg_op)
    } else {
        arg_op
    };
    let arg_ty = ctx.operand_ty(&arg_op);
    if m_name == "getOwnPropertySymbols" {
        return Some(lower_symbols_arm(ctx, args[0], arg_op, &arg_raw, &arg_ty));
    }
    // `Object.keys` filters to enumerable-own per spec §22.1.3.16;
    // Array/String `length` is non-enumerable (§10.4.2.4 step 4 /
    // §22.1.5.1) so it's omitted. `getOwnPropertyNames` / `Reflect.ownKeys`
    // include all own keys (length included). Pick helper by surface
    // name so the three share the SSA arm.
    let is_keys_only = m_name == "keys" || m_name == "__forinKeys";

    if matches!(arg_ty, Type::Arr(_)) {
        return Some(lower_arr_receiver_keys(
            ctx,
            args[0],
            arg_op,
            &arg_raw,
            &m_name,
            is_keys_only,
        ));
    }
    if matches!(arg_ty, Type::Str) {
        return Some(lower_str_receiver_keys(
            ctx,
            args[0],
            arg_op,
            &arg_raw,
            is_keys_only,
        ));
    }
    // W-J Phase C1 — Any receiver: struct identity is only known at
    // runtime. `__torajs_anyv_struct_keys` reads `class_tag@+8`,
    // looks the layout up, and walks the field names. `keys` and
    // `getOwnPropertyNames` coincide for a struct (no prototype
    // chain, no array `length`), so both surfaces share this arm. A
    // non-struct cell throws loudly inside the helper — propagate it.
    if matches!(arg_ty, Type::Any) {
        // RC-4 F1c — a DynObj cell (defineProperty-degraded binding)
        // walks its live entries with the surface's enumerable
        // filter; struct cells keep the compile-time-layout walk.
        // chunk B2 — the for-in surface routes through
        // `anyv_forin_keys` so a null / undefined receiver
        // enumerates nothing (§14.7.5) instead of the ToObject
        // TypeError `Object.keys` raises.
        let call = if m_name == "__forinKeys" {
            InstKind::Call(ctx.intrinsics.anyv_forin_keys, vec![arg_op])
        } else if m_name == "ownKeys" {
            // §28.1.11 wants the symbol bucket too, and an Any
            // receiver is the only route that can carry one — the
            // typed arms above are Arr / Str / struct, none of which
            // holds symbol-keyed properties.
            InstKind::Call(ctx.intrinsics.anyv_own_keys_all, vec![arg_op])
        } else {
            InstKind::Call(
                ctx.intrinsics.anyv_own_keys,
                vec![arg_op, Operand::ConstI64(if is_keys_only { 0 } else { 1 })],
            )
        };
        let v = ctx.f.append_inst(
            ctx.cur_block,
            call,
            Type::Arr(intern_arr_layout(ctx.arr_layouts, Type::Str)),
            None,
        );
        ctx.emit_throw_check(None);
        ctx.release_owned_temp(args[0], &arg_raw);
        return Some(Operand::Value(v));
    }
    if m_name == "__forinKeys" && !matches!(arg_ty, Type::Obj(_)) {
        return Some(lower_forin_nonstruct_keys(ctx, args[0], arg_op, &arg_raw));
    }
    let field_names: Vec<PropKey> = match arg_ty {
        Type::Obj(sid) => {
            // RFC 20260714-objlit-accessor — an accessor lives in the
            // layout under a synthetic slot name (`__getter_v` /
            // `__setter_v`), but ES §10.4 makes it one own property
            // keyed by the plain name. Enumerate it as `v`, once (a
            // get/set pair is a single key), never as the internal
            // spelling.
            let mut out: Vec<PropKey> = Vec::new();
            for (fname, _) in ctx.struct_layouts[sid.0 as usize].iter() {
                let key = match crate::check_type_of_object_lit::accessor_slot(fname) {
                    Some((_, prop)) => PropKey::from(prop),
                    None => fname.clone(),
                };
                if !out.contains(&key) {
                    out.push(key);
                }
            }
            out
        }
        other => panic!("ssa-lower: Object.{m_name} requires a struct arg, got {other:?}"),
    };
    let n = field_names.len() as i64;
    let str_ty = Type::Str;
    let arr_id = intern_arr_layout(ctx.arr_layouts, str_ty);
    let arr_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc, vec![Operand::ConstI64(n)]),
        Type::Arr(arr_id),
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(n), Operand::Value(arr_ptr), ARR_LEN_OFF),
    );
    let data = ctx.emit_arr_data_ptr(Operand::Value(arr_ptr));
    for (i, fname) in field_names.iter().enumerate() {
        let str_v = ctx.intern_string_literal(fname);
        let off = (i as u64) * 8;
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(str_v), data.clone(), off),
        );
    }
    // RC-4 F1c — Object.defineProperty converts a struct receiver to
    // a DynObj and rebinds (emit_any_dynobj_writeback), so the
    // compile-time field list can be stale. Route through the runtime
    // chooser: a DynObj cell walks its live entries (ES §10.1.11.1
    // order; `keys` filters enumerable-only), anything else returns
    // the static list unchanged.
    let chosen = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.obj_own_keys,
            vec![
                arg_op,
                Operand::Value(arr_ptr),
                Operand::ConstI64(if is_keys_only { 0 } else { 1 }),
            ],
        ),
        Type::Arr(arr_id),
        None,
    );
    ctx.release_owned_temp(args[0], &arg_raw);
    Some(Operand::Value(chosen))
}

/// W-N-c — `Object.getOwnPropertySymbols`: an Any receiver routes
/// through the runtime, which reads §10.1.11.1's symbol bucket off
/// the receiver's live property dict (and throws ToObject on
/// undefined / null per §20.1.2.10 step 1). A statically typed
/// receiver takes the compile-time empty array: a symbol key can
/// only arrive through `Object.defineProperty` / a computed key,
/// both of which degrade the binding to the Any dynobj lane first,
/// so a still-typed cell provably holds none.
fn lower_symbols_arm(
    ctx: &mut LowerCtx<'_>,
    recv_eid: ExprId,
    arg_op: Operand,
    arg_raw: &Operand,
    arg_ty: &Type,
) -> Operand {
    let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Symbol);
    if matches!(arg_ty, Type::Any) {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.anyv_own_symbols, vec![arg_op]),
            Type::Arr(arr_id),
            None,
        );
        ctx.emit_throw_check(None);
        ctx.release_owned_temp(recv_eid, arg_raw);
        return Operand::Value(v);
    }
    let empty = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc, vec![Operand::ConstI64(0)]),
        Type::Arr(arr_id),
        None,
    );
    ctx.release_owned_temp(recv_eid, arg_raw);
    Operand::Value(empty)
}

/// W-N-d — Str receiver: keys → `["0", ..., "<len-1>"]`,
/// getOwnPropertyNames → `[..., "length"]` (§22.1.5.2.4). The
/// helper reads the u32 length at `STR_LEN_OFF=8` internally and
/// delegates to its Arr counterpart, so the SSA arm just passes
/// the Str ptr through.
/// W-N-b — Arr<T> receiver: every surface boxes the cell
/// (borrow-shaped, RC-NEUTRAL) and rides the full kernel arm. The old
/// per-surface helpers minted index strings off `arr.len` alone, so
/// expando keys living in the props bag never appeared — `a.p = 2;
/// Object.getOwnPropertyNames(a)` answered without "p" (r330
/// registered defect #4, sm/object/15.2.3.4-02), and `for (k in a)`
/// skipped enumerable expandos the same way (RFC 20260808 knife 5 —
/// `anyv_forin_keys` starts from the same arr_cell_keys walk,
/// enumerable-filtered).
/// §14.7.5.6 — a for-in head whose source is STATICALLY non-struct
/// (an undefined / null literal types Ptr and used to hit the
/// struct-arm panic) enumerates through the same kernel arm the Any
/// receiver rides: `anyv_forin_keys` answers the empty key set for a
/// nullish receiver instead of ToObject's TypeError.
fn lower_forin_nonstruct_keys(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    arg_op: Operand,
    arg_raw: &Operand,
) -> Operand {
    let boxed = ctx.box_to_any(arg_op);
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.anyv_forin_keys, vec![boxed]),
        Type::Arr(intern_arr_layout(ctx.arr_layouts, Type::Str)),
        None,
    );
    ctx.emit_throw_check(None);
    ctx.release_owned_temp(arg_eid, arg_raw);
    Operand::Value(v)
}

fn lower_arr_receiver_keys(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    arg_op: Operand,
    arg_raw: &Operand,
    m_name: &str,
    is_keys_only: bool,
) -> Operand {
    let boxed = ctx.box_to_any(arg_op);
    let call = if m_name == "__forinKeys" {
        InstKind::Call(ctx.intrinsics.anyv_forin_keys, vec![boxed])
    } else {
        InstKind::Call(
            ctx.intrinsics.anyv_own_keys,
            vec![boxed, Operand::ConstI64(if is_keys_only { 0 } else { 1 })],
        )
    };
    let v = ctx.f.append_inst(
        ctx.cur_block,
        call,
        Type::Arr(intern_arr_layout(ctx.arr_layouts, Type::Str)),
        None,
    );
    ctx.release_owned_temp(arg_eid, arg_raw);
    Operand::Value(v)
}

fn lower_str_receiver_keys(
    ctx: &mut LowerCtx<'_>,
    recv_eid: ExprId,
    arg_op: Operand,
    arg_raw: &Operand,
    is_keys_only: bool,
) -> Operand {
    let helper = if is_keys_only {
        ctx.intrinsics.str_keys_only
    } else {
        ctx.intrinsics.str_index_strs
    };
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(helper, vec![arg_op]),
        Type::Arr(intern_arr_layout(ctx.arr_layouts, Type::Str)),
        None,
    );
    ctx.release_owned_temp(recv_eid, arg_raw);
    Operand::Value(v)
}
