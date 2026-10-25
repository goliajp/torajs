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
    Function, InstKind, Module, StringId, ValueId, visit_terminator_values, visit_value_operands,
};

use crate::cmd_build_dispatch_judge_ctor_walk::Walk;

pub(crate) const GOPD: &str = "__torajs_anyv_get_property_descriptor";

/// Readers whose operand 0 is the receiver and operand 1 the key.
pub(crate) const KEYED_READERS: [&str; 12] = [
    "__torajs_any_member_get_tag",
    "__torajs_any_member_get_value",
    "__torajs_any_member_get_with_receiver",
    "__torajs_any_accessor_get",
    // a number dictionary's keyed read: the pair read and the
    // accessor-or-data answer in one call
    "__torajs_dynobj_dict_get",
    "__torajs_dynobj_dict_get_entry",
    GOPD,
    "__torajs_any_index_get_keyed",
    // typed array `xs.<name>`: the own expando, then the
    // Array.prototype chain (`xs.constructor` answers Array)
    "__torajs_arr_member_value",
    // `super[k]`, `super[k](…)`, `super[k]?.(…)`: the read starts at
    // the home object's [[Prototype]] (operand 0), whose chain may hold
    // %Object.prototype% and its `constructor`; the calls answer what
    // the method read returns
    SUPER_PROP_GET,
    SUPER_PROP_CALL,
    SUPER_PROP_OPT_CALL,
];

pub(crate) const SUPER_PROP_GET: &str = "__torajs_anyv_super_prop_get";
pub(crate) const SUPER_PROP_CALL: &str = "__torajs_anyv_super_prop_call";
pub(crate) const SUPER_PROP_OPT_CALL: &str = "__torajs_anyv_super_prop_opt_call";

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
    hooks_fixed: &dyn Fn() -> bool,
) -> Option<String> {
    let lits = literals(f);
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
        hooks_fixed,
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

/// The first use in `f` through which one of `answers`, the answers of
/// keyed reads whose key is computed at run time, can reach anything
/// but a benign sink. Such a key may be `"constructor"`, so each answer
/// is followed as a spelled read's is.
pub(crate) fn runtime_key_escape(
    module: &Module,
    f: &Function,
    answers: HashSet<ValueId>,
    hooks_fixed: &dyn Fn() -> bool,
) -> Option<String> {
    let walk = Walk {
        module,
        f,
        lits: literals(f),
        uses: use_lists(f),
        hooks_fixed,
    };
    walk.follow(answers).err()
}

/// The string literal each value of `f` holds, by value.
fn literals(f: &Function) -> HashMap<ValueId, StringId> {
    let mut lits = HashMap::new();
    for inst in f.blocks.iter().flat_map(|b| &b.insts) {
        if let (InstKind::StaticStrRef(s) | InstKind::StringRef(s), Some(r)) =
            (&inst.kind, inst.result)
        {
            lits.insert(r, *s);
        }
    }
    lits
}

pub(crate) fn use_lists(f: &Function) -> HashMap<ValueId, Vec<Use>> {
    let mut uses: HashMap<ValueId, Vec<Use>> = HashMap::new();
    for (bi, b) in f.blocks.iter().enumerate() {
        for (ii, inst) in b.insts.iter().enumerate() {
            visit_value_operands(&inst.kind, |v| {
                uses.entry(v).or_default().push((bi, Some(ii)))
            });
        }
        visit_terminator_values(&b.term, |v| uses.entry(v).or_default().push((bi, None)));
    }
    uses
}
