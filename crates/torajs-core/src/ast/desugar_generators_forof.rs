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
//! Narrowed faces (registered, not silently wrong): the for-in
//! desugar (`forin_obj`) keeps the catch-all; §14.7.5's
//! abrupt-completion iterator close (`return()` forwarding) is not
//! part of the manual shape.

use super::desugar_generators_sm_rewrite::stmt_contains_yield;
use super::{Ast, Expr, Stmt};

/// Entry — walk the raw generator body, bottom-up.
pub(super) fn rewrite_yield_forof(ast: &mut Ast, body: &mut [Stmt]) {
    let mut counter = 0usize;
    for s in body {
        rewrite_in_stmt(ast, s, &mut counter);
    }
}

fn rewrite_in_seq(ast: &mut Ast, stmts: &mut [Stmt], counter: &mut usize) {
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
            ..
        } = &stmts[i]
            && forin_obj.is_none()
            && i > 0
            && stmt_contains_yield(body)
        {
            let src = src_ident.clone();
            if let Stmt::LetDecl { name, type_ann, .. } = &mut stmts[i - 1]
                && *name == src
                && type_ann.is_none()
            {
                *type_ann = Some("any".into());
            }
        }
        rewrite_in_stmt(ast, &mut stmts[i], counter);
    }
}

fn rewrite_in_stmt(ast: &mut Ast, s: &mut Stmt, counter: &mut usize) {
    match s {
        Stmt::ForOf {
            body, forin_obj, ..
        } => {
            rewrite_in_stmt(ast, body, counter);
            if forin_obj.is_none() && stmt_contains_yield(body) {
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
                *s = build_manual_protocol(ast, var_name, src_ident, *body, counter, is_await);
            }
        }
        Stmt::Block(ss) | Stmt::Multi(ss) => {
            rewrite_in_seq(ast, ss, counter);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_in_stmt(ast, then_branch, counter);
            if let Some(eb) = else_branch {
                rewrite_in_stmt(ast, eb, counter);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::Labeled { body, .. } => rewrite_in_stmt(ast, body, counter),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                rewrite_in_stmt(ast, i, counter);
            }
            rewrite_in_stmt(ast, body, counter);
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                rewrite_in_seq(ast, &mut c.body, counter);
            }
            if let Some(d) = default {
                rewrite_in_seq(ast, d, counter);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            rewrite_in_seq(ast, body, counter);
            rewrite_in_seq(ast, catch_body, counter);
            if let Some(fb) = finally_body {
                rewrite_in_seq(ast, fb, counter);
            }
        }
        _ => {}
    }
}

/// `(src as any)[Symbol.<well_known>]` — the symbol-keyed probe the
/// derive line reads (and, with `Call`, invokes: F0b index-call
/// receiver semantics make `o[k]()` take `o` as `this`).
fn symbol_index_expr(ast: &mut Ast, src_ident: &str, well_known: &str) -> crate::ast::ExprId {
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

/// The parser's `await e` desugar, minted here: `e.value` marked in
/// `await_value_reads` so checker / lowering dispatch it by TYPE
/// (§27.7.5.1 — Promise unwraps, everything else is identity).
fn await_expr(ast: &mut Ast, inner: crate::ast::ExprId) -> crate::ast::ExprId {
    let read = ast.add_expr(Expr::Member {
        obj: inner,
        name: "value".into(),
    });
    ast.await_value_reads.insert(read);
    read
}

/// Build the module-doc shape. `body` is the already-recursed loop
/// body; the caller's slot receives the Multi.
fn build_manual_protocol(
    ast: &mut Ast,
    var_name: String,
    src_ident: String,
    body: Stmt,
    counter: &mut usize,
    is_await: bool,
) -> Stmt {
    let n = *counter;
    *counter += 1;
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
    let iter_call = if is_await {
        let am_name = format!("__gfam_{n}");
        let am_probe = symbol_index_expr(ast, &src_ident, "asyncIterator");
        pre_decls.push(Stmt::LetDecl {
            mutable: false,
            name: am_name.clone(),
            type_ann: Some("any".into()),
            init: am_probe,
            is_var: false,
        });
        let am_ref = ast.add_expr(Expr::Ident(am_name));
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

    // let __gfstep_N: any = [await] __gfit_N.next();
    // (async: §14.7.5.6 step 6.d — a Promise-shaped step settles
    // before done/value are read; identity for a sync iterator's
    // plain result.)
    let it_ref = ast.add_expr(Expr::Ident(it_name));
    let next_member = ast.add_expr(Expr::Member {
        obj: it_ref,
        name: "next".into(),
    });
    let next_call = ast.add_expr(Expr::Call {
        callee: next_member,
        args: Vec::new(),
    });
    let step_init = if is_await {
        await_expr(ast, next_call)
    } else {
        next_call
    };
    let step_decl = Stmt::LetDecl {
        mutable: false,
        name: step_name.clone(),
        type_ann: Some("any".into()),
        init: step_init,
        is_var: false,
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

    // let <v>: any = [await] __gfstep_N.value;
    // (async: §27.1.4.4 — a sync source's Promise-valued element
    // unwraps; an async iterator's already-settled value passes
    // through identity. The double-unwrap corner — an async
    // iterator answering a Promise-valued `value` — is registered
    // with the kernel lane's precise split as the reference.)
    let step_ref_value = ast.add_expr(Expr::Ident(step_name));
    let value_member = ast.add_expr(Expr::Member {
        obj: step_ref_value,
        name: "value".into(),
    });
    let var_init = if is_await {
        await_expr(ast, value_member)
    } else {
        value_member
    };
    let var_decl = Stmt::LetDecl {
        mutable: true,
        name: var_name,
        type_ann: Some("any".into()),
        init: var_init,
        is_var: false,
    };

    let true_lit = ast.add_expr(Expr::Bool(true));
    let while_loop = Stmt::While {
        cond: true_lit,
        body: Box::new(Stmt::Block(vec![step_decl, done_check, var_decl, body])),
    };
    let mut out = pre_decls;
    out.push(it_decl);
    out.push(while_loop);
    Stmt::Multi(out)
}
