//! FnSig-to-Closure store-site collection (chunk 142 extract;
//! chunk 525 collector-struct rework + the L3b #8 axes).
//!
//! The pre-typecheck walk finds positions where a bare top-level
//! FnDecl Ident crosses into a Closure-expected or `any`-boxed slot
//! and marks it for the `__forward_<name>` zero-capture closure
//! rewrite (`ast/forwarders_object.rs` synthesizes the forwarders
//! and applies the rewrites):
//!
//! - `let x: <struct-with-Closure-field> = { ..., f: name, ... }` —
//!   the destination field was `__cls`-tagged.
//! - `let f: any = name` and ObjectLit-field / Array-element
//!   positions inside an `any`-annotated init (chunks 518/519).
//! - `f = name` where `f` was declared with an `any` annotation
//!   (L3b #8 assign-into-any; binding names collect scope-
//!   approximate — a shadowing non-any `f` in another scope would
//!   also match, which only costs an unnecessary wrap).
//! - `callee(name)` where the matching declared param is
//!   `any`-annotated (L3b #8; a fn-typed param stays raw FnSig, for
//!   direct dispatch, unless its body stores it into the heap — the
//!   store-sink axis gives it the closure repr).
//! - `return name` inside a fn whose declared return type is `any`
//!   (L3b #8; closure-typed returns ride the older
//!   `ast/forwarders.rs` pass).
//! - `callee(name)` where the matching declared param carries a
//!   variadic fn-type ann (`__rest(` — RFC 20260708-variadic
//!   chunk 3): the slot is Closure repr and dispatches through the
//!   boxed dual entry, which a raw FnSig doesn't carry.
//! - `s.replace(pat, name)` / `s.replaceAll(pat, name)` (chunk 617)
//!   — the functional-replaceValue runtime invokes the callback
//!   through the closure boxed entry; a bare FnSig has neither env
//!   nor boxed entry (604-era loud panic). A user-class `.replace`
//!   method hit by the same shape only costs the unnecessary wrap
//!   (closure values are legal fn-param arguments).
//! - `cb = name` where `cb` is a Closure-repr top-level binding
//!   (RFC 20260709-closure-global chunk 4): the mutable-closure
//!   global assign lane stores closure cells.
//! - top-level `const f = name` / `const f: (P)=>R = name` read by
//!   a named-fn body (same RFC; the let-init axis lives in
//!   `ast/forwarders_object.rs` — it needs the ast_refs gate, not
//!   the recursive walk).
//! - fn-typed ARRAY positions (chunk 733 — element slots are
//!   Closure-repr per `ssa_lower_parse_type`'s `[]` re-repr):
//!   array-literal elements in a fn-arr-annotated let-init or
//!   call-arg (`const ops: ((n)=>n)[] = [name]` / `run([name])`),
//!   `fns.push(name)` / `fns.unshift(name)`, and `fns[i] = name`
//!   where `fns` was declared with a fn-arr annotation.
//!
//! Without the wrap these positions hold a raw FnSig value: the
//! any-boxing site has no FnSig arm ("box_to_any element type FnSig
//! not supported") and the runtime `__torajs_any_call` rejects
//! non-closure-cell callees — the wrap routes the value through the
//! closure construction site, which carries the boxed dual entry.

use crate::ast::{GenericStructDecls, PropKey};
use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Param, Stmt, is_fn_like_ann};

/// Walk state + outputs for the store-site collection. `targets`
/// is the set of fn names needing a forwarder; `rewrites` the exact
/// ExprIds to replace with `Closure { __forward_<name> }`.
pub(crate) struct FnToClosureCollector<'a> {
    pub(crate) ast: &'a Ast,
    pub(crate) fn_sigs: &'a HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    /// fn name → its type-param names (generic FnDecls only) — the
    /// generic-param call-arg axis: a fn-name argument matching a
    /// TypeVar-annotated param (`sameValue<T>(actual: T, expected:
    /// T)`) instantiates the generic at Any, and the any-boxed argv
    /// slot can't take a raw FnSig. The canonical `__forward_*` cell
    /// keeps identity across wrap sites, so `===` faces still agree.
    pub(crate) fn_type_params: &'a HashMap<String, Vec<String>>,
    pub(crate) struct_field_anns: &'a HashMap<String, HashMap<PropKey, String>>,
    /// Generic TypeDecl snapshots (name → (type params, fields with
    /// the params still spelled inside)) — chunk 795: the wrap axes
    /// resolve `Box<() => number>` instantiations by word-boundary
    /// substitution, as the checker's `resolve_generic` does.
    pub(crate) generic_field_anns: &'a GenericStructDecls,
    /// Binding names declared with an `any` annotation anywhere in
    /// the program (scope-approximate; see module doc).
    pub(crate) any_bindings: &'a HashSet<String>,
    /// UN-annotated binding names with a constructor-call init
    /// (`let f = new C()` — V1b): their method calls ride the
    /// runtime any-method lane, so the V1 axis's typed-ident-receiver
    /// exemption must not cover them. Scope-approximate like the
    /// sets above.
    pub(crate) new_init_bindings: &'a HashSet<String>,
    /// Top-level binding names whose slot is Closure-repr (a lifted
    /// arrow init or a fn-type annotation) — RFC 20260709 chunk 4:
    /// `cb = top_fn` must wrap so the drop-old/store-new global lane
    /// stores a closure cell. Scope-approximate like `any_bindings`
    /// (a shadowing local match only costs the wrap).
    pub(crate) closure_bindings: &'a HashSet<String>,
    /// Binding names declared with a fn-typed ARRAY annotation
    /// (chunk 733, `is_fn_arr_ann`): the element slot is Closure-repr,
    /// so `fns.push(top_fn)` / `fns[i] = top_fn` wrap. Scope-
    /// approximate like the other two sets.
    pub(crate) fn_arr_bindings: &'a HashSet<String>,
    /// What this module might monkey-patch onto a builtin prototype
    /// (RFC 20260806) — the member-call axis reads it: a patched
    /// builtin stands down to the any-lane, whose argv packs
    /// any-boxed slots that a raw FnSig cannot ride. Empty for every
    /// program that never names a builtin prototype, which is what
    /// keeps `xs.map(top_fn)` on its raw-FnSig direct dispatch.
    pub(crate) proto_shadow: &'a crate::builtin_proto_shadow::ShadowSet,
    /// Binding names compared with `===` / `!==` / `==` / `!=`
    /// anywhere in the program (`eq_operand_ident_names`). The
    /// let-decl and assign arms consult it: an immutable binding
    /// initialized with a bare named fn keeps the raw FnSig address
    /// for direct dispatch, which is a DIFFERENT representation from
    /// the canonical `__fncell_*` cell every value site answers, and
    /// `===` is the one observer that can tell them apart.
    pub(crate) ident_census: &'a crate::ast_collect_fn_closure_init::IdentCensus,
    /// Method rows whose vtable slot mixes named-fn returns with other
    /// values (`ast_collect_fn_closure_target::mixed_slot_rows`).
    pub(crate) mixed_slot_rows: &'a HashSet<String>,
    pub(crate) targets: HashSet<String>,
    pub(crate) rewrites: Vec<(ExprId, String)>,
    /// Per-fn shadow stack (rotation 128) — names bound as a param or
    /// body local of the fn currently being walked. `try_mark` refuses
    /// a shadowed name: a local `const f: string` inside an unrelated
    /// fn is NOT a value use of top-level `function f`, and rewriting
    /// its reads to `__forward_f` silently corrupts the local (the
    /// test262 propertyHelper harness compares such a local against
    /// string literals — the eq-operand axis turned 27 passing dstr
    /// cases into whole-program rejects). Fn-granular on purpose: a
    /// pre-declaration use inside the same fn skips the wrap (old
    /// raw-FnSig behavior, the lesser wrong) rather than risk wrapping
    /// a shadowed read.
    pub(crate) shadowed: Vec<HashSet<String>>,
    /// Per-fn stack of param names whose annotation is `any` or
    /// missing (r293) — the any-receiver member-call axis reads it:
    /// a lifted arrow's untyped param IS an `any` binding
    /// (`iter => iter.map(fn)` — the t262 lazy-Iterator family),
    /// but only `any_bindings` (top-level lets) answered before, so
    /// the receiver looked typed and the fn-name argument kept its
    /// raw FnSig into the any-method argv. Same frame discipline as
    /// `shadowed`.
    pub(crate) any_param_frames: Vec<HashSet<String>>,
    /// Per-fn stack of param names the closure-param tag pass gave the
    /// closure repr (`__cls(`): such a param already answers a cell,
    /// so a choice between it and named fns wraps the named fns.
    pub(crate) cls_param_frames: Vec<HashSet<String>>,
}

// Annotation-predicate binding walks (collect_any_bindings /
// collect_fn_arr_bindings / collect_fn_ann_bindings) moved to
// [`crate::ast_collect_bindings`] (chunk 765); re-exported so the
// consumer surface stays put.
pub(crate) use crate::ast_collect_bindings::{
    collect_any_bindings, collect_fn_ann_bindings, collect_fn_arr_bindings,
};

impl<'a> FnToClosureCollector<'a> {
    /// Mark `eid` for the forwarder rewrite when it is a bare
    /// top-FnDecl Ident. Answers whether it matched.
    /// A declaration whose body reads `arguments` never leaves as a
    /// raw FnSig argument. Its definition carries the hidden argc (and
    /// argv) slots of the head-less tier, which a FnSig value's
    /// signature does not show, so a lane that direct-calls the value
    /// handed the first callback argument to the argc slot:
    /// `const xs: number[] = [5, 6]; xs.map(function f(v) { return
    /// arguments.length })` answered `5,6`. The relay knows the tiers
    /// — it forwards its own true argc and, through its boxed entry,
    /// the argv — and it is what an untyped receiver's call already
    /// got. Generator factories keep their own lanes, and so does a
    /// call to a known declaration: its declared parameter types
    /// decide the argument's repr on their own axis.
    pub(crate) fn mark_arguments_reading_args(&mut self, callee: &ExprId, args: &[ExprId]) {
        if matches!(self.ast.get_expr(*callee), Expr::Ident(n) if self.fn_sigs.contains_key(n)) {
            return;
        }
        for &arg in args {
            let Expr::Ident(n) = self.ast.get_expr(arg) else {
                continue;
            };
            let reads_arguments =
                self.fn_sigs.contains_key(n) && crate::ast::fn_decl_reads_arguments(self.ast, n);
            if reads_arguments
                && !self.is_generator_family_ident(arg)
                && !self.rewrites.iter().any(|(e, _)| *e == arg)
            {
                self.try_mark(arg);
            }
        }
    }

    /// The `Stmt::LetDecl` arm of [`Self::walk_stmt`] — extracted
    /// verbatim (rotation 434: the Throw arm pushed the walker past
    /// the 200-line cap; this is the walker's biggest arm).
    fn walk_let_decl(
        &mut self,
        mutable: bool,
        type_ann: Option<&str>,
        init: ExprId,
        is_var: bool,
        name: &str,
    ) {
        self.collect_objectlit_field_sites(init, type_ann);
        // r293 — the for-in head hoist (`let __forin_obj_N = <src>`,
        // parser-minted): a bare fn-name src wraps so the keys call sees
        // a boxable closure cell (§14.7.5). A hoisted generator
        // expression (`let g = __genexpr_N`) binds its fn object in a
        // body as at top level (`closure_holding_lets` counts it a cell).
        let genexpr = matches!(self.ast.get_expr(init), Expr::Ident(n) if self.ast.genexpr_names.contains_key(n));
        if name.starts_with("__forin_obj_") || genexpr {
            self.try_mark(init);
        }
        // RFC 20260729-fn-value-any V4 刀 3 — a `var` slot is
        // an `any` destination whatever the source says: the
        // hoist pass (which runs AFTER this collector) mints
        // every hoisted binding as `any` on purpose, since a
        // pre-init read is `undefined` and a var may be
        // reassigned across types. Reading only the written
        // annotation left `var b = foo` — the plainest fn-value
        // alias there is — panicking the whole program at
        // box_to_any while the `let` / `const` forms worked.
        if is_var || type_ann.is_some_and(|a| a.trim() == "any") {
            self.collect_any_init_sites(init);
        }
        // Chunk 733 — `const fns: ((n)=>n)[] = [top_fn, ...]`:
        // the element slot is Closure-repr, wrap each bare
        // named-fn element.
        if type_ann.is_some_and(crate::ast::is_fn_arr_ann) {
            self.mark_array_lit_elems(init);
        }
        // The binding axes (chunk 736 mutable fn-typed slot, the heap
        // store, rotation 599's identity axis; see
        // `IdentCensus::wraps_let`) and the 424-04 census (a named
        // function whose parameter face differs from the annotation's,
        // so its calls ride the cell's boxed entry) wrap the init.
        // Wrapping turns it into `Expr::Closure`, which steers the
        // lowerer off the immutable-only fn_addr_let direct lane.
        if self.ident_census.wraps_let(mutable, type_ann, name)
            || (!mutable && self.ast.fnsig_mismatch_bindings.contains(name))
        {
            self.try_mark(init);
        }
        self.walk_expr(init);
    }

    /// Walk one Stmt (and any Stmts / Exprs it contains) looking for
    /// store-sites. `ret_is_any` carries the enclosing fn's declared
    /// `any` return annotation down to `Return` sites.
    pub(crate) fn walk_stmt(&mut self, s: &Stmt, ret_is_any: bool) {
        match s {
            Stmt::LetDecl {
                mutable,
                type_ann,
                init,
                is_var,
                name,
                ..
            } => self.walk_let_decl(*mutable, type_ann.as_deref(), *init, *is_var, name),
            Stmt::FnDecl {
                name,
                return_type,
                params,
                body,
                ..
            } => {
                // S2.37 knife 2 — a MISSING return annotation counts:
                // the inferred return slot for a bare-fn-ident return
                // is Any (FnSig is not a propagatable return width),
                // so the box site needs the closure wrap exactly as
                // the explicit-`any` case does. Before this arm every
                // `return <top-fn>` under a None annotation died loud
                // at box_to_any ("element type FnSig not supported")
                // — the t262 static-private accessor family
                // (`static get m() { return this.#m }`, desugared to
                // a None-ann `__sm_*_get`) is the mass consumer.
                let ret_any = return_type.is_none()
                    || return_type.as_deref().is_some_and(|a| a.trim() == "any");
                // Shadow frame (rotation 128) — the fn's params + body
                // locals hide a same-named top-level fn for the whole
                // body walk; see `shadowed` field doc.
                let mut frame: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
                crate::ast_collect_bindings::collect_local_binding_names(body, &mut frame);
                // A lifted closure's captures are neither params nor
                // body locals — they ride the env — and the lift only
                // captures a top-level fn name when some enclosing
                // scope rebound it (RFC 20260828). So a capture
                // spelling one hides the declaration here exactly as a
                // param would; without this the body's reads were
                // rewritten to `__forward_<name>` and the receiver
                // became the top-level function instead of the
                // captured value.
                if let Some(env) = params.first().filter(|p| p.name == "__env")
                    && let Some(caps) = env
                        .type_ann
                        .as_deref()
                        .and_then(|a| a.strip_prefix("__env("))
                        .and_then(|r| r.strip_suffix(')'))
                {
                    frame.extend(
                        caps.split('|')
                            .filter(|c| !c.is_empty())
                            .map(str::to_string),
                    );
                }
                self.shadowed.push(frame);
                self.any_param_frames.push(
                    params
                        .iter()
                        .filter(|p| {
                            p.type_ann.is_none()
                                || p.type_ann.as_deref().is_some_and(|a| a.trim() == "any")
                        })
                        .map(|p| p.name.clone())
                        .collect(),
                );
                self.cls_param_frames
                    .push(crate::ast_collect_fn_closure_target::cls_params(params));
                self.target_mixed_fn_returns(name, params, return_type, body);
                for inner in body {
                    self.walk_stmt(inner, ret_any);
                }
                self.cls_param_frames.pop();
                self.any_param_frames.pop();
                self.shadowed.pop();
            }
            Stmt::Expr(eid) => self.walk_expr(*eid),
            Stmt::Return(Some(eid)) => {
                // L3b #8 — `return name` boxes into an `any` return
                // slot; wrap so the box holds a closure cell.
                if !(ret_is_any && self.try_mark(*eid)) {
                    self.walk_expr(*eid);
                }
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(*cond);
                self.walk_stmt(then_branch, ret_is_any);
                if let Some(eb) = else_branch {
                    self.walk_stmt(eb, ret_is_any);
                }
            }
            Stmt::Labeled { body, .. } => {
                self.walk_stmt(body, ret_is_any);
            }
            Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
                self.walk_expr(*cond);
                self.walk_stmt(body, ret_is_any);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                if let Some(init) = init {
                    self.walk_stmt(init, ret_is_any);
                }
                if let Some(c) = cond {
                    self.walk_expr(*c);
                }
                if let Some(stp) = step {
                    self.walk_expr(*stp);
                }
                self.walk_stmt(body, ret_is_any);
            }
            Stmt::Block(stmts) => {
                // a block's own bindings hide a same-named top-level fn
                // inside it; a fn body's frame already covers its blocks
                let frame = stmts.iter().filter_map(|s| match s {
                    Stmt::LetDecl { name, .. } => Some(name.clone()),
                    _ => None,
                });
                self.shadowed.push(frame.collect());
                for inner in stmts {
                    self.walk_stmt(inner, ret_is_any);
                }
                self.shadowed.pop();
            }
            Stmt::Multi(stmts) => {
                for inner in stmts {
                    self.walk_stmt(inner, ret_is_any);
                }
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                ..
            } => {
                self.walk_eq_operand(*scrutinee);
                for c in cases {
                    self.walk_eq_operand(c.value);
                    for inner in &c.body {
                        self.walk_stmt(inner, ret_is_any);
                    }
                }
                if let Some(d) = default {
                    for inner in d {
                        self.walk_stmt(inner, ret_is_any);
                    }
                }
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                for inner in body {
                    self.walk_stmt(inner, ret_is_any);
                }
                for inner in catch_body {
                    self.walk_stmt(inner, ret_is_any);
                }
                if let Some(fb) = finally_body {
                    for inner in fb {
                        self.walk_stmt(inner, ret_is_any);
                    }
                }
            }
            Stmt::Throw(eid) => {
                // §14.14 — the thrown operand crosses into the
                // (tag, value) pending-throw slot, an any-shaped
                // store: wrap so the slot holds a closure cell. A
                // raw FnSig there made every heap read the catch
                // binding performs land on a code address (typeof
                // answered "object", the scope-end drop SIGBUSed).
                if !self.try_mark(*eid) {
                    self.walk_expr(*eid);
                }
            }
            Stmt::Yield(eid) => self.walk_expr(*eid),
            // r293 — for-of / for-in loops were invisible to the walk
            // (the catch-all swallowed them), so every store-site
            // inside a loop body escaped the wrap axes (`box.cb =
            // top_fn` inside `for (... of ...)` — S13_A10 shape under
            // iteration). The head exprs ride their hoisted LetDecls;
            // the body is what was missing.
            Stmt::ForOf { body, .. } | Stmt::ForOfSplitIter { body, .. } => {
                self.walk_stmt(body, ret_is_any);
            }
            _ => {}
        }
    }
}

pub(crate) fn is_fn_like_field_ann(fann: &str) -> bool {
    let inner = fann
        .strip_prefix("__nullable(")
        .and_then(|r| r.strip_suffix(')'))
        .unwrap_or(fann);
    is_fn_like_ann(inner)
}
