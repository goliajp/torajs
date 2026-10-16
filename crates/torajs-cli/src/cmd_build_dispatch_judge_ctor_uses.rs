//! Where a `constructor` read's answer can go. The answer may be an
//! interned builtin constructor cell, whose statics mint through the
//! ns-static seam only when something looks one up on it: a member
//! read or method call naming a static, an own-key walk, or any
//! runtime path the value is handed to. A function whose every
//! `"constructor"` literal keys a property read, and whose every read
//! answer is only compared, dropped, `typeof`-ed or read for a key no
//! namespace static is named after (`name`, `length`, `prototype`),
//! cannot reach that seam through the read (`(e as Error).constructor
//! .name`, `x.constructor === Foo`).
//!
//! The walk is intra-procedural and follows the answer through every
//! value derived from it (box / retain / φ copies / pointer casts and
//! arithmetic / pointer-typed loads), so a value that escapes — an
//! argument, a store, a return — is seen at the escape.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    FuncId, Function, InstKind, Module, Operand, StringId, Terminator, Type, ValueId,
    visit_value_operands,
};

const GOPD: &str = "__torajs_anyv_get_property_descriptor";

/// Readers whose operand 0 is the receiver and operand 1 the key.
pub(crate) const KEYED_READERS: [&str; 8] = [
    "__torajs_any_member_get_tag",
    "__torajs_any_member_get_value",
    "__torajs_any_member_get_with_receiver",
    "__torajs_any_accessor_get",
    // a number dictionary's keyed read: the pair read and the
    // accessor-or-data answer in one call
    "__torajs_dynobj_dict_get",
    GOPD,
    "__torajs_any_index_get_keyed",
    // typed array `xs.<name>`: the own expando, then the
    // Array.prototype chain (`xs.constructor` answers Array)
    "__torajs_arr_member_value",
];

/// Calls whose answer is derived from the argument (the same cell,
/// re-boxed or retained, or its raw pointer).
const CARRIERS: [&str; 4] = [
    "__torajs_anyv_box_from_pair",
    "__torajs_anyv_retain",
    "__torajs_anyv_cell_ptr",
    "__torajs_anyv_unbox_value",
];

/// Calls that read a fixed own key, compare, or test the value's
/// kind, and never look a static up on their argument. Loose
/// equality runs ToPrimitive, which reads `@@toPrimitive` /
/// `valueOf` / `toString`; no namespace owns a static by those names.
const SINKS: [&str; 9] = [
    "__torajs_any_name_get",
    "__torajs_any_length_get",
    "__torajs_error_name_get",
    "__torajs_anyv_strict_eq",
    "__torajs_anyv_strict_eq_imm_pair",
    "__torajs_anyv_loose_eq",
    "__torajs_anyv_typeof",
    "__torajs_anyv_to_bool",
    "__torajs_anyv_unbox_tag",
];

/// Calls that may take the `"constructor"` literal (at the given
/// operand) without reading a property with it: a drop, the `in`
/// operator's key, and the writes that store a program value under it.
const KEY_SINKS: [(&str, usize); 7] = [
    ("__torajs_str_drop", 0),
    ("__torajs_in_op_any_str", 1),
    ("__torajs_any_member_set", 1),
    ("__torajs_dynobj_dict_set", 1),
    ("__torajs_dynobj_set_fresh", 1),
    ("__torajs_dynobj_define", 1),
    ("__torajs_arr_define", 1),
];

const CTOR_KEY: &[u8] = b"constructor";

/// A use of a value: an instruction (block, index) or a block's
/// terminator (index `None`).
pub(crate) type Use = (usize, Option<usize>);

/// The string ids that spell `constructor`.
pub(crate) fn ctor_key_ids(module: &Module) -> HashSet<StringId> {
    (0..module.strings.len())
        .filter(|&i| module.strings[i].is_latin1 && module.strings[i].bytes == CTOR_KEY)
        .map(|i| StringId(i as u32))
        .collect()
}

/// The first use in `f` through which a `constructor` read's answer
/// (or the literal itself) can reach anything but a benign sink, as a
/// callee name or instruction label; None when there is none.
pub(crate) fn ctor_read_escape(
    module: &Module,
    f: &Function,
    ctor_ids: &HashSet<StringId>,
) -> Option<String> {
    let mut lits: HashMap<ValueId, StringId> = HashMap::new();
    for inst in f.blocks.iter().flat_map(|b| &b.insts) {
        if let (InstKind::StaticStrRef(s) | InstKind::StringRef(s), Some(r)) =
            (&inst.kind, inst.result)
        {
            lits.insert(r, *s);
        }
    }
    let keys: Vec<ValueId> = lits
        .iter()
        .filter(|(_, s)| ctor_ids.contains(s))
        .map(|(v, _)| *v)
        .collect();
    if keys.is_empty() {
        return None;
    }
    let walk = Walk {
        module,
        f,
        lits,
        uses: use_lists(f),
    };
    let mut answers = HashSet::new();
    for k in keys {
        for &u in walk.uses_of(k) {
            match walk.key_use(k, u) {
                Ok(read) => answers.extend(read),
                Err(why) => return Some(why),
            }
        }
    }
    walk.follow(answers).err()
}

struct Walk<'a> {
    module: &'a Module,
    f: &'a Function,
    lits: HashMap<ValueId, StringId>,
    uses: HashMap<ValueId, Vec<Use>>,
}

impl Walk<'_> {
    fn uses_of(&self, v: ValueId) -> &[Use] {
        self.uses.get(&v).map_or(&[], Vec::as_slice)
    }

    fn callee(&self, fid: FuncId) -> &str {
        self.module.funcs[fid.0 as usize].name.as_str()
    }

    /// One use of the `"constructor"` literal `k`: `Ok(Some(answer))`
    /// for a keyed read, `Ok(None)` for a benign sink, `Err` naming
    /// any other use.
    fn key_use(&self, k: ValueId, (bi, ii): Use) -> Result<Option<ValueId>, String> {
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
    fn follow(&self, mut seen: HashSet<ValueId>) -> Result<(), String> {
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
        if SINKS.contains(&name) {
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

pub(crate) fn use_lists(f: &Function) -> HashMap<ValueId, Vec<Use>> {
    let mut uses: HashMap<ValueId, Vec<Use>> = HashMap::new();
    for (bi, b) in f.blocks.iter().enumerate() {
        for (ii, inst) in b.insts.iter().enumerate() {
            visit_value_operands(&inst.kind, |v| {
                uses.entry(v).or_default().push((bi, Some(ii)))
            });
        }
        let mut term = |op: &Operand| {
            if let Operand::Value(v) = op {
                uses.entry(*v).or_default().push((bi, None));
            }
        };
        match &b.term {
            Terminator::CondBr { cond, .. } => term(cond),
            Terminator::Ret(Some(op)) => term(op),
            Terminator::Br(_) | Terminator::Ret(None) | Terminator::Unreachable => {}
        }
    }
    uses
}
