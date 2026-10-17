//! The `__drop_<C>` body of a sid whose instances hold instances of
//! the same sid — a list, a tree — as a worklist instead of one call
//! per dying child (Lean 4's `lean_del_core` shape):
//!
//! ```text
//! prelude: sp = 0 ; next = p ; active = WeakRef observers live
//! head:    c = next ; c == NULL → pop
//!          the release of `c` (class_drop::emit_release): a leg that
//!          releases nothing of the cell's own → pop ; the walk sets
//!          `next` to the last direct own-sid field (NULL without one)
//!          and pushes every other own-sid reference ; → head
//! pop:     sp == 0 → ret ; next = stack[--sp] ; → head
//! ```
//!
//! A push onto a full stack releases the child by a call to the fn
//! itself, so the frame stays fixed and the recursion depth is the
//! number of times a subtree overflowed the stack, not the tree depth:
//! a list walks in one frame. The WeakRef observer count is read once
//! per call: a release runs no user code, so nothing can start
//! observing a cell while it runs, and the notice re-reads the count
//! itself.

use crate::ssa::{self, BlockId, FuncId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower_emit_drop_value::class_drop::{Cx, branch};

/// Stack slots before a push falls back to a call.
const STACK_SLOTS: u64 = 32;

pub(crate) struct Worklist {
    pub sid: u32,
    fid: FuncId,
    stack: Operand,
    sp: Operand,
    pub next: Operand,
    /// the observer count, read once in the prelude
    pub active: Option<Operand>,
}

/// Whether a release of `sid` itself releases references of `sid`:
/// a field of the sid, or an array of it.
pub(crate) fn holds_own_sid(cx: &Cx<'_>, sid: u32) -> bool {
    let own = |ty: &Type| matches!(ty, Type::Obj(s) if s.0 == sid);
    cx.layouts[sid as usize].iter().any(|(_, fty)| {
        own(fty) || matches!(fty, Type::Arr(id) if own(&cx.arr_layouts[id.0 as usize]))
    })
}

/// The slots in `prelude`, seeded with the root `p`.
pub(crate) fn alloc(
    f: &mut ssa::Function,
    prelude: BlockId,
    sid: u32,
    fid: FuncId,
    p: Operand,
    is_class: bool,
) -> Worklist {
    let stack = f.append_inst(
        prelude,
        InstKind::AllocaBytes(STACK_SLOTS * 8),
        Type::Ptr,
        None,
    );
    let sp = Operand::Value(f.append_inst(prelude, InstKind::Alloca(Type::I64), Type::Ptr, None));
    let next = Operand::Value(f.append_inst(prelude, InstKind::Alloca(Type::Ptr), Type::Ptr, None));
    f.append_void(prelude, InstKind::Store(Operand::ConstI64(0), sp, 0));
    f.append_void(prelude, InstKind::Store(p, next, 0));
    let active = is_class.then(|| {
        let ptr = f.append_inst(
            prelude,
            InstKind::GlobalRef(ssa::WEAKREF_ACTIVE_SYM.to_string()),
            Type::Ptr,
            None,
        );
        Operand::Value(f.append_inst(
            prelude,
            InstKind::Load(Type::I64, Operand::Value(ptr), 0),
            Type::I64,
            None,
        ))
    });
    Worklist {
        sid,
        fid,
        stack: Operand::Value(stack),
        sp,
        next,
        active,
    }
}

/// The loop: `head` takes `next`, releases it through `release` (which
/// answers the block its legs rejoin in) and comes back; an empty
/// `next` pops.
pub(crate) fn emit_loop(
    f: &mut ssa::Function,
    wl: &Worklist,
    head: BlockId,
    release: impl FnOnce(&mut ssa::Function, BlockId, Operand, BlockId) -> BlockId,
) {
    // the legs leave through `skip`, so `pop` lays out past the release
    // and the release falls in from `head`
    let skip = f.add_block();
    let c = f.append_inst(head, InstKind::Load(Type::Ptr, wl.next, 0), Type::Ptr, None);
    let after = release(f, head, Operand::Value(c), skip);
    f.set_term(after, Terminator::Br(head));
    let pop = f.add_block();
    f.set_term(skip, Terminator::Br(pop));
    let n = f.append_inst(pop, InstKind::Load(Type::I64, wl.sp, 0), Type::I64, None);
    let empty = f.append_inst(
        pop,
        InstKind::ICmp(IPred::Eq, Operand::Value(n), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let take = f.add_block();
    let done = f.add_block();
    branch(f, pop, Operand::Value(empty), done, take);
    f.set_term(done, Terminator::Ret(None));
    let n1 = f.append_inst(
        take,
        InstKind::BinOp(ssa::BinOp::Sub, Operand::Value(n), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    f.append_void(take, InstKind::Store(Operand::Value(n1), wl.sp, 0));
    let v = f.append_inst(
        take,
        InstKind::LoadDynScaled8(Type::Ptr, wl.stack, Operand::Value(n1)),
        Type::Ptr,
        None,
    );
    f.append_void(take, InstKind::Store(Operand::Value(v), wl.next, 0));
    f.set_term(take, Terminator::Br(head));
}

/// Defer the release of `val`, an own-sid reference or NULL: push it,
/// or call the fn on it once the stack is full. Answers the block the
/// caller continues in.
pub(crate) fn emit_push(
    f: &mut ssa::Function,
    cx: &Cx<'_>,
    wl: &Worklist,
    blk: BlockId,
    val: Operand,
) -> BlockId {
    let after = f.add_block();
    let room_blk = f.add_block();
    let push_blk = f.add_block();
    let is_null = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Eq, val, Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    branch(f, blk, Operand::Value(is_null), after, room_blk);
    let n = f.append_inst(
        room_blk,
        InstKind::Load(Type::I64, wl.sp, 0),
        Type::I64,
        None,
    );
    let full = f.append_inst(
        room_blk,
        InstKind::ICmp(
            IPred::Eq,
            Operand::Value(n),
            Operand::ConstI64(STACK_SLOTS as i64),
        ),
        Type::Bool,
        None,
    );
    f.append_void(
        push_blk,
        InstKind::StoreDynScaled8(val, wl.stack, Operand::Value(n)),
    );
    let n1 = f.append_inst(
        push_blk,
        InstKind::BinOp(ssa::BinOp::Add, Operand::Value(n), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    f.append_void(push_blk, InstKind::Store(Operand::Value(n1), wl.sp, 0));
    f.set_term(push_blk, Terminator::Br(after));
    let fid = wl.fid;
    cx.defer(move |f| {
        let call_blk = f.add_block();
        branch(f, room_blk, Operand::Value(full), call_blk, push_blk);
        f.append_void(call_blk, InstKind::Call(fid, vec![val]));
        f.set_term(call_blk, Terminator::Br(after));
    });
    after
}
