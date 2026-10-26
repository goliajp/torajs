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
//! member + `await_value_reads` mark), each of which suspends the
//! frame; awaiting a non-Promise resumes with the value itself
//! (§27.7.5.3 Await), so a sync source's plain step passes
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
//! The helpers the shape calls are injected once per program whose
//! machines hold such a loop (`desugar_generators_forof_helpers`).
//!
//! A yield-bearing for-in takes its own counted walk over the parser's
//! key snapshot (`desugar_generators_forin`).

use super::desugar_generators_forof_close::{Suspend, clear_live, set_live, wrap_with_close};
use super::desugar_generators_forof_label::{is_loop_tail, label_loop_tail, unlabel};
use super::desugar_generators_forof_step::{build_step_decl, symbol_index_expr};
use super::desugar_generators_sm_rewrite::stmt_contains_yield;
use super::{Ast, Expr, Stmt};

/// One body's rewrite: the loop counter and which suspend spelling
/// the machine reads for a `for await` step.
pub(super) struct Cx {
    pub(super) counter: usize,
    pub(super) suspend: Suspend,
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
/// it, or the async form (whose step suspends by itself)? A
/// destructuring PATTERN head (the parser's `__forof_destr_N` loop
/// local + pattern binds in the body) is no exception since the lift
/// types an object rest's bind as `any` (`objlit_ann`'s spread arm).
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
        // A for-in's hoisted key snapshot is a `string[]` (the
        // checker's answer for `Object.__forinKeys`), which the lift
        // needs spelled out as well.
        if let Stmt::ForOf {
            src_ident,
            forin_obj: Some(_),
            body,
            ..
        } = unlabel(&stmts[i])
            && i > 0
            && stmt_contains_yield(body)
        {
            let src = src_ident.clone();
            if let Stmt::LetDecl { name, type_ann, .. } = &mut stmts[i - 1]
                && *name == src
                && type_ann.is_none()
            {
                *type_ann = Some("string[]".into());
            }
        }
        rewrite_in_stmt(ast, &mut stmts[i], cx);
    }
}

pub(super) fn rewrite_in_stmt(ast: &mut Ast, s: &mut Stmt, cx: &mut Cx) {
    match s {
        Stmt::ForOf {
            body,
            forin_obj,
            is_await,
            ..
        } => {
            rewrite_in_stmt(ast, body, cx);
            if forin_obj.is_some() && stmt_contains_yield(body) {
                let old = std::mem::replace(s, Stmt::Multi(Vec::new()));
                let Stmt::ForOf {
                    var_name,
                    src_ident,
                    forin_obj: Some(obj),
                    body,
                    ..
                } = old
                else {
                    unreachable!()
                };
                let n = cx.counter;
                cx.counter += 1;
                *s = super::desugar_generators_forin::build_forin_walk(
                    ast, var_name, src_ident, obj, *body, n,
                );
            } else if forin_obj.is_none() && takes_protocol(body, *is_await) {
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
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => rewrite_in_stmt(ast, body, cx),
        Stmt::ForOfSplitIter { .. } => super::desugar_generators_forof_split::rewrite(ast, s, cx),
        // A labeled for-of becomes `Multi[decls…, Try { While }]` (the
        // close wrap; a delegation protocol ends in a bare While); the
        // label moves onto the While so the machine still sees a
        // labeled LOOP (`continue label` needs one), not a labeled
        // block — a jump to it from inside the loop's own try routes
        // through that try's finally like any escaping jump.
        Stmt::Labeled { body, .. } => {
            rewrite_in_stmt(ast, body, cx);
            if loop_tail_mut(body).is_some() {
                let Stmt::Labeled { label, mut body } =
                    std::mem::replace(s, Stmt::Multi(Vec::new()))
                else {
                    unreachable!()
                };
                let tail = loop_tail_mut(&mut body).expect("matched a loop tail");
                let last = std::mem::replace(tail, Stmt::Multi(Vec::new()));
                *tail = label_loop_tail(label, last);
                *s = *body;
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

/// Build the module-doc shape. `body` is the already-recursed loop
/// body; the caller's slot receives the Multi.
pub(super) fn build_manual_protocol(
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
    let is_delegation =
        !is_await && (var_name.starts_with("__ysv_") || var_name.starts_with("__ysxv_"));
    let recv_name = format!("__gfrecv_{n}");

    // sync:  let __gfit_N: any = (src as any)[Symbol.iterator]();
    // async: let __gfam_N: any = (src as any)[Symbol.asyncIterator];
    //        let __gfit_N: any = __gfam_N != null
    //          ? __gfam_N.call(src as any)
    //          : (src as any)[Symbol.iterator]();
    // (§7.4.2 hint=async step 2.a — the @@asyncIterator probe
    // outranks the sync symbol; LooseNeq null is GetMethod's
    // nullish-is-absent. The probed method is CALLED, not read a
    // second time: an accessor `@@asyncIterator` is got once —
    // `getiterator-accessor-001` counted two.)
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
        let am_fn = ast.add_expr(Expr::Ident(am_name.clone()));
        let am_call = ast.add_expr(Expr::Member {
            obj: am_fn,
            name: "call".into(),
        });
        let src_ref = ast.add_expr(Expr::Ident(src_ident.clone()));
        let src_any = ast.add_expr(Expr::As {
            expr: src_ref,
            ty_ann: "any".into(),
        });
        let async_call = ast.add_expr(Expr::Call {
            callee: am_call,
            args: vec![src_any],
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
    let step_decl = build_step_decl(
        ast,
        cx,
        &it_name,
        &am_name,
        &step_name,
        is_await,
        is_delegation.then_some(recv_name.as_str()),
    );

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
    let mut loop_body = vec![clear, step_decl, done_check, var_decl, set, body];
    if is_delegation {
        // `__gfrecv_N = this.__sent` — the value the outer `next(v)`
        // resumed the yield above with, for the next inner step.
        let target = ast.add_expr(Expr::Ident(recv_name.clone()));
        let this_id = ast.add_expr(Expr::This);
        let sent = ast.add_expr(Expr::Member {
            obj: this_id,
            name: "__sent".into(),
        });
        let assign = ast.add_expr(Expr::Assign {
            target,
            value: sent,
        });
        loop_body.push(Stmt::Expr(assign));
    }
    let true_lit = ast.add_expr(Expr::Bool(true));
    let while_loop = Stmt::While {
        cond: true_lit,
        body: Box::new(Stmt::Block(loop_body)),
    };
    let mut head = pre_decls;
    head.push(it_decl);
    if is_delegation {
        let undef = ast.add_expr(Expr::Ident("undefined".into()));
        head.push(Stmt::LetDecl {
            mutable: true,
            name: recv_name.clone(),
            type_ann: Some("any".into()),
            init: undef,
            is_var: false,
        });
    }
    // A sync loop's close is a plain call; the async close is awaited
    // through the machine's own suspend.
    let suspend = if is_await { cx.suspend } else { Suspend::None };
    wrap_with_close(ast, n, is_await, suspend, head, while_loop)
}

/// The loop a rewritten labeled statement ends in: the last statement
/// of its `Multi`, looking through a nested one (a for-in's hoists
/// followed by its counted walk).
fn loop_tail_mut(s: &mut Stmt) -> Option<&mut Stmt> {
    let Stmt::Multi(ss) = s else {
        return None;
    };
    let last = ss.last_mut()?;
    if is_loop_tail(last) {
        return Some(last);
    }
    loop_tail_mut(last)
}
