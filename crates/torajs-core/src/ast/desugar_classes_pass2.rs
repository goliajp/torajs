//! `desugar_classes` Pass 2 — expression arena rewrite (chunk 183,
//! 2026-06-28).
//!
//! Extracted from `ast/desugar_classes.rs` after Pass 1.5/1.6
//! super-call rewriting + before Pass 2.5 static-member table build.
//! Walks `ast.exprs` by index (safe: we only mutate in place or
//! append new exprs at the tail; existing ExprIds keep their
//! meaning).
//!
//! Three rewrite shapes handled here:
//!   * `Expr::This` → `Expr::Ident("__this")` for method/ctor bodies
//!     (new.target deliberately NOT rewritten — ssa_lower handles
//!     `Expr::NewTarget` directly so non-ctor fn bodies get the
//!     ANY_UNDEF box).
//!   * `Expr::New { class_name, args }` for user classes →
//!     `Expr::Call { callee: __new_<C>, args }`. Builtin Map/Set/
//!     WeakRef/WeakMap/WeakSet/Array/RegExp left as Expr::New
//!     (SSA-intercepted: see T-26 weak / P6.1 Map / P0.10 Array).
//!   * `Expr::Call { callee: Member { obj, name } }` for method
//!     calls →
//!     - single-owner: `__cm_<Owner>__<name>(obj, ...args)`, with
//!       a guard for `this.<field>` where the field is typed as a
//!       known builtin (Array/string/number) — those dispatch to
//!       intrinsic, not the user class's method (avoids
//!       infinite-recursion in `class C { data: T[]; push(v) {
//!       this.data.push(v); } }`).
//!     - multi-owner ancestor-chain: `__dispatch_<name>(obj, ...args)`.
//!     - sibling collision: leave Member as-is (SSA picks the
//!       right `__cm_<C>__M` from obj's static type).

use super::desugar_classes_super::ClassIndexEntry;
use super::*;
use std::collections::{HashMap, HashSet};

pub(super) fn rewrite_expr_arena_pass2(
    ast: &mut Ast,
    class_index: &[ClassIndexEntry],
    method_owners: &HashMap<String, Vec<String>>,
    chain_methods: &HashSet<String>,
) {
    let n = ast.exprs.len();
    for i in 0..n {
        match &ast.exprs[i] {
            Expr::This => {
                // RFC 20260804-fn-this-channel knife 2 — a `this` the
                // parser recorded as a STATIC body site becomes the
                // constructor object (the class name), exactly the mint
                // the parser itself used to do at the token (S2.37);
                // every other `this` is an instance receiver, `__this`.
                let eid = super::ExprId(i as u32);
                if let Some(cls) = ast.static_this_sites.get(&eid).cloned() {
                    ast.exprs[i] = Expr::Ident(cls);
                } else {
                    ast.exprs[i] = Expr::Ident("__this".into());
                }
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
            Expr::New {
                class_name,
                args,
                type_args,
            } => {
                /* Builtin News (Date, ...) are rewritten by
                 * `desugar_builtin_new` BEFORE this pass, so any
                 * remaining Expr::New here is a user class. */
                /* T-26 — `new WeakRef(target)` / `new WeakMap()` /
                 * `new WeakSet()` are intercepted at SSA-lower
                 * time so target args pass as borrows (no consume
                 * → owning bindings drop normally → registry
                 * cleanup runs). Skip the generic factory rewrite
                 * here to keep the Expr::New shape intact. */
                if ssa_intercepted_builtin(class_name) {
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
                // `new Box<number>()` states what `T` is. A call has
                // nowhere to carry that, so it goes into the side
                // table generic inference reads — otherwise a class
                // whose type parameter appears only in its field
                // types has nothing to infer from.
                let type_args = type_args.clone();
                let callee = ast.add_expr(Expr::Ident(factory));
                ast.exprs[i] = Expr::Call { callee, args };
                if !type_args.is_empty() {
                    ast.call_type_args.insert(ExprId(i as u32), type_args);
                }
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
                                    class_index,
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
}

/// Built-in `new` shapes ssa_lower intercepts directly, so Pass 2
/// leaves the `Expr::New` node alone.
///
/// T-26 `new WeakRef(t)` / `new WeakMap()` / `new WeakSet()` pass
/// their target as a borrow (no consume, so owning bindings drop
/// normally and registry cleanup runs). P6.1/P6.2 Map and Set lower
/// to `__torajs_map_create`. P0.10 `new Array(n)` needs an `arr_id`
/// interned at lower time, which an AST call cannot carry. RFC
/// 20260716 刀 2 keeps the Number / String / Boolean wrappers here for
/// their alloc emit.
pub(super) fn ssa_intercepted_builtin(name: &str) -> bool {
    matches!(
        name,
        "WeakRef"
            | "WeakMap"
            | "WeakSet"
            | "Map"
            | "Set"
            | "Array"
            | "RegExp"
            | "Number"
            | "String"
            | "Boolean"
            // RFC 20260730-iterator-global 刀 1 — `new Iterator()`
            // lowers to the §27.1.3.1 abstract-ctor TypeError kernel.
            | "Iterator"
            // RFC 20260823-proxy-substrate 刀 1 — `new Proxy(t, h)`
            // lowers to §10.5.14 ProxyCreate.
            | "Proxy"
            // RFC 20260823-typedarray-substrate 刀 1 —
            // `new ArrayBuffer(len, opts)` lowers to §25.1.4.1.
            | "ArrayBuffer"
            // 刀 2 — the eleven §23.2 constructors.
            | "Int8Array"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Float32Array"
            | "Float64Array"
            | "BigInt64Array"
            | "BigUint64Array"
            | "Float16Array"
    )
}

/// S-NEW 刀 4 — `new <name>()` where nothing synthesized a factory
/// for that name.
///
/// A factory exists for a class, and for a function used as a
/// constructor. When neither claimed the name, it is a binding holding
/// a value, and what it holds is a run-time question — so the node
/// becomes a construct from that value. Pass 2 mints `__new_<name>`
/// for every name it does not recognize as a built-in, which is why
/// `const K: any = C; new K()` failed as "unknown identifier
/// `__new_K`": a name the program never wrote, reported to whoever
/// wrote `new K()`.
///
/// Two shapes reach here, because the position that can answer "is
/// there a factory" is after every pass that makes one:
///
/// - `Call(__new_<name>)` — what Pass 2 left behind. That mint is
///   load-bearing for function constructors, whose FnDecl is
///   synthesized afterwards against exactly this call, so it cannot
///   be made conditional at the Pass 2 site.
/// - `Expr::New` — a program with no classes at all returns early
///   from the class desugar, so Pass 2 never ran on it. Left alone,
///   the node reaches the checker's `New` fallback, which panics
///   rather than diagnoses.
pub fn route_non_class_new(ast: &mut Ast) {
    let factories: HashSet<String> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => name.strip_prefix("__new_").map(str::to_owned),
            _ => None,
        })
        .collect();
    for i in 0..ast.exprs.len() {
        let rewrite = match &ast.exprs[i] {
            Expr::Call { callee, args } => match ast.get_expr(*callee) {
                // `__new_` is not spellable in source, so a call to
                // one is always Pass 2's.
                // `__new_target` shares the prefix but names the
                // meta-property binding, not a factory.
                Expr::Ident(n) => n
                    .strip_prefix("__new_")
                    .filter(|name| *name != "target" && !factories.contains(*name))
                    .map(|name| (name.to_owned(), args.clone())),
                _ => None,
            },
            Expr::New {
                class_name, args, ..
            } if !ssa_intercepted_builtin(class_name) && !factories.contains(class_name) => {
                Some((class_name.clone(), args.clone()))
            }
            _ => None,
        };
        let Some((name, args)) = rewrite else {
            continue;
        };
        let callee = ast.add_expr(Expr::Ident(name));
        ast.exprs[i] = Expr::NewDynamic { callee, args };
        // The explicit instantiation Pass 2 parked here was for a
        // named generic class; there is no name to instantiate now.
        ast.call_type_args.remove(&ExprId(i as u32));
    }
}
