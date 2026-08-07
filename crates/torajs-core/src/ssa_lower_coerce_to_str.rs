//! `coerce_to_str` — value → Type::Str coercer extracted from
//! `ssa_lower.rs` chunk 375.
//!
//! Type-aware SSA emitter that turns an `(Operand, Type)` into a
//! fresh-owned Type::Str operand. Handles Str (identity), Substr
//! (substr_to_owned), I64/F64 (i64_to_str/f64_to_str intrinsics),
//! Bool (branchy select over "true"/"false" literals via an entry
//! alloca), BigInt (bigint_to_string + `n` suffix concat), and Any
//! (tag/value unbox → runtime any_to_str dispatch). All other types
//! panic; the caller (console multi-arg / template-literal / etc.)
//! only requests coercion for supported shapes. Method body is
//! byte-for-byte preserved from the source; sibling reaches the
//! `LowerCtx` fields + intrinsics through the shared impl block on
//! `crate::ssa_lower::LowerCtx`.

use crate::ssa::{InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    pub(crate) fn coerce_to_str(&mut self, val: Operand, ty: Type) -> Operand {
        match ty {
            Type::Str => val,
            Type::Substr => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.substr_to_owned, vec![val]),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::I64 => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.i64_to_str, vec![val]),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::F64 => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.f64_to_str, vec![val]),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::Bool => {
                let true_ptr = self.intern_string_literal("true");
                let false_ptr = self.intern_string_literal("false");
                let then_blk = self.f.add_block();
                let else_blk = self.f.add_block();
                let after_blk = self.f.add_block();
                let slot = self.alloca_in_entry(Type::Str, Some("__c_bool"));
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: val,
                        then_blk,
                        else_blk,
                    },
                );
                self.f.append_void(
                    then_blk,
                    InstKind::Store(Operand::Value(true_ptr), Operand::Value(slot), 0),
                );
                self.f.set_term(then_blk, Terminator::Br(after_blk));
                self.f.append_void(
                    else_blk,
                    InstKind::Store(Operand::Value(false_ptr), Operand::Value(slot), 0),
                );
                self.f.set_term(else_blk, Terminator::Br(after_blk));
                self.cur_block = after_blk;
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Str, Operand::Value(slot), 0),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::BigInt => {
                /* T-25 — bigint_to_string + concat with `"n"` to
                 * match node/bun's console.log formatting. The
                 * caller will drop the resulting Str. The BigInt
                 * input itself is dropped by the caller's binding-
                 * lifetime walk; nothing to do here.
                 *
                 * Rotation 326 — an OOB / miss read of a BigInt slot
                 * answers the immortal generic undefined cell
                 * (§10.4.2.1 family), and this was the one print
                 * lane that read the cell's CONTENT: bigint_to_string
                 * walked limbs that aren't there (`console.log(bs[5])`
                 * was a two-line SIGBUS). Branch on the ADDRESS like
                 * every other consumer of the family; the sentinel arm
                 * answers the static "undefined" literal (rc no-op
                 * under the caller's drop). */
                let sentinel = self
                    .str_undef_sentinel_for(Type::BigInt)
                    .expect("BigInt spells undefined with the generic cell");
                let is_undef = self.f.append_inst(
                    self.cur_block,
                    InstKind::ICmp(crate::ssa::IPred::Eq, val.clone(), sentinel),
                    Type::Bool,
                    None,
                );
                let undef_blk = self.f.add_block();
                let big_blk = self.f.add_block();
                let after_blk = self.f.add_block();
                let slot = self.alloca_in_entry(Type::Str, Some("__c_bigint"));
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: Operand::Value(is_undef),
                        then_blk: undef_blk,
                        else_blk: big_blk,
                    },
                );
                self.cur_block = undef_blk;
                let undef_lit = self.intern_string_literal("undefined");
                self.f.append_void(
                    undef_blk,
                    InstKind::Store(Operand::Value(undef_lit), Operand::Value(slot), 0),
                );
                self.f.set_term(undef_blk, Terminator::Br(after_blk));
                self.cur_block = big_blk;
                let body = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.bigint_to_string, vec![val]),
                    Type::Str,
                    None,
                );
                let n_lit = self.intern_string_literal("n");
                let formatted = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.str_concat,
                        vec![Operand::Value(body), Operand::Value(n_lit)],
                    ),
                    Type::Str,
                    None,
                );
                self.emit_drop_value(Operand::Value(body), Type::Str);
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(formatted), Operand::Value(slot), 0),
                );
                self.f.set_term(self.cur_block, Terminator::Br(after_blk));
                self.cur_block = after_blk;
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Str, Operand::Value(slot), 0),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::Any => {
                /* Any-boxed value (catch param default / dynobj
                 * lookup result / etc.): split into tag + raw value
                 * via the unbox intrinsics, then route through the
                 * runtime's tag-dispatched ToString implementation.
                 * Returns a fresh-owned Str (rc=1; caller's
                 * post-call drop reclaims). Heap inputs are rc-inc'd
                 * by the runtime so the caller still sees a single
                 * owned ref. */
                let tag = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_tag, vec![val.clone()]),
                    Type::I64,
                    None,
                );
                let raw = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_value, vec![val.clone()]),
                    Type::I64,
                    None,
                );
                let s = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.any_to_str,
                        vec![Operand::Value(tag), Operand::Value(raw)],
                    ),
                    Type::Str,
                    None,
                );
                // any_to_str only borrowed the pair — reclaim a
                // ShortStr-materialized temp (no-op otherwise).
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.any_unbox_settle,
                        vec![val, Operand::Value(raw)],
                    ),
                );
                // OrdinaryToPrimitive can record a catchable
                // TypeError (both methods exhausted / a user method
                // threw) — propagate to the user's try/catch instead
                // of leaking the pending throw past the concat
                // (`"" + objWithShadowedToString` printed the
                // placeholder and unwound as uncaught later).
                self.emit_throw_check(None);
                Operand::Value(s)
            }
            // RFC 20260712 chunk B — a struct operand mirrors the
            // String(struct) S137 emit: a layout carrying a
            // toString / valueOf hook runs OrdinaryToPrimitive at
            // runtime (+ throw check); hook-free layouts keep the
            // static §20.1.4.4 literal (drop is a no-op on it).
            Type::Obj(sid) => {
                let layout = &self.struct_layouts[sid.0 as usize];
                let has_hook = layout
                    .iter()
                    .any(|(n, _)| n == "toString" || n == "valueOf");
                if has_hook {
                    let raw = self.f.append_inst(
                        self.cur_block,
                        InstKind::PtrToInt(val),
                        Type::I64,
                        None,
                    );
                    let s = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.any_to_str,
                            vec![Operand::ConstI64(4), Operand::Value(raw)],
                        ),
                        Type::Str,
                        None,
                    );
                    self.emit_throw_check(None);
                    Operand::Value(s)
                } else {
                    Operand::Value(self.intern_string_literal("[object Object]"))
                }
            }
            // RFC 20260710 C2a — a fn-typed slot value ToStrings via
            // the fnname runtime (null → "null", the undefined
            // sentinel → the sentinel cell itself, a real address →
            // "[Function: name]" / anonymous). Owned result; the
            // caller's drop is a no-op on the static sentinel.
            Type::FnSig(_) => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.fnsig_to_str, vec![val]),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            other => {
                panic!("ssa-lower: console multi-arg coercion of type {other:?} not supported")
            }
        }
    }
}
