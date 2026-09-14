//! Pass 0 of generator body prep — yield-bearing `for…of` rewrite
//! (RFC 20260728-gen-forof-yieldstar F1).
//!
//! The state machine has no ForOf arm: a yield-bearing `for…of` fell
//! into GenSm's catch-all verbatim, so its body's `yield` dropped out
//! of state-machine context ("yield only valid inside function*").
//! This pass rewrites the loop into the manual iterator protocol the
//! machine already lowers (the J.3 / S2.28 parse-time delegations'
//! proven shape):
//!
//! ```text
//! for (const v of src) body   →
//! let __gfit_N: any = (src as any)[Symbol.iterator]();
//! while (true) {
//!   let __gfstep_N: any = __gfit_N.next();
//!   if (__gfstep_N.done) break;
//!   let v: any = __gfstep_N.value;
//!   body
//! }
//! ```
//!
//! It MUST run before the let-lift (prep pass 2): the iterator and
//! step bindings have to become class fields or they would reset on
//! every `next()` re-entry. The parser's `src_ident` hoist stays
//! where it is — the rewrite consumes the name. `break` / `continue`
//! keep their meaning (the advance sits at loop top); a wrapping
//! label still reaches the While through GenSm's `pending_label`
//! hand-off. The element binding promotes to `any` (P10.7 default;
//! typed sources box through the As-any boundary).
//!
//! `for await` (F3) rides the same shape with the async drive's
//! taps: GetIterator runs with hint=async (an `@@asyncIterator`
//! probe ternary-selects the derive call, nullish falling to the
//! sync symbol per §7.3.11 GetMethod), and both the step and the
//! value reads go through the parser's `await` desugar (`.value`
//! member + `await_value_reads` mark) — `__torajs_anyv_await` is
//! identity for non-Promises, so a sync source's plain step passes
//! untouched while its Promise-valued element unwraps
//! (§27.1.4.4), and an async iterator's Promise step settles
//! before `done` / `value` are read (§14.7.5.6 step 6.d).
//!
//! RFC 20260915 blade 4: a `for await` is rewritten in EVERY machine
//! body, yield-bearing or not — its step is a suspend of the machine's
//! own kind (an async function's yields the awaited value, an async
//! generator's yields it under an `__AwaitMark`; `Suspend`), and the
//! sync `for…of` of an async function's body is rewritten once its
//! awaits are yields. Both shapes carry §7.4.9 / §7.4.11 iterator
//! close on the loop's completion (`desugar_generators_forof_close`).
//! The helpers the shape calls are injected once per program that can
//! hold a machine (`desugar_generators_forof_helpers`).
//!
//! Narrowed face (registered, not silently wrong): the for-in desugar
//! (`forin_obj`) keeps the catch-all.

use super::desugar_generators_forof_close::{Suspend, clear_live, set_live, wrap_with_close};
use super::desugar_generators_sm_rewrite::stmt_contains_yield;
use super::{Ast, Expr, Stmt};

/// One body's rewrite: the loop counter and which suspend spelling
/// the machine reads for a `for await` step.
struct Cx {
    counter: usize,
    suspend: Suspend,
}

/// Entry — walk the raw generator body, bottom-up. `is_async_gen`
/// picks the `for await` step's suspend spelling (mark mode for an
/// async generator's machine, the plain yield for an async function's).
pub(super) fn rewrite_yield_forof(ast: &mut Ast, body: &mut [Stmt], is_async_gen: bool) {
    let mut cx = Cx {
        counter: 0,
        suspend: if is_async_gen {
            Suspend::Mark
        } else {
            Suspend::Plain
        },
    };
    for s in body {
        rewrite_in_stmt(ast, s, &mut cx);
    }
}

/// Does this loop take the manual protocol — a body suspending inside
/// it, or the async form (whose step suspends by itself)?
fn takes_protocol(body: &Stmt, is_await: bool) -> bool {
    is_await || stmt_contains_yield(body)
}

fn rewrite_in_seq(ast: &mut Ast, stmts: &mut [Stmt], cx: &mut Cx) {
    for i in 0..stmts.len() {
        // The parser's non-ident-source hoist (`let __forof_src_N =
        // <src>`, no ann) precedes its ForOf in the same Block. The
        // let-lift types an ann-less field as Number; promote the
        // hoist to `any` when its loop is about to become the manual
        // protocol (which reads it through an As-any regardless).
        if let Stmt::ForOf {
            src_ident,
            forin_obj,
            body,
            is_await,
            ..
        } = unlabel(&stmts[i])
            && forin_obj.is_none()
            && i > 0
            && takes_protocol(body, *is_await)
        {
            let src = src_ident.clone();
            if let Stmt::LetDecl { name, type_ann, .. } = &mut stmts[i - 1]
                && *name == src
                && type_ann.is_none()
            {
                *type_ann = Some("any".into());
            }
        }
        rewrite_in_stmt(ast, &mut stmts[i], cx);
    }
}

/// The tail statement a rewritten for-of ends in: the While itself, or
/// the close wrap's Try holding it.
fn is_loop_tail(s: &Stmt) -> bool {
    match s {
        Stmt::While { .. } => true,
        Stmt::Try { body, .. } => matches!(body.as_slice(), [Stmt::While { .. }]),
        _ => false,
    }
}

/// Put `label` on the While inside `tail` (see [`is_loop_tail`]).
fn label_loop_tail(label: String, tail: Stmt) -> Stmt {
    match tail {
        Stmt::Try {
            mut body,
            had_catch,
            catch_param,
            catch_type,
            catch_body,
            finally_body,
        } => {
            let w = body.pop().expect("is_loop_tail matched a While");
            body.push(Stmt::Labeled {
                label,
                body: Box::new(w),
            });
            Stmt::Try {
                body,
                had_catch,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
            }
        }
        w => Stmt::Labeled {
            label,
            body: Box::new(w),
        },
    }
}

/// The statement under a label chain — a labeled for-of whose source
/// was hoisted sits as `Labeled { ForOf }` right after its hoist
/// (`parse_stmt::label_hoisted_for_of`).
fn unlabel(s: &Stmt) -> &Stmt {
    match s {
        Stmt::Labeled { body, .. } => unlabel(body),
        other => other,
    }
}

fn rewrite_in_stmt(ast: &mut Ast, s: &mut Stmt, cx: &mut Cx) {
    match s {
        Stmt::ForOf {
            body,
            forin_obj,
            is_await,
            ..
        } => {
            rewrite_in_stmt(ast, body, cx);
            if forin_obj.is_none() && takes_protocol(body, *is_await) {
                let old = std::mem::replace(s, Stmt::Multi(Vec::new()));
                let Stmt::ForOf {
                    var_name,
                    src_ident,
                    body,
                    is_await,
                    ..
                } = old
                else {
                    unreachable!()
                };
                if is_await && ast.yieldstar_async_vars.contains(&var_name) {
                    // An async generator's `yield*` — the delegation
                    // protocol, not the loop's (`desugar_generators_yieldstar`).
                    let n = cx.counter;
                    cx.counter += 1;
                    *s = super::desugar_generators_yieldstar::build_delegation_protocol(
                        ast, var_name, src_ident, n,
                    );
                } else {
                    *s = build_manual_protocol(ast, var_name, src_ident, *body, cx, is_await);
                }
            }
        }
        Stmt::Block(ss) | Stmt::Multi(ss) => {
            rewrite_in_seq(ast, ss, cx);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_in_stmt(ast, then_branch, cx);
            if let Some(eb) = else_branch {
                rewrite_in_stmt(ast, eb, cx);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::ForOfSplitIter { body, .. } => rewrite_in_stmt(ast, body, cx),
        // A labeled for-of becomes `Multi[decls…, Try { While }]` (the
        // close wrap; a delegation protocol ends in a bare While); the
        // label moves onto the While so the machine still sees a
        // labeled LOOP (`continue label` needs one), not a labeled
        // block — a jump to it from inside the loop's own try routes
        // through that try's finally like any escaping jump.
        Stmt::Labeled { body, .. } => {
            rewrite_in_stmt(ast, body, cx);
            if matches!(&**body, Stmt::Multi(ss) if ss.last().is_some_and(is_loop_tail)) {
                let Stmt::Labeled { label, body } = std::mem::replace(s, Stmt::Multi(Vec::new()))
                else {
                    unreachable!()
                };
                let Stmt::Multi(mut ss) = *body else {
                    unreachable!()
                };
                let last = ss.pop().expect("matched a loop tail");
                ss.push(label_loop_tail(label, last));
                *s = Stmt::Multi(ss);
            }
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                rewrite_in_stmt(ast, i, cx);
            }
            rewrite_in_stmt(ast, body, cx);
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                rewrite_in_seq(ast, &mut c.body, cx);
            }
            if let Some(d) = default {
                rewrite_in_seq(ast, d, cx);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            rewrite_in_seq(ast, body, cx);
            rewrite_in_seq(ast, catch_body, cx);
            if let Some(fb) = finally_body {
                rewrite_in_seq(ast, fb, cx);
            }
        }
        _ => {}
    }
}

/// `(src as any)[Symbol.<well_known>]` — the symbol-keyed probe the
/// derive line reads (and, with `Call`, invokes: F0b index-call
/// receiver semantics make `o[k]()` take `o` as `this`).
pub(super) fn symbol_index_expr(
    ast: &mut Ast,
    src_ident: &str,
    well_known: &str,
) -> crate::ast::ExprId {
    let src_ref = ast.add_expr(Expr::Ident(src_ident.to_string()));
    let src_any = ast.add_expr(Expr::As {
        expr: src_ref,
        ty_ann: "any".into(),
    });
    let sym = ast.add_expr(Expr::Ident("Symbol".into()));
    let sym_key = ast.add_expr(Expr::Member {
        obj: sym,
        name: well_known.into(),
    });
    ast.add_expr(Expr::Index {
        obj: src_any,
        index: sym_key,
    })
}

/// Build the module-doc shape. `body` is the already-recursed loop
/// body; the caller's slot receives the Multi.
fn build_manual_protocol(
    ast: &mut Ast,
    var_name: String,
    src_ident: String,
    body: Stmt,
    cx: &mut Cx,
    is_await: bool,
) -> Stmt {
    let n = cx.counter;
    cx.counter += 1;
    let it_name = format!("__gfit_{n}");
    let step_name = format!("__gfstep_{n}");

    // sync:  let __gfit_N: any = (src as any)[Symbol.iterator]();
    // async: let __gfam_N: any = (src as any)[Symbol.asyncIterator];
    //        let __gfit_N: any = __gfam_N != null
    //          ? (src as any)[Symbol.asyncIterator]()
    //          : (src as any)[Symbol.iterator]();
    // (§7.4.2 hint=async step 2.a — the @@asyncIterator probe
    // outranks the sync symbol; LooseNeq null is GetMethod's
    // nullish-is-absent.)
    let mut pre_decls: Vec<Stmt> = Vec::new();
    let am_name = format!("__gfam_{n}");
    let iter_call = if is_await {
        let am_probe = symbol_index_expr(ast, &src_ident, "asyncIterator");
        pre_decls.push(Stmt::LetDecl {
            mutable: false,
            name: am_name.clone(),
            type_ann: Some("any".into()),
            init: am_probe,
            is_var: false,
        });
        let am_ref = ast.add_expr(Expr::Ident(am_name.clone()));
        let null_lit = ast.add_expr(Expr::Null);
        let has_async = ast.add_expr(Expr::BinOp {
            op: crate::ast::BinOp::LooseNeq,
            left: am_ref,
            right: null_lit,
        });
        let async_get = symbol_index_expr(ast, &src_ident, "asyncIterator");
        let async_call = ast.add_expr(Expr::Call {
            callee: async_get,
            args: Vec::new(),
        });
        let sync_get = symbol_index_expr(ast, &src_ident, "iterator");
        let sync_call = ast.add_expr(Expr::Call {
            callee: sync_get,
            args: Vec::new(),
        });
        ast.add_expr(Expr::Ternary {
            cond: has_async,
            then_branch: async_call,
            else_branch: sync_call,
        })
    } else {
        let get_iter = symbol_index_expr(ast, &src_ident, "iterator");
        ast.add_expr(Expr::Call {
            callee: get_iter,
            args: Vec::new(),
        })
    };
    let it_decl = Stmt::LetDecl {
        mutable: false,
        name: it_name.clone(),
        type_ann: Some("any".into()),
        init: iter_call,
        is_var: false,
    };

    // sync:  let __gfstep_N: any = __gfit_N.next();
    // async: let __gfstep_N: any = yield <suspend>(__agen_step_call(__gfit_N, __gfam_N));
    // (§14.7.5.6 step 6.d / §27.5.3.2 async yield* — the step is
    // awaited by the driver: an async generator's under an await
    // mark, an async function's as the plain yielded value; a SYNC
    // iterator's plain result takes `__agen_step_call`'s Async-from-
    // Sync posture, an async iterator's thenable passes through —
    // `desugar_generators_forof_helpers`.)
    let it_ref = ast.add_expr(Expr::Ident(it_name));
    let step_decl = if is_await {
        let helper = ast.add_expr(Expr::Ident("__agen_step_call".into()));
        // The `@@asyncIterator` probe rides along: it decides whether a
        // throwing `next()` is the body's own throw (async) or the
        // Async-from-Sync wrapper's rejection (sync).
        let am_ref = ast.add_expr(Expr::Ident(am_name.clone()));
        let call = ast.add_expr(Expr::Call {
            callee: helper,
            args: vec![it_ref, am_ref],
        });
        let operand = match cx.suspend {
            Suspend::Mark => super::desugar_generators_async::mint_await_mark(ast, call),
            Suspend::Plain | Suspend::None => call,
        };
        Stmt::YieldInto {
            var: step_name.clone(),
            type_ann: Some("any".into()),
            value: operand,
        }
    } else {
        let next_member = ast.add_expr(Expr::Member {
            obj: it_ref,
            name: "next".into(),
        });
        let next_call = ast.add_expr(Expr::Call {
            callee: next_member,
            args: Vec::new(),
        });
        Stmt::LetDecl {
            mutable: false,
            name: step_name.clone(),
            type_ann: Some("any".into()),
            init: next_call,
            is_var: false,
        }
    };

    // if (__gfstep_N.done) break;
    // Expression-position yield* (parse_yield.rs hoist): the done
    // step's `.value` is the YieldExpression's value (§27.5.3.2 —
    // the async form's step await has already settled it), so the
    // done arm writes it into the registered `__yx_` temp first.
    let step_ref_done = ast.add_expr(Expr::Ident(step_name.clone()));
    let done_member = ast.add_expr(Expr::Member {
        obj: step_ref_done,
        name: "done".into(),
    });
    let done_then: Stmt = match ast.yieldstar_done_targets.get(&var_name).cloned() {
        Some(target) => {
            let step_ref = ast.add_expr(Expr::Ident(step_name.clone()));
            let final_value = ast.add_expr(Expr::Member {
                obj: step_ref,
                name: "value".into(),
            });
            let target_ref = ast.add_expr(Expr::Ident(target));
            let assign = ast.add_expr(Expr::Assign {
                target: target_ref,
                value: final_value,
            });
            Stmt::Block(vec![Stmt::Expr(assign), Stmt::Break(None)])
        }
        None => Stmt::Break(None),
    };
    let done_check = Stmt::If {
        cond: done_member,
        then_branch: Box::new(done_then),
        else_branch: None,
    };

    // let <v>: any = __gfstep_N.value;
    // (async: the step itself was awaited above — a sync iterator's
    // element inside it, per §27.1.4.4 — and a `yield v` in the body
    // is awaited again by the driver, the two Awaits §27.5.3.2 spells
    // for an async `yield*`.)
    let step_ref_value = ast.add_expr(Expr::Ident(step_name));
    let var_init = ast.add_expr(Expr::Member {
        obj: step_ref_value,
        name: "value".into(),
    });
    let var_decl = Stmt::LetDecl {
        mutable: true,
        name: var_name,
        type_ann: Some("any".into()),
        init: var_init,
        is_var: false,
    };

    // `__gflive_N` brackets the body: cleared before the step (a
    // throwing `next()` closes nothing), set once the element is
    // bound — the close in the wrap reads it.
    let clear = clear_live(ast, n);
    let set = set_live(ast, n);
    let true_lit = ast.add_expr(Expr::Bool(true));
    let while_loop = Stmt::While {
        cond: true_lit,
        body: Box::new(Stmt::Block(vec![
            clear, step_decl, done_check, var_decl, set, body,
        ])),
    };
    let mut head = pre_decls;
    head.push(it_decl);
    // A sync loop's close is a plain call; the async close is awaited
    // through the machine's own suspend.
    let suspend = if is_await { cx.suspend } else { Suspend::None };
    wrap_with_close(ast, n, is_await, suspend, head, while_loop)
}
