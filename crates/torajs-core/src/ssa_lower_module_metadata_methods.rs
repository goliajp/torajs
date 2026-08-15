//! Own-method collection + inherited-chain resolution for the
//! class-methods dispatch table — split out of
//! `ssa_lower_module_metadata.rs` (file-size line, rotation 296).
//! `collect_own_class_methods` maps each class to its `__cm_` rows
//! (the blade-3 twin body fid resolved alongside);
//! `resolve_class_methods` walks the parent chain and emits the
//! deduped `MethodMetaSpec` table for one class.

use std::collections::HashMap;

use crate::ssa;

/// 刀 4 (RFC 20260714-t262-top-clusters) — collect every class's OWN
/// `__cm_<C>__<m>` method bodies by scanning the fn table (NOT
/// `ast.method_index`, which only carries chain/vtable methods —
/// single-owner methods are statically rewritten and never enter it).
/// A class name may itself contain `__`, so the `<C>` boundary is
/// resolved by longest-match against the known class-name set. The
/// ctor (`__cm_<C>__ctor`) never enters — `new` is not a method call.
///
/// RFC 20260714-objlit-accessor blade 5 — an ACCESSOR body
/// (`__cm_<C>__<p>_get`) enters under the synthetic slot spelling
/// `__getter_<p>`, the same name an object-literal accessor carries in
/// the layout. Two things ride on that:
///
/// * the runtime `any` member read resolves `o.p` through it (a class
///   accessor is prototype-level, so unlike the literal's it has no
///   layout field to live in);
/// * `<p>_get` STOPS being a callable method name. It was one — probe
///   at `cd0f3caf`: `(new C() as any).b_get()` answered the getter's
///   999 while bun rejects the name outright. The mangled spelling was
///   leaking onto the user-visible method surface.
///
/// The name is taken from `ast.accessor_getters` / `accessor_setters`
/// (keyed by fn name), never by guessing at a `_get` suffix — a plain
/// method really called `b_get` is a legal method.
/// 402-01 — is `name` a generic (type-param-bearing) FnDecl in the
/// post-mono AST? Gates the `$$anywv` suffix strip above: the
/// generic original never lowers, but its decl stays in the AST.
fn is_generic_method_decl(ast: &crate::ast::Ast, name: &str) -> bool {
    ast.stmts.iter().any(|s| {
        matches!(s,
            crate::ast::Stmt::FnDecl { name: n, type_params, .. }
            if n == name && !type_params.is_empty())
    })
}

pub(crate) fn collect_own_class_methods(
    ast: &crate::ast::Ast,
    fn_table: &HashMap<String, ssa::FuncId>,
    class_names: &[&String],
) -> HashMap<String, Vec<(String, ssa::FuncId, Option<ssa::FuncId>)>> {
    let mut accessor_slots: HashMap<&str, String> = HashMap::new();
    for ((_, prop), fname) in &ast.accessor_getters {
        accessor_slots.insert(fname.as_str(), format!("__getter_{prop}"));
    }
    for ((_, prop), fname) in &ast.accessor_setters {
        accessor_slots.insert(fname.as_str(), format!("__setter_{prop}"));
    }
    let mut own: HashMap<String, Vec<(String, ssa::FuncId, Option<ssa::FuncId>)>> = HashMap::new();
    for (fname, &fid) in fn_table {
        let Some(rest) = fname.strip_prefix("__cm_") else {
            continue;
        };
        // A `__cmany_` twin body is itself in the fn_table but is
        // never an own method row (explicit skip — a user class
        // named `any_<X>` would otherwise collide via the shared
        // `__cm` spelling).
        if fname.starts_with("__cmany_") {
            continue;
        }
        // Longest class-name match wins (`__cm_A__b__c` with classes
        // `A` and `A__b` belongs to `A__b`).
        let mut best: Option<(&str, &str)> = None;
        for cname in class_names {
            if let Some(m) = rest.strip_prefix(&format!("{cname}__"))
                && best.is_none_or(|(b, _)| cname.len() > b.len())
            {
                best = Some((cname.as_str(), m));
            }
        }
        let Some((cname, mname)) = best else { continue };
        // `ctor$$_number` / `ctor$$anywv` are a generic class's ctor
        // mono instances — without the prefix form of the skip they
        // registered as dispatchable methods named `ctor$$…` (and,
        // suffix-stripped, `ctor`), handing `g.ctor(…)` a body bun
        // answers with a TypeError for (404-01 registration audit).
        if mname == "ctor" || mname.is_empty() || mname.starts_with("ctor$$") {
            continue;
        }
        // RFC 20260815 刀 5 — a GENERIC accessor's mono instance
        // (`__cm_Box__value_get$$anywv`) resolves its slot spelling
        // through the un-mono'd fn name the accessor maps are keyed
        // by; without the strip the row registered under the mangled
        // `value_get$$anywv` instead of `__getter_value`. Typed mono
        // rows (`…$$_number`) deliberately keep their own spelling —
        // one slot row per property, and the generic retarget below
        // drops them (their layout is one specialization's).
        let slot = accessor_slots.get(fname.as_str()).or_else(|| {
            fname
                .strip_suffix("$$anywv")
                .and_then(|base| accessor_slots.get(base))
        });
        let entry_name = match slot {
            Some(slot) => slot.clone(),
            // 402-01 — a generic method's all-`any` mono instance
            // (`__cm_C__id$$anywv`, pre-seeded by
            // `monomorphize_and_check`) IS the method's any-lane
            // dispatch body: strip the mono suffix so the row
            // registers under the user-visible name. `$` is a legal
            // identifier char, so the strip is gated on the original
            // name really being a generic decl — a user method
            // literally called `id$$anywv` keeps its own spelling.
            // Typed mono rows (`id$$_number`) also keep theirs: user
            // code cannot reach them by name, and their call sites
            // are statically retargeted anyway.
            None => match mname.strip_suffix("$$anywv") {
                Some(base) if is_generic_method_decl(ast, &format!("__cm_{cname}__{base}")) => {
                    base.to_string()
                }
                _ => mname.to_string(),
            },
        };
        // Blade 3 — the receiver-polymorphic twin's body fid, when
        // blade 2 minted one for this mono. `cmany_twins` is keyed by
        // the DESUGAR-time decl name, so a mono row (`…$$anywv` /
        // `…$$_number`) strips its suffix for the lookup and routes to
        // the twin's own all-any instance — the one mono a twin has
        // (404-01; every twin param is already `any`, so one instance
        // serves every specialization's rebind).
        let twin_fid = ast
            .cmany_twins
            .get(fname.as_str())
            .and_then(|twin_name| fn_table.get(twin_name.as_str()).copied())
            .or_else(|| {
                let (base, _) = fname.split_once("$$")?;
                let twin = ast.cmany_twins.get(base)?;
                fn_table.get(format!("{twin}$$anywv").as_str()).copied()
            });
        own.entry(cname.to_string())
            .or_default()
            .push((entry_name, fid, twin_fid));
    }
    own
}

/// 刀 4 — resolve one class's runtime-dispatchable methods: its own
/// `__cm_` bodies plus inherited ones up the parent chain (child
/// declarations shadow, the vtable walk's override semantics). Only
/// bodies with a synthesized boxed adapter survive — a synthesis
/// dropout (>8 params / unboxable type) keeps the runtime miss an
/// honest no-such TypeError.
pub(crate) fn resolve_class_methods(
    ast: &crate::ast::Ast,
    own_methods: &HashMap<String, Vec<(String, ssa::FuncId, Option<ssa::FuncId>)>>,
    boxed_entries: &HashMap<ssa::FuncId, (ssa::FuncId, ssa::SigId)>,
    this_free_fids: &std::collections::HashSet<ssa::FuncId>,
    cname: &str,
) -> Vec<ssa::MethodMetaSpec> {
    let mut out: Vec<ssa::MethodMetaSpec> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut cur: Option<String> = Some(cname.to_string());
    let mut depth = 0u32;
    while let Some(name) = cur {
        if depth > 64 {
            break;
        }
        if let Some(methods) = own_methods.get(&name) {
            for (mname, fid, twin_fid) in methods {
                if seen.contains(mname.as_str()) {
                    continue;
                }
                if let Some(&(adapter_fid, _)) = boxed_entries.get(fid) {
                    out.push(ssa::MethodMetaSpec {
                        name: mname.clone(),
                        adapter_fid,
                        this_free: this_free_fids.contains(fid),
                        twin_adapter_fid: twin_fid
                            .and_then(|t| boxed_entries.get(&t).map(|&(a, _)| a)),
                        twin_primary: false,
                    });
                }
                // Shadow even on adapter dropout — a child decl
                // without an adapter must not expose the parent's.
                seen.insert(mname.as_str());
            }
        }
        cur = ast.class_parents.get(&name).and_then(|p| p.clone());
        depth += 1;
    }
    // Deterministic table order (HashMap iteration fed `own_methods`).
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// 404-01 — re-target a GENERIC class's method rows at the
/// receiver-polymorphic twin's all-any instance.
///
/// The `$$anywv` mono body reads its receiver at ONE specialization's
/// static offsets (its `__this` erases to `G<any>`), while an
/// instance wears whichever specialization it was built with — a
/// `G<number>` k is a raw i64 the mono misdecodes as a NaN box. The
/// twin (`__cmany_…$$anywv`) reads through GetV, which honors the
/// receiver row's per-field type tags, so it is correct under every
/// specialization. A receiver-FREE body keeps its mono adapter (it
/// touches no field, so no layout can betray it); a receiver-reading
/// body whose twin was never minted (accessor slots, super-carrying
/// bodies) drops its row — the honest no-such TypeError over a
/// misread answer.
pub(crate) fn retarget_generic_class_methods(
    ast: &crate::ast::Ast,
    fn_table: &HashMap<String, ssa::FuncId>,
    boxed_entries: &HashMap<ssa::FuncId, (ssa::FuncId, ssa::SigId)>,
    base: &str,
    methods: Vec<ssa::MethodMetaSpec>,
) -> Vec<ssa::MethodMetaSpec> {
    methods
        .into_iter()
        .filter_map(|mut m| {
            // RFC 20260815 刀 5 — an accessor row's name is the slot
            // spelling (`__getter_value`), not a `__cm_` suffix; its
            // twin is keyed by the getter/setter FN name, recovered
            // through the accessor maps (walking the parent chain —
            // the row may be inherited, and the twin reads through
            // GetV so it is sound under a subclass receiver too).
            let decl = accessor_decl_for(ast, base, &m.name)
                .unwrap_or_else(|| format!("__cm_{base}__{}", m.name));
            let Some(twin) = ast.cmany_twins.get(&decl) else {
                return m.this_free.then_some(m);
            };
            let fid = fn_table.get(format!("{twin}$$anywv").as_str()).copied()?;
            let (adapter, _) = boxed_entries.get(&fid).copied()?;
            m.adapter_fid = adapter;
            m.twin_adapter_fid = None;
            m.twin_primary = true;
            Some(m)
        })
        .collect()
}

/// RFC 20260815 刀 5 — recover the declaring getter/setter FN name
/// behind an accessor slot row (`__getter_value` on class `Box` →
/// `__cm_Box__value_get`), walking `class_parents` because a resolved
/// method table merges inherited rows. `None` for a plain method row
/// or an accessor no ancestor declares.
fn accessor_decl_for(ast: &crate::ast::Ast, base: &str, slot: &str) -> Option<String> {
    let (table, prop) = match slot.strip_prefix("__getter_") {
        Some(p) => (&ast.accessor_getters, p),
        None => (&ast.accessor_setters, slot.strip_prefix("__setter_")?),
    };
    let mut cur = Some(base.to_string());
    let mut depth = 0u32;
    while let Some(c) = cur {
        if depth > 64 {
            break;
        }
        if let Some(f) = table.get(&(c.clone(), prop.to_string())) {
            return Some(f.clone());
        }
        cur = ast.class_parents.get(&c).and_then(|p| p.clone());
        depth += 1;
    }
    None
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
pub(crate) fn fn_ignores_receiver(f: &ssa::Function) -> bool {
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
