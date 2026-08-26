//! Closure any-world escape judgment + `__boxed_` adapter address
//! strip (RFC 20260824-s2-5 刀 4 Phase B, A1).
//!
//! Every lifted closure and top-level fn value gets a synthesized
//! `__boxed_<name>(env, argv, argc) -> AnyValue` adapter
//! (`ssa_lower_boxed_entry`), and the closure mint stores its address
//! at `CLOSURE_BOXED_ENTRY_OFF` so the runtime's dynamic call sites
//! (a dynobj field holding a fn, an any-lane HOF, `.call/.apply`,
//! reflection) can invoke the typed body through one uniform ABI.
//! The adapter unboxes every argument per the body's static
//! parameter type — a `number` parameter is `__torajs_anyv_to_number`,
//! ES ToNumber, which on a heap receiver runs OrdinaryToPrimitive
//! through the generic method dispatcher. That single reloc roots
//! the whole any world: Phase A measured a program whose only closure
//! is called directly (`const f = (x: number) => x * 2; f(3)`) at
//! 397 KB against 84 KB for the same program without the closure.
//!
//! The adapter only ever runs when the closure cell is invoked FROM
//! the any world, and a closure can only get there by crossing into
//! an `any`-typed slot — which, at the SSA level, is a runtime
//! intrinsic call taking a `Closure` / `FnSig`-typed operand (the
//! NaN-box encoder, a dynobj / array / struct-field store, a promise
//! reaction, an accessor face registration, …). Typed-lane consumers
//! of a closure value that never hand it to the any world are the
//! closed [`TYPED_LANE_CONSUMERS`] list (refcount traffic, the drop
//! and cycle-buffer hooks); every other intrinsic taking a closure
//! operand is an escape. User-fn calls are typed (an `any` parameter
//! would have boxed the argument first), and a store of a closure
//! value into memory is a typed slot for the same reason.
//!
//! When no closure escapes, the mint stores `0` instead of the
//! adapter address — the pre-existing "no adapter" shape, which the
//! runtime dispatcher answers with a catchable TypeError should the
//! judgment ever be wrong (loud, never silent) — and the orphaned
//! `FnAddr` goes with it, so user-gc strips the adapter and its
//! coercion relocs stop being evidence for the dispatch judgment
//! (`cmd_build_dispatch_judge`). Class-method adapters are rooted by
//! the class-layout tables and stay (their judgment is the class
//! world's, A4).
//!
//! `TORAJS_CLOSURE_ESCAPE_OFF=1` disables the strip (A/B pricing);
//! `TORAJS_CLOSURE_ESCAPE_DIAG=1` prints every escape site and the
//! verdict to stderr.

use crate::ssa::{FuncId, InstKind, Module, Operand, Type, visit_value_operands};
use crate::ssa_lower::CLOSURE_BOXED_ENTRY_OFF;

/// Runtime intrinsics that take a closure operand without exposing
/// it to the any world: refcount traffic on the cell, the drop /
/// cycle-buffer hooks on the way out. Anything not listed is an
/// escape — the safe direction.
const TYPED_LANE_CONSUMERS: [&str; 7] = [
    "__torajs_rc_inc",
    "__torajs_rc_dec",
    "__torajs_value_drop_heap",
    "__torajs_obj_drop_sized",
    "__torajs_cycle_buffer",
    "__torajs_cycle_unbuffer",
    "__torajs_weakref_target_dying",
];

/// One closure-typed operand handed to a runtime intrinsic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscapeSite {
    pub in_fn: String,
    pub callee: String,
    pub arg_index: usize,
}

fn is_closure_ty(ty: Type) -> bool {
    matches!(ty, Type::Closure(_) | Type::FnSig(_))
}

/// Every intrinsic call in the module that hands a closure / fn
/// value to a runtime entry outside [`TYPED_LANE_CONSUMERS`].
pub fn closure_escape_sites(module: &Module) -> Vec<EscapeSite> {
    let mut sites = Vec::new();
    for f in &module.funcs {
        for b in &f.blocks {
            for inst in &b.insts {
                let InstKind::Call(fid, args) = &inst.kind else {
                    continue;
                };
                let callee = &module.funcs[fid.0 as usize];
                if !callee.is_declaration() || !callee.name.starts_with("__torajs_") {
                    continue;
                }
                if TYPED_LANE_CONSUMERS.contains(&callee.name.as_str()) {
                    continue;
                }
                for (i, a) in args.iter().enumerate() {
                    if let Operand::Value(v) = a
                        && is_closure_ty(f.values[v.0 as usize].ty)
                    {
                        sites.push(EscapeSite {
                            in_fn: f.name.clone(),
                            callee: callee.name.clone(),
                            arg_index: i,
                        });
                    }
                }
            }
        }
    }
    sites
}

/// Judge and, when no closure escapes, neuter every `__boxed_`
/// adapter address store at the closure mints. Answers the number
/// of stores rewritten (`None` = escape found or pass disabled).
pub fn strip_boxed_entries_if_closed(module: &mut Module) -> Option<usize> {
    let diag = std::env::var_os("TORAJS_CLOSURE_ESCAPE_DIAG").is_some();
    if std::env::var_os("TORAJS_CLOSURE_ESCAPE_OFF").is_some_and(|v| v == "1") {
        return None;
    }
    let sites = closure_escape_sites(module);
    if diag {
        for s in &sites {
            eprintln!(
                "[closure-escape] {} hands a closure to {} (arg {})",
                s.in_fn, s.callee, s.arg_index
            );
        }
    }
    if !sites.is_empty() {
        if diag {
            eprintln!(
                "[closure-escape] verdict: ESCAPES ({} sites) — adapters kept",
                sites.len()
            );
        }
        return None;
    }
    let n = strip_boxed_entries(module);
    if diag {
        eprintln!("[closure-escape] verdict: CLOSED — {n} adapter address stores neutered");
    }
    Some(n)
}

/// Is `fid` a synthesized any-ABI adapter?
fn is_boxed_adapter(module: &Module, fid: FuncId) -> bool {
    module.funcs[fid.0 as usize].name.starts_with("__boxed_")
}

/// Rewrite `Store(FnAddr(__boxed_*), env, CLOSURE_BOXED_ENTRY_OFF)`
/// to store `0`, then drop the now-unused `FnAddr` instructions.
fn strip_boxed_entries(module: &mut Module) -> usize {
    let mut rewritten = 0usize;
    for fi in 0..module.funcs.len() {
        // FnAddr results that name an adapter, by value id.
        let mut adapter_vals: Vec<(u32, FuncId)> = Vec::new();
        for b in &module.funcs[fi].blocks {
            for inst in &b.insts {
                if let (InstKind::FnAddr(target), Some(r)) = (&inst.kind, inst.result)
                    && is_boxed_adapter(module, *target)
                {
                    adapter_vals.push((r.0, *target));
                }
            }
        }
        if adapter_vals.is_empty() {
            continue;
        }
        let is_adapter_val = |v: u32| adapter_vals.iter().any(|&(x, _)| x == v);
        let f = &mut module.funcs[fi];
        for b in &mut f.blocks {
            for inst in &mut b.insts {
                if let InstKind::Store(val, _, off) = &mut inst.kind
                    && *off == CLOSURE_BOXED_ENTRY_OFF
                    && matches!(val, Operand::Value(v) if is_adapter_val(v.0))
                {
                    *val = Operand::ConstI64(0);
                    rewritten += 1;
                }
            }
        }
        // An adapter address with no remaining use is dead; a use
        // that survives (a class-layout row, a table) keeps its
        // FnAddr — those adapters are rooted elsewhere anyway.
        let mut used = vec![false; adapter_vals.len()];
        for b in &f.blocks {
            for inst in &b.insts {
                visit_value_operands(&inst.kind, |v| {
                    if let Some(i) = adapter_vals.iter().position(|&(x, _)| x == v.0) {
                        used[i] = true;
                    }
                });
            }
            match &b.term {
                crate::ssa::Terminator::CondBr {
                    cond: Operand::Value(v),
                    ..
                }
                | crate::ssa::Terminator::Ret(Some(Operand::Value(v))) => {
                    if let Some(i) = adapter_vals.iter().position(|&(x, _)| x == v.0) {
                        used[i] = true;
                    }
                }
                _ => {}
            }
        }
        for b in &mut f.blocks {
            b.insts.retain(|inst| {
                !(matches!(inst.kind, InstKind::FnAddr(_))
                    && inst.result.is_some_and(|r| {
                        adapter_vals
                            .iter()
                            .position(|&(x, _)| x == r.0)
                            .is_some_and(|i| !used[i])
                    }))
            });
        }
    }
    rewritten
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssa::{Block, BlockId, Function, Inst, Terminator, ValueId, ValueInfo};

    fn decl(name: &str, params: &[Type]) -> Function {
        let mut f = Function {
            name: name.into(),
            params: Vec::new(),
            ret: Type::Void,
            blocks: Vec::new(),
            values: Vec::new(),
            current_origin: None,
        };
        for (i, ty) in params.iter().enumerate() {
            f.values.push(ValueInfo {
                ty: *ty,
                name: None,
            });
            f.params.push(ValueId(i as u32));
        }
        f
    }

    /// `main`: mints one closure env (value 0, Ptr), takes the
    /// adapter's address (value 1), stores it at the boxed-entry
    /// slot, and hands the closure (value 2) to `callee`.
    fn module_with(callee: &str, extra_param_ty: Type) -> Module {
        let mut main = decl("main", &[]);
        main.values.push(ValueInfo {
            ty: Type::Ptr,
            name: None,
        }); // 0: env
        main.values.push(ValueInfo {
            ty: Type::FnSig(crate::ssa::SigId(0)),
            name: None,
        }); // 1
        main.values.push(ValueInfo {
            ty: extra_param_ty,
            name: None,
        }); // 2
        main.blocks.push(Block {
            id: BlockId(0),
            insts: vec![
                Inst {
                    result: Some(ValueId(0)),
                    kind: InstKind::Alloca(Type::I64),
                    origin: None,
                },
                Inst {
                    result: Some(ValueId(1)),
                    kind: InstKind::FnAddr(FuncId(1)),
                    origin: None,
                },
                Inst {
                    result: None,
                    kind: InstKind::Store(
                        Operand::Value(ValueId(1)),
                        Operand::Value(ValueId(0)),
                        CLOSURE_BOXED_ENTRY_OFF,
                    ),
                    origin: None,
                },
                Inst {
                    result: None,
                    kind: InstKind::Call(FuncId(2), vec![Operand::Value(ValueId(2))]),
                    origin: None,
                },
            ],
            term: Terminator::Ret(None),
        });
        let mut adapter = decl("__boxed___closure_0", &[]);
        adapter.blocks.push(Block {
            id: BlockId(0),
            insts: Vec::new(),
            term: Terminator::Ret(None),
        });
        let mut m = Module::default();
        m.funcs.push(main);
        m.funcs.push(adapter);
        m.funcs.push(decl(callee, &[extra_param_ty]));
        m
    }

    #[test]
    fn rc_traffic_on_a_closure_is_not_an_escape_and_the_adapter_store_is_neutered() {
        let mut m = module_with("__torajs_rc_dec", Type::Closure(crate::ssa::SigId(0)));
        assert!(closure_escape_sites(&m).is_empty());
        assert_eq!(strip_boxed_entries(&mut m), 1);
        let main = &m.funcs[0];
        assert!(
            main.blocks[0]
                .insts
                .iter()
                .all(|i| !matches!(i.kind, InstKind::FnAddr(_)))
        );
        assert!(main.blocks[0].insts.iter().any(|i| matches!(
            i.kind,
            InstKind::Store(Operand::ConstI64(0), _, CLOSURE_BOXED_ENTRY_OFF)
        )));
    }

    #[test]
    fn a_closure_handed_to_the_box_encoder_escapes() {
        let m = module_with(
            "__torajs_anyv_box_from_pair",
            Type::Closure(crate::ssa::SigId(0)),
        );
        let sites = closure_escape_sites(&m);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].callee, "__torajs_anyv_box_from_pair");
        assert_eq!(sites[0].arg_index, 0);
    }

    #[test]
    fn a_non_closure_operand_is_no_evidence() {
        let m = module_with("__torajs_anyv_box_from_pair", Type::I64);
        assert!(closure_escape_sites(&m).is_empty());
    }
}
