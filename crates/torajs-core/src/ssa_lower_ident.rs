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

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, name: &str) -> Operand {
    if ctx.locals.get(name).is_none() {
        if name == "NaN" {
            return Operand::ConstF64(f64::NAN);
        }
        if name == "Infinity" {
            return Operand::ConstF64(f64::INFINITY);
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
        if name == "undefined" {
            return Operand::ConstPtrNull;
        }
        if let Some(op) = try_builtin_ctor_ident(ctx, name) {
            return op;
        }
    }
    lower_local_binding(ctx, name)
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
