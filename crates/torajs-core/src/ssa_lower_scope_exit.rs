//! Scope-exit drop channel (RFC 20260901-scope-exit-drops).
//!
//! A block's owned locals used to be released on exactly two paths:
//! the block's own fall-through close (`ssa_lower_stmt_block`) and
//! the fn-level exits (`emit_drops_for_owned_locals`). Every other
//! transfer that leaves a frame without reaching its closing `}` —
//! `break` / `continue`, a `throw` or a may-throw call routing to a
//! catch, `return` routing through a `finally`, the finally tail's
//! own dispatch — branched straight to its target and stranded the
//! frames it crossed (`try { const t = s(i); }` in a main loop leaked
//! one string per iteration; `for (…) { const t = s(i); continue; }`
//! the same).
//!
//! The channel: every control-flow target that can be jumped to from
//! inside deeper frames records the depth of the first frame that
//! dies on that jump ([`ExitTarget::scope_depth`] /
//! [`LoopTargets::scope_depth`]), and the jump site emits
//! [`LowerCtx::emit_drops_for_scopes_from`] for `scope_stack[depth..]`
//! before branching. The per-binding logic is the block-close one
//! ([`LowerCtx::emit_frame_drops`]), shared so the two paths cannot
//! drift; it also writes NULL back into the released slot so a second
//! drop of the same slot (a switch `case` that jumps past the `const`,
//! then the frame is re-entered by a loop) is a no-op rather than a
//! double free.

use crate::ssa::{BlockId, InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// A block control can transfer to from inside deeper scope frames,
/// with the depth those frames start at. Elements of `try_stack`
/// (the innermost catch / finally a throw routes to) and
/// `try_finally_stack` (the finally a `return` / `break` /
/// `continue` routes through).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ExitTarget {
    pub(crate) blk: BlockId,
    /// Index into `scope_stack` of the first frame that dies when
    /// control transfers to `blk`. For a try body that is the body
    /// frame's own index; for the catch → finally route it is the
    /// catch frame's (the same index — the body frame is gone).
    pub(crate) scope_depth: usize,
    /// Index into `for_of_teardown_stack` of the first for-of the
    /// jump leaves — those iterators owe a §7.4.9 IteratorClose before
    /// the frames holding them drop (刀 2).
    pub(crate) teardown_depth: usize,
}

/// One `loop_stack` entry: `continue` → `cont`, `break` → `brk`, and
/// the depth of the first frame either jump leaves behind. Loops that
/// close a frame themselves on every exit path (a `for` init frame,
/// closed in the loop's `after` block) record the depth *below* it;
/// loops whose body frame is only closed on fall-through (the for-of
/// family's `close_body_scope`) record that frame's index so a
/// `break` / `continue` releases what the fall-through would have.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LoopTargets {
    /// `None` for a `switch`: a `break` exits it, a `continue` goes
    /// to the loop around it.
    pub(crate) cont: Option<BlockId>,
    pub(crate) brk: BlockId,
    pub(crate) scope_depth: usize,
    /// `for_of_teardown_stack.len()` at the loop's entry — a for-of
    /// pushes its own teardown BEFORE this, so a plain `break` /
    /// `continue` (whose exit block / header handles the loop's own
    /// iterator) owes nothing here; a labeled jump over inner for-ofs
    /// closes `[teardown_depth..]` (刀 2).
    pub(crate) teardown_depth: usize,
}

impl LoopTargets {
    /// The block a `break` (`want_break`) or `continue` of this entry
    /// lands on. A `continue` never names a `switch` entry — an early
    /// error for a label, [`LowerCtx::continue_loop`] for a bare one.
    pub(crate) fn target(&self, want_break: bool) -> BlockId {
        if want_break {
            self.brk
        } else {
            self.cont
                .expect("a `continue` targets an iteration statement")
        }
    }
}

impl LowerCtx<'_> {
    /// The `loop_stack` entry a plain `continue` targets: the nearest
    /// iteration statement, past any `switch` in between.
    pub(crate) fn continue_loop(&self) -> Option<usize> {
        self.loop_stack.iter().rposition(|lt| lt.cont.is_some())
    }

    /// The `loop_stack` entry a plain `break` (`want_break`: the
    /// innermost one) or `continue` ([`Self::continue_loop`]) exits.
    pub(crate) fn jump_loop(&self, want_break: bool) -> Option<usize> {
        if want_break {
            self.loop_stack.len().checked_sub(1)
        } else {
            self.continue_loop()
        }
    }

    /// Whether the innermost `try … finally` was opened inside
    /// `loop_stack[idx]`, so a jump to that entry's target must run
    /// the finally first.
    pub(crate) fn finally_inside_loop(&self, idx: usize) -> bool {
        self.try_finally_loop_depth.last().is_some_and(|&d| d > idx)
    }

    /// Release the owned bindings of one scope frame, in declaration
    /// order — the block-close protocol: an escape-promoted Copy
    /// local releases its capture-box stake, a promoted mutable
    /// non-Copy capture releases its box, a moved / stack-alloca'd
    /// local is skipped, everything else loads and drops. The slot
    /// is NULLed after a heap drop so the entry NULL-init invariant
    /// (`binding_slot_alloca`) holds again once the frame is left.
    pub(crate) fn emit_frame_drops(&mut self, frame: &[String]) {
        for name in frame {
            let Some(info) = self.locals.get(name).copied() else {
                continue;
            };
            if info.ty.is_copy() {
                if info.boxed && self.escape_captured_lets.contains(name) {
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.capture_box_drop,
                            vec![Operand::Value(info.slot)],
                        ),
                    );
                }
                continue;
            }
            if !info.borrowed && self.boxed_noncopy_lets.contains(name) {
                let fid = self.capture_box_drop_fid(info.ty);
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(fid, vec![Operand::Value(info.slot)]),
                );
                continue;
            }
            if info.moved || self.stack_alloced_locals.contains(name) {
                continue;
            }
            let val = self.f.append_inst(
                self.cur_block,
                InstKind::Load(info.ty, Operand::Value(info.slot), 0),
                info.ty,
                None,
            );
            if matches!(info.ty, Type::Obj(_)) && self.proofs.let_is_nonnull(name) {
                self.proofs.arm_drop_live();
            }
            self.emit_drop_value(Operand::Value(val), info.ty);
            self.proofs.take_drop_live();
            if info.ty.is_refcounted() {
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::ConstPtrNull, Operand::Value(info.slot), 0),
                );
            }
        }
    }

    /// Release every frame at `scope_stack[depth..]`, innermost first
    /// — what a jump out of those frames owes before it branches. The
    /// frames stay on the stack and their names stay in `locals`:
    /// lowering continues on the fall-through path, which closes them
    /// in its own time.
    pub(crate) fn emit_drops_for_scopes_from(&mut self, depth: usize) {
        if depth >= self.scope_stack.len() {
            return;
        }
        let frames: Vec<Vec<String>> = self.scope_stack[depth..].iter().rev().cloned().collect();
        for frame in &frames {
            self.emit_frame_drops(frame);
        }
    }

    /// Would [`Self::emit_drops_for_scopes_from`] emit anything? Lets
    /// a throw-check keep its bare `Br` when nothing is live.
    pub(crate) fn scopes_have_drops_from(&self, depth: usize) -> bool {
        if depth >= self.scope_stack.len() {
            return false;
        }
        self.scope_stack[depth..].iter().flatten().any(|name| {
            let Some(info) = self.locals.get(name) else {
                return false;
            };
            if info.ty.is_copy() {
                return info.boxed && self.escape_captured_lets.contains(name);
            }
            if !info.borrowed && self.boxed_noncopy_lets.contains(name) {
                return true;
            }
            !info.moved && !self.stack_alloced_locals.contains(name)
        })
    }

    /// Pop the innermost scope frame with the block-close protocol:
    /// drop its owners when the current block is still open (a closed
    /// block was left by an exit that already released them), remove
    /// its bindings from `locals`, and reinstate the outer bindings
    /// it shadowed.
    pub(crate) fn close_scope_frame(&mut self) {
        let frame = self.scope_stack.pop().expect("scope frame");
        let shadows = self.shadow_stack.pop().expect("shadow frame");
        if self.cur_open() {
            self.emit_frame_drops(&frame);
        }
        for name in frame {
            self.boxed_noncopy_lets.remove(&name);
            self.locals.remove(&name);
        }
        for (name, prev) in shadows {
            if self.binding_is_boxed_noncopy(&name, &prev) {
                self.boxed_noncopy_lets.insert(name.clone());
            }
            // the name denotes the outer binding again; what the
            // inner one proved does not carry over
            self.proofs.kill(&name);
            self.locals.insert(name, prev);
        }
    }
}

/// The four lazily-allocated slots a `finally` tail reads to decide
/// which non-local exit is still pending when the finally body ends.
///
/// One family, one lifecycle: each is None until the first `return` /
/// `break` / `continue` inside a try-with-finally observes that its
/// jump has to flow through the finally, at which point the site
/// allocates the slot and sets it, and the finally tail checks it to
/// choose between resuming the exit and continuing normally. They sit
/// together because reading any one of them without the others is
/// never the question being asked.
#[derive(Default)]
pub(crate) struct PendingExits {
    /// Alloca slot for a pending return value across finally blocks.
    /// Type matches the enclosing fn's ret type.
    pub(crate) return_slot: Option<ValueId>,
    /// Companion bool flag for [`Self::return_slot`] — set by a
    /// Return inside a try-with-finally, checked at the finally tail.
    pub(crate) return_flag: Option<ValueId>,
    /// Bool slot allocated on the first break-inside-finally; set by
    /// the break site, checked at the finally tail.
    pub(crate) break_flag: Option<ValueId>,
    /// Same shape for continue.
    pub(crate) continue_flag: Option<ValueId>,
    /// The `finally` bodies being lowered, outermost first: the three
    /// slots each one's entry set the pending throw aside in (active,
    /// tag, value). The value is owned there until the tail puts it
    /// back.
    pub(crate) finally_throws: Vec<[ValueId; 3]>,
    /// The words a suspension point reads what the finally bodies keep
    /// aside into (`ssa_lower_fn::suspend::held::aside`): the pending
    /// return's first, then one per finally level. Each point reads
    /// them before it parks and holds the values from then on, so every
    /// point shares one word per level and the frame does not widen
    /// with the number of points.
    pub(crate) aside_words: Vec<Option<ValueId>>,
}
