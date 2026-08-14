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

/// Returns the specialization rows map (`sid → (base class name,
/// method table)`) so `append_fresh_class_layouts` can dress Pass-2
/// fresh instantiation sids the same way the snapshot walk below
/// dresses Pass-1.5 ones. `generic_inst_sids` is the stamp-site
/// record from [`crate::ssa_lower_anon_stamp::AnonStampPool`].
pub(crate) fn populate_class_layouts(
    ast: &crate::ast::Ast,
    fn_table: &HashMap<String, ssa::FuncId>,
    boxed_entries: &HashMap<ssa::FuncId, (ssa::FuncId, ssa::SigId)>,
    class_name_to_tag: &HashMap<String, u32>,
    aliases: &HashMap<String, Type>,
    generic_inst_sids: &HashMap<ssa::StructId, String>,
    module: &mut Module,
    struct_layouts_pass15_len: usize,
) -> HashMap<ssa::StructId, (String, Vec<ssa::MethodMetaSpec>)> {
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
    let this_free_fids = collect_this_free_fids(ast, module, &own_methods);
    for (cname, _tag) in &class_names_by_tag {
        let sid = match module.struct_layouts.iter().enumerate().find_map(|(i, _)| {
            aliases.get(*cname).and_then(|t| match t {
                Type::Obj(s) if s.0 as usize == i => Some(i),
                _ => None,
            })
        }) {
            Some(i) => i,
            // A GENERIC class has no alias sid — its instances live on
            // per-specialization sids (`inst_memo`), never on a class
            // sid. It still must push a row: the runtime indexes
            // `class_layouts` by `class_tag - 1`, so SKIPPING a tag
            // here shifted every later row one slot left — every class
            // sorted after the generic one answered with its
            // neighbor's layout (404-01 root, latent since the walk
            // was written). The row carries no fields (no instance
            // ever wears this tag) but does carry the method table —
            // the specialization rows below inherit it by clone.
            None => {
                module.class_layouts.push(generic_class_placeholder_row(
                    ast,
                    fn_table,
                    boxed_entries,
                    &own_methods,
                    &this_free_fids,
                    cname,
                ));
                continue;
            }
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
    // 404-01 — a generic-class instantiation sid rides the anonymous
    // machinery for its LAYOUT (per-specialization field types: a
    // `G<number>` k is a raw i64, a `G<string>` k a refcounted ptr —
    // the cycle collector and reflection must see the right one), but
    // its IDENTITY is the class: the row gets the base class's name
    // and method table so the any-lane method dispatch reading
    // `class_tag@+8` answers on these instances.
    let inst_rows: HashMap<ssa::StructId, (String, Vec<ssa::MethodMetaSpec>)> = generic_inst_sids
        .iter()
        .map(|(sid, base)| {
            let row = generic_class_placeholder_row(
                ast,
                fn_table,
                boxed_entries,
                &own_methods,
                &this_free_fids,
                base,
            );
            (*sid, (base.clone(), row.methods))
        })
        .collect();
    // W-J Phase A0 — anonymous ObjectLit structs get their own
    // ClassLayoutMeta entries, in [`register_anonymous_struct_layouts`].
    register_anonymous_struct_layouts(
        class_name_to_tag,
        aliases,
        &inst_rows,
        module,
        struct_layouts_pass15_len,
    );
    inst_rows
}

/// S2.38 — the receiver-free / bare-call-safe verdict per `__cm_`
/// body, extracted verbatim from [`populate_class_layouts`] under
/// the 200-line function rule (404-01 pushed the walk over).
fn collect_this_free_fids(
    ast: &crate::ast::Ast,
    module: &Module,
    own_methods: &HashMap<String, Vec<(String, ssa::FuncId, Option<ssa::FuncId>)>>,
) -> std::collections::HashSet<ssa::FuncId> {
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
            // `__torajs_argv` right after `__this` (since S1-T2 the
            // argc rides the hidden sig slot instead of an injected
            // param). The adapter feeds both directly (they never
            // consume argv positions), so a bare call can't poison
            // them with an undefined box — the audit skips them;
            // only the real user slots are examined. Two counts:
            // `f` is the SSA function, whose sig carries the hidden
            // I64 `__torajs_argc` at position 1; `verdicts` aligns
            // with the AST param list, which never sees that slot.
            let (ssa_skip, ast_skip) = if ast.method_argv_fns.contains(f.name.as_str()) {
                (3, 2)
            } else {
                (1, 1)
            };
            crate::ssa_lower_module_metadata_methods::fn_ignores_receiver(f)
                && f.params.len() == verdicts.len() + (ssa_skip - ast_skip)
                && f.params[ssa_skip..]
                    .iter()
                    .zip(&verdicts[ast_skip..])
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
    this_free_fids
}

/// 404-01 — the row a GENERIC class (no alias sid) contributes: no
/// fields (no instance ever wears its tag — instances live on
/// per-specialization sids), but the class name and the method table,
/// re-targeted at the receiver-polymorphic twin instances. The same
/// row shape dresses each specialization sid's anonymous row, which
/// is why this builder serves both the named walk's placeholder and
/// the `inst_rows` map.
fn generic_class_placeholder_row(
    ast: &crate::ast::Ast,
    fn_table: &HashMap<String, ssa::FuncId>,
    boxed_entries: &HashMap<ssa::FuncId, (ssa::FuncId, ssa::SigId)>,
    own_methods: &HashMap<String, Vec<(String, ssa::FuncId, Option<ssa::FuncId>)>>,
    this_free_fids: &std::collections::HashSet<ssa::FuncId>,
    cname: &str,
) -> ClassLayoutMeta {
    let methods = crate::ssa_lower_module_metadata_methods::resolve_class_methods(
        ast,
        own_methods,
        boxed_entries,
        this_free_fids,
        cname,
    );
    ClassLayoutMeta {
        class_name: cname.to_string(),
        child_offsets: Vec::new(),
        field_metadata: Vec::new(),
        is_named: true,
        methods: crate::ssa_lower_module_metadata_methods::retarget_generic_class_methods(
            ast,
            fn_table,
            boxed_entries,
            cname,
            methods,
        ),
    }
}

/// W-J Phase A0 (RFC 20260614-w-j-struct-reflect §3) — the anonymous
/// half of [`populate_class_layouts`], split out for file-size. It
/// reads none of the named walk's locals: the parameters here are
/// the whole of what it needs.
fn register_anonymous_struct_layouts(
    class_name_to_tag: &HashMap<String, u32>,
    aliases: &HashMap<String, Type>,
    inst_rows: &HashMap<ssa::StructId, (String, Vec<ssa::MethodMetaSpec>)>,
    module: &mut Module,
    struct_layouts_pass15_len: usize,
) {
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
        // 404-01 — a generic-class instantiation keeps this row's
        // per-specialization LAYOUT but wears the class's identity.
        let (class_name, methods) = match inst_rows.get(&sid) {
            Some((base, methods)) => (base.clone(), methods.clone()),
            None => (format!("__anon_struct_{sid_idx}"), Vec::new()),
        };
        module.class_layouts.push(ClassLayoutMeta {
            class_name,
            child_offsets,
            field_metadata,
            is_named: false,
            methods,
        });
    }
}
