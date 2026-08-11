//! `Expr::Ident(name)` lowering pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s match arm as chunk-77
//! of the decomp (chunks 1-76 = ... + `Expr::Array` MAIN PRIZE).
//!
//! Six-layer fallback dispatch (locals shadow globals throughout):
//!
//! 1. **V3-18 m1.h.11** — `NaN` / `Infinity` JS spec globals → f64
//!    constants directly (`ConstF64(0.0/0.0)` / `ConstF64(1.0/0.0)`).
//!    Local bindings shadow these per spec (writable in non-strict
//!    mode; tora doesn't model that yet — non-shadowed access
//!    produces canonical value).
//! 2. **M2 Phase B Stage 4** — bare Ident referring to a global fn
//!    (no local shadow) yields the fn's address as a `FnSig` value
//!    via `InstKind::FnAddr(fid)`. Used when passing a fn name
//!    directly as an arg or returning it.
//! 3. **Top-level `const X = LITERAL` inline fallback** — Copy
//!    types only (Number → ConstI64/F64; Bool → ConstBool). Only
//!    IMMUTABLE literal-init globals (V3-18 m1.h.26: mutable globals
//!    like `Counter.value` need GlobalRef + Load so reads see
//!    post-assign value). String literals fall through to K.3
//!    path because inlining `intern_string_literal` per read site
//!    would leak one alloc per read (m-oo-04-static incident:
//!    `Counter.label !== "ctr"` paid LHS alloc on every cmp).
//! 4. **K.3 module-level data global** — `globals[name] → ty`
//!    triggers `GlobalRef + Load` (slot init at main entry by
//!    LetDecl arm).
//! 5. **P4.5 class / proto sentinel** — `__class_<NAME>` and
//!    `__proto_<NAME>` Ident references from inside fn bodies
//!    resolve via `class_name_to_tag` lookup + runtime
//!    `__torajs_class_get(tag)` / `__torajs_proto_get(tag)` (Any
//!    return). Phase A1's rewrite `Ident(C) → Ident(__class_C)`
//!    blocks Type::Any K.3-promotion (refcount/slot-shape
//!    contract not yet in place).
//! 6. **`undefined` literal** — accepted at typecheck as
//!    `Type::Null` (check.rs's bare-name globals). Lower same as
//!    `null` literal: `ConstPtrNull` 0-shaped pointer sentinel.
//!
//! Falls through to local-binding load: `info = locals[name]` →
//! `Load(info.ty, info.slot, 0)`. Unknown ident panics.
//!
//! Returns `Operand` directly (terminal arm — caller's
//! `Expr::Ident` match arm bottoms out here).

use crate::ast::{Expr, Stmt};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, eid: crate::ast::ExprId, name: &str) -> Operand {
    if ctx.locals.get(name).is_none() {
        if let Some(op) = try_undeclared_read_throw(ctx, eid, name) {
            return op;
        }
        if name == "NaN" {
            return Operand::ConstF64(f64::NAN);
        }
        if name == "Infinity" {
            return Operand::ConstF64(f64::INFINITY);
        }
        // L3b ③ — a generic-fn VALUE escape was retargeted by the
        // checker to its all-`any` clone (`$$anywv`); take the
        // CLONE's address — the generic original never lowers.
        if let Some(mono) = ctx.call_retargets.get(&eid).cloned()
            && let Some(op) = try_global_fn_addr(ctx, &mono)
        {
            return op;
        }
        if let Some(op) = try_global_fn_addr(ctx, name) {
            return op;
        }
        if let Some(op) = try_inline_const_literal(ctx, name) {
            return op;
        }
        if let Some(op) = try_k3_global_data(ctx, name) {
            return op;
        }
        if let Some(op) = try_class_sentinel(ctx, name) {
            return op;
        }
        if let Some(op) = try_proto_sentinel(ctx, name) {
            return op;
        }
        if let Some(op) = try_async_tail_undefined(ctx, name) {
            return op;
        }
        if name == "undefined" {
            return Operand::ConstPtrNull;
        }
        if let Some(op) = try_builtin_ctor_ident(ctx, name) {
            return op;
        }
        if let Some(op) = try_ns_object_ident(ctx, name) {
            return op;
        }
    }
    lower_local_binding(ctx, name)
}

/// RFC 20260801-ns-object-value — `Math` in a value position (thisArg
/// / call arg / return / identity compare) answers the interned
/// namespace-object singleton (an immortal pre-filled dynobj, Any).
/// Static member reads and `Math.xxx(...)` calls never reach here —
/// they resolve in the member / call lanes first — and a local
/// binding named Math shadows through the outer `locals` gate.
/// `globalThis` rides the same lane (RFC 20260807-global-object G2 —
/// its singleton pre-fills the ctor cells and value props; static
/// member reads were already rewritten to bare names by the G1
/// desugar, so only the bare value read reaches here).
fn try_ns_object_ident(ctx: &mut LowerCtx<'_>, name: &str) -> Option<Operand> {
    let intrinsic = match name {
        "Math" => ctx.intrinsics.ns_object_math,
        "JSON" => ctx.intrinsics.ns_object_json,
        "Reflect" => ctx.intrinsics.ns_object_reflect,
        "eval" => ctx.intrinsics.global_eval_value,
        "globalThis" => ctx.intrinsics.globalthis_object,
        _ => return None,
    };
    if ctx.ast.class_parents.contains_key(name) {
        return None;
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(intrinsic, vec![]),
        Type::Any,
        None,
    );
    Some(Operand::Value(v))
}

/// RFC 20260730-undeclared-ident — §6.2.5.5 GetValue on an
/// unresolvable Reference. The checker marked this read
/// (`ast.undeclared_reads`), so evaluating it raises a catchable
/// ReferenceError (`<name> is not defined`). The value lane past the
/// throw-check is unreachable; `undefined`'s shape stands in so the
/// enclosing expression still types out.
fn try_undeclared_read_throw(
    ctx: &mut LowerCtx<'_>,
    eid: crate::ast::ExprId,
    name: &str,
) -> Option<Operand> {
    if !ctx.ast.undeclared_reads.contains_key(&eid) {
        return None;
    }
    let name_str = ctx.intern_string_literal(name);
    let raiser = ctx.intrinsics.throw_reference_error_name;
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(raiser, vec![Operand::Value(name_str)]),
    );
    ctx.emit_throw_check(None);
    Some(Operand::ConstPtrNull)
}

/// The `__undef_slot__<T>` marker `desugar_async` plants where an async
/// body runs off its end: ES §10.2.1.4 step 11 settles that path with
/// `undefined`, and this materializes the way width `T` spells it —
/// the same choice [`crate::ssa_lower_fn`] makes for an ordinary
/// function's fall-through path, so the two agree on what an awaiter
/// will see.
///
/// A width with no sentinel of its own (`boolean`, plain I64) keeps the
/// type's zero, which is what the tail carried before any of this — no
/// worse, and there is no bit pattern there to do better with.
fn try_async_tail_undefined(ctx: &mut LowerCtx<'_>, name: &str) -> Option<Operand> {
    let ann = name.strip_prefix(crate::ast::UNDEF_SLOT_MARKER)?;
    // `number` reads back as I64 here — the width is only widened where
    // the promise's value slot is built — but the sentinel IS the reason
    // to be wide, so ask for it by annotation rather than by resolved
    // width.
    if ann == "number" {
        return Some(Operand::ConstF64(f64::from_bits(
            crate::ssa_lower_nullable_guard::F64_UNDEF_SENTINEL_BITS,
        )));
    }
    let ty = ctx.try_resolve_type_ann(Some(ann))?;
    // A fn annotation parses as `FnSig` — a bare function ADDRESS,
    // which is Copy — but the marker only ever stands in a slot that
    // something is stored INTO (a class field, a promise's value, a
    // generator's yield), and a stored function is a `Closure`:
    // refcounted, env-first. The two spell `undefined` differently
    // (RFC 20260710 C2a hands a Copy fn slot the Str-family oddball,
    // C2b hands a refcounted pointer slot the generic cell whose drop
    // stations guard on it), so reading the repr off `FnSig` here put
    // the Str cell in a refcounted field and the rc write faulted on
    // that cell's read-only page — KERN_PROTECTION_FAILURE at
    // `___TORAJS_STR_UNDEF_CELL` + 0, the refcount word. Ask as the
    // slot actually is; `parse_arr_suffix` normalizes the same way for
    // an element slot.
    let ty = match ty {
        Type::FnSig(sig) => Type::Closure(sig),
        t => t,
    };
    Some(ctx.str_undef_sentinel_for(ty.clone()).unwrap_or(match ty {
        Type::F64 => Operand::ConstF64(0.0),
        Type::I64 => Operand::ConstI64(0),
        _ => Operand::ConstPtrNull,
    }))
}

/// L3b ④ — a bare builtin-constructor namespace ident read as a
/// VALUE (`o.constructor === Object` / `x === Array`) boxes the
/// family's interned constructor cell — the same identity the
/// prototype's own `constructor` entry and the `.constructor`
/// reify answer. Locals shadow (outer gate); a user class of the
/// same name resolves its class value through the P4.5 sentinel
/// rewrite instead.
fn try_builtin_ctor_ident(ctx: &mut LowerCtx<'_>, name: &str) -> Option<Operand> {
    if ctx.ast.class_parents.contains_key(name) {
        return None;
    }
    let tag = crate::ssa_lower_member_builtin_namespace::proto_method_tag(name)?;
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.builtin_ctor_value,
            vec![Operand::ConstI64(tag)],
        ),
        Type::Any,
        None,
    );
    Some(Operand::Value(v))
}

fn try_global_fn_addr(ctx: &mut LowerCtx<'_>, name: &str) -> Option<Operand> {
    let fid = ctx.fn_table.get(name).copied()?;
    let sig_id = ctx.fn_sig_ids.get(&fid).copied()?;
    let ty = Type::FnSig(sig_id);
    let cur_block = ctx.cur_block;
    let v = ctx
        .f
        .append_inst(cur_block, InstKind::FnAddr(fid), ty, None);
    Some(Operand::Value(v))
}

fn try_inline_const_literal(ctx: &mut LowerCtx<'_>, name: &str) -> Option<Operand> {
    let name_owned = name.to_string();
    for s in &ctx.ast.stmts {
        let Stmt::LetDecl {
            name: n,
            init,
            mutable,
            ..
        } = s
        else {
            continue;
        };
        if n != &name_owned || *mutable {
            continue;
        }
        match ctx.ast.get_expr(*init) {
            Expr::Number(v) => {
                if v.fract() == 0.0 && v.abs() < (1u64 << 53) as f64 {
                    return Some(Operand::ConstI64(*v as i64));
                }
                return Some(Operand::ConstF64(*v));
            }
            Expr::Bool(b) => return Some(Operand::ConstBool(*b)),
            _ => {}
        }
    }
    None
}

fn try_k3_global_data(ctx: &mut LowerCtx<'_>, name: &str) -> Option<Operand> {
    let ty = ctx.globals.get(name).copied()?;
    let cur_block = ctx.cur_block;
    let ptr = ctx.f.append_inst(
        cur_block,
        InstKind::GlobalRef(name.to_string()),
        Type::Ptr,
        None,
    );
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Load(ty, Operand::Value(ptr), 0),
        ty,
        None,
    );
    Some(Operand::Value(v))
}

fn try_class_sentinel(ctx: &mut LowerCtx<'_>, name: &str) -> Option<Operand> {
    let cname = name.strip_prefix("__class_")?;
    let tag = ctx.class_name_to_tag.get(cname).copied()?;
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.class_get,
            vec![Operand::ConstI64(tag as i64)],
        ),
        Type::Any,
        None,
    );
    Some(Operand::Value(v))
}

fn try_proto_sentinel(ctx: &mut LowerCtx<'_>, name: &str) -> Option<Operand> {
    let cname = name.strip_prefix("__proto_")?;
    let tag = ctx.class_name_to_tag.get(cname).copied()?;
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.proto_get,
            vec![Operand::ConstI64(tag as i64)],
        ),
        Type::Any,
        None,
    );
    Some(Operand::Value(v))
}

fn lower_local_binding(ctx: &mut LowerCtx<'_>, name: &str) -> Operand {
    let info = match ctx.locals.get(name) {
        Some(i) => *i,
        None => panic!("ssa-lower: unknown ident `{name}`"),
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Load(info.ty, Operand::Value(info.slot), 0),
        info.ty,
        None,
    );
    Operand::Value(v)
}
