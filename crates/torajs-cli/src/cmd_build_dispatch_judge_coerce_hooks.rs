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
//! (`setPrototypeOf`, a `__proto__` write) so the chain runs through a
//! program object. The scan is whole-program and receiver-blind: any
//! such write anywhere answers false, whatever object it lands on.
//! Writes that cannot install one are a fixed key that names none of
//! the hooks, an integer index, a fresh object's own initializer (an
//! object literal, a class body: the object is not on a builtin chain
//! until a relink puts it there), and a run-time key whose stored
//! value is provably a primitive (a primitive is never callable, and
//! a `__proto__` write of one changes nothing, B.2.2.1.2 step 4).
//!
//! A callee is classed by its name. The entries that can store under
//! a key or relink a prototype all carry one of [`INSTALLER_MARKS`];
//! a marked entry no row below admits answers false.

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, DOUBLE_ENCODE_OFFSET, FPred, Function, InstKind, Module, Operand, TAG_TYPE_NUMBER,
    Terminator, ValueId,
};

use crate::cmd_build_dispatch_judge_ctor_uses::{KEYED_READERS, Use, use_lists};

/// The keys a hook is installed under, and the Annex B definers whose
/// method names reach the dispatch as keys.
const HOOK_KEYS: [&[u8]; 5] = [
    b"valueOf",
    b"toString",
    b"__proto__",
    b"__defineGetter__",
    b"__defineSetter__",
];

/// Name pieces of every entry that can store under a key or relink a
/// prototype.
const INSTALLER_MARKS: [&str; 8] = [
    "_set", "set_", "define", "assign", "proto", "install", "link", "eval",
];

/// Marked entries that store into a collection, a Date's time value or
/// a RegExp's `lastIndex`, raise or record a throw, release a decode or
/// read the class prototype table — no property key —, the fixed keys
/// the injected Error family writes (`name` / `message` on a
/// prototype, `cause` on an instance), and the integer-indexed any
/// writes (an array index is never a hook key).
const INERT: [&str; 13] = [
    "__torajs_throw_",
    "__torajs_map_",
    "__torajs_set_",
    "__torajs_weakmap_",
    "__torajs_weakset_",
    "__torajs_date_set_",
    "__torajs_regex_set_last_index",
    "__torajs_anyv_unbox_settle",
    "__torajs_proto_cell_raw",
    "__torajs_error_proto_install",
    "__torajs_error_install_cause",
    "__torajs_any_index_set",
    "__torajs_any_index_set_soft",
];

/// Initializers of a fresh object: the computed-key members of an
/// object literal, the members of a class body, and the class
/// prologue's prototype link (both ends minted by the prologue, the
/// link following the class tree) and prototype table entry.
const FRESH: [&str; 5] = [
    "__torajs_dynobj_set_fresh",
    "__torajs_dynobj_set_fresh_dup",
    "__torajs_class_",
    "__torajs_proto_link_fresh",
    "__torajs_anyv_proto_register",
];

/// Keyed writes (receiver, key at operand 1), with the operand of the
/// stored value when it is one `any`.
const KEYED_WRITERS: [(&str, Option<usize>); 12] = [
    ("__torajs_any_member_set", None),
    ("__torajs_any_member_set_soft", None),
    ("__torajs_any_member_set_priv", None),
    ("__torajs_any_member_set_with_receiver", None),
    ("__torajs_dynobj_set", None),
    ("__torajs_dynobj_define", None),
    // `defineProperty(o, key, desc)`: an accessor under a fixed key
    // other than a hook's is never Got by ToPrimitive
    ("__torajs_dynobj_define_from_desc", None),
    ("__torajs_dynobj_define_from_desc_soft", None),
    ("__torajs_arr_define", None),
    ("__torajs_any_index_method_call", None),
    ("__torajs_dynobj_dict_set", Some(2)),
    ("__torajs_dynobj_dict_set_entry", Some(2)),
];

/// The by-name method calls: operand 1 is the method id.
const METHOD_CALLS: [&str; 3] = [
    "__torajs_any_method_call",
    "__torajs_any_method_call_opt",
    "__torajs_any_method_call_spread",
];

/// Entries whose answer is always a primitive: the `+` of two decoded
/// operands answers a number, a string or a bigint.
const PRIMITIVE_ANSWERS: [&str; 1] = ["__torajs_anyv_add_pair"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Role {
    Inert,
    Fresh,
    Keyed(Option<usize>),
    MethodCall,
    Installer,
}

fn role(name: &str) -> Role {
    if METHOD_CALLS.contains(&name) {
        return Role::MethodCall;
    }
    if let Some(&(_, value)) = KEYED_WRITERS.iter().find(|(n, _)| *n == name) {
        return Role::Keyed(value);
    }
    if FRESH.iter().any(|p| name.starts_with(p)) {
        return Role::Fresh;
    }
    if INERT
        .iter()
        .any(|p| name == *p || (p.ends_with('_') && name.starts_with(p)))
    {
        return Role::Inert;
    }
    let marked = |n: &str| INSTALLER_MARKS.iter().any(|m| n.contains(m));
    // a reader that only names a prototype (`proto_get`, `get_proto_of`)
    if !marked(name) || (name.contains("get") && !marked(&name.replace("proto", ""))) {
        return Role::Inert;
    }
    Role::Installer
}

/// The memo of [`hooks_fixed`] for one judgment: worked out on first
/// ask, since only a coercing use of a constructor answer asks.
#[derive(Default)]
pub(crate) struct HookPremise(OnceCell<bool>);

impl HookPremise {
    pub(crate) fn holds(&self, module: &Module, live: &[bool], diag: bool) -> bool {
        *self.0.get_or_init(|| hooks_fixed(module, live, diag))
    }
}

/// True when no live function can install a ToPrimitive hook (see the
/// module doc). `diag` names the first write that can.
pub(crate) fn hooks_fixed(module: &Module, live: &[bool], diag: bool) -> bool {
    let hook_ids: HashSet<u32> = (0..module.strings.len())
        .filter(|&i| HOOK_KEYS.contains(&module.strings[i].bytes.as_slice()))
        .map(|i| i as u32)
        .collect();
    let mut roles: Vec<Option<Role>> = vec![None; module.funcs.len()];
    for f in crate::cmd_build_dispatch_judge_callee::live_funcs(module, live) {
        if let Err(why) = function_fixed(module, f, &hook_ids, &mut roles) {
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
    hook_ids: &HashSet<u32>,
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
    let callee = |fid: torajs_core::ssa::FuncId| module.funcs[fid.0 as usize].name.as_str();
    for inst in f.blocks.iter().flat_map(|b| &b.insts) {
        let InstKind::Call(fid, args) = &inst.kind else {
            continue;
        };
        let name = callee(*fid);
        let role = *roles[fid.0 as usize].get_or_insert_with(|| role(name));
        let literal_key = matches!(args.get(1), Some(Operand::Value(k)) if lits.contains_key(k));
        let fixed = match role {
            Role::Inert | Role::Fresh => true,
            Role::MethodCall => matches!(
                args.get(1),
                Some(Operand::ConstI64(m))
                    if *m != torajs_rc::ANY_METHOD_DEFINE_GETTER
                        && *m != torajs_rc::ANY_METHOD_DEFINE_SETTER
            ),
            Role::Keyed(value) => {
                literal_key
                    || value.is_some_and(|at| {
                        let defs = defs.get_or_init(|| Defs::of(f));
                        args.get(at).is_some_and(|v| defs.primitive(module, f, v))
                    })
            }
            Role::Installer => false,
        };
        if !fixed {
            return Err(format!("calls {name}"));
        }
    }
    let hook_lits: Vec<ValueId> = lits
        .iter()
        .filter(|(_, s)| hook_ids.contains(s))
        .map(|(v, _)| *v)
        .collect();
    if hook_lits.is_empty() {
        return Ok(());
    }
    let uses = use_lists(f);
    for k in hook_lits {
        for &u in uses.get(&k).map_or(&[][..], Vec::as_slice) {
            if !hook_key_use_fixed(module, f, k, u) {
                return Err("uses a hook key".into());
            }
        }
    }
    Ok(())
}

/// A use of a hook-key literal that installs nothing: the key of a
/// read, of `in`, of a fresh object's initializer, or a drop.
fn hook_key_use_fixed(module: &Module, f: &Function, k: ValueId, (bi, ii): Use) -> bool {
    let Some(ii) = ii else {
        return false;
    };
    let InstKind::Call(fid, args) = &f.blocks[bi].insts[ii].kind else {
        return false;
    };
    let name = module.funcs[fid.0 as usize].name.as_str();
    let only = |p: usize| {
        args.iter()
            .enumerate()
            .all(|(i, a)| (i == p) == (*a == Operand::Value(k)))
    };
    (KEYED_READERS.contains(&name) && only(1))
        || (name == "__torajs_in_op_any_str" && only(1))
        || (name == "__torajs_str_drop" && only(0))
        || (role(name) == Role::Fresh && only(1))
}

/// Every definition of each value of a function: the IR keeps a
/// φ-merged value as one id defined by a `copy` on each predecessor.
struct Defs {
    defs: HashMap<ValueId, Vec<(usize, usize)>>,
}

impl Defs {
    fn of(f: &Function) -> Self {
        let mut defs: HashMap<ValueId, Vec<(usize, usize)>> = HashMap::new();
        for (bi, b) in f.blocks.iter().enumerate() {
            for (ii, inst) in b.insts.iter().enumerate() {
                if let Some(r) = inst.result {
                    defs.entry(r).or_default().push((bi, ii));
                }
            }
        }
        Self { defs }
    }

    /// True when `v` is a primitive on every definition.
    fn primitive(&self, module: &Module, f: &Function, v: &Operand) -> bool {
        let Operand::Value(v) = v else {
            return false;
        };
        let mut seen = HashSet::new();
        self.primitive_value(module, f, *v, &mut seen)
    }

    /// A copy cycle adds no value of its own, so a value already on
    /// the walk answers true.
    fn primitive_value(
        &self,
        module: &Module,
        f: &Function,
        v: ValueId,
        seen: &mut HashSet<ValueId>,
    ) -> bool {
        if !seen.insert(v) {
            return true;
        }
        let Some(sites) = self.defs.get(&v) else {
            return false;
        };
        sites.iter().all(|&(bi, ii)| {
            match &f.blocks[bi].insts[ii].kind {
                InstKind::Copy(_, Operand::Value(src)) => {
                    self.primitive_value(module, f, *src, seen)
                }
                // tag 4 is the heap pointer; every other tag encodes a
                // primitive (a double through the canonicalizing box)
                InstKind::BoxAny(tag, _) => *tag != 4,
                InstKind::BoxInt32(_) | InstKind::BoxF64(_) => true,
                InstKind::IntToPtr(bits) => self.number_bits(f, bits),
                InstKind::Call(fid, _) => {
                    PRIMITIVE_ANSWERS.contains(&module.funcs[fid.0 as usize].name.as_str())
                }
                _ => false,
            }
        })
    }

    /// True when `bits` encode a number: a
    /// constant outside the cell range, an int32 under the number tag,
    /// or a double's bits plus the encode offset on the arm where the
    /// double is not a NaN (only a NaN payload wraps into the cell
    /// range, which is why the box tests for one).
    fn number_bits(&self, f: &Function, bits: &Operand) -> bool {
        const TAG: i64 = TAG_TYPE_NUMBER as i64;
        const OFFSET: i64 = DOUBLE_ENCODE_OFFSET as i64;
        let v = match bits {
            Operand::ConstI64(c) => return (*c as u64) & TAG_TYPE_NUMBER != 0,
            Operand::Value(v) => *v,
            _ => return false,
        };
        let Some(&[(bi, ii)]) = self.defs.get(&v).map(Vec::as_slice) else {
            return false;
        };
        match &f.blocks[bi].insts[ii].kind {
            InstKind::BinOp(BinOp::Or, _, Operand::ConstI64(t)) => t & TAG == TAG,
            InstKind::BinOp(BinOp::Add, Operand::Value(raw), Operand::ConstI64(OFFSET)) => {
                self.not_nan_arm(f, bi, *raw)
            }
            _ => false,
        }
    }

    /// True when `raw` is `bitcast` of a double that block `bi` holds
    /// only on the false arm of that double's `f != f` test.
    fn not_nan_arm(&self, f: &Function, bi: usize, raw: ValueId) -> bool {
        let Some(&[(rb, ri)]) = self.defs.get(&raw).map(Vec::as_slice) else {
            return false;
        };
        let InstKind::BitCastF64ToI64(Operand::Value(x)) = &f.blocks[rb].insts[ri].kind else {
            return false;
        };
        let id = f.blocks[bi].id;
        let mut preds = f.blocks.iter().filter(|b| match &b.term {
            Terminator::Br(t) => *t == id,
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => *then_blk == id || *else_blk == id,
            _ => false,
        });
        let (Some(p), None) = (preds.next(), preds.next()) else {
            return false;
        };
        let Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            else_blk,
        } = &p.term
        else {
            return false;
        };
        if *else_blk != id || *then_blk == id {
            return false;
        }
        let Some(&[(cb, ci)]) = self.defs.get(c).map(Vec::as_slice) else {
            return false;
        };
        let nan_test = Operand::Value(*x);
        matches!(
            &f.blocks[cb].insts[ci].kind,
            InstKind::FCmp(FPred::Une, a, b) if *a == nan_test && *b == nan_test
        )
    }
}
