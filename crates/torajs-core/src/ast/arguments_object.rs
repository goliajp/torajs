//! T-11 / T-31 — `arguments` object desugar pass + its 5 yield-free
//! walkers + synthesized-local builder.
//!
//! Chunk 345 — paired with chunk 344's `arguments_object_rewrite`
//! sibling. The main pass (`desugar_arguments_object`) keeps the
//! `pub` symbol (re-exported by ast.rs for the three external
//! callers in `crates/torajs-cli/src/{cmd_build,main,lsp}.rs`), and
//! the helpers (`stmt_uses_dynamic_arguments` / `expr_uses_dynamic_arguments`
//! / `body_has_arguments_length` / `stmt_has_arguments_length` /
//! `expr_has_arguments_length` / `synth_arguments_local`) stay
//! sibling-private here.
//!
//! The cross-sibling call to the actual rewriters lives at
//! `crate::ast::arguments_object_rewrite::rewrite_arguments_in_stmt`.

use super::Ast;
use super::arguments_object_collect::{collect_value_argc, collect_value_argv};
use super::arguments_object_inject::inject_argc_params;
use super::arguments_object_stages::{
    collect_arguments_shadowed_fns, collect_iife_real_argc, snapshot_fn_params,
};
use super::arguments_object_static_argv::{
    collect_iife_static_argv, collect_method_static_argv, collect_named_static_argv,
    collect_objlit_method_static_argv, inject_iife_static_params,
};

/// How `arguments.length` rewrites inside a given fn body (chunk 613).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ArgcMode {
    /// Fn carries a real argc to read: every admitted tier owns the
    /// S1 hidden-ABI `__torajs_argc` sig slot and reads route to it
    /// (`__env`-first closures S3.2, `__cm_` this-first S1-T2,
    /// head-less S1-H2 — the injected argc param is fully retired).
    Real,
    /// Hidden-slot face whose body WRITES `arguments.length`: reads
    /// and writes ride a synthesized mutable `__torajs_argc_len`
    /// local seeded from the S1 hidden argc (the hidden param itself
    /// is an unwritable SSA value) — the exact semantics the injected
    /// writable `injected argc param` param used to provide.
    RealLocal,
    /// Fold to the declared arity (legacy fallback; still serves
    /// class methods, whose ABI is untouched — recorded face).
    FoldArity,
    /// Mutation-free FoldTo (rotation 272 re-admission) — a
    /// static-argv-face body with SIMPLE params that never writes an
    /// arguments element, never writes a param, and never lets the
    /// arguments object escape (see arguments_object_mutation): the
    /// zero-cost literal-index param substitution and inline spread
    /// expansion are then observationally equivalent to the real
    /// unmapped arguments object — nothing can tell the snapshot
    /// from the live view — AND they preserve static element types
    /// (an `...arguments` inline expansion stays `number`-typed
    /// where the materialized `any[]` ride would smuggle boxed
    /// slots into a typed literal). Any mutation / escape / non-
    /// simple param routes to Unmapped instead.
    FoldTo(usize),
    /// Length-write knife (rotation 270) — a static-argv-face body
    /// that WRITES `arguments.length`: a static fold would (a)
    /// mint a literal in the write target ("invalid assignment
    /// target") and (b) keep answering the stale constant on every
    /// later read. Length reads AND writes ride the materialized
    /// `__torajs_arguments.length` instead — the array resize is
    /// the §10.4.2.4 approximation on this face (expansion fills
    /// holes, truncation drops slots), which is what the
    /// arguments-iterator tests observe. The payload still carries
    /// the static argc for the materialize take-count, and
    /// `...arguments` spreads swap to the live array rather than
    /// inline-expanding a stale prefix. Element indices ride the
    /// array too — see Unmapped below for why no mode may alias.
    LiveLength(usize),
    /// The static-argv face's standard mode (rotation 271 introduced
    /// it for default / rest / destructured params per ES
    /// §10.4.4.6/7; rotation 272 made it the whole face's mode):
    /// tr's semantic baseline is bun running TS, and a TS file
    /// executes as an ES module — always strict code — where the
    /// arguments object is UNMAPPED for every fn, simple params
    /// included. The old mapped literal-index param substitution
    /// diverged both ways (`arguments[0] = 2` wrote through to `a`;
    /// `a = 99` showed in a later `arguments[0]` read). Reads and
    /// writes both ride the materialized `__torajs_arguments` array
    /// instead; `arguments.length` still folds to the static argc.
    Unmapped(usize),
    /// RFC 20260810-sloppy-goal-arguments S3 — the sloppy goal's
    /// simple-param static-face mode: the SAME literal-index param
    /// substitution as FoldTo, mutations included, because the
    /// substitution's two-way aliasing (`arguments[0] = 2` writes
    /// through to `a`; `a = 99` shows in a later `arguments[0]`
    /// read) IS §10.4.4 CreateMappedArgumentsObject's semantics —
    /// the exact divergence that exiled mutating bodies from FoldTo
    /// under the strict goal. Admission requires
    /// `body_has_mapped_blockers` to answer false: an escape, a
    /// non-length member touch, an element delete, or a dynamic /
    /// out-of-range index all need the materialized array, and a
    /// mapped body must never mix the two views.
    Mapped(usize),
    /// Leave the node alone so the checker rejects it loudly — a
    /// closure VALUE's real argc needs the ABI face (recorded);
    /// folding the declared arity would be silent-wrong.
    KeepLoud,
}

/// RFC 20260808 escape-store profile — a fn whose binding is stored
/// into a boxed-face position leaves the static face: the store
/// implies a runtime call the AST cannot see (species construct,
/// builtin callback), and the static fold would materialize the
/// direct sites' argc — usually zero — where the boxed dual entry
/// delivers the true one. Rotation 345 added the argument-position
/// variant of the same profile (boxed-consumption arg sites; doc on
/// that fn).
fn retain_escape_stored_off_static(
    ast: &Ast,
    static_argv: &mut std::collections::HashMap<String, usize>,
    value_argv_pre: &std::collections::HashSet<String>,
    argv_locals: &std::collections::HashSet<String>,
) {
    for f in super::arguments_object_escape_store::collect_escape_stored(
        ast,
        value_argv_pre,
        argv_locals,
    ) {
        static_argv.remove(&f);
    }
    for f in super::arguments_object_escape_store::collect_escape_arg_positions(
        ast,
        value_argv_pre,
        argv_locals,
    ) {
        static_argv.remove(&f);
    }
}

pub fn desugar_arguments_object(ast: &mut Ast) {
    // Pre-pass — rewrite exclusively-called fn-value aliases into
    // direct calls so the face analyses below see their sites (and
    // the forwarder relay's arg drop is bypassed). See the module
    // doc in arguments_object_devirt.
    super::arguments_object_devirt::devirtualize_fn_value_aliases(ast);
    // Stage helpers extracted chunk 767 (the pass had drifted past
    // the 200-line fn limit as argc/argv tiers stacked up).
    let shadowed = collect_arguments_shadowed_fns(ast);
    let excluded = super::arguments_object_walkers::collect_face_excluded_fns(ast, &shadowed);
    let (mut fn_params, uses_real_argc, env_fns) = snapshot_fn_params(ast);
    let iife_real_argc = collect_iife_real_argc(ast, &shadowed);

    // RFC 20260801-arguments-escape-face knives 1+3a — static-argv
    // face: any non-length arguments touch (index / spread / bare
    // escape) resolves fully statically when every call site is
    // known and passes the same arg count: an IIFE's single site
    // (knife 1), or a top-level named fn whose every reference is a
    // direct call with one uniform argc (knife 3a). Injects trailing
    // `__torajs_static_extra_*: any` params so over-arity args reach
    // the body, and extends the rewrite param table to match.
    let mut static_argv = collect_iife_static_argv(ast, &iife_real_argc);
    static_argv.extend(collect_named_static_argv(ast, &env_fns));
    // Constructed fn-expr bindings — the factory's direct call is the
    // only real site, so the whole argv is static (see the module doc
    // in arguments_object_ctor_argv).
    static_argv.extend(super::arguments_object_ctor_argv::collect_ctor_bound_static_argv(ast));
    // RFC 20260801-arguments-method-face knife 1 — single-owner
    // class methods ride the same face (receiver slot excluded from
    // the argc count).
    static_argv.extend(collect_method_static_argv(ast));
    // RFC 20260801 objlit branch — object-literal methods whose
    // member call sites are all visible and uniform. A field that
    // ALSO escapes as a value (a member read off the callee
    // position) comes back in `objlit_escaped`.
    let (objlit_face, objlit_escaped) = collect_objlit_method_static_argv(ast, &env_fns);
    static_argv.extend(objlit_face);
    // Shadowed fns (a binding named `arguments` in the body — see
    // collect_arguments_shadowed_fns) and bare-assign fns never
    // join any face.
    static_argv.retain(|n, _| !excluded.contains(n));
    // Knife 4c — escape-vs-static resolution: when the escaped
    // alias qualifies for the argv face (exclusively-called safe
    // chain), that face wins — it answers the TRUE per-call argc,
    // where the static fold would answer the direct sites' count to
    // a call it can't see. A store-only escape (never called, e.g.
    // `typeof ref`) fails the argv chain and the fn stays on the
    // static face — the pre-4c "legal but silent" compromise, which
    // is strictly better than falling off both faces into a loud
    // arity refuse (the objlit fixture caught that regression).
    let (value_argv_pre, argv_locals, argv_boxed_params) =
        collect_value_argv(ast, &env_fns, &iife_real_argc, &excluded);
    ast.argv_boxed_params = argv_boxed_params;
    for f in &objlit_escaped {
        if value_argv_pre.contains(f) {
            static_argv.remove(f);
        }
    }
    retain_escape_stored_off_static(ast, &mut static_argv, &value_argv_pre, &argv_locals);
    let iife_static_argv = static_argv;
    inject_iife_static_params(ast, &iife_static_argv, &mut fn_params);
    // A named fn admitted to the static face must leave the T-31
    // real-argc tier (param injection + call-site argc prepend would
    // double-reshape its signature); a shadowed fn leaves every tier.
    let uses_real_argc: std::collections::HashSet<String> = uses_real_argc
        .into_iter()
        .filter(|n| !iife_static_argv.contains_key(n) && !shadowed.contains(n))
        .collect();

    // RFC 20260708-closure-argc-abi chunk 1 — closure VALUE form
    // seed + binding safety walk (see collect_value_argc). A closure
    // the static face admitted (objlit branch) must leave the value
    // tiers — the face already reshaped its signature, and the value
    // adapters would double-reshape it.
    let (mut value_real_argc, argc_locals) =
        collect_value_argc(ast, &env_fns, &iife_real_argc, &excluded);
    value_real_argc.retain(|n| !iife_static_argv.contains_key(n));
    ast.closure_argc_locals = argc_locals;

    // RFC 20260708-closure-argv-face — full-arguments tier
    // (collected above, pre-static-retain, for the knife-4c
    // escape-vs-static resolution).
    let mut value_argv_fns = value_argv_pre;
    value_argv_fns.retain(|n| !iife_static_argv.contains_key(n));
    // Rotation 365 — boxed-only object-literal method argv (module
    // doc in arguments_object_objlit_argv): a field closure with
    // zero visible sites joins the argv face, split by `__this`
    // promotion into the value / method head shapes.
    let (objlit_value_argv, objlit_method_argv) =
        super::arguments_object_objlit_argv::collect_objlit_boxed_only_argv(ast, &excluded);
    value_argv_fns.extend(objlit_value_argv);
    ast.closure_argv_fns = value_argv_fns.clone();
    ast.closure_argv_locals = argv_locals;

    // RFC 20260801-arguments-method-face knife 4a — class methods
    // reached ONLY through member-value reads (escape / getter-return
    // / reified cell) take the argv face too: the boxed adapter
    // already delivers true argc/argv and forwards both into the
    // injected params. See the module doc for the admission bounds.
    let mut method_argv_fns =
        super::arguments_object_method_argv::collect_method_argv(ast, &excluded, &iife_static_argv);
    method_argv_fns.extend(objlit_method_argv);
    ast.method_argv_fns = method_argv_fns.clone();
    // RFC 20260816-headless-argv-face — the head-less tier's runtime
    // VALUE channel (module doc in arguments_object_headless_argv).
    // Its bodies read `__torajs_argc` too (the materialize take-count
    // is the hidden slot), so the argc tier absorbs them even when
    // the source never spells `arguments.length`.
    let headless_argv_fns = super::arguments_object_headless_argv::collect_headless_argv(
        ast,
        &shadowed,
        &excluded,
        &iife_static_argv,
        &env_fns,
    );
    let uses_real_argc: std::collections::HashSet<String> =
        uses_real_argc.union(&headless_argv_fns).cloned().collect();
    // RFC 20260810-indirect-argc-abi H1 — record the final head-less
    // tier membership: the SSA sig side (pass 1 / setup_fn_params /
    // direct-call terminal) keys the hidden-argc slot on this set,
    // and the mono pass mirrors clones into it.
    ast.headless_argc_fns = uses_real_argc.clone();
    ast.headless_argv_fns = headless_argv_fns.clone();

    inject_argc_params(ast, &value_argv_fns, &method_argv_fns, &headless_argv_fns);

    // The per-FnDecl body rewrite (mode classification, the
    // materialize / mark / sloppy-callee prologue, and the sloppy
    // shims it mints) lives in the `arguments_object_bodies`
    // sibling — this pass answers WHICH tier each fn is in, that
    // one answers what a body on that tier turns into.
    super::arguments_object_bodies::rewrite_fn_bodies(
        ast,
        &fn_params,
        super::arguments_object_bodies::BodyTiers {
            shadowed: &shadowed,
            iife_static_argv: &iife_static_argv,
            uses_real_argc: &uses_real_argc,
            method_argv_fns: &method_argv_fns,
            iife_real_argc: &iife_real_argc,
            value_real_argc: &value_real_argc,
            value_argv_fns: &value_argv_fns,
            env_fns: &env_fns,
            headless_argv_fns: &headless_argv_fns,
        },
    );
}
