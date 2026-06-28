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
    // method name → ordered list of declaring classes. Source order
    // (deepest sub last) — this matters for dispatcher emission since
    // we walk in reverse to check the deepest class first. Tracks
    // every class that declares a method body, including overrides.
    let mut method_owners: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut class_field_inits: std::collections::HashMap<String, Vec<(String, ExprId)>> =
        std::collections::HashMap::new();
    let mut class_field_preludes: std::collections::HashMap<String, Vec<Stmt>> =
        std::collections::HashMap::new();
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

    // M-OO.6 — collect abstract-class names + per-class abstract-method
    // names. Concrete subclasses must override every inherited abstract;
    // `new` of an abstract class is rejected (in check.rs). Side-channel
    // (HashSet / HashMap) instead of inflating class_index's tuple.
    let mut abstract_classes: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut abstract_methods: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for s in ast.stmts.iter() {
        if let Stmt::ClassDecl {
            name,
            is_abstract,
            methods,
            ..
        } = s
        {
            if *is_abstract {
                abstract_classes.insert(name.clone());
            }
            let abs: Vec<String> = methods
                .iter()
                .filter(|m| m.is_abstract)
                .map(|m| m.name.clone())
                .collect();
            if !abs.is_empty() {
                abstract_methods.insert(name.clone(), abs);
            }
            // Abstract method only allowed inside abstract class.
            // (Parser already rejects this for the immediate case, but
            // a desugar-time double-check catches programmatically-built
            // classes from upstream desugars.)
            if !is_abstract && methods.iter().any(|m| m.is_abstract) {
                panic!("M-OO.6: concrete class `{name}` cannot declare abstract methods");
            }
        }
    }
    // Walk every concrete class's inheritance chain (root → leaf,
    // accumulating "unimplemented" abstract names along the way) and
    // verify that none survive into the concrete leaf.
    for (_, cname, _, _, _, _, _, _, _) in &class_index {
        if abstract_classes.contains(cname) {
            continue;
        }
        let mut chain: Vec<String> = Vec::new();
        let mut cur: Option<String> = Some(cname.clone());
        while let Some(c) = cur {
            chain.push(c.clone());
            cur = class_index
                .iter()
                .find(|t| t.1 == c)
                .and_then(|t| t.3.clone());
        }
        chain.reverse();
        let mut unimplemented: std::collections::HashSet<String> = std::collections::HashSet::new();
        for cls in &chain {
            if let Some(absms) = abstract_methods.get(cls) {
                for m in absms {
                    unimplemented.insert(m.clone());
                }
            }
            if let Some(t) = class_index.iter().find(|t| &t.1 == cls) {
                let cls_methods = &t.7;
                for m in cls_methods.iter() {
                    if !m.is_abstract {
                        unimplemented.remove(&m.name);
                    }
                }
            }
        }
        if !unimplemented.is_empty() {
            let mut names: Vec<&String> = unimplemented.iter().collect();
            names.sort();
            panic!("M-OO.6: concrete class `{cname}` must override abstract method(s): {names:?}");
        }
    }

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
    // Detect missing-parent and cycle errors. We don't allow forward
    // references to classes that come later in source order — every
    // ancestor must be declared before its descendants. This keeps
    // field-flattening + factory-emission order trivially correct.
    let mut declared_so_far: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, cname, _tp, parent, _, _, _, _, _) in &class_index {
        if let Some(p) = parent {
            if !declared_so_far.contains(p) {
                panic!(
                    "M5.2: `{cname} extends {p}` — parent class `{p}` must be declared \
                     before `{cname}` (and must exist as a class, not a type alias)"
                );
            }
        }
        declared_so_far.insert(cname.clone());
    }

    // Compute the flattened (full) field list for each class along the
    // inheritance chain: parent's fields followed by self's. This is the
    // layout that `type C = { ... }` will declare and the factory will
    // default-initialize.
    let mut full_fields: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();
    for (_, cname, _tp, parent, fields, _, _, _, _) in &class_index {
        let mut combined: Vec<(String, String)> = Vec::new();
        if let Some(p) = parent {
            // Parent must be in full_fields by now (declaration order check
            // above guarantees this).
            let pfields = full_fields.get(p).unwrap_or_else(|| {
                panic!("internal: parent `{p}` of `{cname}` had no flattened fields")
            });
            combined.extend(pfields.iter().cloned());
        }
        for (fn_, ft) in fields {
            // Subclass fields must not collide with parent fields. (TS
            // allows shadowing with the same type, but M5.2.a keeps this
            // simple — disallow.)
            if combined.iter().any(|(n, _)| n == fn_) {
                panic!(
                    "M5.2: subclass `{cname}` redeclares parent field `{fn_}` — \
                     not yet supported"
                );
            }
            combined.push((fn_.clone(), ft.clone()));
        }
        full_fields.insert(cname.clone(), combined);
    }

    // Build the method dispatch table. Phase H.3.b: ancestor-descendant
    // overrides go through a generated `__dispatch_<method>` fn (walks
    // runtime class tag). Phase I.1 lifted the sibling-collision panic:
    // unrelated classes are allowed to share a method name now — call
    // sites pick the right `__cm_<C>__M` from obj's static type at SSA
    // lower time (handled by the `Type::Obj` Member-call arm).
    for (_, cname, _tp, _, _, _, _, methods, _) in &class_index {
        for m in methods {
            // P8.2 — accessor methods (`get X` / `set X`) don't go through
            // the `__dispatch_<M>` method-dispatch path; `c.X` /
            // `c.X = v` are Member access / Assign sites that ssa_lower
            // routes to the synthesised `__cm_<C>__X_get` / `_set`
            // directly via the accessor side-channel maps. Keeping them
            // out of `method_owners` prevents (a) a spurious
            // `__dispatch_X` body that calls the never-emitted
            // `__cm_<C>__X` (collision with the regular-method name)
            // and (b) a same-name getter+setter pair (or accessor +
            // regular method) from being miscounted as the override-
            // case multi-owner chain.
            if m.accessor_kind.is_some() {
                continue;
            }
            method_owners
                .entry(m.name.clone())
                .or_default()
                .push(cname.clone());
        }
    }
    // Phase I.1 — categorize each multi-owner method. If owners[0]
    // (source-first, the topmost in source order) is an ancestor of
    // every other owner, the method forms a single inheritance chain
    // and gets the `__dispatch_<M>` runtime-tag dispatcher (override
    // case). Otherwise (siblings in unrelated hierarchies, or a mix),
    // call sites stay as Member-shape and ssa_lower picks the right
    // `__cm_<C>__M` from obj's static type.
    let chain_methods: std::collections::HashSet<String> = method_owners
        .iter()
        .filter(|(_, owners)| owners.len() > 1)
        .filter(|(_, owners)| {
            let base = &owners[0];
            owners
                .iter()
                .skip(1)
                .all(|sub| method_owner_is_in_chain(&parent_map, base, sub))
        })
        .map(|(n, _)| n.clone())
        .collect();

    // Phase H.3.b — emit `__dispatch_<method>(__this, args...)` for every
    // method whose name has multiple owners (the override case). Body is
    // an instanceof-chain checking subclasses deepest-first, falling
    // through to the base owner's `__cm_<Base>__<method>`. Single-owner
    // methods stay on the static `__cm_<Owner>__M` path — no dispatcher
    // fn, no extra indirection.
    for (m_name, owners) in &method_owners {
        if !chain_methods.contains(m_name) {
            continue;
        }
        // Locate the base owner's method to copy its signature.
        let base_owner = &owners[0];
        let (_, _, base_tp, _, _, _, _, base_methods, _) = class_index
            .iter()
            .find(|(_, n, ..)| n == base_owner)
            .expect("base owner must exist in class_index");
        let base_method = base_methods
            .iter()
            .find(|m| &m.name == m_name)
            .expect("base owner declared the method by construction");
        // Dispatcher params: `__this: Base, ...method_params`.
        let mut params: Vec<Param> = Vec::with_capacity(base_method.params.len() + 1);
        let this_ann = if base_tp.is_empty() {
            base_owner.clone()
        } else {
            format!("{base_owner}<{}>", base_tp.join("|"))
        };
        params.push(Param {
            name: "__this".into(),
            type_ann: Some(this_ann),
            default: None,
            is_rest: false,
        });
        params.extend(base_method.params.iter().cloned());
        // Body is a typecheck-clean stub that just forwards to the base
        // owner's `__cm_<Base>__M` — passing `__this: Base` to a fn
        // expecting `__this: Base` typechecks fine, and the SSA layer
        // bypasses this body entirely (see `__dispatch_` interception
        // in ssa_lower's Call arm). The stub is what tr would do if
        // override were ignored; the real virtual dispatch happens at
        // SSA level where untyped pointer args dodge the contravariance
        // problem (subclass __cm fns expect __this: Sub which the
        // typechecker won't widen Animal → Sub for, even though the
        // runtime layout is compatible).
        let mut body: Vec<Stmt> = Vec::new();
        let stub_callee = ast.add_expr(Expr::Ident(format!("__cm_{base_owner}__{m_name}")));
        let stub_this = ast.add_expr(Expr::Ident("__this".into()));
        let mut stub_args: Vec<ExprId> = Vec::with_capacity(base_method.params.len() + 1);
        stub_args.push(stub_this);
        for p in &base_method.params {
            stub_args.push(ast.add_expr(Expr::Ident(p.name.clone())));
        }
        let stub_call = ast.add_expr(Expr::Call {
            callee: stub_callee,
            args: stub_args,
        });
        body.push(Stmt::Return(Some(stub_call)));
        appended.push(Stmt::FnDecl {
            name: format!("__dispatch_{m_name}"),
            type_params: base_tp.clone(),
            params,
            return_type: base_method.return_type.clone(),
            body,
            is_generator: false,
        });
    }

    // Build a snapshot of every TypeDecl's field layout. Used by the
    // default-init helper below so a class field whose type is a type
    // alias (`type Step = { value: number, done: boolean }`) gets a
    // structurally-correct zero rather than a Number(0).
    let mut type_alias_fields: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();
    for s in &ast.stmts {
        if let Stmt::TypeDecl { name, fields, .. } = s {
            type_alias_fields.insert(name.clone(), fields.clone());
        }
    }
    let combined_fields_map = full_fields.clone();

    // For each class, build the list of typed default-initializer expressions
    // that the factory will use to seed the `__this` object literal. We use
    // the FLATTENED field list (parent fields + self fields) so subclass
    // factories produce a fully-initialized object.
    //
    // Empty `T[]` defaults need special handling: a bare `[]` in expression
    // position has no inferable element type. We hoist these out into a
    // typed prelude let — `let __def_arr_<field>: T[] = []` — and use the
    // ident as the field init. The let-binding's annotation gives ssa-lower
    // enough context to emit a typed `arr_alloc(0)`.
    //
    // Class- or alias-typed fields recursively expand into a nested
    // ObjectLit of zero-initialized children, looked up via
    // `combined_fields_map` (classes) and `type_alias_fields` (aliases).
    // This is what makes `__Gen_<X>` / `__step_<X>` fields work as
    // class fields on outer iterator classes (J.3 / I.2-inside-gen).
    for (_, cname, _tp, _, _, _, _, _, _) in &class_index {
        let combined = full_fields.get(cname).unwrap().clone();
        let mut init_pairs: Vec<(String, ExprId)> = Vec::with_capacity(combined.len());
        let mut prelude: Vec<Stmt> = Vec::new();
        for (fname, fty) in &combined {
            let id = default_init_for_field(
                ast,
                fty,
                &combined_fields_map,
                &type_alias_fields,
                &mut prelude,
                cname,
                fname,
                &mut std::collections::HashSet::new(),
            );
            init_pairs.push((fname.clone(), id));
        }
        class_field_inits.insert(cname.clone(), init_pairs);
        class_field_preludes.insert(cname.clone(), prelude);
    }

    // Pass 1.5 + Pass 1.6 — super-call rewriting (super(args) in ctor
    // bodies + super.<m>(args) in method bodies). Extracted to
    // `desugar_classes_super.rs` sub-sibling (chunk 176, 2026-06-28).
    super::desugar_classes_super::rewrite_super_ctor_calls(ast, &class_index);
    super::desugar_classes_super::rewrite_super_method_calls(ast, &class_index);

    // Pass 2 — rewrite the expression arena. Walking by index is safe
    // because we only mutate Exprs in place (or append new ones at the
    // tail; existing ExprIds keep their meaning).
    let n = ast.exprs.len();
    for i in 0..n {
        match &ast.exprs[i] {
            Expr::This => {
                ast.exprs[i] = Expr::Ident("__this".into());
            }
            // P4.5 — `new.target` deliberately NOT rewritten here.
            // Unlike `this` (which is only valid inside class methods,
            // where __this is always bound), new.target is valid in
            // ANY fn body (per spec §13.3.10) and evaluates to
            // `undefined` outside a ctor. Rewriting globally would
            // emit Ident("__new_target") in non-ctor fns where the
            // binding doesn't exist → check.rs unknown-ident reject.
            // Instead ssa_lower handles Expr::NewTarget directly:
            // if `__new_target` is a local (ctor body), load it;
            // otherwise emit ANY_UNDEF box.
            Expr::New { class_name, args } => {
                /* Builtin News (Date, ...) are rewritten by
                 * `desugar_builtin_new` BEFORE this pass, so any
                 * remaining Expr::New here is a user class. */
                /* T-26 — `new WeakRef(target)` / `new WeakMap()` /
                 * `new WeakSet()` are intercepted at SSA-lower
                 * time so target args pass as borrows (no consume
                 * → owning bindings drop normally → registry
                 * cleanup runs). Skip the generic factory rewrite
                 * here to keep the Expr::New shape intact. */
                if class_name == "WeakRef"
                    || class_name == "WeakMap"
                    || class_name == "WeakSet"
                    || class_name == "Map"
                    || class_name == "Set"
                    || class_name == "Array"
                    || class_name == "RegExp"
                {
                    /* P6.1 — `new Map()` is the same shape: SSA
                     * intercepts to emit __torajs_map_create.
                     * P6.2 — `new Set()` reuses the same Map storage,
                     * SSA-side typed as Type::Set; the Map runtime
                     * helpers serve add/has/delete/clear/size with
                     * the value-side pinned to ANY_UNDEF.
                     * P0.10 — `new Array(n)` 1-arg numeric form
                     * stays as Expr::New so ssa_lower (line 13107)
                     * intercepts it directly (the AST-Call route
                     * can't express Array<Any> with arr_id intern'd
                     * at lower time). 0-arg / ≥2-arg forms were
                     * already rewritten to array literals by
                     * desugar_builtin_new. Before this skip was
                     * added, the rewrite below sent `new Array(n)`
                     * to `__new_Array(n)`, an undefined identifier
                     * — used to be hidden by desugar_classes'
                     * early-return-on-empty-class_index, but
                     * inject_builtin_classes can leave Error /
                     * TypeError / RangeError stmts behind for
                     * runtime-throw safety, so class_index is no
                     * longer empty for typical programs. */
                    let _ = args;
                    continue;
                }
                let factory = format!("__new_{class_name}");
                let args = args.clone();
                let callee = ast.add_expr(Expr::Ident(factory));
                ast.exprs[i] = Expr::Call { callee, args };
            }
            Expr::Call { callee, args } => {
                let callee_id = *callee;
                let args_clone = args.clone();
                // Look at what the callee is pointing at.
                if let Expr::Member { obj, name } = &ast.exprs[callee_id.0 as usize] {
                    let m_name = name.clone();
                    let obj_id = *obj;
                    if let Some(owners) = method_owners.get(&m_name) {
                        // Three cases:
                        // (1) Single owner — keep static dispatch via
                        //     `__cm_<C>__<M>`, EXCEPT when the receiver
                        //     is `this.<field>` and the field is typed
                        //     as a known builtin (Array `T[]`, `string`,
                        //     `number`). Those calls dispatch to the
                        //     intrinsic, not the user class's method
                        //     — without the guard, `class C { data:
                        //     T[]; push(v) { this.data.push(v); } }`
                        //     would rewrite the inner `this.data.push`
                        //     to `__cm_C__push(this.data, v)` and
                        //     infinite-recurse.
                        // (2) Multi-owner forming a single inheritance
                        //     chain (override case) — route through
                        //     `__dispatch_<M>` runtime-tag dispatcher.
                        // (3) Multi-owner across unrelated hierarchies
                        //     (sibling collision) — leave Member as-is.
                        if owners.len() == 1 {
                            let skip_for_builtin_field =
                                crate::cm_demote::receiver_is_this_builtin_field(
                                    ast,
                                    obj_id,
                                    owners[0].as_str(),
                                    &class_index,
                                );
                            if skip_for_builtin_field {
                                // Leave Member; ssa_lower picks the
                                // builtin intrinsic from the field's
                                // actual type at SSA time.
                            } else {
                                crate::cm_demote::record_speculative_rewrite(
                                    ast,
                                    i,
                                    callee_id,
                                    obj_id,
                                    &args_clone,
                                );
                                let mangled = format!("__cm_{}__{m_name}", owners[0]);
                                let new_callee = ast.add_expr(Expr::Ident(mangled));
                                let mut new_args = Vec::with_capacity(args_clone.len() + 1);
                                new_args.push(obj_id);
                                new_args.extend(args_clone);
                                ast.exprs[i] = Expr::Call {
                                    callee: new_callee,
                                    args: new_args,
                                };
                            }
                        } else if chain_methods.contains(&m_name) {
                            crate::cm_demote::record_speculative_rewrite(
                                ast,
                                i,
                                callee_id,
                                obj_id,
                                &args_clone,
                            );
                            let mangled = format!("__dispatch_{m_name}");
                            let new_callee = ast.add_expr(Expr::Ident(mangled));
                            let mut new_args = Vec::with_capacity(args_clone.len() + 1);
                            new_args.push(obj_id);
                            new_args.extend(args_clone);
                            ast.exprs[i] = Expr::Call {
                                callee: new_callee,
                                args: new_args,
                            };
                        }
                        // else: sibling collision — leave Member call AS-IS.
                    }
                }
            }
            _ => {}
        }
    }

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
