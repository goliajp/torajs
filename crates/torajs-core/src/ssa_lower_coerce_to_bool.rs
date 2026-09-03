//! §7.1.2 ToBoolean for branch conditions, split out of
//! `ssa_lower_logical` (rotation 572 — the `&&` / `||` join rework
//! pushed that file past 500). The seam is the one its module doc
//! already drew: one side lowers the short-circuit operators, this
//! side answers what "truthy" means for each representation, and the
//! `if` / ternary / predicate lanes ask it too.

use crate::ssa::{FPred, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
    /// V3-18 m1.g — JS spec §7.1.2 ToBoolean. Coerces `op` to a
    /// Type::Bool for branch conditions in `&&` / `||` / `if` /
    /// ternary on non-bool inputs.
    ///   undefined → false  (post-V3-18 m1.h)
    ///   null      → false
    ///   Bool      → as-is
    ///   Number i64 → 0 = false, else true
    ///   F64       → 0/-0/NaN = false, else true
    ///   String / Substr → empty = false, else true
    ///   Object / Array / Closure / etc → always true (non-null heap)
    pub(crate) fn coerce_to_bool(&mut self, op: Operand) -> Operand {
        let ty = self.operand_ty(&op);
        match ty {
            Type::Bool => op,
            Type::I64 => self.cmp(IPred::Ne, op, Operand::ConstI64(0)),
            Type::F64 => {
                // ToBoolean(NaN) = false, ToBoolean(+0/-0) = false,
                // else true. FPred::One ("ordered, not equal") is
                // true iff both operands are non-NaN AND unequal —
                // exactly NaN→false, ±0→false, others→true.
                self.fcmp(FPred::One, op, Operand::ConstF64(0.0))
            }
            Type::Str | Type::Substr => {
                // ToBoolean(string) per spec §7.1.2 — falsy iff "",
                // null, or undefined. A Str slot may hold NULL (JS
                // null via a Nullable<String> truthy-narrow) or the
                // undefined sentinel cell (missed exec/match capture,
                // RFC 20260707 chunk 2 — its payload is "undefined"
                // so the len>0 walk would wrongly answer true); the
                // runtime nullish probe covers both before the
                // length load.
                let is_null = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.str_is_nullish, vec![op.clone()]),
                    Type::Bool,
                    None,
                );
                let null_blk = self.f.add_block();
                let nn_blk = self.f.add_block();
                let merge = self.f.add_block();
                let result_slot = self.alloca_in_entry(Type::Bool, Some("__strbool"));
                let cb = self.cur_block;
                self.f.set_term(
                    cb,
                    Terminator::CondBr {
                        cond: Operand::Value(is_null),
                        then_blk: null_blk,
                        else_blk: nn_blk,
                    },
                );
                // null branch: store false.
                self.f.append_void(
                    null_blk,
                    InstKind::Store(Operand::ConstBool(false), Operand::Value(result_slot), 0),
                );
                self.f.set_term(null_blk, Terminator::Br(merge));
                // non-null branch: load len via the encoding-aware
                // helper, compare > 0.
                self.cur_block = nn_blk;
                let len_op = crate::ssa_lower_str::load_str_or_substr_length(self, op, ty);
                let nz = self.cmp(IPred::Sgt, len_op, Operand::ConstI64(0));
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(nz, Operand::Value(result_slot), 0),
                );
                let cb = self.cur_block;
                self.f.set_term(cb, Terminator::Br(merge));
                self.cur_block = merge;
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Bool, Operand::Value(result_slot), 0),
                    Type::Bool,
                    None,
                );
                Operand::Value(r)
            }
            Type::Ptr => {
                // null literal or any raw pointer — null = false.
                self.cmp(IPred::Ne, op, Operand::ConstPtrNull)
            }
            Type::BigInt => self.bigint_to_bool(op),
            // P0.4 — ToBoolean(Any) per JS spec §7.1.2 routes through
            // __torajs_any_to_bool which unboxes the tag + payload
            // and applies spec rules: NULL → false, BOOL → value,
            // I64 → !=0, F64 → !=0 && !NaN, HEAP/Str → len>0, other
            // HEAP → true. Other heap-pointer types continue to use
            // the simple null-check fallback (still correct because
            // they're statically non-Any objects: object → true,
            // null → false).
            Type::Any => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_to_bool, vec![op]),
                    Type::Bool,
                    None,
                );
                Operand::Value(v)
            }
            // Heap-typed values (Obj / Arr / Closure / Symbol /
            // RegExp / Date / BigInt / FnSig / ...) lower to a single
            // pointer at codegen, so ToBoolean per spec §7.1.2 is a
            // nullish check: NULL (JS null) and the slot type's
            // immortal undefined sentinel (RFC 20260710 C2a FnSig →
            // Str-shaped oddball; C2b Obj/Arr/Closure → the generic
            // Tag::Undefined cell) are the two falsy pointers;
            // everything else is a live object → true. Two inline
            // cmps + and, zero calls. Slot types with no sentinel
            // yet (Symbol / RegExp / Date / ...) keep the plain
            // null check.
            _ => {
                let ne_null = self.cmp(IPred::Ne, op.clone(), Operand::ConstPtrNull);
                let Some(sentinel) = self.str_undef_sentinel_for(ty) else {
                    return ne_null;
                };
                let ne_undef = self.cmp(IPred::Ne, op, sentinel);
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(crate::ssa::BinOp::And, ne_null, ne_undef),
                    Type::Bool,
                    None,
                );
                Operand::Value(r)
            }
        }
    }

    /// ES §7.1.4 ToBoolean(BigInt) — `0n` is falsy, every other
    /// BigInt is truthy. The heap layout keeps `len == 0` iff the
    /// value is `0n`, so a runtime probe answers it without leaking
    /// LEN_OFF into SSA.
    ///
    /// The probe reads the payload, so a slot that can hold the
    /// generic undefined cell (a read past the end of a `bigint[]`,
    /// a `find` miss) has to be sorted out first — the cell is a
    /// bare header with no words behind it, and asking it for its
    /// length answered `true` for a value that is `undefined`. Same
    /// two falsy pointers as the pointer-slot arm below, only with a
    /// branch rather than an `and` because the probe must not run on
    /// the sentinel at all.
    fn bigint_to_bool(&mut self, op: Operand) -> Operand {
        let sentinel = self
            .str_undef_sentinel_for(Type::BigInt)
            .expect("BigInt spells undefined with the generic cell");
        let ne_null = self.cmp(IPred::Ne, op.clone(), Operand::ConstPtrNull);
        let ne_undef = self.cmp(IPred::Ne, op.clone(), sentinel);
        let live = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(crate::ssa::BinOp::And, ne_null, ne_undef),
            Type::Bool,
            None,
        );
        let result_slot = self.alloca_in_entry(Type::Bool, Some("__bigbool"));
        let live_blk = self.f.add_block();
        let merge = self.f.add_block();
        let cb = self.cur_block;
        self.f.append_void(
            cb,
            InstKind::Store(Operand::ConstBool(false), Operand::Value(result_slot), 0),
        );
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(live),
                then_blk: live_blk,
                else_blk: merge,
            },
        );
        self.cur_block = live_blk;
        let v = self.f.append_inst(
            live_blk,
            InstKind::Call(self.intrinsics.bigint_is_nonzero, vec![op]),
            Type::I64,
            None,
        );
        let nz = self.cmp(IPred::Ne, Operand::Value(v), Operand::ConstI64(0));
        let cb = self.cur_block;
        self.f
            .append_void(cb, InstKind::Store(nz, Operand::Value(result_slot), 0));
        self.f.set_term(cb, Terminator::Br(merge));
        self.cur_block = merge;
        let r = self.f.append_inst(
            merge,
            InstKind::Load(Type::Bool, Operand::Value(result_slot), 0),
            Type::Bool,
            None,
        );
        Operand::Value(r)
    }
}
