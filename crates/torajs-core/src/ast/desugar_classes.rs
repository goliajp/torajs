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

pub fn desugar_classes(ast: &mut Ast) {
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
    let mut accessor_getter_records: Vec<(String, String, String)> = Vec::new();
    let mut accessor_setter_records: Vec<(String, String, String)> = Vec::new();
    // M-OO.4 — accumulator for `let __sf_<C>__<name>: T = init;`
    // declarations. These get **prepended** to `ast.stmts` (not
    // appended) so the synthetic `main` fn runs them before any
    // user top-level code; the alternative leaves `check()` reading
    // uninitialized slots when the user-visible call comes first
    // in source order.
    let mut static_field_inits: Vec<Stmt> = Vec::new();

    let mut class_index = snapshot_class_index(ast);
    if class_index.is_empty() {
        return;
    }
    synthesize_derived_default_ctors(ast, &mut class_index);
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
    for (_, cname, _tp, parent, _, _, _, _, _) in &class_index {
        parent_map.insert(cname.clone(), parent.clone());
    }
    // Make the parent map visible to post-desugar passes so `instanceof`
    // can walk the chain when the LHS is a subclass and the RHS names
    // an ancestor.
    ast.class_parents = parent_map.clone();
    // method_owners populated below; expose only the multi-owner entries
    // so ssa_lower's `__dispatch_` interception is a constant-time
    // contains lookup.
    // (Filled in after the per-method walk; HashMap moved at end.)
    // Field-inheritance flattening + declaration-order validation
    // extracted to `desugar_classes_fields.rs` sub-sibling (chunk 179,
    // 2026-06-28). Pure data computation over `class_index`; panics on
    // forward-reference parent / subclass field collision.
    let full_fields = super::desugar_classes_fields::compute_full_fields(&class_index);

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
    super::desugar_classes_super::rewrite_super_method_calls(ast, &class_index);

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
    let static_member_rewrites =
        super::desugar_classes_statics::build_static_member_rewrites(&class_index);

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
        &mut static_field_inits,
        &mut accessor_getter_records,
        &mut accessor_setter_records,
    );

    ast.stmts.extend(appended);

    // M-OO.4 — prepend static-field LetDecls so they init before any
    // user code. Maintains insertion order across multiple classes
    // (declaration-order, source-order). Doing this AFTER
    // `ast.stmts.extend(appended)` keeps the source-position of
    // appended decls (factory / __cm_*/__sm_*) unchanged; they're
    // already at the back where check.rs / ssa_lower expect them.
    if !static_field_inits.is_empty() {
        let mut new_stmts = static_field_inits;
        new_stmts.extend(std::mem::take(&mut ast.stmts));
        ast.stmts = new_stmts;
    }

    rewrite_static_member_accesses(ast, &static_member_rewrites);
    reject_abstract_new(ast, &abstract_classes);
    finalize_side_tables(
        ast,
        method_owners,
        &chain_methods,
        accessor_getter_records,
        accessor_setter_records,
    );
}

/// Snapshot the class metadata (cloned out so `ast.stmts` can be
/// mutated in-place without aliasing). M5.2 adds `parent` to the
/// tuple — for inheritance flattening + super(args) rewriting;
/// M-OO.4 adds the static-init / static-methods slices.
/// ES §15.7.10 — a ctor-less derived class carries the implicit
/// default ctor `constructor(...args) { super(...args) }`. The static
/// world spells it concretely: params are the nearest explicit
/// ancestor ctor's params verbatim, the body a single
/// `super(<param names>)` (Pass 1.5 rewrites it into the
/// `__cm_<Parent>__ctor` call like any user-written super, and the
/// factory / `C.length` / spread lanes all follow from the filled
/// ctor). Pre-fix the factory elided the ctor call entirely —
/// `new B(7)` on `class B extends A {}` rejected loud with
/// "expected 0 argument(s)", and a zero-param parent ctor's body
/// silently never ran.
///
/// Skipped (keeping today's behavior) when the whole chain is
/// ctor-less (nothing to forward), when the class or any ancestor on
/// the walk is generic (ctor param anns may reference that class's
/// type params), or when the ancestor ctor is variadic (rest
/// forwarding needs a spread — apply_rest_args territory). The walk
/// is hop-capped by the class count; malformed parent chains fall
/// through to `compute_full_fields`' existing declaration-order
/// panic.
fn synthesize_derived_default_ctors(
    ast: &mut Ast,
    class_index: &mut [super::desugar_classes_super::ClassIndexEntry],
) {
    struct ChainInfo {
        parent: Option<String>,
        ctor_params: Option<Vec<Param>>,
        generic: bool,
    }
    let by_name: std::collections::HashMap<String, ChainInfo> = class_index
        .iter()
        .map(|(_, cname, tp, parent, _, _, ctor, _, _)| {
            (
                cname.clone(),
                ChainInfo {
                    parent: parent.clone(),
                    ctor_params: ctor.as_ref().map(|c| c.params.clone()),
                    generic: !tp.is_empty(),
                },
            )
        })
        .collect();
    let max_hops = class_index.len();
    for (_, _, type_params, parent, _, _, ctor, _, _) in class_index.iter_mut() {
        if ctor.is_some() || parent.is_none() || !type_params.is_empty() {
            continue;
        }
        let mut cur = parent.clone();
        let mut found: Option<Vec<Param>> = None;
        for _ in 0..max_hops {
            let Some(info) = cur.as_ref().and_then(|n| by_name.get(n)) else {
                break;
            };
            if info.generic {
                break;
            }
            if let Some(params) = &info.ctor_params {
                found = Some(params.clone());
                break;
            }
            cur = info.parent.clone();
        }
        let Some(params) = found else { continue };
        if params.iter().any(|p| p.is_rest) {
            continue;
        }
        let args: Vec<ExprId> = params
            .iter()
            .map(|p| ast.add_expr(Expr::Ident(p.name.clone())))
            .collect();
        let super_eid = ast.add_expr(Expr::Super { args });
        *ctor = Some(ClassCtor {
            params,
            body: vec![Stmt::Expr(super_eid)],
        });
    }
}

fn snapshot_class_index(ast: &Ast) -> Vec<super::desugar_classes_super::ClassIndexEntry> {
    ast.stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Stmt::ClassDecl {
                name,
                type_params,
                parent,
                is_abstract: _,
                fields,
                static_init,
                ctor,
                methods,
                static_methods,
            } => Some((
                i,
                name.clone(),
                type_params.clone(),
                parent.clone(),
                fields.clone(),
                static_init.clone(),
                ctor.clone(),
                methods.clone(),
                static_methods.clone(),
            )),
            _ => None,
        })
        .collect()
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
    static_member_rewrites: &std::collections::HashMap<(String, String), String>,
) {
    if static_member_rewrites.is_empty() {
        return;
    }
    for i in 0..ast.exprs.len() {
        let replacement = match &ast.exprs[i] {
            Expr::Member { obj, name } => {
                if let Expr::Ident(class_name) = &ast.exprs[obj.0 as usize] {
                    let key = (class_name.clone(), name.clone());
                    static_member_rewrites.get(&key).cloned()
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(new_name) = replacement {
            ast.exprs[i] = Expr::Ident(new_name);
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

/// Tail side-table population: multi-owner `method_owners` for the
/// `__dispatch_<M>` runtime-tag dispatch (single-owner entries are
/// statically rewritten already), T-24 name-sorted stable vtable slots
/// in `method_index`, and the P8.2 accessor-record drain (done last so
/// the maps are complete before any check / lower pass runs; duplicate
/// (class, prop) entries are overwritten — the FnDecl-level
/// redeclaration error halts the pipeline before they're consulted).
fn finalize_side_tables(
    ast: &mut Ast,
    method_owners: std::collections::HashMap<String, Vec<String>>,
    chain_methods: &std::collections::HashSet<String>,
    accessor_getter_records: Vec<(String, String, String)>,
    accessor_setter_records: Vec<(String, String, String)>,
) {
    ast.method_owners = method_owners
        .into_iter()
        .filter(|(_, owners)| owners.len() > 1)
        .collect();

    let mut chain_methods_sorted: Vec<&String> = chain_methods.iter().collect();
    chain_methods_sorted.sort();
    ast.method_index = chain_methods_sorted
        .into_iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i as u32))
        .collect();

    for (cname, prop, fn_name) in accessor_getter_records {
        ast.accessor_getters.insert((cname, prop), fn_name);
    }
    for (cname, prop, fn_name) in accessor_setter_records {
        ast.accessor_setters.insert((cname, prop), fn_name);
    }
}
