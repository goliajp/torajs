//! Number-coerce pair for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 388 — Path A.3-batch9.
//!
//! Two methods forming the "coerce to concrete numeric" sink family
//! (mirror to the int32-coerce family in `ssa_lower_int32_coerce.rs`
//! — that side handles Number → int32 semantics; this side handles
//! I64/Any → F64):
//!
//! - `coerce_to_f64(op)` — promote an i64 operand to f64. Constants
//!   rewrite in place; SSA values emit `InstKind::SiToFp`.
//! - `coerce_any_to_number(op, target)` — P7.2b: coerce a `Type::Any`
//!   operand to a concrete numeric via the JS spec §7.1.4 ToNumber
//!   sink (`__torajs_any_to_number` runtime helper → F64), then narrow
//!   to `target` (F64 as-is, or I64 via the existing F64→i64
//!   ToInteger path). Single place for the Any→number sink so
//!   `Stmt::Return` / `Assign` / call-arg coercion can't drift apart.
//!
//! Method bodies preserved byte-for-byte; the sibling reaches LowerCtx
//! fields via `impl<'a> super::LowerCtx<'a>`, so call sites (~50 across
//! sibling modules — call_terminal / binop_inner_f64 / stmt_return /
//! array / arr_push / assign_member / …) need zero edits.

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// Whether this operand is the null / undefined VALUE, however it
    /// was spelled. Five sites across the coercion family used to ask
    /// `op == Operand::ConstPtrNull`, which only ever holds for the
    /// LITERAL: bind the same value and the read is a Load, so
    /// `const s = null; +s` / `s | 0` / `s * 3` / `String(s)` each
    /// fell off its own arm — one panicked, one answered the pointer.
    /// Asking the TYPE covers both spellings at every site, and one
    /// predicate keeps them from drifting apart again.
    ///
    /// A `Type::Ptr` operand reaching a coercion is null or undefined
    /// and nothing else: every object shape carries its own SSA type
    /// (`Obj` / `Arr` / `Str` / `Closure` / …), and the checker's
    /// operand gates keep the rest out of these sites entirely.
    pub(crate) fn operand_is_nullish(&self, op: &Operand) -> bool {
        matches!(op, Operand::ConstPtrNull) || matches!(self.operand_ty(op), Type::Ptr)
    }

    /// Promote an i64 operand to f64. Constants are rewritten in place
    /// (cheaper than emitting a sitofp instruction LLVM would constant-fold
    /// anyway). Value operands emit an explicit InstKind::SiToFp.
    ///
    /// Panics on a shape it has no primitive rule for. That panic is
    /// load-bearing: this sink emits INSTRUCTIONS only, never a block
    /// split, and ~60 call sites append to a block they cached around
    /// it. A slot whose spec step is ToNumber wants
    /// [`Self::coerce_to_f64_tonumber`] instead — that one may branch.
    pub(crate) fn coerce_to_f64(&mut self, op: Operand) -> Operand {
        self.try_coerce_to_f64(op.clone())
            .unwrap_or_else(|| panic!("ssa-lower: cannot coerce {:?} to f64", self.operand_ty(&op)))
    }

    /// The primitive half of [`Self::coerce_to_f64`]: `Some` for every
    /// shape §7.1.4 answers without running user code, `None` for the
    /// rest. Split out so the ToNumber sink below can ask "is this one
    /// of the cheap ones?" without keeping a second copy of the list —
    /// two copies of a shape list that must change together is how
    /// `1n > true` answered `true` for a rotation (r676).
    fn try_coerce_to_f64(&mut self, op: Operand) -> Option<Operand> {
        Some(match self.operand_ty(&op) {
            Type::F64 => op,
            Type::I64 => match op {
                Operand::ConstI64(n) => Operand::ConstF64(n as f64),
                Operand::Value(_) => {
                    let v =
                        self.f
                            .append_inst(self.cur_block, InstKind::SiToFp(op), Type::F64, None);
                    Operand::Value(v)
                }
                _ => op,
            },
            // §7.1.4 ToNumber(null) is +0. The literal folds long
            // before this; a BINDING of it arrives as a Ptr, and
            // `const s = null; s * 3` used to panic here while
            // `null * 3` answered 0. Undefined never reaches this
            // sink — the checker's arith gate turns it away first
            // (registered residue).
            Type::Ptr => Operand::ConstF64(0.0),
            // §7.1.4 ToNumber(true) is 1 and ToNumber(false) is +0.
            // A Bool reaches this sink only beside the undefined
            // lane's NaN sentinel: `true + 1` folds on the integer
            // path long before here, but `true + undefined` puts the
            // Bool on the f64 path, and that was the panic. The
            // undefined side already answers NaN, so the sum does
            // too — which is what §13.15.3 asks for.
            Type::Bool => match op {
                Operand::ConstBool(b) => Operand::ConstF64(if b { 1.0 } else { 0.0 }),
                _ => {
                    let i = self.f.append_inst(
                        self.cur_block,
                        InstKind::ZExtBoolToI64(op),
                        Type::I64,
                        None,
                    );
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::SiToFp(Operand::Value(i)),
                        Type::F64,
                        None,
                    );
                    Operand::Value(v)
                }
            },
            // §7.1.4 ToNumber(String) is StringToNumber, and this sink
            // is chosen by the operand's REPR, not by its static type:
            // `Math.max(s as any, 1)` keeps a Str repr under an Any
            // annotation and landed on the panic below, while the same
            // value bound as `const s: any = "5"` boxes and answers 5.
            // `str_to_number` borrows (no rc traffic), so ownership of
            // the operand is unchanged — whoever released it still does.
            Type::Str | Type::Substr => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.str_to_number, vec![op]),
                    Type::F64,
                    None,
                );
                Operand::Value(v)
            }
            _ => return None,
        })
    }

    /// ES §7.1.4 `ToNumber` at an f64 slot — the sink for a parameter
    /// whose spec step is ToNumber, which **reaches every value**.
    ///
    /// `Math.abs`, `Math.pow` and friends each declare `Number` params,
    /// and the lane's admission wedge used to name `Number`, then
    /// `Any`, and refuse the rest: `Math.max({valueOf(){return 7}}, 1)`
    /// died at lower time while the same value behind
    /// `const b: any = o` answered 7 — kernel right, boxing right, the
    /// call site simply never boxed.
    ///
    /// Unlike [`Self::coerce_to_f64`] this one may BRANCH: an object's
    /// ToPrimitive is user code and can throw, so the emitted throw
    /// check splits the current block. A caller that cached
    /// `self.cur_block` before calling must re-read it after.
    pub(crate) fn coerce_to_f64_tonumber(&mut self, op: Operand) -> Operand {
        if self.operand_ty(&op) == Type::Any {
            return self.coerce_any_to_number(op, Type::F64);
        }
        if let Some(v) = self.try_coerce_to_f64(op.clone()) {
            return v;
        }
        let boxed = self.box_to_any(op);
        self.coerce_any_to_number(boxed, Type::F64)
    }

    /// P7.2b — coerce an Any operand to a concrete numeric: JS spec
    /// §7.1.4 ToNumber via the one `__torajs_any_to_number` runtime
    /// helper, then narrowed to `target` (F64 as-is, or I64 via the
    /// existing F64→i64 ToInteger path). Single place for the
    /// Any→number sink so Stmt::Return and Assign can't drift apart
    /// (mirrors coerce_to_bool's `Type::Any => any_to_bool`
    /// precedent). Caller guarantees `operand_ty(op) == Type::Any`
    /// and `target` ∈ {I64, F64}.
    pub(crate) fn coerce_any_to_number(&mut self, op: Operand, target: Type) -> Operand {
        // §7.1.4 ToNumber can record a pending throw (Symbol / BigInt
        // reject, OrdinaryToPrimitive both-methods-answer-objects
        // TypeError); the helper call's arm checks for it. Without the
        // check the NaN placeholder leaks out and the stranded pending
        // throw poisons the next runtime entry (the second `Number(x)`
        // over a both-object receiver SIGSEGVed on a placeholder
        // rc_dec). Twin of `coerce_any_to_bigint` below.
        let num = crate::ssa_lower_anyv_number::to_number(self, op);
        if target == Type::F64 {
            num
        } else {
            self.coerce_to_i64(num)
        }
    }

    /// Any → BigInt at a call boundary (§7.1.13 ToBigInt via the
    /// any-lane kernel; RFC 20260720 刀 5b-2). The result is a fresh
    /// OWNED BigInt the caller must release after the consuming call;
    /// a coercion reject records a pending throw (TypeError /
    /// SyntaxError) and the emitted throw check unwinds before the
    /// NULL sentinel is ever read.
    pub(crate) fn coerce_any_to_bigint(&mut self, op: Operand) -> Operand {
        let b = Operand::Value(self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_to_bigint, vec![op]),
            Type::BigInt,
            None,
        ));
        self.emit_throw_check(None);
        b
    }
}
