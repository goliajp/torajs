//! Module-metadata globals builders drained from the tail of
//! `lower_inner` (chunk-331 of the lower_inner RFC decomp, after Pass
//! 0.5 / Pass 1 / Intrinsics-table siblings in chunks 328-330):
//!
//! * `populate_vtables` — T-24 per-class vtable globals. Slot order
//!   matches `ast.method_index` (sorted-by-name); for each class C,
//!   slot i resolves to `__cm_<X>__M[i]` where X is the deepest
//!   ancestor of C (incl. itself) with an own impl. Classes that
//!   don't appear in any chain method's MRO still get an empty
//!   vtable so the layout stays uniform.
//!
//! * `populate_class_layouts` — T-26.C named classes + W-J Phase A0
//!   anonymous structs. Both produce `ClassLayoutMeta` rows with
//!   per-field offsets, refcounted-children list, and per-field type
//!   tags driving the runtime's gOPD / Object.keys / inspect.rs /
//!   cycle-collector substrates. W-J Phase A1's `append_fresh_*` for
//!   Pass-2-fresh sids still lives in `ssa_lower_anon_stamp` —
//!   `lower_inner` calls it directly after this sibling returns.

use std::collections::HashMap;

use crate::ssa::{self, ClassLayoutMeta, FieldMetaSpec, Module, Type, VtableGlobal};
use crate::ssa_lower::OBJ_HEADER_SIZE;

pub(crate) fn populate_vtables(
    ast: &crate::ast::Ast,
    fn_table: &HashMap<String, ssa::FuncId>,
    module: &mut Module,
) {
    if ast.method_index.is_empty() {
        return;
    }
    let n_methods = ast.method_index.len();
    // Reverse method_index → ordered method names by slot.
    let mut methods_by_slot: Vec<&str> = vec![""; n_methods];
    for (m_name, idx) in &ast.method_index {
        methods_by_slot[*idx as usize] = m_name.as_str();
    }
    let mut class_names: Vec<&String> = ast.class_parents.keys().collect();
    class_names.sort();
    for cname in class_names {
        let mut fn_ids: Vec<Option<ssa::FuncId>> = Vec::with_capacity(n_methods);
        for &m_name in &methods_by_slot {
            let mut found: Option<ssa::FuncId> = None;
            let mut cur: Option<String> = Some(cname.clone());
            let mut depth = 0u32;
            while let Some(name) = cur {
                if depth > 64 {
                    break;
                }
                let candidate = format!("__cm_{name}__{m_name}");
                if let Some(fid) = fn_table.get(&candidate) {
                    found = Some(*fid);
                    break;
                }
                cur = ast.class_parents.get(&name).and_then(|p| p.clone());
                depth += 1;
            }
            fn_ids.push(found);
        }
        module.vtable_globals.push(VtableGlobal {
            class_name: cname.clone(),
            fn_ids,
        });
    }
}

/// S2.38 — `true` when the function never observes its first param
/// (the `__cm_` receiver slot). The lowerer spills every param into
/// an alloca at entry, so a plain "no operand mentions params[0]"
/// test refuses everything; the real predicate is two-step:
///
/// 1. every use of `recv` is a `Store(recv, <alloca>, _)` — the
///    entry spill (any other use — arithmetic, call arg, return,
///    branch — is an observation);
/// 2. every OTHER use of each such spill slot is another one of
///    those stores — a Load, a call arg, a re-store elsewhere, or a
///    dyn access would let the value escape.
///
/// A missing param 0 answers `false` (not a method shape, nothing
/// to prove).
fn fn_ignores_receiver(f: &ssa::Function) -> bool {
    let Some(&recv) = f.params.first() else {
        return false;
    };
    let is_alloca: Vec<bool> = f
        .values
        .iter()
        .enumerate()
        .map(|(i, _)| {
            f.blocks.iter().any(|b| {
                b.insts.iter().any(|inst| {
                    inst.result == Some(ssa::ValueId(i as u32))
                        && matches!(
                            inst.kind,
                            ssa::InstKind::Alloca(_) | ssa::InstKind::AllocaBytes(_)
                        )
                })
            })
        })
        .collect();
    let op_is =
        |op: &ssa::Operand, v: ssa::ValueId| matches!(op, ssa::Operand::Value(x) if *x == v);
    // Pass 1 — every recv use must be an entry spill into an alloca.
    let mut spill_slots: Vec<ssa::ValueId> = Vec::new();
    for b in &f.blocks {
        for inst in &b.insts {
            let mut uses_recv = false;
            ssa::visit_value_operands(&inst.kind, |v| uses_recv |= v == recv);
            if !uses_recv {
                continue;
            }
            match &inst.kind {
                ssa::InstKind::Store(val, ptr, _)
                    if op_is(val, recv)
                        && matches!(ptr, ssa::Operand::Value(s)
                            if is_alloca.get(s.0 as usize).copied().unwrap_or(false)) =>
                {
                    if let ssa::Operand::Value(s) = ptr {
                        spill_slots.push(*s);
                    }
                }
                _ => return false,
            }
        }
        match &b.term {
            ssa::Terminator::CondBr { cond, .. } if op_is(cond, recv) => return false,
            ssa::Terminator::Ret(Some(op)) if op_is(op, recv) => return false,
            _ => {}
        }
    }
    // Pass 2 — the spill slots themselves must never be read or
    // escape: their only uses are the recv spills counted above.
    for b in &f.blocks {
        for inst in &b.insts {
            let mut touches_slot = false;
            ssa::visit_value_operands(&inst.kind, |v| touches_slot |= spill_slots.contains(&v));
            if !touches_slot {
                continue;
            }
            match &inst.kind {
                ssa::InstKind::Store(val, _, _) if op_is(val, recv) => {}
                _ => return false,
            }
        }
        match &b.term {
            ssa::Terminator::CondBr { cond, .. } if matches!(cond, ssa::Operand::Value(v) if spill_slots.contains(v)) =>
            {
                return false;
            }
            ssa::Terminator::Ret(Some(op)) if matches!(op, ssa::Operand::Value(v) if spill_slots.contains(v)) =>
            {
                return false;
            }
            _ => {}
        }
    }
    true
}

pub(crate) fn populate_class_layouts(
    ast: &crate::ast::Ast,
    fn_table: &HashMap<String, ssa::FuncId>,
    boxed_entries: &HashMap<ssa::FuncId, (ssa::FuncId, ssa::SigId)>,
    class_name_to_tag: &HashMap<String, u32>,
    aliases: &HashMap<String, Type>,
    module: &mut Module,
    struct_layouts_pass15_len: usize,
) {
    // T-26.C — named-class metadata, walked in `class_name_to_tag`
    // order so the resulting Vec lines up with the runtime's index
    // arithmetic (cycle collector indexes class_layouts via
    // `class_tag - 1`). Class instances live behind a 24-byte object
    // header so field i is at `OBJ_HEADER_SIZE + i*8`. Non-class types
    // (anonymous `type X = {...}` aliases) get tag 0 and are
    // excluded — cycle detection on them needs heap-header-keyed sid
    // lookup as a follow-up.
    let mut class_names_by_tag: Vec<(&String, u32)> =
        class_name_to_tag.iter().map(|(n, t)| (n, *t)).collect();
    class_names_by_tag.sort_by_key(|(_, t)| *t);
    // 刀 4 — own `__cm_` bodies per class, resolved once for the
    // whole table (the per-class walk below merges parent chains).
    let all_class_names: Vec<&String> = class_names_by_tag.iter().map(|(n, _)| *n).collect();
    let own_methods = crate::ssa_lower_module_metadata_methods::collect_own_class_methods(
        ast,
        fn_table,
        &all_class_names,
    );
    // S2.38 — `__cm_` bodies proven safe to run through the boxed
    // adapter with a NULL receiver: (1) the body never observes its
    // receiver param, and (2) the adapter-visible argument surface
    // is lossless — every user param is `Any` (a typed slot would
    // silently unbox an undefined argv box to 0/"" instead of ES's
    // undefined) and none carries a default (defaults are
    // caller-side injected, which a runtime bare call bypasses —
    // trading the loud TypeError for a silent wrong answer is
    // forbidden). A generator-method forwarder feeds `__this` into
    // the `__Gen_*` factory, so it stays receiver-bound naturally.
    // Per-param default verdicts, positionally aligned with the
    // FnDecl's params: None = no default, Some(true) = a literal the
    // adapter substitutes itself (S2.39 — Number / Bool, plus the
    // `undefined` literal V2b's materialize pass leaves behind: the
    // real default now lives as a body-head guard that fires on the
    // undefined argv a bare call sends, so runtime dispatch is
    // exactly as correct as a padded site), Some(false) = an
    // expression default only caller-side injection can evaluate.
    let fn_dflt_verdicts: std::collections::HashMap<&str, Vec<Option<bool>>> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            crate::ast::Stmt::FnDecl { name, params, .. } => Some((
                name.as_str(),
                params
                    .iter()
                    .map(|p| {
                        p.default.map(|d| match ast.get_expr(d) {
                            crate::ast::Expr::Number(_) | crate::ast::Expr::Bool(_) => true,
                            crate::ast::Expr::Ident(n) if n == "undefined" => true,
                            _ => false,
                        })
                    })
                    .collect(),
            )),
            _ => None,
        })
        .collect();
    let this_free_fids: std::collections::HashSet<ssa::FuncId> = own_methods
        .values()
        .flatten()
        .map(|(_, fid, _)| *fid)
        .filter(|fid| {
            let f = &module.funcs[fid.0 as usize];
            let Some(verdicts) = fn_dflt_verdicts.get(f.name.as_str()) else {
                return false;
            };
            // Knife 4a — an argv-face method carries the synthetic
            // `__torajs_real_argc` / `__torajs_argv` right after
            // `__this`. The boxed adapter feeds those two directly
            // (they never consume argv positions), so a bare call
            // can't poison them with an undefined box — the audit
            // skips them; only the real user slots are examined.
            let skip = if ast.method_argv_fns.contains(f.name.as_str()) {
                3
            } else {
                1
            };
            fn_ignores_receiver(f)
                && f.params.len() == verdicts.len()
                && f.params[skip..]
                    .iter()
                    .zip(&verdicts[skip..])
                    .all(|(&p, v)| {
                        let ty = &f.values[p.0 as usize].ty;
                        match v {
                            // No default: only an Any slot passes an
                            // undefined argv box through losslessly.
                            None => *ty == Type::Any,
                            // Adapter-substituted literal: the undefined
                            // case never reaches the typed unbox, so any
                            // scalar slot is safe.
                            Some(true) => matches!(
                                ty,
                                Type::Any | Type::I64 | Type::I32 | Type::F64 | Type::Bool
                            ),
                            // Expression default: a runtime bare call
                            // bypasses caller-side injection — refuse.
                            Some(false) => false,
                        }
                    })
        })
        .collect();
    for (cname, _tag) in &class_names_by_tag {
        let sid = match module.struct_layouts.iter().enumerate().find_map(|(i, _)| {
            aliases.get(*cname).and_then(|t| match t {
                Type::Obj(s) if s.0 as usize == i => Some(i),
                _ => None,
            })
        }) {
            Some(i) => i,
            None => continue,
        };
        let layout = &module.struct_layouts[sid];
        let mut child_offsets: Vec<u32> = Vec::new();
        let mut field_metadata: Vec<FieldMetaSpec> = Vec::new();
        for (i, (fname, fty)) in layout.iter().enumerate() {
            let off = OBJ_HEADER_SIZE as u32 + (i as u32) * 8;
            if fty.is_refcounted() {
                child_offsets.push(off);
            }
            // W-J Phase A3: per-field metadata for the reflection
            // consumers (Phase B `gOPD` struct cell arm / Phase C
            // `Object.keys`/`values`/`entries` / Phase D `inspect.rs`
            // Tag::Obj walker). Carried through to Phase A3b's
            // `.__class_fields_<i>` rodata emit.
            field_metadata.push(FieldMetaSpec {
                name: fname.clone(),
                offset: off,
                type_tag: ssa::field_type_tag_of(*fty),
            });
        }
        module.class_layouts.push(ClassLayoutMeta {
            class_name: (*cname).clone(),
            child_offsets,
            field_metadata,
            is_named: true,
            methods: crate::ssa_lower_module_metadata_methods::resolve_class_methods(
                ast,
                &own_methods,
                boxed_entries,
                &this_free_fids,
                cname,
            ),
        });
    }

    // W-J Phase A0 (RFC 20260614-w-j-struct-reflect §3) — anonymous
    // ObjectLit struct also registers a ClassLayoutMeta entry so the
    // downstream reflection substrate (Phase B `gOPD` struct cell arm /
    // Phase C `Object.keys`/`values`/`entries` / Phase D `inspect.rs`
    // Tag::Obj walker) can look up field metadata by `class_tag@+8`.
    //
    // A0 keeps stamp paths unchanged — `class_tag@+8` continues to be
    // 0 for ObjectLit, so these new entries are dead from the cycle
    // collector's perspective; the stamp is wired in Phase A1. The
    // only observable here is `__torajs_n_class_layouts` count grows
    // by anonymous-only sid count, validating that the dyld
    // chain-fixup substrate scales with entry growth.
    let named_sids: std::collections::HashSet<ssa::StructId> = class_name_to_tag
        .keys()
        .filter_map(|cname| match aliases.get(cname) {
            Some(Type::Obj(sid)) => Some(*sid),
            _ => None,
        })
        .collect();
    // W-J Phase A1 fix (P1 rc bug 2026-07-23) — only walk struct_layouts
    // up to the Pass-1.5 snapshot boundary. Every sid appended past
    // this index is Pass-2 territory: pool-assigned fresh anons get
    // emitted by `append_fresh_class_layouts` (matching the pool's
    // `next_tag_start = n_named + snapshot.len() + 1` invariant), and
    // pool-agnostic Pass-2 sids (e.g. iter_next's IteratorResult with
    // hardcoded class_tag=0) are never looked up in class_layouts.
    // Emitting either shape here shifts populate's implicit tag =
    // index+1, causing pool-vs-populate mismatch — for the crash
    // trigger `const t = xs.values(); t.next(); const objs = [{id:1}]`,
    // populate would emit IteratorResult at {id:1}'s pool tag → cycle
    // walker reads {id:1}'s scalar `1` as a heap child → SIGSEGV.
    let layouts = module.struct_layouts.clone();
    for (sid_idx, layout) in layouts.iter().enumerate().take(struct_layouts_pass15_len) {
        let sid = ssa::StructId(sid_idx as u32);
        if named_sids.contains(&sid) {
            continue;
        }
        let mut child_offsets: Vec<u32> = Vec::new();
        let mut field_metadata: Vec<FieldMetaSpec> = Vec::new();
        for (i, (fname, fty)) in layout.iter().enumerate() {
            let off = OBJ_HEADER_SIZE as u32 + (i as u32) * 8;
            if fty.is_refcounted() {
                child_offsets.push(off);
            }
            // W-J Phase A3 — same per-field metadata population as the
            // named-class branch above. Anonymous structs share the
            // reflection consumer surface (`{a:1}` as `gOPD` target,
            // `Object.keys({a:1})` etc.).
            field_metadata.push(FieldMetaSpec {
                name: fname.clone(),
                offset: off,
                type_tag: ssa::field_type_tag_of(*fty),
            });
        }
        module.class_layouts.push(ClassLayoutMeta {
            class_name: format!("__anon_struct_{sid_idx}"),
            child_offsets,
            field_metadata,
            is_named: false,
            methods: Vec::new(),
        });
    }
}
