//! The body of a `__drop_<C>` release fn (registered by
//! `class_drop_setup`, filled in once the body passes have
//! settled every layout). It is the inline class-instance release
//! (`ssa_lower_emit_drop_value::obj`) as one fn:
//!
//! ```text
//! entry:   nullish(p) → ret
//! tag:     class_tag(p) ∈ tags → dec ; else __torajs_obj_drop_rc(p), ret
//! dec:     --rc == 0 → walk ; else __torajs_cycle_buffer(p), ret
//! walk:    weakref observers live → __torajs_weakref_target_dying(p)
//!          release each non-Copy field
//!          props bag → __torajs_obj_props_drop_slow(p)
//!          FLAG_BUFFERED → colour black, ret ; else obj_drop_sized
//! ```
//!
//! The tag test is new against the inline form. A layout is a prefix
//! of every wider shape assignable to it — a subclass in a base-typed
//! slot, a wider literal in a narrower anonymous slot — and the inline
//! walk released the declared fields only and leaked the rest. A tag
//! outside the sid's set hands the cell to the runtime walk over the
//! emitted `__torajs_class_layouts` row for its own tag. A sid's set
//! is the tags of the classes aliased to it, or, for an anonymous
//! shape, its stamp and 0 (a cell minted without a stamp has no row
//! to walk by; the declared layout is all there is).
//!
//! An `Obj` field with its own `__drop_` calls it (the self-referential
//! field included). Any other `Obj` field is released in place, the
//! same expansion under its own sid's tag set; an `Arr` field by its
//! element type (`class_drop_arr`), as the inline form does
//! — the runtime's tag walk only knows an array's element kind once
//! the array crossed into `any`. `Any` goes through `any_box_drop`,
//! `Substr` through `substr_drop`, every other non-Copy value through
//! the tag-dispatched `value_drop_heap`.

use std::collections::{HashMap, HashSet};

use crate::ast::PropKey;
use crate::ssa::{
    self, BinOp as SsaBinOp, BlockId, FuncId, IPred, InstKind, Module, Operand, Terminator, Type,
};
use crate::ssa_lower::{Intrinsics, OBJ_CLASS_TAG_OFF, OBJ_HEADER_SIZE, OBJ_PROPS_OFF};
use crate::ssa_lower_emit_drop_value::class_drop_setup::ClassDropFn;

/// What a release expansion reads besides the function it writes into.
pub(crate) struct Cx<'a> {
    pub layouts: &'a [Vec<(PropKey, Type)>],
    pub arr_layouts: &'a [Type],
    /// sids a declared class is aliased to (they buffer as cycle roots
    /// and notify the WeakRef registry; anonymous shapes do neither)
    pub class_sids: &'a HashSet<u32>,
    /// the tags a cell of each sid walks by its compile-time layout
    /// under; an anonymous sid the pool never stamped answers `[0]`
    pub tags: &'a HashMap<u32, Vec<u32>>,
    pub drop_fids: &'a HashMap<u32, FuncId>,
    pub intr: &'a Intrinsics,
    /// the fn's first block, where every loop counter slot is
    /// allocated; it branches to the release once the body is built
    pub prelude: BlockId,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn populate_class_drop_bodies(
    class_drops: &[ClassDropFn],
    class_name_to_tag: &HashMap<String, u32>,
    aliases: &HashMap<String, Type>,
    struct_layouts: &[Vec<(PropKey, Type)>],
    arr_layouts: &[Type],
    anon_stamp_pool: &crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    intrinsics: &Intrinsics,
    module: &mut Module,
) {
    let sid_of = |cn: &String| match aliases.get(cn) {
        Some(Type::Obj(s)) => Some(s.0),
        _ => None,
    };
    let mut class_sids: HashSet<u32> = class_name_to_tag.keys().filter_map(sid_of).collect();
    // sids a factory stamped a class tag onto besides the class's own
    for sid in 0..struct_layouts.len() as u32 {
        if anon_stamp_pool
            .borrow()
            .is_class_minted(crate::ssa::StructId(sid))
        {
            class_sids.insert(sid);
        }
    }
    let drop_fids: HashMap<u32, FuncId> = class_drops.iter().map(|d| (d.sid.0, d.fid)).collect();
    let mut tags: HashMap<u32, Vec<u32>> = HashMap::new();
    for (cn, tag) in class_name_to_tag {
        if let Some(sid) = sid_of(cn) {
            tags.entry(sid).or_default().push(*tag);
        }
    }
    for (sid, tag) in anon_stamp_pool.borrow().sid_to_tag_iter() {
        if !class_sids.contains(&sid.0) {
            tags.entry(sid.0).or_insert_with(|| vec![0]).push(tag);
        }
    }
    tags.values_mut().for_each(|t| t.sort_unstable());
    for d in class_drops {
        let mut f = ssa::Function::new(&d.name, Type::Void);
        let p = f.add_param(Type::Ptr, "p");
        let prelude = f.add_block();
        let entry = f.add_block();
        let cx = Cx {
            layouts: struct_layouts,
            arr_layouts,
            class_sids: &class_sids,
            tags: &tags,
            drop_fids: &drop_fids,
            intr: intrinsics,
            prelude,
        };
        let mut stack = Vec::new();
        let after = emit_release(&mut f, &cx, entry, Operand::Value(p), d.sid.0, &mut stack);
        f.set_term(after, Terminator::Ret(None));
        f.set_term(prelude, Terminator::Br(entry));
        module.funcs[d.fid.0 as usize] = f;
    }
}

/// Release one reference of `val` of type `ty` starting in `blk`;
/// answers the block the caller continues in.
pub(crate) fn emit_release_value(
    f: &mut ssa::Function,
    cx: &Cx<'_>,
    blk: BlockId,
    val: Operand,
    ty: Type,
    stack: &mut Vec<u32>,
) -> BlockId {
    let drop_fid = match ty {
        Type::Obj(s) if cx.drop_fids.contains_key(&s.0) => cx.drop_fids[&s.0],
        Type::Obj(s) if !stack.contains(&s.0) => return emit_release(f, cx, blk, val, s.0, stack),
        Type::Arr(id) => {
            return crate::ssa_lower_emit_drop_value::class_drop_arr::emit_release_arr(
                f, cx, blk, val, id, stack,
            );
        }
        Type::Any => cx.intr.any_box_drop,
        Type::Substr => cx.intr.substr_drop,
        _ => cx.intr.value_drop_heap,
    };
    f.append_void(blk, InstKind::Call(drop_fid, vec![val]));
    blk
}

/// Release one reference of `val`, an instance of `sid` or nullish,
/// starting in `blk`; answers the block every leg rejoins in. `stack`
/// holds the sids being expanded around this one.
fn emit_release(
    f: &mut ssa::Function,
    cx: &Cx<'_>,
    blk: BlockId,
    val: Operand,
    sid: u32,
    stack: &mut Vec<u32>,
) -> BlockId {
    let layout = &cx.layouts[sid as usize];
    let is_class = cx.class_sids.contains(&sid);
    let after = f.add_block();
    let tag_blk = f.add_block();
    let foreign_blk = f.add_block();
    let dec_blk = f.add_block();
    let nullish = emit_nullish(f, blk, val);
    branch(f, blk, nullish, after, tag_blk);
    let tags = cx.tags.get(&sid).map_or(&[0][..], Vec::as_slice);
    let known = emit_tag_in_set(f, tag_blk, val, tags);
    branch(f, tag_blk, known, dec_blk, foreign_blk);
    f.append_void(foreign_blk, InstKind::Call(cx.intr.obj_drop_rc, vec![val]));
    f.set_term(foreign_blk, Terminator::Br(after));
    let walk_blk = f.add_block();
    let is_zero = emit_rc_dec(f, dec_blk, val);
    if is_class {
        // references remain: a candidate cycle root; a fields-all-
        // scalar instance can only close a cycle through its expando
        // bag, so it buffers behind the bag's seam
        let buffer_blk = f.add_block();
        branch(f, dec_blk, is_zero, walk_blk, buffer_blk);
        let end = if layout.iter().all(|(_, fty)| fty.is_copy()) {
            emit_if_props(f, buffer_blk, val, cx.intr.obj_buffer_slow)
        } else {
            f.append_void(buffer_blk, InstKind::Call(cx.intr.cycle_buffer, vec![val]));
            buffer_blk
        };
        f.set_term(end, Terminator::Br(after));
    } else {
        branch(f, dec_blk, is_zero, walk_blk, after);
    }
    let mut cur = walk_blk;
    if is_class {
        cur = emit_weakref_notify(f, cur, val, cx.intr.weakref_target_dying);
    }
    stack.push(sid);
    for (i, (_, fty)) in layout.iter().enumerate() {
        if fty.is_copy() {
            continue;
        }
        let off = OBJ_HEADER_SIZE + i as u64 * 8;
        let field = Operand::Value(f.append_inst(cur, InstKind::Load(*fty, val, off), *fty, None));
        cur = emit_release_value(f, cx, cur, field, *fty, stack);
    }
    stack.pop();
    cur = emit_if_props(f, cur, val, cx.intr.obj_props_drop_slow);
    if is_class {
        cur = emit_defer_if_buffered(f, cur, val, after);
    }
    // A class sid's tag set names classes of this very layout, so a
    // cell past the tag test has this size. An anonymous sid's set
    // holds 0, a cell minted without a stamp, which says nothing of
    // its layout: the sid rides along and `cell_size::settle` decides
    // the free once every layout is known, as for the inline drop.
    let size = OBJ_HEADER_SIZE + layout.len() as u64 * 8;
    let mut args = vec![val, Operand::ConstI64(size as i64)];
    if !is_class {
        args.push(Operand::ConstI64(sid as i64));
    }
    f.append_void(cur, InstKind::Call(cx.intr.obj_drop_sized, args));
    f.set_term(cur, Terminator::Br(after));
    after
}

pub(crate) fn branch(
    f: &mut ssa::Function,
    blk: BlockId,
    cond: Operand,
    then_blk: BlockId,
    else_blk: BlockId,
) {
    f.set_term(
        blk,
        Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        },
    );
}

/// `p == NULL | p == @undefined_cell` — the two values a nullable
/// `Obj` slot holds that own nothing (`nullish_skip_cond`).
pub(crate) fn emit_nullish(f: &mut ssa::Function, blk: BlockId, p: Operand) -> Operand {
    let is_null = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Eq, p, Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    let undef = f.append_inst(
        blk,
        InstKind::GlobalRef(crate::ssa_lower_binop_null_undef::UNDEF_CELL_SYM.to_string()),
        Type::Ptr,
        None,
    );
    let is_undef = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Eq, p, Operand::Value(undef)),
        Type::Bool,
        None,
    );
    let nullish = f.append_inst(
        blk,
        InstKind::BinOp(
            SsaBinOp::Or,
            Operand::Value(is_null),
            Operand::Value(is_undef),
        ),
        Type::Bool,
        None,
    );
    Operand::Value(nullish)
}

/// The low 32 bits of the +8 slot against each tag of the sid
/// (`load_obj_class_tag`'s mask: the install cursor shares the word).
fn emit_tag_in_set(f: &mut ssa::Function, blk: BlockId, p: Operand, tags: &[u32]) -> Operand {
    let slot = f.append_inst(
        blk,
        InstKind::Load(Type::I64, p, OBJ_CLASS_TAG_OFF),
        Type::I64,
        None,
    );
    let tag = f.append_inst(
        blk,
        InstKind::BinOp(
            SsaBinOp::And,
            Operand::Value(slot),
            Operand::ConstI64(0xFFFF_FFFF),
        ),
        Type::I64,
        None,
    );
    let mut acc: Option<Operand> = None;
    for &t in tags {
        let hit = f.append_inst(
            blk,
            InstKind::ICmp(IPred::Eq, Operand::Value(tag), Operand::ConstI64(t as i64)),
            Type::Bool,
            None,
        );
        acc = Some(match acc {
            None => Operand::Value(hit),
            Some(prev) => Operand::Value(f.append_inst(
                blk,
                InstKind::BinOp(SsaBinOp::Or, prev, Operand::Value(hit)),
                Type::Bool,
                None,
            )),
        });
    }
    acc.expect("a sid's tag set is never empty")
}

/// `--rc == 0`, the header write `emit_rc_dec_inline` makes.
fn emit_rc_dec(f: &mut ssa::Function, blk: BlockId, p: Operand) -> Operand {
    let rc = f.append_inst(blk, InstKind::Load(Type::I32, p, 0), Type::I32, None);
    let rc_new = f.append_inst(
        blk,
        InstKind::BinOp(SsaBinOp::Sub, Operand::Value(rc), Operand::ConstI32(1)),
        Type::I32,
        None,
    );
    f.append_void(blk, InstKind::Store(Operand::Value(rc_new), p, 0));
    let is_zero = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Eq, Operand::Value(rc_new), Operand::ConstI32(0)),
        Type::Bool,
        None,
    );
    Operand::Value(is_zero)
}

/// Notify the WeakRef registry only while something observes
/// (the inline `torajs_rc::__torajs_weakref_active` read). Answers
/// the block the caller continues in.
fn emit_weakref_notify(f: &mut ssa::Function, blk: BlockId, p: Operand, dying: FuncId) -> BlockId {
    let active_ptr = f.append_inst(
        blk,
        InstKind::GlobalRef(ssa::WEAKREF_ACTIVE_SYM.to_string()),
        Type::Ptr,
        None,
    );
    let active = f.append_inst(
        blk,
        InstKind::Load(Type::I64, Operand::Value(active_ptr), 0),
        Type::I64,
        None,
    );
    let observed = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Ne, Operand::Value(active), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    emit_cond_call(f, blk, observed, dying, p)
}

/// `if (p.props != NULL) seam(p)` — the expando bag's two link seams
/// (release, cycle-root buffering) run only for a cell that has one.
fn emit_if_props(f: &mut ssa::Function, blk: BlockId, p: Operand, seam: FuncId) -> BlockId {
    let props = f.append_inst(
        blk,
        InstKind::Load(Type::Ptr, p, OBJ_PROPS_OFF),
        Type::Ptr,
        None,
    );
    let nonnull = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Ne, Operand::Value(props), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    emit_cond_call(f, blk, nonnull, seam, p)
}

/// `if (cond) fid(arg)` — answers the join block.
fn emit_cond_call(
    f: &mut ssa::Function,
    blk: BlockId,
    cond: ssa::ValueId,
    fid: FuncId,
    arg: Operand,
) -> BlockId {
    let call_blk = f.add_block();
    let join_blk = f.add_block();
    branch(f, blk, Operand::Value(cond), call_blk, join_blk);
    f.append_void(call_blk, InstKind::Call(fid, vec![arg]));
    f.set_term(call_blk, Terminator::Br(join_blk));
    join_blk
}

/// A cell the cycle collector holds as a candidate root stays
/// allocated, coloured black with rc 0, for the next collect's root
/// walk to free; that leg goes to `after`. Answers the free leg.
fn emit_defer_if_buffered(
    f: &mut ssa::Function,
    blk: BlockId,
    p: Operand,
    after: BlockId,
) -> BlockId {
    let word = f.append_inst(blk, InstKind::Load(Type::I32, p, 4), Type::I32, None);
    let bit = f.append_inst(
        blk,
        InstKind::BinOp(
            SsaBinOp::And,
            Operand::Value(word),
            Operand::ConstI32(((torajs_rc::FLAG_BUFFERED as u32) << 16) as i32),
        ),
        Type::I32,
        None,
    );
    let buffered = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Ne, Operand::Value(bit), Operand::ConstI32(0)),
        Type::Bool,
        None,
    );
    let defer_blk = f.add_block();
    let free_blk = f.add_block();
    branch(f, blk, Operand::Value(buffered), defer_blk, free_blk);
    let black = f.append_inst(
        defer_blk,
        InstKind::BinOp(
            SsaBinOp::And,
            Operand::Value(word),
            Operand::ConstI32(!((torajs_rc::COLOR_MASK as u32) << 16) as i32),
        ),
        Type::I32,
        None,
    );
    f.append_void(defer_blk, InstKind::Store(Operand::Value(black), p, 4));
    f.set_term(defer_blk, Terminator::Br(after));
    free_blk
}
