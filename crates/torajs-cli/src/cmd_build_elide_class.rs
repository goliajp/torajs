//! r502 (RFC 20260824-s2-5 刀 4 A8) — the class register call as a
//! link-judged site.
//!
//! Every class declaration's prologue ends in
//! `__torajs_anyv_class_register(tag, class, …)`: it stashes the
//! class cell in the by-tag registry, marks the cell a constructor,
//! links its `[[Prototype]]` to `%Function.prototype%`, defines
//! `prototype.constructor`, and reifies one callable face per method
//! onto the prototype. Each of those steps roots a runtime world (the
//! builtin-prototype installer, the dynobj define machinery, the
//! method faces' any-lane twins) — on a class program that never
//! lets an instance or the class cell into the any world the call is
//! the single largest root (s3: 324 KB of text, 87 KB without it and
//! the twins).
//!
//! The effects are visible through exactly two channels, and the
//! judgment closes both:
//!
//! - the by-tag registry — read by `__torajs_anyv_class_get` /
//!   `__torajs_anyv_proto_get` / `__torajs_class_source_for_cell`
//!   (instance `.constructor`, `Object.getPrototypeOf`, `instanceof`
//!   through any, `new.target`, `C.toString()`), every one a torajs-
//!   meta entry: the link guard, exact and inline-proof. The
//!   prologue's own writeback reads (`class_cell_raw` /
//!   `proto_cell_raw`) are not readers — with the call assumed away
//!   the slot stays 0 and the writeback keeps the cell it minted
//!   (`emit_class_binding_writeback`'s select).
//! - the class and prototype cells themselves — the constructor mark,
//!   the locked slots, the link, the faces all live on those two
//!   dynobjs, and a program observes them only by reading the cells:
//!   `A.prototype`, `console.log(A)`, `const K: any = A`. That is a
//!   flow question the linker cannot see but the SSA answers
//!   exactly: the cells are minted in the prologue and handed to a
//!   short, enumerated list of register / link kernels; any other
//!   use (a member get, a store to a global or an env, a user-fn
//!   argument, a return) means the cell escapes and NO register site
//!   is offered. The walk is a taint closure over the fn's values,
//!   conservative in every unknown direction — an unlisted callee is
//!   an escape.
//!
//! A subclass prologue reads its parent's registry slot inside the
//! register kernel (`[[Prototype]](Sub) = Super`), so the sites are
//! all-or-nothing: either every class in the program keeps its cells
//! private and every register call is offered, or none is.
//!
//! Two more prologue calls write into the same closure and ride the
//! same guard: `__torajs_proto_link_fresh` (a subclass prototype's
//! `[[Prototype]]` link to its parent's — a dynobj define whose
//! own-write bookkeeping interns method names, 8 KB of torajs-rc on
//! the subclass fixture) and `__torajs_class_source_register` (the
//! declaration text `C.toString()` answers, read only by
//! `class_source_for_cell`).
//!
//! `TORAJS_CLASS_ELIDE_DIAG=1` prints the first escape to stderr.

use std::collections::{HashMap, HashSet};

use torajs_codegen::CompiledFunction;
use torajs_codegen::reloc::{CallTarget, RelocKind};
use torajs_core::ssa::{
    Function, InstKind, Module, Operand, Terminator, ValueId, visit_value_operands,
};
use torajs_link::exec::{ElidableSite, Guard, SiteShape};

const NOP: u32 = 0xD503_201F;
/// The prologue kernels whose every effect the guard's readers (or a
/// cell read the taint walk would have caught) observe, as the relocs
/// name them.
const PROLOGUE_SYMS: [&str; 3] = [
    "___torajs_anyv_class_register",
    "___torajs_proto_link_fresh",
    "___torajs_class_source_register",
];
#[cfg(test)]
const REGISTER_SYM: &str = PROLOGUE_SYMS[0];
/// The registry's readers (torajs-meta `classmeta`).
const REGISTRY_READERS: [&str; 3] = [
    "___torajs_anyv_class_get",
    "___torajs_anyv_proto_get",
    "___torajs_class_source_for_cell",
];

/// The prologue calls whose second operand mints the taint: the two
/// register kernels take the cell by value.
const SEEDS: [&str; 2] = [
    "__torajs_anyv_proto_register",
    "__torajs_anyv_class_register",
];
/// Kernels a private cell may be handed to without escaping: the
/// registers, the rc traffic, the ctor-side registries a reader can
/// only query with a cell it already holds, and the prologue's
/// prototype-chain links between private cells.
const SINKS: [&str; 10] = [
    "__torajs_anyv_proto_register",
    "__torajs_anyv_class_register",
    "__torajs_anyv_rc_dec",
    "__torajs_anyv_rc_inc",
    "__torajs_anyv_ctor_mark_arr_species",
    "__torajs_anyv_ctorany_register",
    "__torajs_anyv_ctor_register",
    "__torajs_proto_link_fresh",
    "__torajs_genfn_chain",
    "__torajs_proto_chain_builtin",
];
/// Kernels whose result is the cell again (box / unbox).
const PASS_THROUGH: [&str; 4] = [
    "__torajs_anyv_box_from_pair",
    "__torajs_anyv_unbox_tag",
    "__torajs_anyv_unbox_value",
    "__torajs_anyv_unbox_value_owned",
];
/// The object-literal store: a tainted value written into the
/// literal's init slot taints the literal (the class cell's
/// `prototype` entry holds the prototype cell).
const CONTAINER_STORE: &str = "__torajs_dynobj_set_fresh";

/// Every `bl` to a prologue kernel ([`PROLOGUE_SYMS`]) in any live
/// fn, offered only when no class cell escapes its prologue.
pub(crate) fn class_register_sites(
    funcs: &[CompiledFunction],
    module: &Module,
) -> Vec<ElidableSite> {
    if !class_cells_stay_private(module) {
        return Vec::new();
    }
    let guard = Guard::Symbols(REGISTRY_READERS.iter().map(|s| (*s).to_string()).collect());
    funcs
        .iter()
        .filter(|f| !f.bytes.is_empty())
        .flat_map(|f| {
            f.relocs.iter().filter_map(|r| {
                let RelocKind::CallSite {
                    target: CallTarget::Extern(name),
                } = &r.kind
                else {
                    return None;
                };
                PROLOGUE_SYMS
                    .contains(&name.as_str())
                    .then(|| ElidableSite {
                        func: f.name.clone(),
                        guard: guard.clone(),
                        shape: SiteShape::Call {
                            byte_offset: r.byte_offset,
                            replacement: NOP,
                        },
                    })
            })
        })
        .collect()
}

/// Do every class's cells stay inside their prologue, in every fn?
fn class_cells_stay_private(module: &Module) -> bool {
    let diag = std::env::var_os("TORAJS_CLASS_ELIDE_DIAG").is_some();
    module.funcs.iter().all(|f| match first_escape(module, f) {
        None => true,
        Some(why) => {
            if diag {
                eprintln!("[class-elide] escape in {}: {why}", f.name);
            }
            false
        }
    })
}

fn callee_name<'m>(module: &'m Module, kind: &InstKind) -> Option<&'m str> {
    match kind {
        InstKind::Call(fid, _) => Some(module.funcs[fid.0 as usize].name.as_str()),
        _ => None,
    }
}

/// The taint closure over one fn: seeds are the cells the register
/// kernels take; taint flows forward through every derivation and
/// backward through the aliases (a box's payload, a slot a tainted
/// load came from, a value stored into a tainted slot). Answers the
/// first use a private cell must not have, `None` when there is
/// none.
fn first_escape(module: &Module, f: &Function) -> Option<String> {
    let defs: HashMap<ValueId, &InstKind> = f
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .filter_map(|i| i.result.map(|r| (r, &i.kind)))
        .collect();
    let mut tainted: HashSet<ValueId> = HashSet::new();
    for b in &f.blocks {
        for i in &b.insts {
            if let InstKind::Call(_, args) = &i.kind
                && callee_name(module, &i.kind).is_some_and(|n| SEEDS.contains(&n))
                && let Some(Operand::Value(cell)) = args.get(1)
            {
                tainted.insert(*cell);
            }
        }
    }
    if tainted.is_empty() {
        return None;
    }
    let is_slot = |v: &Operand| match v {
        Operand::Value(p) => matches!(
            defs.get(p),
            Some(InstKind::Alloca(_) | InstKind::AllocaBytes(_))
        ),
        _ => false,
    };
    loop {
        let before = tainted.len();
        for b in &f.blocks {
            for i in &b.insts {
                let mut reads = Vec::new();
                visit_value_operands(&i.kind, |v| reads.push(v));
                let read_tainted = reads.iter().any(|v| tainted.contains(v));
                let result_tainted = i.result.is_some_and(|r| tainted.contains(&r));
                match &i.kind {
                    InstKind::Call(_, args) => {
                        let name = callee_name(module, &i.kind).unwrap_or("");
                        if read_tainted {
                            if PASS_THROUGH.contains(&name) {
                                if let Some(r) = i.result {
                                    tainted.insert(r);
                                }
                            } else if name == CONTAINER_STORE {
                                if let Some(Operand::Value(slot)) = args.first() {
                                    tainted.insert(*slot);
                                }
                            } else if !SINKS.contains(&name) {
                                return Some(format!("handed to `{name}`"));
                            }
                        }
                        if result_tainted && PASS_THROUGH.contains(&name) {
                            tainted.extend(reads.iter().copied());
                        }
                    }
                    InstKind::CallIndirect(..) if read_tainted => {
                        return Some("handed to an indirect call".into());
                    }
                    InstKind::Store(val, ptr, _) => {
                        let val_tainted = matches!(val, Operand::Value(v) if tainted.contains(v));
                        let ptr_tainted = matches!(ptr, Operand::Value(p) if tainted.contains(p));
                        if val_tainted {
                            if is_slot(ptr) {
                                if let Operand::Value(p) = ptr {
                                    tainted.insert(*p);
                                }
                            } else if !ptr_tainted {
                                return Some("stored through a non-local pointer".into());
                            }
                        }
                        if ptr_tainted && let Operand::Value(v) = val {
                            tainted.insert(*v);
                        }
                    }
                    InstKind::StoreDyn(val, ..) | InstKind::StoreDynScaled8(val, ..) if matches!(val, Operand::Value(v) if tainted.contains(v)) =>
                    {
                        return Some("stored through a dynamic index".into());
                    }
                    InstKind::ICmp(..) | InstKind::FCmp(..) => {}
                    InstKind::Load(_, ptr, _) => {
                        if read_tainted && let Some(r) = i.result {
                            tainted.insert(r);
                        }
                        if result_tainted && let Operand::Value(p) = ptr {
                            tainted.insert(*p);
                        }
                    }
                    InstKind::Select(_, _, t, e) => {
                        if read_tainted && let Some(r) = i.result {
                            tainted.insert(r);
                        }
                        if result_tainted {
                            for o in [t, e] {
                                if let Operand::Value(v) = o {
                                    tainted.insert(*v);
                                }
                            }
                        }
                    }
                    _ => {
                        if read_tainted && let Some(r) = i.result {
                            tainted.insert(r);
                        }
                        if result_tainted {
                            tainted.extend(reads.iter().copied());
                        }
                    }
                }
            }
            if let Terminator::Ret(Some(Operand::Value(v))) = &b.term
                && tainted.contains(v)
            {
                return Some("returned".into());
            }
        }
        if tainted.len() == before {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_codegen::frame::FrameLayout;
    use torajs_codegen::reloc::Reloc;
    use torajs_core::ssa::{BlockId, Type};

    fn f(name: &str, callees: &[&str]) -> CompiledFunction {
        CompiledFunction {
            name: name.into(),
            bytes: vec![0; 4 * callees.len()],
            relocs: callees
                .iter()
                .enumerate()
                .map(|(i, c)| Reloc {
                    byte_offset: (4 * i) as u32,
                    kind: RelocKind::CallSite {
                        target: CallTarget::Extern((*c).into()),
                    },
                })
                .collect(),
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    /// A module whose `main` mints a class cell, registers it, and
    /// then does `extra` with it.
    fn module_with(extra: impl FnOnce(&mut Function, ValueId)) -> Module {
        let mut m = Module::default();
        let register = m.add_function(Function::new("__torajs_anyv_class_register", Type::Void));
        let rc_dec = m.add_function(Function::new("__torajs_anyv_rc_dec", Type::Void));
        let get_tag = m.add_function(Function::new("__torajs_any_member_get_tag", Type::I64));
        let _ = (rc_dec, get_tag);
        let mut main = Function::new("main", Type::I32);
        let bb = main.add_block();
        let cell = main.append_inst(bb, InstKind::Alloca(Type::Any), Type::Any, None);
        main.append_void(
            bb,
            InstKind::Call(
                register,
                vec![
                    Operand::ConstI64(1),
                    Operand::Value(cell),
                    Operand::ConstI64(0),
                ],
            ),
        );
        extra(&mut main, cell);
        main.blocks[bb.0 as usize].term = Terminator::Ret(Some(Operand::ConstI32(0)));
        m.add_function(main);
        m
    }

    #[test]
    fn a_private_cell_offers_every_register_site() {
        let m = module_with(|main, cell| {
            let bb = BlockId(0);
            main.append_void(
                bb,
                InstKind::Call(torajs_core::ssa::FuncId(1), vec![Operand::Value(cell)]),
            );
        });
        let funcs = vec![
            f(
                "_main_user",
                &[
                    "___torajs_print_i64",
                    REGISTER_SYM,
                    "___torajs_proto_link_fresh",
                    "___torajs_class_source_register",
                ],
            ),
            f("helper", &[REGISTER_SYM]),
        ];
        let sites = class_register_sites(&funcs, &m);
        assert_eq!(sites.len(), 4, "every prologue kernel call is a site");
        assert_eq!(sites[0].func, "_main_user");
        assert_eq!(sites[3].func, "helper");
        assert_eq!(
            sites[0].guard.to_string(),
            "syms:___torajs_anyv_class_get|___torajs_anyv_proto_get|___torajs_class_source_for_cell"
        );
        let SiteShape::Call {
            byte_offset,
            replacement,
        } = sites[0].shape
        else {
            panic!("a call site");
        };
        assert_eq!((byte_offset, replacement), (4, NOP));
    }

    #[test]
    fn a_member_read_of_the_cell_offers_nothing() {
        let m = module_with(|main, cell| {
            let bb = BlockId(0);
            let boxed = main.append_inst(
                bb,
                InstKind::Copy(Type::Any, Operand::Value(cell)),
                Type::Any,
                None,
            );
            main.append_inst(
                bb,
                InstKind::Call(torajs_core::ssa::FuncId(2), vec![Operand::Value(boxed)]),
                Type::I64,
                None,
            );
        });
        assert!(class_register_sites(&[f("_main_user", &[REGISTER_SYM])], &m).is_empty());
    }

    #[test]
    fn a_cell_stored_to_a_global_offers_nothing() {
        let m = module_with(|main, cell| {
            let bb = BlockId(0);
            let g = main.append_inst(
                bb,
                InstKind::GlobalRef("__torajs_g".into()),
                Type::Ptr,
                None,
            );
            main.append_void(
                bb,
                InstKind::Store(Operand::Value(cell), Operand::Value(g), 0),
            );
        });
        assert!(class_register_sites(&[f("_main_user", &[REGISTER_SYM])], &m).is_empty());
    }

    #[test]
    fn a_cell_parked_in_a_local_slot_stays_private() {
        let m = module_with(|main, cell| {
            let bb = BlockId(0);
            let slot = main.append_inst(bb, InstKind::Alloca(Type::Any), Type::Ptr, None);
            main.append_void(
                bb,
                InstKind::Store(Operand::Value(cell), Operand::Value(slot), 0),
            );
            let back = main.append_inst(
                bb,
                InstKind::Load(Type::Any, Operand::Value(slot), 0),
                Type::Any,
                None,
            );
            main.append_void(
                bb,
                InstKind::Call(torajs_core::ssa::FuncId(1), vec![Operand::Value(back)]),
            );
        });
        assert_eq!(
            class_register_sites(&[f("_main_user", &[REGISTER_SYM])], &m).len(),
            1
        );
    }
}
