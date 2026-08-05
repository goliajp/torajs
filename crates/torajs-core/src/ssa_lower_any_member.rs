//! `Type::Any` member-read inline class-tag dispatch — carved out
//! of `ssa_lower.rs::lower_expr_inner` to keep that god-file from
//! growing further (file-size HARD RULE per
//! `.claude/rules/torajs-file-size-debt.md`).
//!
//! Pre-fix the Any-member read site decoded the Any-box's value
//! field as a ptr and unconditionally dispatched through
//! `__torajs_dynobj_get_tag / value`. For class instances
//! (type_tag = OBJ = 1) the dynobj entry-table walk found nothing
//! and returned ANY_UNDEF — so `(e: any).message` on a thrown
//! TypeError, `(y: any).a` on a `new Foo()`, and every other class-
//! instance-through-Any member read silently produced undefined.
//!
//! Fix: monomorphic inline dispatch on `class_tag` for the candidate
//! classes the module declares + whose layout resolves the field
//! name. AOT means `class_name_to_tag` + `struct_layouts` are known
//! here; the member key is a String literal so candidate enumeration
//! is a compile-time loop. Each candidate emits one `class_tag` cmp
//! + direct field load + `box_to_any` (which bumps the refcount for
//! Heap-typed fields). Non-OBJ header tags + classes whose layout
//! omits the field fall through to the original dynobj dispatch —
//! preserving plain `{}`-shape Any semantics.
//!
//! Hot path = 1 cmp + 1 load + 1 box for the common monomorphic
//! case; ceiling matches what a JIT hidden-class inline cache
//! produces in steady state, without runtime cache slots (AOT-time
//! monomorphization).

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{LowerCtx, OBJ_CLASS_TAG_OFF, OBJ_HEADER_SIZE};

/// Lower a `Type::Any` Member read for `name`. `obj_val` is the
/// already-lowered Any-box operand; `eid` is the consumer-visible
/// expression id (Member / Index-with-literal-key / OptChain /
/// OptIndex) this read lowers.
///
/// Chunk 717 — the result is OWNED on every arm: the class-candidate
/// arm's `box_to_any` bumps heap fields, the special-prop helpers
/// (`any_length_get` / `any_name_get` / `any_size_get` /
/// `any_regexp_prop`) answer fresh/retained values, and the probe
/// fallback's data arm takes `any_payload_rc_inc` inside
/// `emit_dynobj_get_result`. `eid` is recorded in
/// `owned_member_reads` so every consumer (let-decl slot, discard,
/// call arg, BinOp temp, console arg, assign) takes the release over
/// instead of treating the read as a receiver borrow — pre-717 the
/// owned arms' +1 was stranded (32B leaked per `re.source` /
/// `t.name` / accessor / class-field read through any).
pub(crate) fn lower_any_member_read(
    ctx: &mut LowerCtx,
    eid: ExprId,
    obj_val: Operand,
    name: &str,
) -> Operand {
    ctx.owned_member_reads.insert(eid);
    // RFC 20260714-objlit-accessor blade 5 — an accessor SLOT name is
    // not a property. The IC below enumerates candidates by LAYOUT
    // FIELD name, and `__getter_v` really is one, so without this the
    // mangled spelling read back as the getter closure itself
    // (`(o as any).__getter_v` → `[Function: __getter_v]`; bun:
    // undefined). The runtime probe rejects it too — this closes the
    // compile-time half.
    if crate::check_type_of_object_lit::accessor_slot(name).is_some() {
        let key_str = ctx.intern_string_literal(name);
        return emit_member_fallback(ctx, &obj_val, key_str, name);
    }
    let mut candidates = collect_class_field_candidates(ctx, name);

    let key_str = ctx.intern_string_literal(name);

    // No candidates → original dynobj-only path (plain ObjectLit,
    // empty class set, or class without this field), with the
    // RFC-20260704 S4 `.length` runtime dispatch layered in.
    if candidates.is_empty() {
        return emit_member_fallback(ctx, &obj_val, key_str, name);
    }

    // chunk 712 — borrow-shaped cell read for the class dispatch:
    // the materializing unbox_value leaked an owned Str per read on
    // a ShortStr receiver (this site never dropped it) and handed
    // int immediates back as dereferenceable "pointer" bits for the
    // class-tag load. Immediates decode to NULL and take the
    // dynobj_blk fallback like any non-Obj cell.
    let dynobj = ctx.any_cell_ptr_as_ptr(obj_val.clone());

    // Sort by class_tag for deterministic dispatch order.
    candidates.sort_by_key(|(t, _, _, _)| *t);

    let res_slot = ctx.alloca_in_entry(Type::Any, Some("__any_member_res"));
    let dynobj_blk = ctx.f.add_block();
    let class_blk = ctx.f.add_block();
    let after = ctx.f.add_block();

    // NULL guard: a non-Heap Any-box decodes to value=0 / NULL ptr;
    // skip class dispatch entirely so the dynobj_get path can return
    // ANY_UNDEF as before.
    let null_chk = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, Operand::Value(dynobj), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(null_chk),
            then_blk: dynobj_blk,
            else_blk: class_blk,
        },
    );

    let cls_dispatch = emit_obj_tag_gate(ctx, class_blk, dynobj, dynobj_blk);

    // cls_dispatch: load class_tag + emit one cmp arm per candidate.
    let ct = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(dynobj), OBJ_CLASS_TAG_OFF),
        Type::I64,
        None,
    );
    let mut current = cls_dispatch;
    for (ctag, offset, field_ty, is_err_slot) in &candidates {
        ctx.cur_block = current;
        let eq = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(
                IPred::Eq,
                Operand::Value(ct),
                Operand::ConstI64(*ctag as i64),
            ),
            Type::Bool,
            None,
        );
        let match_blk = ctx.f.add_block();
        let next_blk = ctx.f.add_block();
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(eq),
                then_blk: match_blk,
                else_blk: next_blk,
            },
        );
        ctx.cur_block = match_blk;
        // An Error-derived candidate's `message` / `name` is runtime
        // own-state: the helper reads the own slot or walks the
        // prototype chain (BORROWED Str, Load-equivalent). Every
        // other field keeps the direct load.
        let field_v = if *is_err_slot {
            let target = if name == "message" {
                ctx.intrinsics.error_message_get
            } else {
                ctx.intrinsics.error_name_get
            };
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(target, vec![Operand::Value(dynobj)]),
                Type::Str,
                None,
            )
        } else {
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(*field_ty, Operand::Value(dynobj), *offset),
                *field_ty,
                None,
            )
        };
        // Chunk 717's owned contract vs chunk 753's borrow box — the
        // direct Load is a BORROW off the struct slot and every
        // box_to_any arm is a pure encoding (rotation 184 made the
        // Str-slot helper rc-neutral too), so the owned-member-read
        // release would steal the slot's stake without a
        // compensating inc. Str is no longer excluded (the old "Str
        // boxes through the Str-slot helper (rc_inc inside)" claim
        // is stale — rotation 185 stake audit); the err_msg helper
        // answers a BORROWED Str, so it incs the same way. The inc
        // no-ops on NULL / the sentinels through the kernel gates.
        // Any slots keep their existing story.
        if field_ty.is_refcounted() && !matches!(field_ty, Type::Any) {
            ctx.emit_rc_inc(Operand::Value(field_v));
        }
        // RFC 20260710 C2b readback — a pointer-family slot may hold
        // the generic undefined cell (a `{r: undefined}` Ptr slot, an
        // optional heap field): normalize to ANY_UNDEF, mirroring the
        // runtime probe (`struct_field_pair_bytes`).
        let boxed = if field_ty.spells_undef_with_generic_cell() {
            ctx.box_heap_slot_or_undef(Operand::Value(field_v))
        } else {
            ctx.box_to_any(Operand::Value(field_v))
        };
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(boxed, Operand::Value(res_slot), 0),
        );
        ctx.f.set_term(ctx.cur_block, Terminator::Br(after));
        current = next_blk;
    }
    // OBJ-tagged but class_tag missed every candidate (e.g. a class
    // without this field) — fall through to dynobj_get which returns
    // ANY_UNDEF for a non-dynobj ptr.
    ctx.f.set_term(current, Terminator::Br(dynobj_blk));

    // dynobj_blk: original tag/value-pair path (`.length` routes to
    // the S4 runtime dispatch — see `emit_member_fallback`).
    ctx.cur_block = dynobj_blk;
    let box_v = emit_member_fallback(ctx, &obj_val, key_str, name);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(box_v, Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after));

    // after: collect into one SSA value.
    ctx.cur_block = after;
    let r = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Any, Operand::Value(res_slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(r)
}

/// class_blk body — load the header's type_tag (low 16 bits of the
/// i32 at +4; high 16 bits = flags), mask + cmp against OBJ_TAG (1,
/// mirrored from `torajs_rc::Tag::Obj`), and branch to a fresh
/// `cls_dispatch` block for Obj cells / `dynobj_blk` for everything
/// else. Leaves `ctx.cur_block` at the returned `cls_dispatch`.
fn emit_obj_tag_gate(
    ctx: &mut LowerCtx,
    class_blk: crate::ssa::BlockId,
    dynobj: crate::ssa::ValueId,
    dynobj_blk: crate::ssa::BlockId,
) -> crate::ssa::BlockId {
    ctx.cur_block = class_blk;
    let tt_word = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I32, Operand::Value(dynobj), 4),
        Type::I32,
        None,
    );
    let tt_low = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            SsaBinOp::And,
            Operand::Value(tt_word),
            Operand::ConstI32(0xFFFF),
        ),
        Type::I32,
        None,
    );
    let is_obj = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, Operand::Value(tt_low), Operand::ConstI32(1)),
        Type::Bool,
        None,
    );
    let cls_dispatch = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_obj),
            then_blk: cls_dispatch,
            else_blk: dynobj_blk,
        },
    );
    ctx.cur_block = cls_dispatch;
    cls_dispatch
}

/// Compile-time enumerate class candidates whose layout declares
/// `name` as a field (used by `lower_any_member_read`'s monomorphic
/// IC dispatch). Walks both `class_name_to_tag` (named classes) and
/// `anon_stamp_pool` (S126-5 anonymous ObjectLit Pass 1.5 / Pass 2
/// fresh-sid stamps — they land in `class_tag@+8` per W-J A1 follow-up
/// `cc6416a6` but don't appear in `class_name_to_tag`, so without this
/// second walk `const da: any = {x:1,y:2}; da.x` falls through to the
/// dynobj path returning ANY_UNDEF for struct-cell receivers). AOT —
/// both maps are stable by this point.
fn collect_class_field_candidates(ctx: &LowerCtx, name: &str) -> Vec<(u32, u64, Type, bool)> {
    let mut candidates: Vec<(u32, u64, Type, bool)> = Vec::new();
    for (cname, ctag) in ctx.class_name_to_tag.iter() {
        let Some(Type::Obj(sid)) = ctx.aliases.get(cname) else {
            continue;
        };
        let layout = &ctx.struct_layouts[sid.0 as usize];
        if let Some((idx, (_, fty))) = layout.iter().enumerate().find(|(_, (n, _))| n == name) {
            let offset = OBJ_HEADER_SIZE + (idx as u64) * 8;
            // An error candidate's `message` (§20.5.6.1.1) and `name`
            // (§20.5.3.2) are runtime own-state, so they route through
            // the own-or-proto read helper instead of the direct load.
            // `name` matters most: the class name is the PROTOTYPE's,
            // so the slot holds the own-absence sentinel and a direct
            // load reads it as `undefined` — which is what made
            // `(err as any).name` disagree with `err.name`.
            let is_err_slot =
                matches!(name, "message" | "name") && ctx.class_is_error_derived(cname);
            candidates.push((*ctag, offset, *fty, is_err_slot));
        }
    }
    let pool = ctx.anon_stamp_pool.borrow();
    for (sid, atag) in pool.sid_to_tag_iter() {
        let layout_idx = sid.0 as usize;
        if layout_idx >= ctx.struct_layouts.len() {
            continue;
        }
        let layout = &ctx.struct_layouts[layout_idx];
        if let Some((idx, (_, fty))) = layout.iter().enumerate().find(|(_, (n, _))| n == name) {
            let offset = OBJ_HEADER_SIZE + (idx as u64) * 8;
            candidates.push((atag, offset, *fty, false));
        }
    }
    candidates
}

/// Member-read fallback once class-candidate dispatch is exhausted
/// (or absent). Any-dynamic-access RFC (20260704) S4 — `.length`
/// routes to `__torajs_any_length_get`, whose tag dispatch answers
/// strings (UTF-16 units), arrays (element count), fn arity
/// (method cells + registry-hit closures, chunks 715/716) and
/// plain-object `{ length: .. }` probes, and raises a catchable
/// TypeError for a null/undefined receiver (matching bun; the
/// pre-RFC dynobj path answered silent `undefined`). `.name`
/// routes to `__torajs_any_name_get` (chunk 716, same shape).
/// Every other member name keeps the original dynobj-only path.
fn emit_member_fallback(
    ctx: &mut LowerCtx,
    obj_val: &Operand,
    key_str: crate::ssa::ValueId,
    name: &str,
) -> Operand {
    if name.starts_with("__priv_") {
        // §7.3.31 PrivateGet / PrivateBrandCheck — reading a private
        // element off a receiver whose class did not declare it
        // throws TypeError, never answers undefined. Statically
        // selected here (private names only ever reach the any lane
        // pre-mangled), so the ordinary tag channel pays nothing;
        // the value channel stays the base intrinsic (it answers 0
        // on the thrown path without a second throw — the
        // null-receiver convention).
        let tag = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_member_get_priv_tag,
                vec![obj_val.clone(), Operand::Value(key_str)],
            ),
            Type::I64,
            None,
        );
        let value = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_member_get_value,
                vec![obj_val.clone(), Operand::Value(key_str)],
            ),
            Type::I64,
            None,
        );
        ctx.emit_throw_check(None);
        return crate::ssa_lower_accessor::emit_any_get_result(ctx, obj_val, key_str, tag, value);
    }
    if name == "length" {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_length_get, vec![obj_val.clone()]),
            Type::Any,
            None,
        );
        ctx.emit_throw_check(None);
        return Operand::Value(v);
    }
    if name == "__proto__" {
        // Annex B §B.2.2.1 — `o.__proto__` IS the [[Prototype]]
        // getter, so it answers whatever `Object.getPrototypeOf(o)`
        // does (one intrinsic, no second copy of the walk). The
        // dynobj-only path below would have probed for an own entry
        // named "__proto__" and answered undefined for every object
        // that never got one. Owned return, the `.length` shape.
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.proto_member_get, vec![obj_val.clone()]),
            Type::Any,
            None,
        );
        ctx.emit_throw_check(None);
        return Operand::Value(v);
    }
    if name == "name" {
        // chunk 716 — fn reflection metadata (reified method cell
        // interned name / fn-addr registry / dynobj `{ name: .. }`
        // probe) rides its own owned-return dispatch, the exact
        // `.length` shape.
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_name_get, vec![obj_val.clone()]),
            Type::Any,
            None,
        );
        ctx.emit_throw_check(None);
        return Operand::Value(v);
    }
    if name == "size" {
        // RFC 20260704 C4-2 — Map/Set `.size` (+ dynobj `{ size: .. }`
        // probe) rides its own tag dispatch; `.length` stays
        // length-only (a Map has no length, matching bun).
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_size_get, vec![obj_val.clone()]),
            Type::Any,
            None,
        );
        ctx.emit_throw_check(None);
        return Operand::Value(v);
    }
    if let Some(prop) = torajs_rc::any_regexp_prop_id(name) {
        // RFC 20260704 C4-3c-2 — the RegExp accessor surface
        // (source / flags / lastIndex / six flag booleans). The
        // interned key rides along so a DynObj receiver keeps
        // ordinary own-property semantics.
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_regexp_prop,
                vec![
                    obj_val.clone(),
                    Operand::ConstI64(prop),
                    Operand::Value(key_str),
                ],
            ),
            Type::Any,
            None,
        );
        ctx.emit_throw_check(None);
        return Operand::Value(v);
    }
    emit_any_member_probe(ctx, obj_val, key_str)
}

/// Emit the arbitrary-name probe pair — RFC 20260704 C4+ tag-gated
/// `__torajs_any_member_get_tag/_value` (was the raw
/// `dynobj_get_tag/value` layout read; an Arr expando probe missed
/// by accident, every non-DynObj tag was an out-of-layout read).
/// The pair keeps the dynobj probe's borrow shape and accessor
/// sentinel, so `emit_dynobj_get_result` consumes it unchanged; a
/// null/undefined receiver records a catchable TypeError.
pub(crate) fn emit_any_member_probe(
    ctx: &mut LowerCtx,
    obj_val: &Operand,
    key_str: crate::ssa::ValueId,
) -> Operand {
    let tag = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_member_get_tag,
            vec![obj_val.clone(), Operand::Value(key_str)],
        ),
        Type::I64,
        None,
    );
    let value = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_member_get_value,
            vec![obj_val.clone(), Operand::Value(key_str)],
        ),
        Type::I64,
        None,
    );
    ctx.emit_throw_check(None);
    crate::ssa_lower_accessor::emit_any_get_result(ctx, obj_val, key_str, tag, value)
}
