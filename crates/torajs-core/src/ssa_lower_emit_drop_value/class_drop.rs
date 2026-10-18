//! The body of a `__drop_<C>` release fn (registered by
//! `class_drop_setup`, filled in once the body passes have
//! settled every layout). It is the inline class-instance release
//! (`ssa_lower_emit_drop_value::obj`) as one fn:
//!
//! ```text
//! entry:   p == NULL → ret ; p == @undefined_cell → ret
//! tag:     class_tag(p) ∈ tags → dec ; else __torajs_obj_drop_rc(p), ret
//! dec:     --rc == 0 → walk ; else __torajs_cycle_buffer(p), ret
//! walk:    weakref observers live → __torajs_weakref_target_dying(p)
//!          release each non-Copy field
//!          props bag → __torajs_obj_props_drop_slow(p)
//!          FLAG_BUFFERED → colour black, ret ; else obj_drop_sized
//! ```
//!
//! Blocks lay out in creation order: a dying leaf's path is created
//! first and falls through; legs to runtime seams come last (`defer`).
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

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::ast::PropKey;
use crate::ssa::{self, BlockId, FuncId, IPred, InstKind, Module, Operand, Terminator, Type};
use crate::ssa_lower::{Intrinsics, OBJ_HEADER_SIZE};
pub(crate) use crate::ssa_lower_emit_drop_value::class_drop_cells::{branch, branch_nullish};
use crate::ssa_lower_emit_drop_value::class_drop_cells::{
    emit_cold_call, emit_colour_black, emit_cond_call, emit_has_props, emit_is_buffered,
    emit_is_null, emit_is_undef, emit_rc_dec, emit_tag_in_set, emit_weakref_notify,
};
use crate::ssa_lower_emit_drop_value::class_drop_setup::ClassDropFn;
use crate::ssa_lower_emit_drop_value::class_drop_worklist::{self as worklist, Worklist};

/// What a release expansion reads besides the function it writes into.
pub(crate) struct Cx<'a> {
    pub layouts: &'a [Vec<(PropKey, Type)>],
    pub arr_layouts: &'a crate::ssa::ArrLayouts,
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
    pub cold: RefCell<Vec<ColdLeg>>,
    /// set when the sid's instances hold the sid (`class_drop_worklist`)
    pub worklist: Option<Worklist>,
}

/// a leg off the hot path, emitted once the body is complete
type ColdLeg = Box<dyn FnOnce(&mut ssa::Function)>;
impl Cx<'_> {
    pub(crate) fn defer(&self, leg: impl FnOnce(&mut ssa::Function) + 'static) {
        self.cold.borrow_mut().push(Box::new(leg));
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn populate_class_drop_bodies(
    class_drops: &[ClassDropFn],
    class_name_to_tag: &HashMap<String, u32>,
    aliases: &HashMap<String, Type>,
    struct_layouts: &[Vec<(PropKey, Type)>],
    arr_layouts: &crate::ssa::ArrLayouts,
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
        let mut cx = Cx {
            layouts: struct_layouts,
            arr_layouts,
            class_sids: &class_sids,
            tags: &tags,
            drop_fids: &drop_fids,
            intr: intrinsics,
            prelude,
            cold: RefCell::new(Vec::new()),
            worklist: None,
        };
        let (sid, p) = (d.sid.0, Operand::Value(p));
        let mut stack = Vec::new();
        if worklist::holds_own_sid(&cx, sid) {
            let is_class = class_sids.contains(&sid);
            cx.worklist = Some(worklist::alloc(&mut f, prelude, sid, d.fid, p, is_class));
            let wl = cx.worklist.as_ref().expect("just set");
            worklist::emit_loop(&mut f, wl, entry, |f, head, c, pop| {
                emit_release(f, &cx, head, c, sid, &mut stack, Some(pop))
            });
        } else {
            let after = emit_release(&mut f, &cx, entry, p, sid, &mut stack, None);
            f.set_term(after, Terminator::Ret(None));
        }
        for leg in cx.cold.take() {
            leg(&mut f);
        }
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
        Type::Obj(s) if cx.worklist.as_ref().is_some_and(|wl| wl.sid == s.0) => {
            let wl = cx.worklist.as_ref().expect("checked");
            return worklist::emit_push(f, cx, wl, blk, val);
        }
        Type::Obj(s) if cx.drop_fids.contains_key(&s.0) => {
            return emit_call_unless_null(f, blk, val, cx.drop_fids[&s.0]);
        }
        Type::Obj(s) if !stack.contains(&s.0) => {
            return emit_release(f, cx, blk, val, s.0, stack, None);
        }
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

/// A field holding another class's instance: a null child (a leaf's
/// absent subtree) skips the call outright; the callee still answers
/// every other nullish value itself.
fn emit_call_unless_null(
    f: &mut ssa::Function,
    blk: BlockId,
    val: Operand,
    fid: FuncId,
) -> BlockId {
    let call_blk = f.add_block();
    let after = f.add_block();
    let is_null = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Eq, val, Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    branch(f, blk, Operand::Value(is_null), after, call_blk);
    f.append_void(call_blk, InstKind::Call(fid, vec![val]));
    f.set_term(call_blk, Terminator::Br(after));
    after
}

/// Release one reference of `val`, an instance of `sid` or nullish,
/// starting in `blk`; answers the block every leg rejoins in. Given
/// `skip_to`, the legs that release nothing of the cell's own (nullish,
/// a foreign tag, references remaining) leave for it instead, and the
/// answered block is reached from the walk alone. `stack` holds the
/// sids being expanded around this one.
fn emit_release(
    f: &mut ssa::Function,
    cx: &Cx<'_>,
    blk: BlockId,
    val: Operand,
    sid: u32,
    stack: &mut Vec<u32>,
    skip_to: Option<BlockId>,
) -> BlockId {
    let layout = &cx.layouts[sid as usize];
    let is_class = cx.class_sids.contains(&sid);
    let undef_blk = f.add_block();
    let tag_blk = f.add_block();
    let dec_blk = f.add_block();
    let walk_blk = f.add_block();
    let is_null = emit_is_null(f, blk, val);
    let is_undef = emit_is_undef(f, undef_blk, val);
    let tags = cx.tags.get(&sid).map_or(&[0][..], Vec::as_slice);
    let known = emit_tag_in_set(f, tag_blk, val, tags);
    let is_zero = emit_rc_dec(f, dec_blk, val);
    let walk_end = emit_walk(f, cx, walk_blk, val, sid, stack);
    let buffered = is_class.then(|| emit_is_buffered(f, walk_end, val));
    let free_blk = if is_class { f.add_block() } else { walk_end };
    emit_free(f, cx, free_blk, val, sid);
    let after = f.add_block();
    f.set_term(free_blk, Terminator::Br(after));
    let skip = skip_to.unwrap_or(after);
    branch(f, blk, is_null, skip, undef_blk);
    branch(f, undef_blk, is_undef, skip, tag_blk);
    let (obj_drop_rc, cycle_buffer, buffer_slow) = (
        cx.intr.obj_drop_rc,
        cx.intr.cycle_buffer,
        cx.intr.obj_buffer_slow,
    );
    cx.defer(move |f| {
        let foreign_blk = f.add_block();
        branch(f, tag_blk, known, dec_blk, foreign_blk);
        f.append_void(foreign_blk, InstKind::Call(obj_drop_rc, vec![val]));
        f.set_term(foreign_blk, Terminator::Br(skip));
    });
    let Some((buffered, word)) = buffered else {
        branch(f, dec_blk, is_zero, walk_blk, skip);
        return after;
    };
    // references remain: a candidate cycle root; a fields-all-scalar
    // instance can only close a cycle through its expando bag, so it
    // buffers behind the bag's seam
    let scalar = layout.iter().all(|(_, fty)| fty.is_copy());
    cx.defer(move |f| {
        let buffer_blk = f.add_block();
        branch(f, dec_blk, is_zero, walk_blk, buffer_blk);
        let end = if scalar {
            let has = emit_has_props(f, buffer_blk, val);
            emit_cond_call(f, buffer_blk, has, buffer_slow, val)
        } else {
            f.append_void(buffer_blk, InstKind::Call(cycle_buffer, vec![val]));
            buffer_blk
        };
        f.set_term(end, Terminator::Br(skip));
        let defer_blk = f.add_block();
        branch(f, walk_end, buffered, defer_blk, free_blk);
        emit_colour_black(f, defer_blk, val, word);
        f.set_term(defer_blk, Terminator::Br(after));
    });
    after
}

/// Free the cell. A class sid's tag set names classes of this very
/// layout, so a cell past the tag test has this size. An anonymous
/// sid's set holds 0, a cell minted without a stamp, which says
/// nothing of its layout: the sid rides along and `cell_size::settle`
/// decides the free once every layout is known, as for the inline drop.
fn emit_free(f: &mut ssa::Function, cx: &Cx<'_>, blk: BlockId, val: Operand, sid: u32) {
    let size = OBJ_HEADER_SIZE + cx.layouts[sid as usize].len() as u64 * 8;
    let mut args = vec![val, Operand::ConstI64(size as i64)];
    if !cx.class_sids.contains(&sid) {
        args.push(Operand::ConstI64(sid as i64));
    }
    f.append_void(blk, InstKind::Call(cx.intr.obj_drop_sized, args));
}

/// The release of a cell whose count reached zero, up to the free:
/// the WeakRef notice, each non-Copy field, the expando bag.
fn emit_walk(
    f: &mut ssa::Function,
    cx: &Cx<'_>,
    blk: BlockId,
    val: Operand,
    sid: u32,
    stack: &mut Vec<u32>,
) -> BlockId {
    let mut cur = blk;
    if cx.class_sids.contains(&sid) {
        cur = emit_weakref_notify(f, cx, cur, val);
    }
    // the worklist's own instance goes on with its last own-sid field
    let layout = &cx.layouts[sid as usize];
    let next = (cx.worklist.as_ref()).filter(|wl| wl.sid == sid).map(|wl| {
        let own = |fty: &Type| matches!(fty, Type::Obj(s) if s.0 == sid);
        (layout.iter().rposition(|(_, fty)| own(fty)), wl.next)
    });
    if let Some((None, slot)) = next {
        f.append_void(cur, InstKind::Store(Operand::ConstPtrNull, slot, 0));
    }
    stack.push(sid);
    for (i, (_, fty)) in layout.iter().enumerate() {
        if fty.is_copy() {
            continue;
        }
        let off = OBJ_HEADER_SIZE + i as u64 * 8;
        let field = Operand::Value(f.append_inst(cur, InstKind::Load(*fty, val, off), *fty, None));
        match next {
            Some((Some(last), slot)) if last == i => {
                f.append_void(cur, InstKind::Store(field, slot, 0))
            }
            _ => cur = emit_release_value(f, cx, cur, field, *fty, stack),
        }
    }
    stack.pop();
    let has = emit_has_props(f, cur, val);
    emit_cold_call(f, cx, cur, has, cx.intr.obj_props_drop_slow, val)
}
