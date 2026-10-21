//! Whether a builtin constructor cell can reach the any world: through
//! a `constructor` read, a runtime-keyed property read, or a class whose
//! heritage is a builtin constructor (its [[Prototype]] is that cell).
//!
//! A builtin constructor value owns the namespace statics of its
//! family (`Object.keys`, `Array.of`, `Number.isInteger`), and a
//! static called off it (`O.keys(x)` with `O = o.constructor`) mints
//! its cell through the ns-static seam. The compiler-emitted ways to
//! hold such a value — the bare `Object` ident read, a namespace or
//! globalThis object — are visible to the scan by symbol. The
//! runtime hands one out from inside too: every builtin prototype has
//! an own `constructor` (§20.1.3.1 and its family), so a read of that
//! key off any object, array, number, string or function answers the
//! interned constructor cell without the user `.o` naming any
//! constructor symbol.
//!
//! Such a read needs the key `"constructor"` as a runtime string. A
//! program either spells it (a string literal instruction, whatever
//! syntax carried it: `o.constructor`, `o["constructor"]`,
//! `Reflect.get(o, "constructor")`, a destructuring pattern), or
//! computes a key at run time and reads with it, or reads the
//! descriptor bag that holds the value under that key. The scan
//! answers true for the last two, and for a spelled key unless every
//! answer it reads stays with benign uses
//! ([`crate::cmd_build_dispatch_judge_ctor_uses`]).
//!
//! The other runtime handout is a class heritage: `class A extends
//! Array` links the class object's [[Prototype]] to the interned
//! Array cell at register time (§15.7.14 step 8.a), so
//! `Object.getPrototypeOf(A)`, `Reflect.getPrototypeOf(A)`,
//! `A.__proto__` and an inherited static read (`(A as any).of(1)`)
//! all answer it with no `constructor` key anywhere. The register
//! call names the builtin parent's tag as a constant, so a
//! non-negative one keeps the seam. The prototype readers themselves
//! need no entry: a builtin cell becomes some object's [[Prototype]]
//! only through that link or through a ctor value the program
//! already holds (the typed-array ctors' own %TypedArray% parent
//! included).

use std::collections::HashSet;

use torajs_core::ssa::{InstKind, Module, Operand, Type};

use crate::cmd_build_dispatch_judge_ctor_uses::KEYED_READERS;

/// Readers that hand out every own property or answer a constructor
/// directly — any call may surface a constructor cell.
const OPEN_READERS: [&str; 2] = [
    "__torajs_anyv_get_property_descriptors",
    // typed `f.constructor` on a closure answers the builtin
    // Function / AsyncFunction cell through its own entry
    "__torajs_closure_ctor_value",
];

/// The class register entry and the operand index of its builtin
/// parent tag (`-1` when the heritage is not a builtin).
const CLASS_REGISTER: (&str, usize) = ("__torajs_anyv_class_register", 4);

/// True when the program can read a builtin constructor cell off a
/// value (see the module doc). `diag` names the first reason.
pub(crate) fn ctor_cell_reachable(module: &Module, live: &[bool], diag: bool) -> bool {
    let ctor_ids = crate::cmd_build_dispatch_judge_ctor_uses::ctor_key_ids(module);
    for f in crate::cmd_build_dispatch_judge_callee::live_funcs(module, live) {
        if let Some(why) =
            crate::cmd_build_dispatch_judge_ctor_uses::ctor_read_escape(module, f, &ctor_ids)
        {
            if diag {
                eprintln!(
                    "[judge] ns-world: {} reads a constructor into {why}",
                    f.name
                );
            }
            return true;
        }
        // values that cannot be a runtime "constructor": string
        // literals (judged above), boxed scalars (`box_any` tags other
        // than 4, the heap pointer), and symbols and their boxes (a
        // symbol key is its own property key, never a string)
        let symbol =
            |op: &Operand| matches!(op, Operand::Value(v) if f.value_type(*v) == Type::Symbol);
        let mut fixed_keys = HashSet::new();
        for b in &f.blocks {
            for inst in &b.insts {
                let fixed = match &inst.kind {
                    InstKind::StaticStrRef(_) | InstKind::StringRef(_) => true,
                    InstKind::BoxAny(tag, v) => *tag != 4 || symbol(v),
                    InstKind::Call(fid, args) => {
                        module.funcs[fid.0 as usize].name == "__torajs_anyv_box_from_pair"
                            && args.get(1).is_some_and(symbol)
                    }
                    _ => false,
                };
                if let (true, Some(r)) = (fixed, inst.result) {
                    fixed_keys.insert(r);
                }
            }
        }
        for b in &f.blocks {
            for inst in &b.insts {
                let InstKind::Call(fid, args) = &inst.kind else {
                    continue;
                };
                let name = module.funcs[fid.0 as usize].name.as_str();
                if name == CLASS_REGISTER.0
                    && !matches!(args.get(CLASS_REGISTER.1), Some(Operand::ConstI64(t)) if *t < 0)
                {
                    if diag {
                        eprintln!("[judge] ns-world: {} registers a builtin heir", f.name);
                    }
                    return true;
                }
                let open = OPEN_READERS.contains(&name)
                    || (KEYED_READERS.contains(&name)
                        && !args.get(1).is_some_and(|k| {
                            symbol(k) || matches!(k, Operand::Value(v) if fixed_keys.contains(v))
                        }));
                if open {
                    if diag {
                        eprintln!("[judge] ns-world: {} calls {name}", f.name);
                    }
                    return true;
                }
            }
        }
    }
    false
}
