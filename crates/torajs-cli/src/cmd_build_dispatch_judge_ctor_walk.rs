//! The walk behind [`crate::cmd_build_dispatch_judge_ctor_uses`]:
//! which uses of a `"constructor"` literal read with it, and which
//! uses of a read answer keep the cell from any namespace static.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, FuncId, Function, InstKind, Module, Operand, StringId, Terminator, Type, ValueId,
};

use crate::cmd_build_dispatch_judge_ctor_uses::{GOPD, KEYED_READERS, Use};

/// Calls whose answer is derived from the argument (the same cell,
/// re-boxed or retained, or its raw pointer).
const CARRIERS: [&str; 4] = [
    "__torajs_anyv_box_from_pair",
    "__torajs_anyv_retain",
    "__torajs_anyv_cell_ptr",
    "__torajs_anyv_unbox_value",
];

/// Calls that read a fixed own key, compare, or test the value's
/// kind, and never look a static up on their argument; the settle
/// after a pair decode only frees the string the decode materialized.
const SINKS: [&str; 9] = [
    "__torajs_any_name_get",
    "__torajs_any_length_get",
    "__torajs_error_name_get",
    "__torajs_anyv_strict_eq",
    "__torajs_anyv_strict_eq_imm_pair",
    "__torajs_anyv_typeof",
    "__torajs_anyv_to_bool",
    "__torajs_anyv_unbox_tag",
    "__torajs_anyv_unbox_settle",
];

/// Calls that run ToPrimitive on their argument and answer a
/// primitive: loose equality, the `+` of two pair-decoded boxes and
/// the string conversions of one. The builtin `@@toPrimitive` /
/// `valueOf` / `toString` a constructor cell finds keep nothing, so
/// these are benign while the program cannot install its own
/// ([`crate::cmd_build_dispatch_judge_coerce_hooks`]).
const COERCING_SINKS: [&str; 4] = [
    "__torajs_anyv_loose_eq",
    "__torajs_anyv_add_pair",
    "__torajs_anyv_to_str_pair",
    "__torajs_anyv_to_str_pair_prim",
];

/// Calls that may take the `"constructor"` literal (at the given
/// operand) without reading a property with it: a drop, the `in`
/// operator's key, and the writes that store a program value under it.
const KEY_SINKS: [(&str, usize); 8] = [
    ("__torajs_str_drop", 0),
    ("__torajs_in_op_any_str", 1),
    ("__torajs_any_member_set", 1),
    ("__torajs_dynobj_dict_set", 1),
    ("__torajs_dynobj_dict_set_entry", 1),
    ("__torajs_dynobj_set_fresh", 1),
    ("__torajs_dynobj_define", 1),
    ("__torajs_arr_define", 1),
];

pub(crate) struct Walk<'a> {
    pub(crate) module: &'a Module,
    pub(crate) f: &'a Function,
    pub(crate) lits: HashMap<ValueId, StringId>,
    pub(crate) uses: HashMap<ValueId, Vec<Use>>,
    /// Whether the program cannot install a ToPrimitive hook, asked
    /// only at a [`COERCING_SINKS`] use.
    pub(crate) hooks_fixed: &'a dyn Fn() -> bool,
}

impl Walk<'_> {
    pub(crate) fn uses_of(&self, v: ValueId) -> &[Use] {
        self.uses.get(&v).map_or(&[], Vec::as_slice)
    }

    fn callee(&self, fid: FuncId) -> &str {
        self.module.funcs[fid.0 as usize].name.as_str()
    }

    /// One use of the `"constructor"` literal `k`: `Ok(Some(answer))`
    /// for a keyed read, `Ok(None)` for a benign sink, `Err` naming
    /// any other use.
    pub(crate) fn key_use(&self, k: ValueId, (bi, ii): Use) -> Result<Option<ValueId>, String> {
        let Some(ii) = ii else {
            return Err("a terminator as the key".into());
        };
        let inst = &self.f.blocks[bi].insts[ii];
        let InstKind::Call(fid, args) = &inst.kind else {
            return Err(format!("the key of {:?}", inst.kind));
        };
        let name = self.callee(*fid);
        let mut at = args
            .iter()
            .enumerate()
            .filter(|(_, a)| **a == Operand::Value(k));
        let only = match (at.next(), at.next()) {
            (Some((p, _)), None) => Some(p),
            _ => None,
        };
        // a descriptor bag holds the constructor under `value`, so a
        // later fixed-key read off the answer is no longer benign
        if KEYED_READERS.contains(&name) && name != GOPD && only == Some(1) {
            Ok(inst.result)
        } else if KEY_SINKS
            .iter()
            .any(|(n, p)| *n == name && only == Some(*p))
        {
            Ok(None)
        } else {
            Err(format!("the key of {name}"))
        }
    }

    /// Follow every value derived from the read answers; `Err` names
    /// the first use that is not a benign sink.
    pub(crate) fn follow(&self, mut seen: HashSet<ValueId>) -> Result<(), String> {
        let mut work: Vec<ValueId> = seen.iter().copied().collect();
        while let Some(t) = work.pop() {
            for &(bi, ii) in self.uses_of(t) {
                let Some(ii) = ii else {
                    match &self.f.blocks[bi].term {
                        Terminator::CondBr { .. } => continue,
                        _ => return Err("return".into()),
                    }
                };
                let inst = &self.f.blocks[bi].insts[ii];
                if self.answer_use(t, &inst.kind)?
                    && let Some(r) = inst.result
                    && seen.insert(r)
                {
                    work.push(r);
                }
            }
        }
        Ok(())
    }

    /// One use of a value `t` derived from a read answer: `Ok(true)`
    /// when the instruction's result is derived from it too,
    /// `Ok(false)` for a benign sink.
    fn answer_use(&self, t: ValueId, kind: &InstKind) -> Result<bool, String> {
        Ok(match kind {
            InstKind::Call(fid, args) => return self.answer_call(t, self.callee(*fid), args),
            InstKind::RcInc(_) | InstKind::RcIncAny(_) | InstKind::RcDecAny(_) => false,
            InstKind::ICmp(..)
            | InstKind::FCmp(..)
            | InstKind::AnyIsNumber(_)
            | InstKind::AnyIsInt32(_) => false,
            // the decodes of an int32 payload (`shl 32` then `ashr 32`)
            // and of a tag (`lshr 48`): a right shift by 32 or more
            // answers at most 32 bits, either the high half of a cell
            // address (below 2^16 on a 47-bit address space) or the
            // low half (below 4 GiB, which the Mach-O zero page keeps
            // free of cells). Neither addresses a cell, and the
            // lowering never ORs two halves back together. A left
            // shift stays derived: its answer is still followed.
            InstKind::BinOp(BinOp::AShr | BinOp::LShr, _, Operand::ConstI64(n)) if *n >= 32 => {
                false
            }
            // a number; the lowering boxes a NaN answer as the
            // canonical NaN, not with the operands' payload
            InstKind::BinOp(
                BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::FDiv | BinOp::FRem,
                _,
                _,
            ) => false,
            InstKind::Load(ty, _, _) => {
                !matches!(ty, Type::I64 | Type::I32 | Type::F64 | Type::Bool)
            }
            InstKind::Store(..)
            | InstKind::StoreDyn(..)
            | InstKind::StoreDynScaled8(..)
            | InstKind::CallIndirect(..) => return Err(format!("{kind:?}")),
            _ => true,
        })
    }

    fn answer_call(&self, t: ValueId, name: &str, args: &[Operand]) -> Result<bool, String> {
        let is_t = |i: usize| args.get(i) == Some(&Operand::Value(t));
        if CARRIERS.contains(&name) || (name == "__torajs_any_accessor_get" && is_t(2)) {
            return Ok(true);
        }
        if SINKS.contains(&name) || (COERCING_SINKS.contains(&name) && (self.hooks_fixed)()) {
            return Ok(false);
        }
        let receiver_only = (0..args.len())
            .all(|i| !is_t(i) || i == 0 || (i == 2 && name.ends_with("_with_receiver")));
        if KEYED_READERS.contains(&name) && receiver_only {
            let key = match args.get(1) {
                Some(Operand::Value(v)) => self.lits.get(v),
                _ => None,
            };
            return match key.map(|s| &self.module.strings[s.0 as usize]) {
                Some(k) if !names_static(&k.bytes) => Ok(false),
                _ => Err(format!("{name} with a static or runtime key")),
            };
        }
        Err(name.to_string())
    }
}

/// True when some namespace owns a static named `key` (a read of it
/// off a ctor cell would mint that static's cell).
fn names_static(key: &[u8]) -> bool {
    (0..)
        .map_while(torajs_rc::ns_static_meta)
        .any(|r| r.name.as_bytes() == key)
}
