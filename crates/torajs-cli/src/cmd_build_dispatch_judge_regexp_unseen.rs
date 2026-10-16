//! Whether a fallback program can reach a RegExp protocol method by an
//! id the judgment scan does not see.
//!
//! A fallback trigger makes the general judgment give up on the
//! observed-id set, but the RegExp protocol has narrower doors. A
//! protocol call runs only for one of the ids `match`, `matchAll`,
//! `replace`, `replaceAll`, `search`, `split` (and `compile`, which can
//! change a RegExp under a running one), and only two kinds of call can
//! carry such an id unseen:
//!
//! - a call by a runtime key: `recv[key](…)`, `super[key](…)`, a
//!   prototype method read by key — the key names the method;
//! - a call of a method value someone read first (`f.call(…)`, a user
//!   method slot holding it): the value is a String or RegExp method
//!   cell, and reading one takes its key, a protocol name or one of the
//!   five protocol symbols.
//!
//! Both need the key as a runtime value. A program either spells it (a
//! string literal naming a protocol method anywhere but as the name of
//! a call whose id the scan observes, or `Symbol.match` and its
//! siblings), or reads or calls with a key the scan cannot pin down.
//! The scan answers true for both, and for the two readers that walk
//! keys themselves (descriptor bags, proxies). Every other key it can
//! pin: a string literal, a boxed number or boolean, a symbol that is
//! not a protocol symbol (the other well-known ones, a fresh or a
//! registered symbol).

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{Function, InstKind, Module, Operand, Type, ValueId};

use crate::cmd_build_dispatch_judge::MID_CARRIERS;
use crate::cmd_build_dispatch_judge_ctor_uses::{KEYED_READERS, Use, use_lists};

/// The String.prototype / RegExp.prototype method names whose call
/// can enter the protocol seam.
const PROTOCOL_NAMES: [&[u8]; 7] = [
    b"match",
    b"matchAll",
    b"replace",
    b"replaceAll",
    b"search",
    b"split",
    b"compile",
];

/// `__torajs_symbol_well_known` indexes of `Symbol.match`,
/// `.matchAll`, `.replace`, `.search` and `.split` (torajs-core
/// `WELL_KNOWN_SYMBOL_NAMES`, alphabetical).
const PROTOCOL_SYMBOL_IDX: [i64; 5] = [6, 7, 8, 9, 11];

/// Symbol producers that never answer a protocol symbol.
const OTHER_SYMBOLS: [&str; 6] = [
    "__torajs_symbol_iterator",
    "__torajs_symbol_async_iterator",
    "__torajs_symbol_to_primitive",
    "__torajs_symbol_alloc",
    "__torajs_symbol_for",
    "__torajs_symbol_for_any",
];

/// Readers and calls beside [`KEYED_READERS`] whose operand 1 is a
/// property key.
const KEYED_CALLS: [&str; 4] = [
    "__torajs_any_index_method_call",
    "__torajs_builtin_proto_method_value",
    "__torajs_any_member_get_pair",
    "__torajs_anyv_super_prop_",
];

/// Its operand 0 is a prototype tag, not a receiver.
const PROTO_METHOD_VALUE: &str = "__torajs_builtin_proto_method_value";

/// Calls that hand out values under keys the program never names.
const KEY_WALKERS: [&str; 2] = ["__torajs_anyv_get_property_descriptors", "__torajs_proxy"];

/// True when some call in `module` can reach a protocol method under
/// an id the judgment scan does not observe (see the module doc).
/// `diag` names the first reason.
pub(crate) fn unseen_protocol_call(module: &Module, diag: bool) -> bool {
    for f in &module.funcs {
        if let Some(why) = unseen_in(module, f) {
            if diag {
                eprintln!("[judge] regexp unseen protocol call: {} {why}", f.name);
            }
            return true;
        }
    }
    false
}

fn unseen_in(module: &Module, f: &Function) -> Option<&'static str> {
    let (fixed, boxes) = pinned_keys(module, f);
    let mut uses = None;
    for inst in f.blocks.iter().flat_map(|b| &b.insts) {
        match &inst.kind {
            InstKind::StaticStrRef(s) | InstKind::StringRef(s) => {
                let lit = &module.strings[s.0 as usize];
                if !lit.is_latin1 || !PROTOCOL_NAMES.contains(&lit.bytes.as_slice()) {
                    continue;
                }
                let Some(r) = inst.result else { continue };
                let uses = uses.get_or_insert_with(|| use_lists(f));
                if !names_observed_call(module, f, uses.get(&r).map_or(&[], Vec::as_slice)) {
                    return Some("spells a protocol method name");
                }
            }
            InstKind::Call(fid, args) => {
                let name = module.funcs[fid.0 as usize].name.as_str();
                if name == "__torajs_symbol_well_known"
                    && !matches!(args.first(), Some(Operand::ConstI64(i))
                        if !PROTOCOL_SYMBOL_IDX.contains(i))
                {
                    return Some("reads a protocol symbol");
                }
                if KEY_WALKERS.iter().any(|p| name.starts_with(p)) {
                    return Some("walks keys");
                }
                if (KEYED_READERS.contains(&name)
                    || KEYED_CALLS.iter().any(|p| name.starts_with(p)))
                    && !matches!(args.get(1), Some(Operand::Value(v)) if fixed.contains(v))
                    && !matches!(args.first(), Some(Operand::Value(v))
                        if name != PROTO_METHOD_VALUE && no_protocol_face(f, &boxes, *v))
                {
                    return Some("uses a key it cannot pin");
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether every use of a spelled name is the name operand of a call
/// whose method id the judgment observes ([`MID_CARRIERS`]), or its
/// drop.
fn names_observed_call(module: &Module, f: &Function, uses: &[Use]) -> bool {
    uses.iter().all(|&(bi, ii)| {
        let Some(ii) = ii else { return false };
        let InstKind::Call(fid, _) = &f.blocks[bi].insts[ii].kind else {
            return false;
        };
        let name = module.funcs[fid.0 as usize].name.as_str();
        name == "__torajs_str_drop" || MID_CARRIERS.iter().any(|(n, _)| *n == name)
    })
}

/// The values of `f` that are keys no protocol method is found under:
/// string literals (a spelled protocol name is judged on its own),
/// boxed numbers and other scalars, and non-protocol symbols and their
/// boxes.
/// Also answers each `box_any 4` result's boxed value.
fn pinned_keys(module: &Module, f: &Function) -> (HashSet<ValueId>, HashMap<ValueId, ValueId>) {
    let insts = || f.blocks.iter().flat_map(|b| &b.insts);
    let mut symbols = HashSet::new();
    let mut fixed = HashSet::new();
    let mut boxes = HashMap::new();
    for inst in insts() {
        let Some(r) = inst.result else { continue };
        match &inst.kind {
            InstKind::StaticStrRef(_)
            | InstKind::StringRef(_)
            | InstKind::BoxInt32(_)
            | InstKind::BoxF64(_) => {
                fixed.insert(r);
            }
            InstKind::BoxAny(tag, _) if *tag != 4 => {
                fixed.insert(r);
            }
            InstKind::Call(fid, args) => {
                let name = module.funcs[fid.0 as usize].name.as_str();
                let other_well_known = name == "__torajs_symbol_well_known"
                    && matches!(args.first(), Some(Operand::ConstI64(i))
                        if !PROTOCOL_SYMBOL_IDX.contains(i));
                if other_well_known || OTHER_SYMBOLS.contains(&name) {
                    symbols.insert(r);
                    fixed.insert(r);
                }
            }
            _ => {}
        }
    }
    for inst in insts() {
        let Some(r) = inst.result else { continue };
        let boxed = match &inst.kind {
            InstKind::BoxAny(4, Operand::Value(v)) => {
                boxes.insert(r, *v);
                Some(v)
            }
            InstKind::Call(fid, args)
                if module.funcs[fid.0 as usize].name == "__torajs_anyv_box_from_pair" =>
            {
                match args.get(1) {
                    Some(Operand::Value(v)) => Some(v),
                    _ => None,
                }
            }
            _ => None,
        };
        if boxed.is_some_and(|v| symbols.contains(v)) {
            fixed.insert(r);
        }
    }
    (fixed, boxes)
}

/// Whether the receiver `v` (or the typed value it boxes) is of a type
/// whose prototype chain carries no String or RegExp method: a read or
/// call on it answers a protocol method only if the program stored one
/// there, which some other, judged read had to produce first. Not a
/// struct instance, whose class may extend `String`.
fn no_protocol_face(f: &Function, boxes: &HashMap<ValueId, ValueId>, v: ValueId) -> bool {
    let t = f.value_type(*boxes.get(&v).unwrap_or(&v));
    matches!(
        t,
        Type::FnSig(_)
            | Type::Closure(_)
            | Type::Arr(_)
            | Type::Vec(_)
            | Type::Date
            | Type::Symbol
            | Type::Promise
            | Type::BigInt
            | Type::WeakRef
            | Type::WeakMap
            | Type::WeakSet
            | Type::Map
            | Type::Set
            | Type::MapIter
            | Type::ArrIter
            | Type::I64
            | Type::I32
            | Type::F64
            | Type::Bool
    )
}
