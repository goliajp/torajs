//! T-11 / T-31 — `arguments` Ident → params / `__torajs_arguments`
//! rewriter for desugar_arguments_object.
//!
//! Chunk 344 — extracted from ast.rs. The two recursive rewriters
//! (`rewrite_arguments_in_stmt` walks every statement form,
//! `rewrite_arguments_in_expr` walks every Expr variant, and they
//! call each other on Expr inside Stmt) form one logical unit and
//! lift cleanly into a single sibling. Caller
//! (`ast::desugar_arguments_object`) reaches them via `super::`
//! and the pub(super) markers below.

use super::arguments_object::ArgcMode;
use super::arguments_object_rewrite_recurse::rewrite_recurse_arm;
use super::arguments_object_rewrite_spread::{rewrite_array_arm, rewrite_call_arm};
pub(super) use super::arguments_object_rewrite_stmt::rewrite_arguments_in_stmt;
use super::{Ast, Expr, ExprId};

/// RFC 20260810-sloppy-goal-arguments S2 — how `arguments.callee`
/// spells inside this fn body. Strict (the module goal) keeps the
/// %ThrowTypeError% thrower call; the sloppy goal answers the fn
/// value instead (§10.4.4 CreateMappedArgumentsObject step 21's
/// ordinary data property).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SloppyCallee<'a> {
    /// Strict goal — every callee touch rides the thrower.
    Strict,
    /// Sloppy, un-materialized body (pure reads, no escape): the
    /// callee IS the enclosing fn and nothing can have deleted or
    /// redefined it — rewrite to `Closure { <shim>, [] }`, the fn's
    /// `__forward_` closure shim (a bare fn Ident in value position
    /// is not a closure-shaped value this late in the pipeline;
    /// typeof answered "object"). Each read mints a fresh cell —
    /// callee-identity comparisons are a recorded window (L3b).
    Closure(&'a str),
    /// Same position, but the fn is itself closure-shaped (a lifted
    /// zero-capture fn expression): rewrite to a CALL of the
    /// `__calleeval_` wrapper that returns `Closure { fn, [] }`.
    /// The indirection is load-bearing — a self-referential Closure
    /// literal inside the fn's own body sends the checker's closure
    /// walk into unbounded recursion; the wrapper hop breaks the
    /// literal chain the same way the `__forward_` shim does for
    /// plain fns.
    EvalCall(&'a str),
    /// Sloppy, materialized body: the mint defined `callee` into the
    /// array's expando bag, so keyed reads observe later deletes /
    /// redefines — rewrite to `__torajs_arguments["callee"]`.
    Keyed,
}

pub(super) fn rewrite_arguments_in_expr(
    ast: &mut Ast,
    eid: ExprId,
    params: &[String],
    argc_mode: ArgcMode,
    is_argv_fn: bool,
    sloppy_callee: SloppyCallee<'_>,
) -> ExprId {
    let e = ast.get_expr(eid).clone();
    match e {
        // `arguments.callee` — §10.4.4.6 step 21: on tr's
        // always-strict module goal the read runs the
        // %ThrowTypeError% getter. Rewrites to the synthetic
        // `__torajs_arguments_callee()` call (checker ident
        // special-case, class-synth lowering arm, runtime
        // TypeError) — the 10.6-13-c strict family's direct-read
        // spelling; the escaped keyed read rides the gOPD /
        // member-get arguments arms instead.
        // S2 — the composite `arguments.callee.caller` spelling: tr
        // implements no caller extension (the §10.2.4 poisoned
        // accessor on %Function.prototype% serves the strict
        // semantics), so the sloppy read answers undefined — the
        // "extension not supported" leg 10.6-13-a-2/3 accept. The
        // escaped spelling (`var c = arguments.callee; c.caller`)
        // still rides the poisoned proto walk (recorded window,
        // plan-state L3b).
        Expr::Member { obj, name }
            if name == "caller"
                && sloppy_callee != SloppyCallee::Strict
                && matches!(ast.get_expr(obj), Expr::Member { obj: o2, name: n2 }
                    if n2 == "callee"
                        && matches!(ast.get_expr(*o2), Expr::Ident(n3) if n3 == "arguments")) =>
        {
            // `as any` so a call position (`arguments.callee.caller
            // (true)` behind the extension probe) compiles — the
            // checker rejects a bare-undefined callee outright, but
            // the guarded branch never runs.
            let u = ast.add_expr(Expr::Ident("undefined".into()));
            ast.add_expr(Expr::As {
                expr: u,
                ty_ann: "any".into(),
            })
        }
        Expr::Member { obj, name } if name == "callee" => {
            if let Expr::Ident(n) = ast.get_expr(obj)
                && n == "arguments"
            {
                // S2 sloppy read — the fn value (§10.4.4 step 21's
                // ordinary data property), spelled per the body's
                // materialization state (see SloppyCallee).
                match sloppy_callee {
                    SloppyCallee::Closure(fwd_name) => {
                        return ast.add_expr(Expr::Closure {
                            fn_name: fwd_name.to_string(),
                            captures: Vec::new(),
                        });
                    }
                    SloppyCallee::EvalCall(wrapper) => {
                        let callee = ast.add_expr(Expr::Ident(wrapper.to_string()));
                        return ast.add_expr(Expr::Call {
                            callee,
                            args: Vec::new(),
                        });
                    }
                    SloppyCallee::Keyed => return keyed_callee_ref(ast),
                    SloppyCallee::Strict => {}
                }
                let callee = ast.add_expr(Expr::Ident("__torajs_arguments_callee".into()));
                return ast.add_expr(Expr::Call {
                    callee,
                    args: Vec::new(),
                });
            }
            let o =
                rewrite_arguments_in_expr(ast, obj, params, argc_mode, is_argv_fn, sloppy_callee);
            if o == obj {
                return eid;
            }
            return ast.add_expr(Expr::Member { obj: o, name });
        }
        // `arguments.length` — T-31: when this fn carries a real
        // argc, route to the S1 hidden `__torajs_argc`. Otherwise
        // fall back to the declared-arity fold (`Number(<arity>)`)
        // — that path still serves closures and class methods that
        // don't qualify for the T-31 ABI change.
        Expr::Member { obj, name } if name == "length" => {
            if let Expr::Ident(n) = ast.get_expr(obj)
                && n == "arguments"
            {
                return rewrite_length_read(ast, argc_mode, params, eid);
            }
            // Recurse through the receiver. Copy-on-write: an
            // unchanged child keeps the original node — ExprId-keyed
            // side tables (speculative_cm_rewrites & co.) stay valid
            // for every subtree this pass didn't actually rewrite.
            let new_obj =
                rewrite_arguments_in_expr(ast, obj, params, argc_mode, is_argv_fn, sloppy_callee);
            if new_obj == obj {
                return eid;
            }
            ast.add_expr(Expr::Member { obj: new_obj, name })
        }
        // `arguments[N]` with literal N in [0, arity) → Ident(param[N]).
        // T-11 — `arguments[<non-literal>]` (or out-of-range literal)
        // → `__torajs_arguments[<i>]` reading from the synthesized
        // Array<Any>. The synth let is prepended at fn body start by
        // the FnDecl-walk pre-pass when any dynamic use is detected.
        Expr::Index { obj, index } => {
            let is_arguments = matches!(
                ast.get_expr(obj),
                Expr::Ident(n) if n == "arguments"
            );
            if is_arguments {
                // KeepLoud — leave every `arguments[...]` node
                // untouched so the checker rejects the body loudly
                // (RFC 20260708-closure-argv-face chunk 2: the old
                // unconditional rewrite fed a declared-params-only
                // array to bodies whose real argv face was killed,
                // silently answering undefined beyond declared).
                if argc_mode == ArgcMode::KeepLoud {
                    return eid;
                }
                // Unmapped / LiveLength faces (module code is strict,
                // ES §10.4.4.6/7) — `arguments[i]` never aliases the
                // param: the literal-index substitution below would
                // diverge both ways (`arguments[0] = 2` mutating `a`;
                // `a = 99` showing in a later read). Every index
                // rides the materialized array instead.
                if !matches!(argc_mode, ArgcMode::Unmapped(_) | ArgcMode::LiveLength(_))
                    && let Expr::Number(n) = ast.get_expr(index)
                    && n.fract() == 0.0
                    && (*n as usize) < params.len()
                {
                    let pname = params[*n as usize].clone();
                    return ast.add_expr(Expr::Ident(pname));
                }
                // Dynamic index (or out-of-range literal): route to
                // the materialized Array<Any> via __torajs_arguments.
                let new_index = rewrite_arguments_in_expr(
                    ast,
                    index,
                    params,
                    argc_mode,
                    is_argv_fn,
                    sloppy_callee,
                );
                let synth_obj = ast.add_expr(Expr::Ident("__torajs_arguments".into()));
                return ast.add_expr(Expr::Index {
                    obj: synth_obj,
                    index: new_index,
                });
            }
            let new_obj =
                rewrite_arguments_in_expr(ast, obj, params, argc_mode, is_argv_fn, sloppy_callee);
            let new_index =
                rewrite_arguments_in_expr(ast, index, params, argc_mode, is_argv_fn, sloppy_callee);
            if new_obj == obj && new_index == index {
                return eid;
            }
            ast.add_expr(Expr::Index {
                obj: new_obj,
                index: new_index,
            })
        }
        Expr::Call { callee, args } => rewrite_call_arm(
            ast,
            eid,
            callee,
            args,
            params,
            argc_mode,
            is_argv_fn,
            sloppy_callee,
        ),
        Expr::Array(elems) => rewrite_array_arm(
            ast,
            eid,
            elems,
            params,
            argc_mode,
            is_argv_fn,
            sloppy_callee,
        ),
        // RFC 20260801-arguments-escape-face — bare `arguments`
        // escape (return / assign value / call arg / for-of source
        // hoist init): under a materializing mode it becomes the
        // synthesized `__torajs_arguments: any[]` local, which the
        // consumer then treats as an ordinary array-like. Every other
        // mode leaves the node for the checker's loud reject.
        Expr::Ident(n) if n == "arguments" => {
            if is_argv_fn
                || matches!(
                    argc_mode,
                    ArgcMode::FoldTo(_) | ArgcMode::LiveLength(_) | ArgcMode::Unmapped(_)
                )
            {
                return ast.add_expr(Expr::Ident("__torajs_arguments".into()));
            }
            eid
        }
        other => rewrite_recurse_arm(
            ast,
            eid,
            other,
            params,
            argc_mode,
            is_argv_fn,
            sloppy_callee,
        ),
    }
}

/// The `arguments.length` fold, one arm per [`ArgcMode`]. Returns
/// `None` for [`ArgcMode::KeepLoud`]'s untouched node (the caller
/// returns the original ExprId so the checker rejects it loudly — a
/// closure VALUE's real argc needs the ABI face; folding the declared
/// arity would be silent-wrong).
fn rewrite_length_read(
    ast: &mut Ast,
    argc_mode: ArgcMode,
    params: &[String],
    eid: ExprId,
) -> ExprId {
    match argc_mode {
        // S3.2 — every real-argc face reads the S1 hidden ABI argc
        // (env-first S3.2, this-first S1-T2, head-less S1-H2).
        ArgcMode::Real => ast.add_expr(Expr::Ident("__torajs_argc".into())),
        // Write-shaped env-first face — both positions (read and
        // Assign / PostIncr target) land on the synthesized local.
        ArgcMode::RealLocal => ast.add_expr(Expr::Ident("__torajs_argc_len".into())),
        ArgcMode::FoldArity => ast.add_expr(Expr::Number(params.len() as f64)),
        // RFC 20260801 — IIFE static-argv face: the call site's exact
        // arg count (NOT params.len(), which over-counts on an
        // under-filled site and carries the injected extras
        // otherwise). The unmapped face folds the same count — only
        // element aliasing differs (see the Index arm).
        ArgcMode::FoldTo(n) | ArgcMode::Unmapped(n) | ArgcMode::Mapped(n) => {
            ast.add_expr(Expr::Number(n as f64))
        }
        // Length-write knife — reads AND writes ride the materialized
        // array's live `.length` (this arm serves both positions: an
        // Assign target and a PostIncr target flow through it
        // unchanged).
        ArgcMode::LiveLength(_) => {
            let synth_obj = ast.add_expr(Expr::Ident("__torajs_arguments".into()));
            ast.add_expr(Expr::Member {
                obj: synth_obj,
                name: "length".into(),
            })
        }
        ArgcMode::KeepLoud => eid,
    }
}
/// RFC 20260801 knife 2 — the mechanical copy-on-write recursion
/// arms (walker-mirror family): every shape here mirrors one the
/// walkers' detection scan reaches, so a body the pass decided to
/// materialize gets its touches swapped at any depth — a stale
/// `arguments` ident would silently ride a fallback lane (`typeof
/// arguments` folded to "undefined" through the undeclared-ident
/// typeof fold). Split from [`rewrite_arguments_in_expr`] (length-
/// write knife pushed the match past the 200-line fn limit); the
/// `arguments`-special arms (Member-length / Index / Call / Array /
/// bare Ident) stay in the main fn, everything below only recurses.
/// `(__torajs_arguments as any).callee` — the keyed spelling every
/// callee touch in a materialized sloppy body rides. The mint's
/// synthesized defineProperty seeded the bag entry, so keyed reads
/// observe later deletes / redefines. The `as any` hop routes the
/// member through the any-lane keyed machinery (read / write /
/// delete all live there); the checker rejects an unknown member on
/// the `any[]`-typed local itself, and the lowering has no
/// String-index assign lane on it either.
pub(super) fn keyed_callee_ref(ast: &mut Ast) -> ExprId {
    let arr = ast.add_expr(Expr::Ident("__torajs_arguments".into()));
    let arr_any = ast.add_expr(Expr::As {
        expr: arr,
        ty_ann: "any".into(),
    });
    ast.add_expr(Expr::Member {
        obj: arr_any,
        name: "callee".into(),
    })
}
