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
    if let Some(op) = crate::ssa_lower_member_promise_value::try_lower(ctx, eid, obj, name) {
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
    if let Some(op) =
        crate::ssa_lower_member_regexp_props::try_lower(ctx, eid, obj_val, obj_ty, name)
    {
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
    if let Some(op) = try_lower_str_method_value(ctx, eid, obj, &obj_val, &obj_ty, name) {
        return op;
    }
    if matches!(obj_ty, Type::Any) {
        return crate::ssa_lower_any_member::lower_any_member_read(ctx, eid, obj_val, name);
    }
    if obj_ty == Type::Promise {
        // Typed Promise receiver expando read — box into the any
        // member lane, whose Tag::Promise arm probes the +32 bag and
        // falls through to the builtin reify (rotation 353 get
        // channel). The `.value` fast-path family answered earlier in
        // the ladder; whatever reaches here is an expando (or proto)
        // name the typed world has no slot for.
        let boxed = ctx.box_to_any(obj_val);
        return crate::ssa_lower_any_member::lower_any_member_read(ctx, eid, boxed, name);
    }
    if matches!(obj_ty, Type::Closure(_)) {
        // RFC 20260722-find-miss chunk C — an expando read through a
        // find/findLast miss must throw like bun, not read past the
        // sentinel header. No-op for plain receivers.
        crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, obj, &obj_val);
        // Chunk 717 — the expando read answers owned on every arm
        // (`emit_dynobj_get_result`'s data arm takes the payload inc;
        // the NULL-props arm boxes an immediate undef). Record the
        // eid so consumers release it.
        ctx.owned_member_reads.insert(eid);
        return ctx.fn_props_get(obj_val, name);
    }
    if let Some(op) =
        crate::ssa_lower_member_props_read::try_lower(ctx, eid, obj, obj_val, obj_ty, name)
    {
        return op;
    }
    let sid = match obj_ty {
        Type::Obj(sid) => sid,
        _ => panic!("ssa-lower: member access on non-object {obj_ty:?} (.{name})"),
    };
    // S2.24 刀 4 — a desugar-minted default-guarded pattern load
    // (`Ast::dstr_default_member_loads`) whose ANCHOR layout lacks
    // the field becomes a RUNTIME GetV (§13.15.5.4): a static miss is
    // not a runtime miss — a prefix-compatible heterogeneous array
    // types its elements by the anchor (`[{}, {b: 3}]` → Struct([])),
    // and the wider element really carries the field. Box the
    // receiver and ride the any-member IC + runtime probe: a hit
    // answers the value, a true miss answers ANY_UNDEF and the
    // guard's default fires. User reads keep the layout panic.
    if ctx.ast.dstr_default_member_loads.contains(&eid)
        && !ctx.struct_layouts[sid.0 as usize]
            .iter()
            .any(|(f, _)| f == name)
    {
        let boxed_recv = ctx.box_to_any(obj_val);
        return crate::ssa_lower_any_member::lower_any_member_read(ctx, eid, boxed_recv, name);
    }
    // S2.34 — a class-instance read whose name misses the layout but
    // IS a method of the receiver's class (own or inherited; private
    // names arrive pre-mangled as `__priv_<C>__<m>`; generator
    // methods live under the parser-hoisted `__cm_gen_` spelling)
    // answers the method VALUE per §10.1.8.1 [[Get]]: box the
    // receiver and ride the any-member lane, which resolves the
    // reified class-method cell / generator factory the runtime
    // registration wired onto the prototype. Field reads keep the
    // typed fast path; a genuinely unknown name keeps the loud
    // layout panic (silent-wrong guard).
    if !ctx.struct_layouts[sid.0 as usize]
        .iter()
        .any(|(f, _)| f == name)
        && receiver_class_owns_method(ctx, obj, name)
    {
        let boxed_recv = ctx.box_to_any(obj_val);
        return crate::ssa_lower_any_member::lower_any_member_read(ctx, eid, boxed_recv, name);
    }
    crate::ssa_lower_member_obj_field::try_lower(ctx, eid, obj, obj_val, sid, name)
}

/// The builtin proto family a receiver's method VALUE read mints
/// against (`torajs_rc::builtin_proto` tags), or `None` when the
/// receiver is not reifiable. Str / Substr, Number and Boolean
/// receivers each reify off their own prototype so the runtime's
/// family-generic gate — §22.1.3 ToString(this) for the String
/// family, the §21.1.3 / §20.3.3 brand checks for the wrapper
/// families — selects correctly on `.call` / `.bind`. Array
/// receivers reify off the Array prototype: an Array-minted cell's
/// `.call` re-dispatch reaches the ES "intentionally generic"
/// array-like arm on a plain-object thisArg (`[].flat.call(obj)`).
pub(crate) fn mv_family_of_ssa_ty(t: &Type) -> Option<i64> {
    Some(match t {
        Type::Str | Type::Substr => torajs_rc::builtin_proto::STRING_PROTO_TAG as i64,
        Type::I64 | Type::F64 => torajs_rc::builtin_proto::NUMBER_PROTO_TAG as i64,
        Type::Bool => torajs_rc::builtin_proto::BOOLEAN_PROTO_TAG as i64,
        Type::Arr(_) => torajs_rc::builtin_proto::ARRAY_PROTO_TAG as i64,
        _ => return None,
    })
}

/// Checker-type twin of [`mv_family_of_ssa_ty`] — the let-decl
/// recorder and the checker's own gates classify before lowering.
pub(crate) fn mv_family_of_checker_ty(t: &crate::check::Type) -> Option<i64> {
    Some(match t {
        crate::check::Type::String => torajs_rc::builtin_proto::STRING_PROTO_TAG as i64,
        crate::check::Type::Number => torajs_rc::builtin_proto::NUMBER_PROTO_TAG as i64,
        crate::check::Type::Boolean => torajs_rc::builtin_proto::BOOLEAN_PROTO_TAG as i64,
        crate::check::Type::Array(_) => torajs_rc::builtin_proto::ARRAY_PROTO_TAG as i64,
        _ => return None,
    })
}

/// RFC 20260725-str-method-value-reify — a builtin method read as a
/// VALUE off a String-typed receiver (`const m = s.slice`) resolves
/// the interned mid-cell (family 3), typed as the checker
/// signature's Closure repr. ES semantics: the read is UNBOUND —
/// the receiver evaluates for effect only (the enclosing dispatch
/// already lowered it; the owned-receiver release in `lower` still
/// runs, and the detach-inc no-ops on the immortal cell). Calls
/// route through the boxed dual entry (`variadic_locals` — the
/// let-decl shape recorder), where a bare call is the spec
/// this-undefined TypeError; `.call`/`.apply`/`.bind` ride the
/// any-lane method dispatch (`any_method_call`'s sugar arm).
/// S2.34 — true iff `name` is a method of the receiver's class or any
/// ancestor: a `__cm_<C>__<name>` mono body or the parser-hoisted
/// generator spelling `__cm_gen_<C>__<name>` exists in the fn table.
/// The class comes off the receiver's checked `ClassRef` — a receiver
/// with no nominal identity never fires (the layout panic stands).
fn receiver_class_owns_method(ctx: &LowerCtx<'_>, obj: ExprId, name: &str) -> bool {
    let Some(mut cname) = crate::ssa_lower_member_obj_field::class_name_of_expr(ctx, obj) else {
        return false;
    };
    loop {
        if ctx.fn_table.contains_key(&format!("__cm_{cname}__{name}"))
            || ctx
                .fn_table
                .contains_key(&format!("__cm_gen_{cname}__{name}"))
        {
            return true;
        }
        match ctx.ast.class_parents.get(&cname) {
            Some(Some(p)) => cname = p.clone(),
            _ => return false,
        }
    }
}

fn try_lower_str_method_value(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    obj_val: &Operand,
    obj_ty: &Type,
    name: &str,
) -> Option<Operand> {
    let fam = mv_family_of_ssa_ty(obj_ty)?;
    let Some(crate::check::Type::Function(ps, ret)) = ctx.expr_types.get(&eid) else {
        return None;
    };
    let mid = torajs_rc::any_method_id(name);
    if mid == torajs_rc::ANY_METHOD_UNKNOWN || torajs_rc::any_method_meta(mid).is_none() {
        return None;
    }
    // A nullable receiver (un-narrowed exec/match capture miss) must
    // throw before handing out a method value — same guard as the
    // `.length` arm (no-op for non-nullable receivers). Number /
    // Boolean receivers have no nullable form to guard.
    if matches!(obj_ty, Type::Str | Type::Substr) {
        crate::ssa_lower_nullable_guard::emit_nullable_str_guard(ctx, obj, obj_val);
    }
    let params: Vec<Type> = ps
        .iter()
        .map(crate::ssa_lower_member_builtin_namespace::check_ty_to_ssa)
        .collect();
    let ret = crate::ssa_lower_member_builtin_namespace::check_ty_to_ssa(ret.as_ref());
    let sig = crate::ssa_lower::intern_fn_sig(ctx.fn_sigs, params, ret);
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.builtin_method_cell_tagged,
            vec![Operand::ConstI64(fam), Operand::ConstI64(mid)],
        ),
        Type::Closure(sig),
        None,
    );
    Some(Operand::Value(v))
}
