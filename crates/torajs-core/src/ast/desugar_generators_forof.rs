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
//! Narrowed faces (registered, not silently wrong): the for-in
//! desugar (`forin_obj`) and `for await` (F3 — needs for-await-any)
//! keep the catch-all; §14.7.5's abrupt-completion iterator close
//! (`return()` forwarding) is not part of the manual shape.

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
            is_await,
            body,
            ..
        } = &stmts[i]
            && forin_obj.is_none()
            && !*is_await
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
            body,
            forin_obj,
            is_await,
            ..
        } => {
            rewrite_in_stmt(ast, body, counter);
            if forin_obj.is_none() && !*is_await && stmt_contains_yield(body) {
                let old = std::mem::replace(s, Stmt::Multi(Vec::new()));
                let Stmt::ForOf {
                    var_name,
                    src_ident,
                    body,
                    ..
                } = old
                else {
                    unreachable!()
                };
                *s = build_manual_protocol(ast, var_name, src_ident, *body, counter);
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

/// Build the module-doc shape. `body` is the already-recursed loop
/// body; the caller's slot receives the Multi.
fn build_manual_protocol(
    ast: &mut Ast,
    var_name: String,
    src_ident: String,
    body: Stmt,
    counter: &mut usize,
) -> Stmt {
    let n = *counter;
    *counter += 1;
    let it_name = format!("__gfit_{n}");
    let step_name = format!("__gfstep_{n}");

    // let __gfit_N: any = (src as any)[Symbol.iterator]();
    let src_ref = ast.add_expr(Expr::Ident(src_ident));
    let src_any = ast.add_expr(Expr::As {
        expr: src_ref,
        ty_ann: "any".into(),
    });
    let sym = ast.add_expr(Expr::Ident("Symbol".into()));
    let sym_iter = ast.add_expr(Expr::Member {
        obj: sym,
        name: "iterator".into(),
    });
    let get_iter = ast.add_expr(Expr::Index {
        obj: src_any,
        index: sym_iter,
    });
    let iter_call = ast.add_expr(Expr::Call {
        callee: get_iter,
        args: Vec::new(),
    });
    let it_decl = Stmt::LetDecl {
        mutable: false,
        name: it_name.clone(),
        type_ann: Some("any".into()),
        init: iter_call,
        is_var: false,
    };

    // let __gfstep_N: any = __gfit_N.next();
    let it_ref = ast.add_expr(Expr::Ident(it_name));
    let next_member = ast.add_expr(Expr::Member {
        obj: it_ref,
        name: "next".into(),
    });
    let next_call = ast.add_expr(Expr::Call {
        callee: next_member,
        args: Vec::new(),
    });
    let step_decl = Stmt::LetDecl {
        mutable: false,
        name: step_name.clone(),
        type_ann: Some("any".into()),
        init: next_call,
        is_var: false,
    };

    // if (__gfstep_N.done) break;
    let step_ref_done = ast.add_expr(Expr::Ident(step_name.clone()));
    let done_member = ast.add_expr(Expr::Member {
        obj: step_ref_done,
        name: "done".into(),
    });
    let done_check = Stmt::If {
        cond: done_member,
        then_branch: Box::new(Stmt::Break(None)),
        else_branch: None,
    };

    // let <v>: any = __gfstep_N.value;
    let step_ref_value = ast.add_expr(Expr::Ident(step_name));
    let value_member = ast.add_expr(Expr::Member {
        obj: step_ref_value,
        name: "value".into(),
    });
    let var_decl = Stmt::LetDecl {
        mutable: true,
        name: var_name,
        type_ann: Some("any".into()),
        init: value_member,
        is_var: false,
    };

    let true_lit = ast.add_expr(Expr::Bool(true));
    let while_loop = Stmt::While {
        cond: true_lit,
        body: Box::new(Stmt::Block(vec![step_decl, done_check, var_decl, body])),
    };
    Stmt::Multi(vec![it_decl, while_loop])
}
