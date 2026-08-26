//! r498 knife 4 — user-fn dead-strip (ld64 `-dead_strip` at the
//! user-atom granularity the member closure never had). Lives in the
//! link crate since r501 so `dead_strip_elide::assume` can re-run it
//! over a patched fn list (an adapter whose every mint was rewritten
//! to `movz #0` has no reference left and must stop seeding).
//!
//! Every compiled user fn's relocs used to seed the member closure
//! and the reachability pass, so a synthesized fn nothing references
//! (`__env_drop_trivial` in a closure-free program) still rooted
//! whole runtime members (its `__torajs_value_drop_heap` reloc alone
//! kept the cycle collector alive: −7,192 text on the empty
//! program). This pass computes which user fns are reachable and
//! empties the rest — `bytes.clear()` + `relocs.clear()` is the
//! whole strip, because the pipeline already treats empty-bytes
//! slots as definitionless (no layout bytes, no symtab row, no
//! seeds; `compile_module_funcs`' declaration-slot convention).
//!
//! Roots (collected from the materialized `LinkConfig` itself, so a
//! table the link layer bakes can never be missed by a caller-side
//! enumeration — the r498 gate caught exactly that: class-method
//! rows' boxed adapters lived only in `class_layouts[].methods` and
//! the first cut stripped every dispatchable method body):
//! - the entry wrapper (`_main`),
//! - every `FuncId` a link-emitted table takes the address of:
//!   vtable `slot_syms` + fn-name registry `fn_addr_sym` (both
//!   `__torajs_fn_<fid>`-shaped) + class-method rows'
//!   `adapter_fn_id` / `twin_fn_id`,
//! - every fn whose name is `___torajs_*`-shaped: those are
//!   runtime-facing definitions (dispatch-arm stubs, obj_alloc /
//!   obj_drop_sized) that live archive members may call by name —
//!   member→user edges are not walked here, so the whole
//!   interposable face stays rooted (over-keeping is the safe
//!   direction; each stub is a handful of bytes).
//!
//! Edges (from a live fn's relocs): `CallTarget::Func(fid)`;
//! `Extern`/`target_sym` names that match another user fn's name;
//! the fn-address aliases (`fn_addr_syms::FN_ADDR_ALIAS_PREFIXES`).
//!
//! `TORAJS_USER_GC_OFF=1` disables the pass (A/B pricing);
//! `TORAJS_USER_GC_DIAG=1` prints the dead set to stderr.

use crate::exec::LinkConfig;
use crate::fn_addr_syms::parse_fn_addr_alias;
use torajs_codegen::CompiledFunction;
use torajs_codegen::reloc::{CallTarget, RelocKind};

/// The symbol names a reloc can carry (mirrors the link layer's
/// `reloc_target_name`, plus the `Func(fid)` edge it skips).
enum Edge<'a> {
    Fid(usize),
    Name(&'a str),
}

fn reloc_edge(kind: &RelocKind) -> Edge<'_> {
    match kind {
        RelocKind::CallSite {
            target: CallTarget::Func(fid),
        } => Edge::Fid(fid.0 as usize),
        RelocKind::CallSite {
            target: CallTarget::Extern(name),
        } => Edge::Name(name),
        RelocKind::Page21 { target_sym }
        | RelocKind::PageOff12 { target_sym }
        | RelocKind::AbsPtr64 { target_sym } => Edge::Name(target_sym),
    }
}

/// Entry point — collect the address-taken roots from every
/// fn-referencing table the config carries, then run the walk.
/// Answers the live bit per fn index.
pub fn strip_dead_user_fns(cfg: &mut LinkConfig) -> Vec<bool> {
    let roots = table_root_fids(cfg);
    let entry = cfg.entry.clone();
    let diag = std::env::var_os("TORAJS_USER_GC_DIAG").is_some();
    strip_with_roots(&mut cfg.funcs, &entry, &roots, diag)
}

/// Every `FuncId` a link-emitted table takes the address of.
pub(crate) fn table_root_fids(cfg: &LinkConfig) -> Vec<usize> {
    cfg.vtable_globals
        .iter()
        .flat_map(|vt| vt.slot_syms.iter().flatten())
        .filter_map(|s| parse_fn_addr_alias(s))
        .chain(
            cfg.fn_name_globals
                .iter()
                .filter_map(|e| parse_fn_addr_alias(&e.fn_addr_sym)),
        )
        .chain(cfg.class_layouts.iter().flat_map(|cl| {
            cl.methods.iter().flat_map(|m| {
                std::iter::once(m.adapter_fn_id as usize).chain(m.twin_fn_id.map(|t| t as usize))
            })
        }))
        .collect()
}

/// Reachability walk + strip, separated from the root collection so
/// the tests (and the elide pre-pass) can drive it with their own
/// root sets. Answers the live bit per index (all true when the pass
/// is disabled).
pub(crate) fn strip_with_roots(
    funcs: &mut [CompiledFunction],
    entry: &str,
    table_root_fids: &[usize],
    diag: bool,
) -> Vec<bool> {
    if std::env::var_os("TORAJS_USER_GC_OFF").is_some() {
        return vec![true; funcs.len()];
    }
    let n = funcs.len();
    let idx_by_name: std::collections::HashMap<&str, usize> = funcs
        .iter()
        .enumerate()
        .map(|(i, f)| (f.name.as_str(), i))
        .collect();
    let mut live = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mark = |i: usize, live: &mut Vec<bool>, stack: &mut Vec<usize>| {
        if i < n && !live[i] {
            live[i] = true;
            stack.push(i);
        }
    };
    if let Some(&e) = idx_by_name.get(entry) {
        mark(e, &mut live, &mut stack);
    }
    for &fid in table_root_fids {
        mark(fid, &mut live, &mut stack);
    }
    for (i, f) in funcs.iter().enumerate() {
        if f.name.starts_with("___torajs_") {
            mark(i, &mut live, &mut stack);
        }
    }
    while let Some(i) = stack.pop() {
        // Split-borrow dance: collect targets before marking.
        let targets: Vec<usize> = funcs[i]
            .relocs
            .iter()
            .filter_map(|r| match reloc_edge(&r.kind) {
                Edge::Fid(j) => Some(j),
                Edge::Name(name) => {
                    if let Some(&j) = idx_by_name.get(name) {
                        Some(j)
                    } else {
                        parse_fn_addr_alias(name)
                    }
                }
            })
            .collect();
        for j in targets {
            mark(j, &mut live, &mut stack);
        }
    }
    let mut dead_fns = 0usize;
    let mut dead_bytes = 0usize;
    for (i, f) in funcs.iter_mut().enumerate() {
        if live[i] || f.bytes.is_empty() {
            continue;
        }
        dead_fns += 1;
        dead_bytes += f.bytes.len();
        if diag {
            eprintln!("[user-gc] dead: {} ({} bytes)", f.name, f.bytes.len());
        }
        f.bytes.clear();
        f.relocs.clear();
    }
    if diag {
        eprintln!("[user-gc] {dead_fns} fns / {dead_bytes} bytes stripped");
    }
    live
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_codegen::frame::FrameLayout;
    use torajs_codegen::reloc::Reloc;
    use torajs_core::ssa::FuncId;

    fn f(name: &str, relocs: Vec<Reloc>) -> CompiledFunction {
        CompiledFunction {
            name: name.into(),
            bytes: vec![0; 4],
            relocs,
            frame: FrameLayout::leaf_no_spill(),
        }
    }
    fn call_fid(i: usize) -> Reloc {
        Reloc {
            byte_offset: 0,
            kind: RelocKind::CallSite {
                target: CallTarget::Func(FuncId(i as u32)),
            },
        }
    }
    fn call_name(n: &str) -> Reloc {
        Reloc {
            byte_offset: 0,
            kind: RelocKind::CallSite {
                target: CallTarget::Extern(n.into()),
            },
        }
    }

    #[test]
    fn dead_helper_is_emptied_live_chain_survives() {
        let mut funcs = vec![
            f("_main", vec![call_name("_main_user")]),
            f("_main_user", vec![call_fid(3)]),
            f(
                "__env_drop_trivial",
                vec![call_name("___torajs_value_drop_heap")],
            ),
            f("helper", Vec::new()),
        ];
        strip_with_roots(&mut funcs, "_main", &[], false);
        assert!(!funcs[0].bytes.is_empty());
        assert!(!funcs[1].bytes.is_empty());
        assert!(funcs[2].bytes.is_empty(), "unreferenced helper stripped");
        assert!(funcs[2].relocs.is_empty(), "its seeds go with it");
        assert!(!funcs[3].bytes.is_empty(), "Func(fid) edge keeps it");
    }

    #[test]
    fn table_and_runtime_face_roots_survive() {
        let mut funcs = vec![
            f("_main", Vec::new()),
            f("cb", Vec::new()),
            f("___torajs_dispatch_str_arm", Vec::new()),
            f("addr_taken", Vec::new()),
        ];
        // fn_name table references __torajs_fn_3; vtable references 1.
        strip_with_roots(&mut funcs, "_main", &[1, 3], false);
        assert!(funcs.iter().all(|f| !f.bytes.is_empty()));
    }

    #[test]
    fn class_method_table_roots_survive() {
        use crate::exec::{UserClassLayoutEntry, UserMethodMetaEntry};
        use crate::resolve::SymTable;
        let mut cfg = LinkConfig {
            funcs: vec![f("_main", Vec::new()), f("__boxed___cm_P__m", Vec::new())],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            dead_strip: false,
            strip_member_symbols: false,
            elidable_sites: Vec::new(),
            guarded_stubs: Vec::new(),
            archives: Vec::new(),
            strings: Vec::new(),
            data_globals: Vec::new(),
            vtable_globals: Vec::new(),
            class_layouts: vec![UserClassLayoutEntry {
                child_offsets: Vec::new(),
                fields: Vec::new(),
                is_named: true,
                is_generic: false,
                methods: vec![UserMethodMetaEntry {
                    name: "m".into(),
                    adapter_fn_id: 1,
                    flags: 0,
                    twin_fn_id: None,
                }],
            }],
            force_emit_class_layouts_globals: false,
            fn_name_globals: Vec::new(),
            force_emit_fn_name_globals: false,
            class_names: Vec::new(),
            force_emit_class_names_globals: false,
            baked_regex_entries: Vec::new(),
        };
        strip_dead_user_fns(&mut cfg);
        assert!(
            !cfg.funcs[1].bytes.is_empty(),
            "method-table adapter row roots its fn"
        );
    }

    #[test]
    fn fn_addr_alias_edge_keeps_target() {
        let mut funcs = vec![
            f("_main", vec![call_name("__torajs_fn_1")]),
            f("taken", Vec::new()),
            f("dead", Vec::new()),
        ];
        strip_with_roots(&mut funcs, "_main", &[], false);
        assert!(!funcs[1].bytes.is_empty());
        assert!(funcs[2].bytes.is_empty());
    }
}
