//! `desugar_classes` extracted from `ast.rs` (chunk 161).
//!
//! Pre-extract this function was 970 LOC at the top of ast.rs and
//! was the only god-fn left in the file. Body verbatim moves here;
//! ast.rs re-exports `pub use desugar_classes::desugar_classes` so
//! all external call sites (torajs-cli / num_width / repl / lsp /
//! cmd_build) keep working through `ast::desugar_classes`.
//!
//! Multi-pass class lowering:
//! - **Pass 1** — extract every ClassDecl; replace original Stmt
//!   in-place with the generated TypeDecl; accumulate ctor / method /
//!   factory FnDecls in `appended`.
//! - Method-owner table tracks every class declaring a method body
//!   (incl. overrides) in source order — dispatcher walks reverse
//!   so deepest sub checks first.
//! - P8.2 accessor records: buffered then drained into
//!   `ast.accessor_getters / accessor_setters` after the &mut Ast
//!   borrow releases.
//! - Per-class FnDecl synthesis delegates to
//!   `desugar_classes_emit::emit_class_instance_methods` /
//!   `emit_class_static_methods`.

use super::*;

mod factory_unset;

pub fn desugar_classes(ast: &mut Ast) {
    // RFC 20260815 knife 2a — extract each non-Ident heritage
    // expression to a `__ccp<N>` binding before the hoist runs, so
    // the hoist's capture check and the capturing lane's admit both
    // see an ordinary named parent binding.
    ast.extract_value_heritage();
    // Pre-pass — move capture-free ClassDecls out of nested statement
    // containers (fn bodies, fn-expression bodies, blocks) to the top
    // level, where the snapshot below can see them. Doing it here
    // rather than in the pipeline covers every caller (cli / repl /
    // lsp / num_width) at once.
    super::hoist_nested_classes::hoist_nested_classes(ast);

    // Pass 1 — extract every ClassDecl. After this loop the original
    // ClassDecl stmts are replaced by their generated TypeDecl in-place;
    // ctor / methods / factory FnDecls accumulate in `appended`.
    // (method_owners / chain_methods now built later via
    // `desugar_classes_method_owners::compute_method_owners_and_chain_methods`,
    // chunk 180 — needs `class_index` + `parent_map`.)
    // class_field_inits / class_field_preludes now built later via
    // `desugar_classes_field_inits::compute_class_field_default_inits`,
    // chunk 182 — needs `class_index` + `full_fields` + `&mut ast`.
    let mut appended: Vec<Stmt> = Vec::new();
    // P8.2 — buffered accessor map population. Each entry is
    // (class_name, property_name, synthesised_fn_name). Drained into
    // `ast.accessor_getters` / `ast.accessor_setters` at the end of
    // desugar_classes, after the borrow on `ast` is released.
    let mut accessor_getter_records: Vec<(String, PropKey, String)> = Vec::new();
    let mut accessor_setter_records: Vec<(String, PropKey, String)> = Vec::new();
    // M-OO.4 — accumulator for `let __sf_<C>__<name>: T = init;`
    // declarations. These get **prepended** to `ast.stmts` (not
    // appended) so the synthetic `main` fn runs them before any
    // user top-level code; the alternative leaves `check()` reading
    // uninitialized slots when the user-visible call comes first
    // in source order.

    // NO empty-index early return: for years every program carried at
    // least the injected Error hierarchy, so a zero-class AST never
    // reached this line and a `class_index.is_empty()` bail looked
    // free. The pipeline has since grown sub-passes that rewrite
    // NON-class code too (pass2 walks the whole expr arena; the
    // fnexpr-this promotion downstream depends on those rewrites),
    // and the first genuinely class-free programs (RFC
    // 20260825-injection-reachability skip shape) lost them — every
    // this-reading fn died with "bare `this` reached check.rs". Each
    // sub-pass already no-ops its class-keyed half on an empty index.
    super::desugar_classes_elements::dedupe_class_elements(ast);
    let mut class_index = super::desugar_classes_index::snapshot_class_index(ast);
    // M5.N — a builtin parent (`extends Object`) strips to base-class
    // shape before any pass keyed on `parent` runs (see the sibling's
    // module doc for the two seams it handles).
    super::desugar_classes_builtin_heritage::strip_builtin_heritage(ast, &mut class_index);
    super::desugar_classes_default_ctor::synthesize_derived_default_ctors(ast, &mut class_index);
    // 刀 3 (RFC 20260718-error-message-own-prop) — super-less derived
    // USER ctors get the no-super ReferenceError raiser appended
    // (§9.2.2 this-TDZ); must run after the default-ctor synthesis
    // (those carry a super() and are skipped) and before the freeze.
    super::desugar_classes_super::append_no_super_throw(ast, &mut class_index);
    let this_tdz = super::desugar_classes_this_tdz::plan(ast, &mut class_index);
    // RFC 20260820-ctor-return-override 刀 1 — name the chains whose
    // ctor ABI has to answer a value (§10.2.2 step 13). Reads the
    // index after the default-ctor synthesis so a ctor-less derived
    // class is already carrying its `super(…)` body, and before the
    // freeze so the later passes all see the same set.
    super::desugar_classes_ctor_return::collect(ast, &class_index);
    let class_index = class_index;

    // M-OO.6 — abstract-class collection + validation extracted to
    // `desugar_classes_abstract.rs` sub-sibling (chunk 178, 2026-06-28).
    // Pure read of ast.stmts + class_index; panics on invalid abstract
    // usage (concrete-class abstract-method, missing override).
    let abstract_classes =
        super::desugar_classes_abstract::collect_abstract_classes(ast, &class_index);

    // Build the parent map and validate the inheritance graph.
    let mut parent_map: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    for (_, cname, _tp, parent, _, _, _, _, _, _) in &class_index {
        parent_map.insert(cname.clone(), parent.clone());
    }
    // Make the parent map visible to post-desugar passes so `instanceof`
    // can walk the chain when the LHS is a subclass and the RHS names
    // an ancestor.
    ast.class_parents = parent_map.clone();
    super::private_brand::run(ast, &class_index);
    for (_, cname, tp, ..) in &class_index {
        ast.class_type_params.insert(cname.clone(), tp.clone());
    }
    super::desugar_classes_ctor_twin::close_real_parents_over_chain(ast, &parent_map);
    // method_owners populated below; expose only the multi-owner entries
    // so ssa_lower's `__dispatch_` interception is a constant-time
    // contains lookup.
    // (Filled in after the per-method walk; HashMap moved at end.)
    // Field-inheritance flattening + declaration-order validation
    // extracted to `desugar_classes_fields.rs` sub-sibling (chunk 179,
    // 2026-06-28). Pure data computation over `class_index`; panics on
    // forward-reference parent / subclass field collision.
    let full_fields = super::desugar_classes_fields::compute_full_fields(ast, &class_index);

    // Method-owner table + Phase I.1 chain-classification extracted to
    // `desugar_classes_method_owners.rs` sub-sibling (chunk 180,
    // 2026-06-28). Pure data computation over `class_index` +
    // `parent_map`; no `&mut Ast` mutation.
    let (method_owners, chain_methods) =
        super::desugar_classes_method_owners::compute_method_owners_and_chain_methods(
            &class_index,
            &parent_map,
        );

    // Phase H.3.b — `__dispatch_<method>` synthesis extracted to
    // `desugar_classes_dispatch.rs` sub-sibling (chunk 181, 2026-06-28).
    // Mutates `ast.exprs` via `add_expr` + pushes Stmt::FnDecl per
    // chain method into `appended`. SSA intercepts the stub
    // (`__dispatch_` interception in lower_expr Call arm) so the body
    // never runs at runtime.
    super::desugar_classes_dispatch::emit_dispatch_method_stubs(
        ast,
        &class_index,
        &method_owners,
        &chain_methods,
        &mut appended,
    );

    // Per-class default-init synthesis (type_alias_fields snapshot +
    // class_field_inits + class_field_preludes) extracted to
    // `desugar_classes_field_inits.rs` sub-sibling (chunk 182,
    // 2026-06-28). Mutates `ast.exprs` via `default_init_for_field`'s
    // `add_expr` calls.
    let (class_field_inits, class_field_preludes) =
        super::desugar_classes_field_inits::compute_class_field_default_inits(
            ast,
            &class_index,
            &full_fields,
        );

    // Pass 1.5 + Pass 1.6 — super-call rewriting (super(args) in ctor
    // bodies + super.<m>(args) in method bodies). Extracted to
    // `desugar_classes_super.rs` sub-sibling (chunk 176, 2026-06-28).
    super::desugar_classes_super::rewrite_super_ctor_calls(ast, &class_index);
    let dyn_home = super::desugar_classes_super::rewrite_super_method_calls(ast, &class_index);
    // Pass 1.7 — SuperProperty reads/writes (`super.x` / `super[k]`,
    // parser-encoded off the `__superbase__` marker). Must also run
    // before Pass 2: the rewrite mints `Expr::This` nodes.
    super::desugar_classes_super_prop::rewrite_super_prop_sites(ast, &class_index, dyn_home);
    super::desugar_classes_this_tdz::apply(ast, this_tdz);

    // Pass 2 — rewrite the expression arena (This → __this, New →
    // __new_<C> Call, Member-call → __cm_/__dispatch_ Call).
    // Extracted to `desugar_classes_pass2.rs` sub-sibling (chunk 183,
    // 2026-06-28). In-place mutation of `ast.exprs[i]` + add_expr
    // calls; existing ExprIds keep their meaning.
    super::desugar_classes_pass2::rewrite_expr_arena_pass2(
        ast,
        &class_index,
        &method_owners,
        &chain_methods,
    );

    // Pass 2.5 — build static-member rewrite table (consumed by
    // Pass 3 below). Extracted to `desugar_classes_statics.rs`
    // sub-sibling (chunk 177, 2026-06-28) — pure function, no
    // `&mut Ast` mutation.
    let (static_member_rewrites, static_accessor_rewrites, inherited_static_owners) =
        super::desugar_classes_statics::build_static_member_rewrites(ast, &class_index);

    // Pass 3 — ClassDecl → TypeDecl replacement + per-class synthetic
    // FnDecl emission (ctor / methods / factory / static fields /
    // static methods) extracted to `desugar_classes_pass3.rs`
    // sub-sibling (chunk 184, 2026-06-28). Caller flushes `appended`
    // into `ast.stmts` immediately after.
    super::desugar_classes_pass3::rewrite_classdecls_pass3(
        ast,
        &class_index,
        &full_fields,
        &class_field_inits,
        &class_field_preludes,
        &mut appended,
        &mut accessor_getter_records,
        &mut accessor_setter_records,
    );

    ast.stmts.extend(appended);

    // RFC 20260718-accessor-reify 刀 3 — static-accessor rewrites run
    // BEFORE the data-member flat rewrite: the assign walk consumes
    // whole `C.p = v` nodes (setter call), then the read walk turns
    // the remaining `C.p` Members into getter calls; neither shape
    // survives into the Ident rewrite below.
    rewrite_static_accessor_accesses(ast, &static_accessor_rewrites, &class_index);
    super::desugar_classes_static_route::route_static_calls(ast, &inherited_static_owners);
    rewrite_static_member_accesses(ast, &static_member_rewrites, &class_index);
    reject_abstract_new(ast, &abstract_classes);
    super::desugar_classes_method_owners::finalize_side_tables(
        ast,
        method_owners,
        accessor_getter_records,
        accessor_setter_records,
    );
    factory_unset::seed_written_fields_unset(ast);
}

/// RFC 20260718-accessor-reify 刀 3 — rewrite static-accessor
/// accesses to face CALLS: `C.p = v` becomes `__sm_<C>__<p>_set(v)`
/// (whole-Assign replacement, walked FIRST so the read pass below
/// never sees its target Member), then every remaining `C.p` Member
/// becomes `__sm_<C>__<p>_get()`. A write with no setter / a read
/// with no getter keeps its node, and a kept write TARGET is not a
/// read: the read pass skips it, so `C.#g = v` over a getter-only
/// accessor stays an assignment and PrivateSet raises its TypeError
/// at run time (§7.3.32 step 5.c) instead of the target turning into
/// a getter call.
/// Compound assigns (`C.p += v`) desugar to `C.p = C.p + v` upstream,
/// so the simple shape covers them.
///
/// §15.7.14 — an accessor whose name is ALSO a static field is the
/// accessor only inside the static initializers up to and including
/// the field's own (`static_field_shadow_windows`); every other site
/// keeps its Member and falls to the data rewrite, where the field's
/// `__sf_` entry answers — the same positional rule the method case
/// takes, and the class object's own entry follows (the field define
/// lands after the accessor reify).
fn rewrite_static_accessor_accesses(
    ast: &mut Ast,
    accessor_rewrites: &super::desugar_classes_statics::StaticAccessorRewrites,
    class_index: &[super::desugar_classes_super::ClassIndexEntry],
) {
    if accessor_rewrites.is_empty() {
        return;
    }
    let windows = super::desugar_classes_statics::static_field_shadow_windows(ast, class_index);
    // K4-3 — a unit class's accessor face takes the evaluation's
    // class object first (K4-1's `__unit` param), so the call passes
    // the binding: `C.p` → `__sm_C__p_get(C)`, `C.p = v` →
    // `__sm_C__p_set(C, v)`.
    let member_face = |ast: &Ast, eid: ExprId, want_get: bool| -> Option<(String, Vec<ExprId>)> {
        let Expr::Member { obj, name } = ast.get_expr(eid) else {
            return None;
        };
        let Expr::Ident(class_name) = ast.get_expr(*obj) else {
            return None;
        };
        let key = (class_name.clone(), PropKey::from(name));
        if let Some(ranges) = windows.get(&key)
            && !ranges.iter().any(|(lo, hi)| *lo <= eid.0 && eid.0 < *hi)
        {
            return None;
        }
        let (g, s) = accessor_rewrites.get(&key)?;
        let face = if want_get { g.clone() } else { s.clone() }?;
        let head = if ast.eval_unit_classes.contains(class_name) {
            vec![*obj]
        } else {
            Vec::new()
        };
        Some((face, head))
    };
    // Write pass — whole-Assign nodes first.
    let mut kept_targets: std::collections::HashSet<ExprId> = std::collections::HashSet::new();
    for i in 0..ast.exprs.len() {
        let (target, value) = match &ast.exprs[i] {
            Expr::Assign { target, value } => (*target, *value),
            _ => continue,
        };
        if let Some((set_fn, mut args)) = member_face(ast, target, false) {
            let callee = ast.add_expr(Expr::Ident(set_fn));
            args.push(value);
            ast.exprs[i] = Expr::Call { callee, args };
        } else {
            kept_targets.insert(target);
        }
    }
    // Read pass — remaining Members.
    for i in 0..ast.exprs.len() {
        let eid = ExprId(i as u32);
        if kept_targets.contains(&eid) {
            continue;
        }
        if let Some((get_fn, args)) = member_face(ast, eid, true) {
            let callee = ast.add_expr(Expr::Ident(get_fn));
            ast.exprs[i] = Expr::Call { callee, args };
        }
    }
}

/// M-OO.4 — rewrite `<ClassName>.<member>` accesses to flat
/// `__sf_<C>__<member>` / `__sm_<C>__<member>` Idents wherever they
/// appear (all bodies live in the arena-allocated `ast.exprs`, so one
/// arena walk covers every site). In-place and shape-preserving — the
/// Member's ExprId is reused for the new Ident, so downstream passes
/// (lift_arrow_fns, check.rs, ssa_lower) see plain Idents resolved
/// through the top-level fn / globals tables.
fn rewrite_static_member_accesses(
    ast: &mut Ast,
    static_member_rewrites: &std::collections::HashMap<(String, PropKey), String>,
    class_index: &[super::desugar_classes_super::ClassIndexEntry],
) {
    if static_member_rewrites.is_empty() {
        return;
    }
    // A name that is both a static method and a static field: the
    // sites inside the static initializers up to the field's own
    // still mean the method (§15.7.14; sibling doc).
    let positional =
        super::desugar_classes_statics::positional_static_method_sites(ast, class_index);
    // K4-1 — a unit class's static method takes the evaluation's
    // class object first, so `C.m(a)` becomes `__sm_C__m(C, a)` (the
    // binding `C` is the evaluation) and a VALUE read `C.m` stays a
    // Member: the class object carries a per-evaluation face for it.
    // Which Member is a callee is known only from its Call.
    let callee_of: std::collections::HashMap<u32, usize> = ast
        .exprs
        .iter()
        .enumerate()
        .filter_map(|(ci, e)| match e {
            Expr::Call { callee, .. } => Some((callee.0, ci)),
            _ => None,
        })
        .collect();
    for i in 0..ast.exprs.len() {
        let replacement = match &ast.exprs[i] {
            Expr::Member { obj, name } => {
                if let Some(method) = positional.get(&(i as u32)) {
                    Some((method.clone(), None))
                } else if let Expr::Ident(class_name) = &ast.exprs[obj.0 as usize] {
                    let key = (class_name.clone(), PropKey::from(name));
                    static_member_rewrites.get(&key).cloned().map(|n| {
                        let unit = ast.eval_unit_classes.contains(class_name).then_some(*obj);
                        (n, unit)
                    })
                } else {
                    None
                }
            }
            _ => None,
        };
        match replacement {
            Some((new_name, None)) => ast.exprs[i] = Expr::Ident(new_name),
            Some((new_name, Some(unit_binding))) => {
                if let Some(&ci) = callee_of.get(&(i as u32))
                    && let Expr::Call { args, .. } = &ast.exprs[ci]
                {
                    let mut new_args = Vec::with_capacity(args.len() + 1);
                    new_args.push(unit_binding);
                    new_args.extend(args.iter().copied());
                    let callee = ast.add_expr(Expr::Ident(new_name));
                    ast.exprs[ci] = Expr::Call {
                        callee,
                        args: new_args,
                    };
                }
            }
            None => {}
        }
    }
}

/// M-OO.6 — reject `new AbstractClass()` after the desugar walk
/// (abstract metadata is local to this pass; the SSA layer never sees
/// it). Walking ast.exprs catches every construction site regardless
/// of where in the tree it lives.
fn reject_abstract_new(ast: &Ast, abstract_classes: &std::collections::HashSet<String>) {
    if abstract_classes.is_empty() {
        return;
    }
    for expr in &ast.exprs {
        if let Expr::New { class_name, .. } = expr
            && abstract_classes.contains(class_name)
        {
            panic!(
                "M-OO.6: cannot instantiate abstract class `{class_name}` — use a concrete subclass"
            );
        }
    }
}
