//! `Expr::Member { obj, name }` (Member READ) dispatch pulled out
//! of [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Member` match
//! arm as chunk-83 of the decomp.
//!
//! Pure dispatcher — every layer is a `try_lower`-shaped sibling
//! call. Order matters: each layer claims a specific Member-shape
//! and short-circuits; later layers see only the leftovers.
//!
//! 1. **T-27.c** built-in `f.length` / `f.name` for top-level FnDecl
//!    or closure local-binding (synthetic `__env` / `__this` /
//!    `__torajs_real_argc` / rest params filtered; `__closure_N`
//!    emitted as `""` per JS spec).
//! 2. **T-15.g.2 + P10.4** `await p` (= `p.value`) on built-in
//!    `Type::Promise(T)` + primitive identity fast-path
//!    (Number/String/Boolean/Array/BigInt collapse to obj itself
//!    per ES spec).
//! 3. **T-13.c** well-known Symbol singletons
//!    (Symbol.{iterator|asyncIterator|toPrimitive}).
//! 4. **T-18.c + T-21** `Bun.file(p).size` synchronous fs lookup +
//!    `<response>.status` Response struct field (web/runtime).
//! 5. **v0.3 #3** `process.{platform|argv|env}` + `Bun.argv` +
//!    `process.env.NAME` namespace cluster.
//! 6. `Math.<C>` / `Number.<C>` / `<Ctor>.{prototype,name,length}`
//!    builtin-namespace constants + singleton-lookup.
//! 7. **typed-receiver props** — prim.constructor /
//!    Symbol.description / Arr.length / Map/Set.size /
//!    Closure/FnSig.{length,name}.
//! 8. **Type::RegExp accessor** — source / flags / 6 bool flag
//!    accessors / lastIndex (T-37 followup + ES §22.2.6.4-10 +
//!    P9.4).
//! 9. **Str/Substr.length** — read u64 length at STR_LEN_OFF
//!    (offset 8) via `ssa_lower_str::load_str_or_substr_length`.
//! 10. **Type::Any** — RFC 20260613 any-class-member-read
//!    dispatch.
//! 11. **Type::Closure** (T-27) — Function-as-Object read. Loads
//!    the closure's lazy props_dynobj at CLOSURE_PROPS_OFF.
//!    NULL → undef box per ECMAScript missing-prop semantics.
//! 12. **T-27.b + T-29** — FnSig-as-Object + Array-as-Object
//!    Member read via side-table-keyed-by-ptr storage
//!    (NULL/missing key → ANY_UNDEF).
//! 13. **Type::Obj(sid) terminal** (P8.2) — accessor read +
//!    struct-field-layout fallback.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, eid: ExprId, obj: ExprId, name: &str) -> Operand {
    if let Some(op) = crate::ssa_lower_member_fn_intro::try_lower(ctx, obj, name) {
        return op;
    }
    if let Some(op) = crate::ssa_lower_member_promise_value::try_lower(ctx, obj, name) {
        return op;
    }
    if let Some(op) = crate::ssa_lower_member_symbol_wellknown::try_lower(ctx, obj, name) {
        return op;
    }
    if let Some(op) = crate::ssa_lower_member_web_runtime::try_lower(ctx, obj, name) {
        return op;
    }
    if let Some(op) = crate::ssa_lower_member_process::try_lower(ctx, obj, name) {
        return op;
    }
    if let Some(op) = crate::ssa_lower_member_builtin_namespace::try_lower(ctx, eid, obj, name) {
        return op;
    }
    let obj_val = ctx.lower_expr(obj);
    let obj_ty = ctx.operand_ty(&obj_val);
    let result = lower_with_val(ctx, eid, obj, obj_val, obj_ty, name);
    // Chunk 637 — an owned receiver temp (`f().x`, `new K(i).x`,
    // `(wr.deref() as K).x`) had no release site: the field READ
    // itself only borrows, so probe l16f leaked every receiver
    // (300k `new K(i).x` churn: 25.5 MB vs 6.4 MB flat). A non-Copy
    // result borrows receiver memory — detach it with an owned inc
    // BEFORE the receiver drop (rc math: field 1 → inc 2 → receiver
    // teardown dec 1 → independent), and record the Member eid so
    // consumers take the ref over (`expr_owned_shape`) instead of
    // stacking their own. Copy results (i64/f64/bool loads) need
    // no detach. Chunk 717 — reads that are already owned (the
    // Any-member / Closure-props lanes record their eid inside
    // `lower_with_val`) skip the detach inc: the result carries its
    // own stake independent of the receiver, a second inc would
    // strand one ref per read. The receiver temp still drops.
    if ctx.expr_owned_shape(obj) && !obj_ty.is_copy() {
        let res_ty = ctx.operand_ty(&result);
        if !res_ty.is_copy() && !ctx.owned_member_reads.contains(&eid) {
            ctx.emit_owned_result_inc(result, res_ty);
            ctx.owned_member_reads.insert(eid);
        }
        ctx.emit_drop_value(obj_val, obj_ty);
    }
    result
}

/// Post-receiver dispatch ladder (layers 7-13 of the module doc).
/// `eid` is the Member expression's own id — the Any-member and
/// Closure-props lanes record it in `owned_member_reads` (chunk 717
/// owned-result contract). Also the OptChain hit path's dispatch
/// (chunk 791 — non-Obj receivers reuse the ladder instead of
/// re-implementing per-type member reads).
pub(crate) fn lower_with_val(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    obj_val: Operand,
    obj_ty: Type,
    name: &str,
) -> Operand {
    if name == "__proto__" {
        // Annex B §B.2.2.1 — `o.__proto__` IS the [[Prototype]]
        // getter, and it answers for every receiver shape, so it sits
        // ahead of the typed ladder: a typed receiver boxes into the
        // Any lane the intrinsic reads (which is also what
        // `Object.getPrototypeOf` calls, so the two cannot drift).
        // Owned return, the `.length` shape.
        ctx.owned_member_reads.insert(eid);
        let any_op = ctx.box_to_any(obj_val);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.proto_member_get, vec![any_op]),
            Type::Any,
            None,
        );
        ctx.emit_throw_check(None);
        return Operand::Value(v);
    }
    if let Some(op) =
        crate::ssa_lower_member_typed_props::try_lower(ctx, obj, obj_val, obj_ty, name)
    {
        return op;
    }
    if let Some(op) = crate::ssa_lower_member_regexp_props::try_lower(ctx, obj_val, obj_ty, name) {
        return op;
    }
    if (obj_ty == Type::Str || obj_ty == Type::Substr) && name == "length" {
        // RFC 20260707-undefined-sentinel-repr chunk 1 — a missed
        // exec/match capture slot is NULL; the inline length load
        // below would SIGSEGV. Guard arms a catchable TypeError
        // (no-op for non-nullable receivers).
        crate::ssa_lower_nullable_guard::emit_nullable_str_guard(ctx, obj, &obj_val);
        return crate::ssa_lower_str::load_str_or_substr_length(ctx, obj_val, obj_ty);
    }
    if matches!(obj_ty, Type::Any) {
        return crate::ssa_lower_any_member::lower_any_member_read(ctx, eid, obj_val, name);
    }
    if matches!(obj_ty, Type::Closure(_)) {
        // Chunk 717 — the expando read answers owned on every arm
        // (`emit_dynobj_get_result`'s data arm takes the payload inc;
        // the NULL-props arm boxes an immediate undef). Record the
        // eid so consumers release it.
        ctx.owned_member_reads.insert(eid);
        return ctx.fn_props_get(obj_val, name);
    }
    if let Some(op) = crate::ssa_lower_member_props_read::try_lower(ctx, obj, obj_val, obj_ty, name)
    {
        return op;
    }
    let sid = match obj_ty {
        Type::Obj(sid) => sid,
        _ => panic!("ssa-lower: member access on non-object {obj_ty:?} (.{name})"),
    };
    crate::ssa_lower_member_obj_field::try_lower(ctx, obj, obj_val, sid, name)
}
