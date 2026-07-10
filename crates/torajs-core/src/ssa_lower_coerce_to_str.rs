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
                 * lifetime walk; nothing to do here. */
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
                Operand::Value(formatted)
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
                Operand::Value(s)
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
