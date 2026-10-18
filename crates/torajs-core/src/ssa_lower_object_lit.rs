//! `Expr::ObjectLit { fields }` lowering.
//!
//! Lowers each field; spread members (sentinel name `__spread__`)
//! are unfolded by reading each source-struct field offset and
//! copying it into the destination. Inline members win on key
//! collision (later occurrences replace earlier slots). Spread
//! sources are lowered once; their values are read field-by-field.
//!
//! Five phases:
//!
//! 1. **Field lowering** — walk `fields` entries; `__spread__`
//!    sentinel unfolds a typed-struct source's layout; regular
//!    members lower the value with refcounted-borrow rc_inc
//!    discipline (same shape as the array-literal fix; without it,
//!    two struct lits sharing a refcounted Obj field
//!    `{x:a}; {x:a}` would double-walk-drop the shared element).
//! 2. **W4 field-width widen** — the literal's field widths come
//!    from its alias class (the Anon origin key unions with the
//!    receiving slot during analysis): `{x:1}` with a later
//!    `o.x = 0.5` carries an F64 `x` slot up front. `num_f64_slots`
//!    side-channel via `SlotKey::Anon(eid.0)` lookup; per-field
//!    `coerce_to_f64` when widening kicks in.
//! 3. **Layout resolution** — delegates to
//!    `ssa_lower_objlit_layout::resolve_objlit_layout` for exact
//!    match / numeric-width coercion / anonymous auto-register.
//! 4. **Allocation dispatch** — 11-A2-a: if `let_stack_alloc_hint`
//!    is set AND no field is refcounted, swap heap `obj_alloc` for
//!    stack `AllocaBytes`. Refcounted fields force back to heap
//!    because end-of-scope drop emission skips stack locals; a
//!    stack-alloc obj with refcounted children would leak the
//!    children's rc.
//! 5. **Header + field writes** — Phase 2B universal heap header
//!    init (refcount=1, type_tag=OBJ, flags=0); Error-derived
//!    factory class flag store (FLAG_ERROR in u16 @+6 so the
//!    uncaught-throw reporter renders `name: message` from
//!    Error layout prefix; MUST precede the class_tag store
//!    because the +4 64-bit Store spills its high half into +8,
//!    overwritten by the class_tag store at +8); Phase H.1.b
//!    class_tag at OBJ_CLASS_TAG_OFF (factory tag from
//!    `__new_<C>` name strip + W-J Phase A1 anon stamp pool
//!    fallback for inline `{x:1,y:2}` literals + generic mono
//!    shapes); T-24 vtable pointer at OBJ_VTABLE_OFF (when chain
//!    methods exist AND we're inside a `__new_<C>` factory of a
//!    known class). Then per-field Store at
//!    `OBJ_HEADER_SIZE + i*8`.

use crate::ast::ExprId;
use crate::ast::PropKey;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};

pub(crate) fn lower(
    ctx: &mut LowerCtx<'_>,
    fields: Vec<(PropKey, ExprId)>,
    eid: ExprId,
) -> Operand {
    // Rotation 267 — a literal the checker typed Any (an any-spread
    // member has no static field list) has no struct layout to
    // resolve; the whole literal lives on the dynobj lane, where the
    // spread runs the runtime CopyDataProperties walk. Claim-and-drop
    // both let-decl hints so they don't leak into a nested literal.
    if matches!(ctx.expr_types.get(&eid), Some(crate::check::Type::Any)) {
        let _ = ctx.let_stack_alloc_hint.take();
        let _ = ctx.let_declared_obj_layout.take();
        let dynobj = ctx.lower_dynobj_init(eid);
        return ctx.box_to_any(dynobj);
    }
    // Chunk 760 — claim the let-decl stack hint BEFORE the field
    // lowering: a nested object literal in a field value otherwise
    // consumed it inside `lower_field_entries` — the INNER block got
    // stack-alloc'd (its pointer escaped into the outer heap
    // object's slot, dangling on frame reuse) AND the let name
    // landed in `stack_alloced_locals`, so the OUTER heap alloc was
    // never scope-dropped (churn c3: the whole per-iteration field
    // graph leaked, ~120B/iter).
    let stack_hint = ctx.let_stack_alloc_hint.take();
    // Chunk 780 — claim the declared-layout hint HERE for the same
    // reason as the stack hint above: a nested object literal in a
    // field value would otherwise consume it inside
    // `lower_field_entries` and pin the OUTER declared layout onto
    // the inner literal.
    let declared = (ctx.let_declared_obj_layout.take()).and_then(declared::DeclaredLit::into_obj);
    let declared_hint = declared.as_ref().map(|d| d.sid);
    // RFC 20260710-optional-undefined-repr C1 — fields initialized
    // with an undefined LITERAL (frontend-distinguished from null)
    // must land the per-type undefined sentinel in the slot, not the
    // shared NULL (which means JS null): remember which field names
    // carried one before lowering collapses both to ConstPtrNull.
    let mut undef_fields: std::collections::HashSet<PropKey> = fields
        .iter()
        .filter(|(_, veid)| {
            matches!(
                ctx.expr_types.get(veid),
                Some(crate::check::Type::Undefined)
            )
        })
        .map(|(n, _)| n.clone())
        .collect();
    let (mut field_tys, mut field_vals, parked, spreads, spread_absent) =
        lower_field_entries(ctx, &fields, declared.as_ref());
    // Every field is lowered; nothing below can throw before the
    // stores transfer the owned values into the object.
    for t in parked {
        ctx.pop_throw_temp(t);
    }
    let absent = absent::absent_fields(ctx, eid, &fields);
    absent::insert_absent(&absent, &mut field_tys, &mut field_vals);
    undef_fields.extend(absent.iter().map(|(_, n)| n.clone()));
    apply_w4_widen(ctx, &mut field_tys, &mut field_vals, eid);
    // An `undefined` literal has no type of its own — its field's
    // DECLARED slot type is the truth, and the sentinel it lands is
    // re-materialized for that slot after resolution (below). Lowered
    // on its own it wears a placeholder (`Closure(SigId(0))` for a
    // closure field, `Ptr` for the rest), and a placeholder that
    // happens to differ from the declared slot (a closure signature id
    // moves with every signature the program minted before it)
    // failed `resolve_objlit_layout`'s hint check: the class literal
    // took the un-hinted scan or minted a fresh layout with `Ptr`
    // slots, and its `any` fields' `undefined` lowered to the generic
    // undefined CELL — an async generator's `__q` read back as a
    // non-undefined object in exactly the programs whose signature
    // count had shifted. Retype those fields to the declared slot
    // before the match.
    if let Some(h) = declared_hint
        && let Some(decl) = ctx.struct_layouts.get(h.0 as usize).cloned()
    {
        for (name, ty) in field_tys.iter_mut() {
            if undef_fields.contains(name)
                && let Some((_, slot_ty)) = decl.iter().find(|(n, _)| n == name)
            {
                *ty = *slot_ty;
            }
        }
    }
    let order = order::to_declared_order(ctx, eid, declared_hint, &mut field_tys, &mut field_vals);
    let undef_lits: Vec<bool> = field_tys
        .iter()
        .map(|(n, _)| undef_fields.contains(n))
        .collect();
    let sid = crate::ssa_lower_objlit_layout::resolve_objlit_layout(
        ctx.struct_layouts,
        ctx.f,
        ctx.cur_block,
        &mut field_tys,
        &mut field_vals,
        declared_hint,
        &undef_lits,
    );
    let slot = |n: &PropKey| {
        field_tys
            .iter()
            .position(|(k, _)| k == n)
            .unwrap_or(usize::MAX)
    };
    let fixed: Vec<usize> = absent.iter().map(|(_, n)| slot(n)).collect();
    let cond: Vec<(usize, Operand)> = spread_absent.iter().map(|(n, v)| (slot(n), *v)).collect();
    let in_set = absent::fits_set(fixed.iter().chain(cond.iter().map(|(i, _)| i)).copied());
    // a deletion record hangs a dict off the cell, which a frame
    // slot's end of scope would never release
    let stack_hint = stack_hint.filter(|_| in_set || (absent.is_empty() && cond.is_empty()));
    let any_refcounted = field_tys.iter().any(|(_, ty)| ty.is_refcounted());
    let obj_ptr = alloc_obj(ctx, sid, field_tys.len(), any_refcounted, stack_hint);
    // Which class is this cell? Normally the factory being lowered
    // builds its own instances, but a nested field default is a cell
    // of the FIELD's class sitting inside someone else's factory
    // (`Ast::field_default_objlits`) — the identity writes below
    // would otherwise stamp the factory's tag, vtable and Error flag
    // onto it.
    let owner = ctx.ast.field_default_objlits.get(&eid).cloned();
    let owner = owner.as_deref();
    let hdr = init_header(ctx, obj_ptr, sid, owner);
    write_class_tag(ctx, obj_ptr, sid, owner, order.as_deref());
    if owner.is_none() && !ctx.f.name.starts_with("__new_") {
        order::retag_for_spread_sources(ctx, obj_ptr, sid, &fields, &spreads);
    }
    write_vtable_ptr(ctx, obj_ptr, sid, owner);
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    let unspelled = slot_fit::fit_slot_values(
        ctx,
        &layout,
        &fields,
        &undef_fields,
        &absent,
        &mut field_vals,
    );
    for (i, val) in field_vals.iter().enumerate() {
        // L3b #6 crash fix — a typed Array stored into a struct field
        // reaches runtime walkers (inspect's Tag::Obj field printer,
        // the cycle collector's child walk) that read the header's
        // elem-kind to pick a slot interpretation. Record it here,
        // same as the `any`-boxing boundary (RFC 20260704 S1) —
        // without the mark a raw-i64 array walks as NaN-box cells
        // and SIGSEGVs on the first small-int deref. No-op for
        // non-Arr fields; chunk 621 derives the chain from the
        // value's own type inside the helper (an `any[]` field
        // taking a typed array marked chain 0 off the field type).
        ctx.emit_arr_mark_kind(val);
        let offset = OBJ_HEADER_SIZE + i as u64 * 8;
        let cur_block = ctx.cur_block;
        ctx.f.append_void(
            cur_block,
            InstKind::Store(*val, Operand::Value(obj_ptr), offset),
        );
    }
    if in_set {
        absent::raise_set(ctx, obj_ptr, hdr, &fixed, &cond);
    } else {
        let fixed: Vec<PropKey> = absent.into_iter().map(|(_, n)| n).collect();
        absent::record_deleted(ctx, obj_ptr, &fixed, &spread_absent);
    }
    slot_fit::store_unspelled(ctx, obj_ptr, unspelled);
    Operand::Value(obj_ptr)
}

/// field types, values, the indices that are undefined, the spread
/// sources, and the spread fields each source may lack
type FieldEntries = (
    Vec<(PropKey, Type)>,
    Vec<Operand>,
    Vec<usize>,
    Vec<order::SpreadSrc>,
    Vec<(PropKey, Operand)>,
);

fn lower_field_entries(
    ctx: &mut LowerCtx<'_>,
    entries: &[(PropKey, ExprId)],
    declared: Option<&declared::DeclaredObj>,
) -> FieldEntries {
    // Chunk 785 — the outer declared layout also pins NESTED object
    // literals: a field whose declared type is a struct passes its
    // StructId down through the same take-once hint channel, so
    // `const h: Holder = { sub: { v: 1 } }` lowers the inner literal
    // against Inner's slot reprs instead of first-matching a
    // same-shaped layout registered under an unrelated TypeDecl
    // (recursion covers arbitrary depth — each objlit re-enters
    // `lower` and forwards its own declared layout).
    let declared_fields: Option<Vec<(PropKey, Type)>> =
        declared.map(|d| ctx.struct_layouts[d.sid.0 as usize].clone());
    let owner_ann = declared.and_then(|d| d.ann.as_deref());
    let mut field_tys: Vec<(PropKey, Type)> = Vec::new();
    let mut field_vals: Vec<Operand> = Vec::new();
    // Rotation 549 — an owned field value (call product, nested
    // literal) is alive across every later field's lower; park it so
    // a throw there drops it (`{ a: mk(i), b: boom() }` on the struct
    // lane stranded the value per caught throw, 40MB over 600k).
    let mut parked: Vec<usize> = Vec::new();
    let mut spreads: Vec<order::SpreadSrc> = Vec::new();
    let mut spread_absent: Vec<(PropKey, Operand)> = Vec::new();
    for (entry, (n, eid)) in entries.iter().enumerate() {
        // RFC 20260725-objlit-computed-key 刀 1 — a computed key has
        // no static field name, so the struct lane cannot place it;
        // only the dynobj lane (degraded / any-annotated /
        // any-argv-promoted declarations) evaluates it. Loud reject —
        // silently minting the sentinel name would orphan the key's
        // side effects and the property.
        if ctx.ast.objlit_computed_keys.contains_key(eid) {
            panic!(
                "ssa-lower: computed property key outside the dynobj lane \
                 is not yet supported (field `{}`)",
                n.lossy()
            );
        }
        if let Some(omit) = crate::check_type_of_object_lit::spread_omit_set(n) {
            let omit: Vec<PropKey> = omit.iter().map(|s| PropKey::from(*s)).collect();
            let (op, sid) = crate::ssa_lower_object_lit_spread::unfold_spread(
                ctx,
                (*eid, &entries[..entry]),
                &omit,
                &mut field_tys,
                &mut field_vals,
                &mut spread_absent,
            );
            spreads.push(order::SpreadSrc {
                entry,
                op,
                sid,
                omit,
            });
            continue;
        }
        spread_absent.retain(|(k, _)| k != n);
        let declared_field_ty = declared_fields
            .as_ref()
            .and_then(|fs| fs.iter().find(|(fname, _)| fname == n))
            .map(|(_, fty)| *fty);
        lower_regular_field(
            ctx,
            n,
            *eid,
            declared_field_ty,
            owner_ann,
            &mut field_tys,
            &mut field_vals,
            &mut parked,
        );
    }
    (field_tys, field_vals, parked, spreads, spread_absent)
}

// The cell-construction tail (alloc dispatch / header init / class
// tag / vtable pointer) lives in the child module at the 500-line
// file cap; `lower` above is its only caller.
pub(crate) mod absent;
mod alloc;
use alloc::{alloc_obj, init_header, write_class_tag, write_vtable_ptr};
mod any_slot;
pub(crate) mod declared;
pub(crate) mod optional_number;
mod order;
mod regular_field;
mod slot_fit;
use regular_field::lower_regular_field;
mod widen;
use widen::apply_w4_widen;
