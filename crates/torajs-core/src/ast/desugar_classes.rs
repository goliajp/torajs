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

use super::desugar_classes_emit::{emit_class_instance_methods, emit_class_static_methods};
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

    // Snapshot the class metadata first (cloned out so we can mutate
    // ast.stmts in-place without aliasing). M5.2 adds `parent` to the
    // tuple — for inheritance flattening + super(args) rewriting.
    // M-OO.4 adds the static-fields / static-methods slices for the
    // post-collect emission of `__sf_<C>__<n>` LetDecls and
    // `__sm_<C>__<m>` FnDecls.
    let class_index: Vec<(
        usize,
        String,
        Vec<String>, // type_params
        Option<String>,
        Vec<(String, String)>,
        Vec<StaticInit>, // static_init (Field | Block, source-ordered)
        Option<ClassCtor>,
        Vec<ClassMethod>,
        Vec<ClassMethod>, // static_methods
    )> = ast
        .stmts
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
        .collect();

    if class_index.is_empty() {
        return;
    }

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

    // Pass 3 — rewrite the stmt list. Replace each ClassDecl in-place
    // with its TypeDecl (using the flattened field list so subclasses
    // carry parent fields too), and accumulate the generated FnDecls.
    for (
        idx,
        cname,
        type_params,
        _parent,
        _own_fields,
        static_init,
        ctor,
        methods,
        static_methods,
    ) in class_index
    {
        let type_decl = Stmt::TypeDecl {
            name: cname.clone(),
            type_params: type_params.clone(),
            fields: full_fields[&cname].clone(),
        };
        ast.stmts[idx] = type_decl;

        // For generic classes, the `__this` type ann must reference
        // the instantiated form, e.g. `Wrapper<T>` not bare `Wrapper`.
        let this_ann = if type_params.is_empty() {
            cname.clone()
        } else {
            format!("{cname}<{}>", type_params.join("|"))
        };

        // Constructor → C__ctor(__this: C, params...): void { body }
        //
        // V3-18 wedge — always emit a `__cm_<C>__ctor` symbol, even
        // when the user wrote no explicit constructor. Per ES spec
        // §15.7.10 every class has a default ctor (empty body, or
        // `super(...args)` for subclasses); subclass `super()` calls
        // need a callable parent ctor, and pre-fix tora panicked at
        // typecheck with `unknown identifier __cm_<Parent>__ctor`
        // when the parent had no explicit constructor.
        //
        // The factory still elides the no-ctor call (build_factory_body
        // gates on `ctor.is_some()`), so this only adds an unreferenced
        // empty fn for ctor-less classes — observable only via
        // `super()` in a subclass, which is exactly what we want.
        let mut ctor_params_for_factory: Vec<Param> = Vec::new();
        let (ctor_body, ctor_user_params): (Vec<Stmt>, Vec<Param>) = if let Some(c) = &ctor {
            ctor_params_for_factory = c.params.clone();
            (c.body.clone(), c.params.clone())
        } else {
            (Vec::new(), Vec::new())
        };
        let mut params: Vec<Param> = Vec::with_capacity(ctor_user_params.len() + 2);
        params.push(Param {
            name: "__this".into(),
            type_ann: Some(this_ann.clone()),
            default: None,
            is_rest: false,
        });
        // P4.5 — `__new_target: any` carries the class function that
        // was used at the `new` site. Threaded through `super()` so
        // chain ancestors see the actual derived class, not the
        // static ctor owner. Used inside ctor body via the
        // Expr::NewTarget → Ident("__new_target") rewrite (Pass 2).
        params.push(Param {
            name: "__new_target".into(),
            type_ann: Some("any".into()),
            default: None,
            is_rest: false,
        });
        params.extend(ctor_user_params);
        appended.push(Stmt::FnDecl {
            name: format!("__cm_{cname}__ctor"),
            type_params: type_params.clone(),
            params,
            return_type: Some("void".into()),
            body: ctor_body,
            is_generator: false,
        });

        // Methods → __cm_C__m(__this: C, params...): R { body }
        // See `ast/desugar_classes_emit.rs`.
        emit_class_instance_methods(
            ast,
            &methods,
            &cname,
            &type_params,
            &this_ann,
            &full_fields[&cname],
            &mut accessor_getter_records,
            &mut accessor_setter_records,
            &mut appended,
        );

        // Factory: __new_C(ctor_params...): C {
        //   let __this: C = { f0: <init>, f1: <init>, ... };
        //   C__ctor(__this, ctor_params...);   // only if a ctor was declared
        //   return __this;
        // }
        let factory_body = build_factory_body(
            ast,
            &cname,
            &type_params,
            &class_field_inits[&cname],
            class_field_preludes
                .get(&cname)
                .cloned()
                .unwrap_or_default(),
            ctor.as_ref(),
        );
        appended.push(Stmt::FnDecl {
            name: format!("__new_{cname}"),
            type_params: type_params.clone(),
            params: ctor_params_for_factory,
            return_type: Some(this_ann.clone()),
            body: factory_body,
            is_generator: false,
        });

        // M-OO.4 — emit `let __sf_<C>__<name>: T = init;` for each
        // static field. const-form (mutable=false) so K.4 refcount
        // globals accept it. The `init` ExprId is reused — desugar
        // runs before any pass that might mutate the expression
        // referenced by it.
        //
        // P8.3-A3 — `static { ... }` blocks share this walk with
        // static fields so spec §15.7.10 source-order interleaving
        // is preserved (a `static x = 1; static { use(C.x) } static
        // y = 2; static { use(C.y) }` sequence emits four entries
        // into `static_field_inits` in that exact order). Each
        // Block desugars to a named-fn `__sb_<C>__<idx>` (mirrors
        // `__sm_<C>__<m>`, no `__this` param, void return) appended
        // to `ast.stmts`, plus a top-level `Stmt::Expr(Call(...))`
        // at the block's source-order position.
        //
        // CRITICAL: static field LetDecls + static block Calls go
        // into `static_field_inits` (NOT `appended`) so they can be
        // prepended to `ast.stmts` before the user's top-level code
        // runs. Otherwise the synth main fn would call `check()`
        // BEFORE the static slot was initialized — every read of
        // `Counter.label` inside `check()` would see the slot's
        // null/zero default. This was a real silent leak +
        // correctness bug uncovered by the m-oo-04-static
        // `leaks --atExit` audit.
        for (block_idx, si) in static_init.iter().enumerate() {
            match si {
                StaticInit::Field(sf) => {
                    // V3-18 m1.h.26 — static fields with primitive Copy
                    // types (number / boolean / int width-specifiers) are
                    // mutable by default (`Counter.value = 5` is valid
                    // TS). Refcount-typed fields (string / string[] /
                    // Foo[] / etc) stay `mutable: false` because
                    // ssa_lower's globals registry can't yet handle
                    // mutable refcount globals — Str writes would need
                    // ARC-dec-old + ARC-inc-new + writeback to the slot,
                    // which the K.6 globals path doesn't yet emit. Marking
                    // those as mutable makes ssa_lower skip them from
                    // globals entirely (line ~3947), and the read path
                    // then fails with "unknown ident".
                    let is_copy_prim = matches!(
                        sf.type_ann.as_str(),
                        "number" | "boolean" | "i64" | "f64" | "bool" | "i32"
                    );
                    static_field_inits.push(Stmt::LetDecl {
                        mutable: is_copy_prim,
                        name: format!("__sf_{cname}__{}", sf.name),
                        type_ann: Some(sf.type_ann.clone()),
                        init: sf.init,
                        is_var: false,
                    });
                }
                StaticInit::Block(stmts) => {
                    let fn_name = format!("__sb_{cname}__{block_idx}");
                    appended.push(Stmt::FnDecl {
                        name: fn_name.clone(),
                        type_params: type_params.clone(),
                        params: Vec::new(),
                        return_type: Some("void".into()),
                        body: stmts.clone(),
                        is_generator: false,
                    });
                    let callee_id = ast.add_expr(Expr::Ident(fn_name));
                    let call_id = ast.add_expr(Expr::Call {
                        callee: callee_id,
                        args: Vec::new(),
                    });
                    static_field_inits.push(Stmt::Expr(call_id));
                }
            }
        }

        // M-OO.4 — emit `function __sm_<C>__<name>(...): R { body }`
        // for each static method. See `ast/desugar_classes_emit.rs`.
        emit_class_static_methods(ast, &static_methods, &cname, &type_params, &mut appended);
    }

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

    // M-OO.4 — rewrite `<ClassName>.<member>` accesses to flat
    // `__sf_<C>__<member>` / `__sm_<C>__<member>` Idents wherever
    // they appear in the program (top-level + every fn body / arrow
    // body / nested struct field initializer — all live in
    // `ast.exprs` since exprs are arena-allocated). This walks the
    // arena once; the rewrite is in-place and shape-preserving (a
    // Member is one ExprId; the new Ident is the same ExprId with a
    // new variant). Downstream passes (lift_arrow_fns, check.rs,
    // ssa_lower) see plain Idents and resolve them through the
    // top-level fn / globals tables already populated above.
    if !static_member_rewrites.is_empty() {
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

    // M-OO.6 — reject `new AbstractClass()` after the desugar walk
    // (abstract metadata is local to this pass; the SSA layer never
    // sees it). Walking ast.exprs catches every construction site
    // regardless of where in the tree it lives.
    if !abstract_classes.is_empty() {
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

    // Hand multi-owner method_owners to ssa_lower for the
    // `__dispatch_<M>` runtime-tag dispatch. Single-owner entries are
    // dropped since they don't need runtime resolution (already
    // statically rewritten unless the builtin-name guard skipped them,
    // in which case ssa_lower's sibling-class path picks them up via
    // the Type::Obj match — see the (Expr::Member ...) Call arm in
    // lower_expr).
    ast.method_owners = method_owners
        .into_iter()
        .filter(|(_, owners)| owners.len() > 1)
        .collect();

    /* T-24 — assign each chain method a stable vtable slot. Sorted
     * by name so codegen stays deterministic; the index becomes the
     * per-class vtable's `[N x ptr]` slot offset (in u64 units). */
    let mut chain_methods_sorted: Vec<&String> = chain_methods.iter().collect();
    chain_methods_sorted.sort();
    ast.method_index = chain_methods_sorted
        .into_iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i as u32))
        .collect();

    // P8.2 — drain the accessor records into Ast's side-channel maps.
    // Done at the tail so the map is complete before any check / lower
    // pass runs. Duplicate entries (same (class, prop) declared twice
    // as a getter, etc.) are silently overwritten — the parser already
    // would have produced two ClassMethod entries that desugar emits
    // with the same FnDecl name, which the existing dedup at the
    // FnDecl level catches as "redeclaration". The maps just point
    // at the final winner, which is fine since the FnDecl-level
    // error halts the pipeline before any of this is consulted.
    for (cname, prop, fn_name) in accessor_getter_records {
        ast.accessor_getters.insert((cname, prop), fn_name);
    }
    for (cname, prop, fn_name) in accessor_setter_records {
        ast.accessor_setters.insert((cname, prop), fn_name);
    }
}
