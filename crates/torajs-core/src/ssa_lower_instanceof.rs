//! `Expr::InstanceOf { expr, rhs }` lowering pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s match arm as chunk-72
//! of the decomp (chunks 1-71 = ... + `Expr::TypeOf` 6-layer fold).
//!
//! The target is an expression, and only the shape that names
//! something — an `Expr::Ident` — takes the compile-time ladder below.
//! Everything else goes straight to the §13.10.2 operator in
//! [`dynamic::lower_general_rhs`], which needs no new runtime code:
//! the kernel behind it already implements the whole operator.
//!
//! Phase H.1.c — runtime class membership via the header tag stored
//! at `OBJ_CLASS_TAG_OFF`. Compile-time we compute the closure of
//! `class_name` and every class that extends it (transitively);
//! runtime reads the tag and checks membership in that set.
//! Heterogeneous arrays distinguish subclasses correctly: an
//! `Animal[]` slot holding a `Dog` instance carries the Dog tag in
//! its header, so `dog instanceof Animal` reads Dog's tag and
//! matches because Dog is in Animal's descendant set.
//!
//! The descendant tag set is a small `Vec<u32>` emitted as an
//! `ICmp::Eq` + `Or`-chain. LLVM converts it into a switch table for
//! hierarchies past a threshold; for typical 1-3 class hierarchies
//! the chain is shorter than a switch table.
//!
//! Four dispatch layers tried in source order:
//!
//! 1. **Compile-time static fold** — V3-18 m2.i: built-in
//!    constructor `instanceof` checks against the operand's static
//!    SSA type. Per JS spec, primitives are NOT instances of their
//!    boxing constructor (`5 instanceof Number === false`); only
//!    boxed `new Number(5)` would be true (tora's subset doesn't
//!    ship boxed primitives so they're always false). Gate the
//!    static-fold catch-all arms on `actual_ty != Type::Any` —
//!    Any operands carry runtime tag dispatch (handled next) and
//!    pre-folding to `false` here would mask `arrAny instanceof
//!    Array` (and sibling built-in cases) as a constant false.
//! 2. **`Type::Any` runtime dispatch** — for `catch (e: any) { e
//!    instanceof TypeError }` shape, the inline `Type::Obj(_)`
//!    path can't see through the NaN-box, so emit a runtime
//!    dispatch:
//!    - Built-in tag check first via
//!      `__torajs_instanceof_builtin_any_tag(v, type_tag)`
//!      because Tag::Arr / Date / RegExp / Promise / Map / Set /
//!      WeakMap / WeakSet / Closure cells don't carry a
//!      `class_tag@+8` slot (only user-declared classes do), so
//!      the user-class OR-chain below would always emit false.
//!      Tags mirror `torajs-rc::Tag`: Arr=2, Closure=3 (Function),
//!      RegExp=4, Date=5, Promise=8, WeakMap=12, WeakSet=13,
//!      Map=15, Set=19.
//!    - `instanceof Object` accepts any heap cell whose tag isn't
//!      a primitive wrapper via `__torajs_instanceof_object_any`.
//!    - User-class descendant_tags loop: per-tag
//!      `__torajs_instanceof_class_any_tag(v, expected_tag)` calls
//!      OR-chained.
//! 3. **Non-Obj early return** — operand isn't `Type::Obj(_)` and
//!    didn't match a built-in static-fold → trivially `false`.
//! 4. **`Type::Obj` typed-receiver** — Load class tag at
//!    `OBJ_CLASS_TAG_OFF`, compute descendant_tags set, emit
//!    `ICmp::Eq` + `Or`-chain. Empty set (unknown class name) →
//!    constant-false.
//!
//! Returns `Operand` directly (terminal arm — caller's
//! `Expr::InstanceOf` match arm bottoms out here).

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Type, ValueId};
use crate::ssa_lower::{LowerCtx, OBJ_CLASS_TAG_OFF};

mod dynamic;

use dynamic::{emit_has_instance, lower_general_rhs, try_lower_dynamic_target};

/// Which of the two lanes the right-hand side takes.
///
/// A bare name keeps the whole compile-time ladder below — the class
/// hierarchy IS the spec answer for it, and folding is what makes
/// `instanceof` free in the typed tier. Anything larger is a value
/// nobody can name at compile time, so it goes to the §13.10.2
/// operator, which the runtime lane already implements in full.
pub(crate) fn lower(ctx: &mut LowerCtx<'_>, expr: ExprId, rhs: ExprId) -> Operand {
    match ctx.ast.get_expr(rhs) {
        Expr::Ident(n) => {
            let name = n.clone();
            lower_named(ctx, expr, &name)
        }
        _ => lower_general_rhs(ctx, expr, rhs),
    }
}

fn lower_named(ctx: &mut LowerCtx<'_>, expr: ExprId, class_name: &str) -> Operand {
    // RFC 20260713 blade 5 — `x instanceof g` where g is a generator
    // factory: §27.5.3 [[HasInstance]] walks g.prototype, which IS
    // the desugared __Gen class's prototype object, so the class
    // tag-set dispatch below is the exact equivalent.
    let class_name = ctx
        .ast
        .generator_factory_classes
        .get(class_name)
        .cloned()
        .unwrap_or_else(|| class_name.to_string());
    let class_name = class_name.as_str();
    let v = ctx.lower_expr(expr);
    let actual_ty = ctx.operand_ty(&v);
    // RFC 20260730-iterator-global 刀 1 — `x instanceof Iterator` is
    // pure §7.3.22 prototype-chain membership (no per-instance heap
    // tag): iterator cells fold true, object-shaped operands walk
    // the chain at runtime (generator instances / `extends Iterator`
    // heirs / arbitrary objects), everything else is spec-false
    // (OrdinaryHasInstance step 3 rejects non-Objects; a closure's
    // chain tops out at %Function.prototype% — recorded boundary
    // for a monkey-patched Function.prototype.__proto__).
    if class_name == "Iterator" {
        return lower_iterator_instanceof(ctx, v, actual_ty);
    }
    // §13.10.2 step 2 — a name bound to a plain VALUE can carry an
    // `@@hasInstance` handler, which every fold below is blind to.
    // Ahead of them all: the handler decides the answer for ANY V,
    // so the operand-type early-outs (`3 instanceof Odd` is not an
    // object, so step 3 of the ordinary walk would say false) must
    // not pre-empt it either.
    if let Some(op) = try_lower_dynamic_target(ctx, v.clone(), class_name) {
        return op;
    }
    if let Some(r) = try_compile_time_fold(actual_ty, class_name) {
        return Operand::ConstBool(r);
    }
    if matches!(actual_ty, Type::Any) {
        return lower_any_dispatch(ctx, v, class_name);
    }
    if !matches!(actual_ty, Type::Obj(_)) {
        return Operand::ConstBool(false);
    }
    lower_typed_obj_dispatch(ctx, v, class_name)
}

/// `x instanceof Iterator` — see the call site's rationale. The
/// runtime walk is `__torajs_instanceof_builtin_proto(v, 15)` (15 =
/// %Iterator.prototype%'s builtin-proto tag).
fn lower_iterator_instanceof(ctx: &mut LowerCtx<'_>, v: Operand, actual_ty: Type) -> Operand {
    const ITERATOR_PROTO_TAG: i64 = 15;
    let v_any = match actual_ty {
        Type::MapIter | Type::ArrIter => return Operand::ConstBool(true),
        Type::Any => v,
        Type::Obj(_) => ctx.box_to_any(v),
        _ => return Operand::ConstBool(false),
    };
    let cur_block = ctx.cur_block;
    let r = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.instanceof_builtin_proto,
            vec![v_any, Operand::ConstI64(ITERATOR_PROTO_TAG)],
        ),
        Type::Bool,
        None,
    );
    Operand::Value(r)
}

fn try_compile_time_fold(actual_ty: Type, class_name: &str) -> Option<bool> {
    if matches!(actual_ty, Type::Any) {
        return match class_name {
            // RFC 20260716 刀 2 — `x instanceof {Number,String,Boolean}`
            // for Any x now dispatches at runtime against
            // Tag::*Wrapper. BigInt / Symbol still compile-time false
            // (primitives with no dedicated wrapper substrate).
            "BigInt" | "Symbol" => Some(false),
            _ => None,
        };
    }
    match (class_name, &actual_ty) {
        ("Array", Type::Arr(_)) => Some(true),
        ("Array", _) => Some(false),
        ("Function", Type::Closure(_)) | ("Function", Type::FnSig(_)) => Some(true),
        ("Function", _) => Some(false),
        ("Object", Type::Obj(_)) => Some(true),
        ("Object", Type::Arr(_)) => Some(true),
        ("Object", Type::Date) => Some(true),
        ("Object", Type::RegExp) => Some(true),
        ("Object", Type::Promise) => Some(true),
        ("Object", Type::Map | Type::Set | Type::WeakMap | Type::WeakSet) => Some(true),
        ("Object", Type::MapIter | Type::ArrIter) => Some(true),
        ("Object", Type::Closure(_)) | ("Object", Type::FnSig(_)) => Some(true),
        ("Object", _) => Some(false),
        ("Number" | "String" | "Boolean" | "BigInt" | "Symbol", _) => Some(false),
        ("Date", Type::Date) => Some(true),
        ("Date", _) => Some(false),
        ("RegExp", Type::RegExp) => Some(true),
        ("RegExp", _) => Some(false),
        ("Promise", Type::Promise) => Some(true),
        ("Promise", _) => Some(false),
        ("Map", Type::Map) => Some(true),
        ("Map", _) => Some(false),
        ("Set", Type::Set) => Some(true),
        ("Set", _) => Some(false),
        ("WeakMap", Type::WeakMap) => Some(true),
        ("WeakMap", _) => Some(false),
        ("WeakSet", Type::WeakSet) => Some(true),
        ("WeakSet", _) => Some(false),
        ("WeakRef", Type::WeakRef) => Some(true),
        ("WeakRef", _) => Some(false),
        _ => None,
    }
}

fn lower_any_dispatch(ctx: &mut LowerCtx<'_>, v: Operand, class_name: &str) -> Operand {
    if let Some(t) = builtin_type_tag(class_name) {
        let cur_block = ctx.cur_block;
        let r = ctx.f.append_inst(
            cur_block,
            InstKind::Call(
                ctx.intrinsics.instanceof_builtin_any_tag,
                vec![v, Operand::ConstI64(t)],
            ),
            Type::Bool,
            None,
        );
        return Operand::Value(r);
    }
    if class_name == "Object" {
        let cur_block = ctx.cur_block;
        let r = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.instanceof_object_any, vec![v]),
            Type::Bool,
            None,
        );
        return Operand::Value(r);
    }
    let descendant_tags = compute_descendant_tags(ctx, class_name);
    if descendant_tags.is_empty() {
        if let Some(r) = try_lower_fn_value(ctx, v, class_name) {
            return r;
        }
        return Operand::ConstBool(false);
    }
    emit_any_class_or_chain(ctx, v, &descendant_tags)
}

/// Rotation 345 (RFC 20260808-construct-channel) — `o instanceof C`
/// where the name resolves to a Closure-typed BINDING rather than a
/// class: §7.3.22 OrdinaryHasInstance against the fn value's
/// canonical `.prototype` (the same fnprops cell the construct
/// kernel links, so `Array.from.call(C, …)` products answer true).
/// The runtime helper throws on a prototype-less callable (step 5),
/// so the call carries a throw check.
fn try_lower_fn_value(ctx: &mut LowerCtx<'_>, v_any: Operand, class_name: &str) -> Option<Operand> {
    if let Some(info) =
        crate::ssa_lower_call_closure_local::resolve_closure_binding(ctx, class_name)
        && matches!(info.ty, Type::Closure(_))
    {
        let cur_block = ctx.cur_block;
        let cell = ctx.f.append_inst(
            cur_block,
            InstKind::Load(info.ty.clone(), Operand::Value(info.slot), 0),
            info.ty,
            None,
        );
        return Some(emit_has_instance(ctx, v_any, Operand::Value(cell), None));
    }
    // Bare FnDecl name (nested or top-level `function F() {}`): no
    // Closure binding exists, but a VALUE use elsewhere synthesized
    // the `__forward_F` wrapper whose canonical `__fncell_*` cell is
    // the ES singleton every channel shares — the construct path's
    // `fn_prototype_pair` hangs the canonical `.prototype` off that
    // same cell, so the §7.3.22 walk converges. The canonical mint
    // answers +1 per use; the probe releases it after the call. A fn
    // never used as a value has no forwarder and keeps the
    // constant-false fallback (recorded residue: nothing constructed
    // through it either, so the answer is right anyway).
    let fwd = format!("__forward_{class_name}");
    let fid = ctx.fn_table.get(&fwd).copied()?;
    let closure_ty = crate::ssa_lower_closure::closure_value_ty(ctx, &fwd);
    let cell = crate::ssa_lower_closure_canonical::try_lower_canonical_cell(
        ctx, &fwd, fid, closure_ty, false,
    )?;
    Some(emit_has_instance(ctx, v_any, cell, Some(closure_ty)))
}

fn builtin_type_tag(class_name: &str) -> Option<i64> {
    match class_name {
        "Array" => Some(2),
        "Function" => Some(3),
        "RegExp" => Some(4),
        "Date" => Some(5),
        "Promise" => Some(8),
        "Map" => Some(15),
        "Set" => Some(19),
        "WeakMap" => Some(12),
        "WeakSet" => Some(13),
        "WeakRef" => Some(11),
        // RFC 20260716 刀 2 — Tag::NumberWrapper = 21,
        // Tag::StringWrapper = 22, Tag::BooleanWrapper = 23.
        "Number" => Some(21),
        "String" => Some(22),
        "Boolean" => Some(23),
        _ => None,
    }
}

fn lower_typed_obj_dispatch(ctx: &mut LowerCtx<'_>, v: Operand, class_name: &str) -> Operand {
    let descendant_tags = compute_descendant_tags(ctx, class_name);
    if descendant_tags.is_empty() {
        // Fn-value binding behind the name — box the typed receiver
        // and take the OrdinaryHasInstance walk (doc on the fn). The
        // cheap existence probe keeps the constant-false path free
        // of a dead box (resolve emits a GlobalRef when it hits).
        let is_fn_binding = matches!(
            ctx.locals.get(class_name).map(|i| &i.ty),
            Some(Type::Closure(_))
        ) || matches!(ctx.globals.get(class_name), Some(Type::Closure(_)))
            // Bare FnDecl name with a synthesized forwarder — the
            // canonical-cell lane inside try_lower_fn_value.
            || ctx
                .fn_table
                .contains_key(format!("__forward_{class_name}").as_str());
        if is_fn_binding {
            let v_any = ctx.box_to_any(v);
            if let Some(r) = try_lower_fn_value(ctx, v_any, class_name) {
                return r;
            }
        }
        return Operand::ConstBool(false);
    }
    let cur_block = ctx.cur_block;
    let tag_v = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::I64, v, OBJ_CLASS_TAG_OFF),
        Type::I64,
        None,
    );
    emit_obj_tag_or_chain(ctx, tag_v, &descendant_tags)
}

fn compute_descendant_tags(ctx: &LowerCtx<'_>, class_name: &str) -> Vec<u32> {
    let mut descendant_tags: Vec<u32> = Vec::new();
    if !ctx.ast.class_parents.contains_key(class_name) {
        return descendant_tags;
    }
    for c in ctx.ast.class_parents.keys() {
        let mut cur = Some(c.clone());
        let mut depth = 0u32;
        while let Some(name) = cur {
            if depth > 64 {
                break;
            }
            if name == class_name {
                if let Some(tag) = ctx.class_name_to_tag.get(c) {
                    descendant_tags.push(*tag);
                }
                break;
            }
            cur = ctx.ast.class_parents.get(&name).and_then(|p| p.clone());
            depth += 1;
        }
    }
    descendant_tags.sort();
    descendant_tags.dedup();
    descendant_tags
}

fn emit_any_class_or_chain(ctx: &mut LowerCtx<'_>, v: Operand, tags: &[u32]) -> Operand {
    let mut acc: Option<ValueId> = None;
    for &t in tags {
        let cur_block = ctx.cur_block;
        let one = ctx.f.append_inst(
            cur_block,
            InstKind::Call(
                ctx.intrinsics.instanceof_class_any_tag,
                vec![v.clone(), Operand::ConstI64(t as i64)],
            ),
            Type::Bool,
            None,
        );
        acc = Some(or_chain_step(ctx, acc, one));
    }
    Operand::Value(acc.unwrap())
}

fn emit_obj_tag_or_chain(ctx: &mut LowerCtx<'_>, tag_v: ValueId, tags: &[u32]) -> Operand {
    let mut acc: Option<ValueId> = None;
    for &t in tags {
        let cur_block = ctx.cur_block;
        let eq = ctx.f.append_inst(
            cur_block,
            InstKind::ICmp(
                IPred::Eq,
                Operand::Value(tag_v),
                Operand::ConstI64(t as i64),
            ),
            Type::Bool,
            None,
        );
        acc = Some(or_chain_step(ctx, acc, eq));
    }
    Operand::Value(acc.unwrap())
}

fn or_chain_step(ctx: &mut LowerCtx<'_>, acc: Option<ValueId>, next: ValueId) -> ValueId {
    match acc {
        None => next,
        Some(prev) => {
            let cur_block = ctx.cur_block;
            ctx.f.append_inst(
                cur_block,
                InstKind::BinOp(SsaBinOp::Or, Operand::Value(prev), Operand::Value(next)),
                Type::Bool,
                None,
            )
        }
    }
}
