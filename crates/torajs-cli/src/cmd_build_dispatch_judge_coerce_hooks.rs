//! Whether the program can install a ToPrimitive hook a builtin
//! constructor cell would find. ToPrimitive (§7.1.1) Gets
//! `@@toPrimitive`, `valueOf` and `toString` off its operand and calls
//! what it finds with the operand as `this`; on a constructor cell the
//! lookup walks the cell's own properties, %Function.prototype% and
//! %Object.prototype%. The builtin methods found there answer a
//! primitive and keep nothing, but a method the program put there
//! receives the cell, and can look a static up on it — so `"" + c`,
//! `c + 1` and `c == 1` hand the cell out exactly when such a method
//! can exist.
//!
//! Putting one there takes a write: under one of those keys, under a
//! key computed at run time, through a bulk definer (`Object.assign`,
//! `defineProperty`, `__defineGetter__`), or by relinking a prototype
//! (`setPrototypeOf`, a `__proto__` write, the `__proto__` setter
//! taken out of its descriptor or through `__lookupSetter__`) so the
//! chain runs through a program object. The scan is whole-program and
//! receiver-blind: any such write anywhere answers false, whatever
//! object it lands on. Writes that cannot install one are a fixed key
//! that names none of the hooks, an integer index, a fresh object's
//! own initializer (an object literal, a class body: the object is not
//! on a builtin chain until a relink puts it there), and a run-time
//! key whose stored value is provably a primitive (a primitive is
//! never callable, and a `__proto__` write of one changes nothing,
//! B.2.2.1.2 step 4).
//!
//! A reader is fixed whatever its key: reading installs nothing, and
//! when a run-time key names a definer, the answer still has to be
//! called to install anything. The premise rests on
//! [`crate::cmd_build_dispatch_judge_ctor_reach`] following every
//! answer of a read with a run-time key: a call of one keeps the
//! statics whatever this premise says. The readers that hand the
//! `__proto__` setter out inside a descriptor, or call what they read
//! (`super[k](…)`), take a literal key only.
//!
//! What each callee can do is read off
//! [`crate::cmd_build_dispatch_judge_coerce_roles`].

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{Function, InstKind, Module, Operand, ValueId};

use crate::cmd_build_dispatch_judge_coerce_defs::Defs;
use crate::cmd_build_dispatch_judge_coerce_roles::{Role, role};
use crate::cmd_build_dispatch_judge_ctor_reach::BOX_STR;
use crate::cmd_build_dispatch_judge_ctor_uses::{
    GOPD, SUPER_PROP_CALL, SUPER_PROP_OPT_CALL, Use, use_lists,
};

/// The string keys a hook is installed under. `@@toPrimitive` needs
/// no entry: a symbol key is never a string literal, so a write under
/// it is a write under a run-time key, which already installs unless
/// the stored value is a primitive.
pub(crate) const HOOK_KEYS: [&[u8]; 3] = [b"valueOf", b"toString", b"__proto__"];

/// The Annex B methods every object inherits that define an accessor
/// under any key or hand out the `__proto__` setter. Held as a value,
/// one can be stored under a method name the dispatch knows and
/// called with a run-time key, so any use of these names but a drop
/// installs.
pub(crate) const DEFINER_KEYS: [&[u8]; 4] = [
    b"__defineGetter__",
    b"__defineSetter__",
    b"__lookupGetter__",
    b"__lookupSetter__",
];

/// The memo of [`hooks_fixed`] for one judgment: worked out on first
/// ask, since only a coercing use of a constructor answer asks.
#[derive(Default)]
pub(crate) struct HookPremise(OnceCell<bool>);

impl HookPremise {
    pub(crate) fn holds(&self, module: &Module, live: &[bool], diag: bool) -> bool {
        *self.0.get_or_init(|| hooks_fixed(module, live, diag))
    }
}

/// Readers that take only a literal key. A descriptor read hands out
/// the `__proto__` setter as its `set`, and a `super[k](…)` call calls
/// what it reads, a definer included, before any use of its answer
/// is seen.
const LITERAL_KEY_READERS: [&str; 3] = [GOPD, SUPER_PROP_CALL, SUPER_PROP_OPT_CALL];

/// The boxes of `f`'s string literals, and its any constants: a
/// number, or a string of at most five bytes held in the box itself,
/// shorter than every hook and definer key.
fn boxed_literals(module: &Module, f: &Function, lits: &HashMap<ValueId, u32>) -> HashSet<ValueId> {
    (f.blocks.iter().flat_map(|b| &b.insts))
        .filter(|i| match &i.kind {
            InstKind::IntToPtr(Operand::ConstI64(_)) => true,
            InstKind::Call(fid, args) => {
                module.funcs[fid.0 as usize].name == BOX_STR
                    && matches!(args.as_slice(), [Operand::Value(v)] if lits.contains_key(v))
            }
            _ => false,
        })
        .filter_map(|i| i.result)
        .collect()
}

/// The literals of a function that spell a hook or a definer key.
#[derive(Default)]
struct KeyIds {
    hooks: HashSet<u32>,
    definers: HashSet<u32>,
}

/// True when no live function can install a ToPrimitive hook (see the
/// module doc). `diag` names the first write that can.
pub(crate) fn hooks_fixed(module: &Module, live: &[bool], diag: bool) -> bool {
    let mut ids = KeyIds::default();
    for (i, s) in module.strings.iter().enumerate() {
        let bytes = s.bytes.as_slice();
        if HOOK_KEYS.contains(&bytes) {
            ids.hooks.insert(i as u32);
        } else if DEFINER_KEYS.contains(&bytes) {
            ids.definers.insert(i as u32);
        }
    }
    let mut roles: Vec<Option<Role>> = vec![None; module.funcs.len()];
    for f in crate::cmd_build_dispatch_judge_callee::live_funcs(module, live) {
        if let Err(why) = function_fixed(module, f, &ids, &mut roles) {
            if diag {
                eprintln!("[judge] to-primitive hooks open: {} {why}", f.name);
            }
            return false;
        }
    }
    true
}

fn function_fixed(
    module: &Module,
    f: &Function,
    ids: &KeyIds,
    roles: &mut [Option<Role>],
) -> Result<(), String> {
    let mut lits: HashMap<ValueId, u32> = HashMap::new();
    for inst in f.blocks.iter().flat_map(|b| &b.insts) {
        if let (InstKind::StaticStrRef(s) | InstKind::StringRef(s), Some(r)) =
            (&inst.kind, inst.result)
        {
            lits.insert(r, s.0);
        }
    }
    let defs = OnceCell::new();
    let boxed = OnceCell::new();
    for inst in f.blocks.iter().flat_map(|b| &b.insts) {
        let InstKind::Call(fid, args) = &inst.kind else {
            continue;
        };
        let callee = &module.funcs[fid.0 as usize];
        let role = *roles[fid.0 as usize].get_or_insert_with(|| role(callee));
        // a literal, its box or an any constant: the literal walk
        // below takes the box as a use no hook or definer literal may
        // have
        let literal = |at: usize| {
            matches!(args.get(at), Some(Operand::Value(k))
                if lits.contains_key(k)
                    || boxed.get_or_init(|| boxed_literals(module, f, &lits)).contains(k))
        };
        let fixed = match role {
            Role::Inert | Role::Fresh => true,
            Role::Reader => !LITERAL_KEY_READERS.contains(&callee.name.as_str()) || literal(1),
            Role::MethodCall { mid } => matches!(
                args.get(mid),
                Some(Operand::ConstI64(m))
                    if !(torajs_rc::ANY_METHOD_DEFINE_GETTER..=torajs_rc::ANY_METHOD_LOOKUP_SETTER)
                        .contains(m)
            ),
            Role::Keyed { key, value } => {
                literal(key)
                    || value.is_some_and(|at| {
                        let defs = defs.get_or_init(|| Defs::of(f));
                        args.get(at).is_some_and(|v| defs.primitive(module, f, v))
                    })
            }
            Role::Installer => false,
        };
        if !fixed {
            return Err(format!("calls {}", callee.name));
        }
    }
    let keyed: Vec<(ValueId, bool)> = lits
        .iter()
        .filter_map(|(v, s)| {
            let definer = ids.definers.contains(s);
            (definer || ids.hooks.contains(s)).then_some((*v, definer))
        })
        .collect();
    if keyed.is_empty() {
        return Ok(());
    }
    let uses = use_lists(f);
    for (k, definer) in keyed {
        for &u in uses.get(&k).map_or(&[][..], Vec::as_slice) {
            if !key_use_fixed(module, f, k, definer, u) {
                return Err("uses a hook or definer key".into());
            }
        }
    }
    Ok(())
}

/// A use of a hook-key literal that installs nothing: the key of a
/// read (not of a descriptor read), of `in`, of a fresh object's
/// initializer, or a drop. A definer key admits only the drop.
fn key_use_fixed(module: &Module, f: &Function, k: ValueId, definer: bool, (bi, ii): Use) -> bool {
    let Some(ii) = ii else {
        return false;
    };
    let InstKind::Call(fid, args) = &f.blocks[bi].insts[ii].kind else {
        return false;
    };
    let callee = &module.funcs[fid.0 as usize];
    let only = |p: usize| {
        args.iter()
            .enumerate()
            .all(|(i, a)| (i == p) == (*a == Operand::Value(k)))
    };
    if callee.name == "__torajs_str_drop" {
        return only(0);
    }
    if definer || callee.name == GOPD {
        return false;
    }
    matches!(role(callee), Role::Reader | Role::Fresh) && only(1)
}
