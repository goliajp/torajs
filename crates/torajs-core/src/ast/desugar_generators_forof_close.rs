//! The completion-aware wrap around a state machine's manual iterator
//! protocol — §7.4.9 IteratorClose / §7.4.11 AsyncIteratorClose for
//! `desugar_generators_forof` (RFC 20260915 blade 4). The transpiler
//! shape (Babel's `_asyncIterator` / TypeScript's `__asyncValues`):
//!
//! ```text
//! <pre_decls>; <it_decl>;
//! let __gflive_N: boolean = false;   // a value step's body is running
//! let __gfthrew_N: boolean = false;  // the loop completed by throwing
//! let __gferr_N: any = undefined;
//! try { <while_loop> }
//! catch (__gfc_N: any) { __gfthrew_N = true; __gferr_N = __gfc_N; }
//! finally {
//!   if (__gflive_N) {
//!     sync:  __gfor_close(__gfit_N, __gfthrew_N);
//!     async: let __gfcp_N: any = __gfor_close_async(__gfit_N, __gfam_N, __gfthrew_N);
//!            if (__gfcp_N != null) {
//!              if (__gfthrew_N) { try { <yield __gfcp_N> } catch (__gfce_N: any) { } }
//!              else { let __gfcr_N: any = <yield __gfcp_N>; __gfor_close_result(__gfcr_N); }
//!            }
//!   }
//!   if (__gfthrew_N) throw __gferr_N;
//! }
//! ```
//!
//! `__gflive_N` is true exactly while a value step's body runs (the
//! loop clears it before each `next()` and sets it after the element
//! is bound), so a throwing `next()` closes nothing (§14.7.5.7 — the
//! iterator is not closed after an abrupt `next`), while `break`, a
//! labeled jump out, `return` and a body throw all close. The machine
//! routes every one of those through this finally (D3a / D4 copies);
//! the async close's own suspend sits inside them. On the throw path
//! the close's rejection is swallowed by the inner try/catch — the
//! original completion wins (§7.4.11 step 4) — while the await itself
//! still happens, keeping the tick count the spec's Await gives.

use super::{Ast, BinOp, Expr, ExprId, Stmt};

/// Which suspend spelling the enclosing machine reads.
#[derive(Clone, Copy)]
pub(super) enum Suspend {
    /// A `for…of` in a generator, or a `for await` whose step is not
    /// a suspend of this machine's own kind — no await on the close.
    None,
    /// An async FUNCTION's machine (D1): the awaited value is yielded.
    Plain,
    /// An async GENERATOR's machine (D4): the value is yielded under an
    /// `__AwaitMark`.
    Mark,
}

fn ident(ast: &mut Ast, name: &str) -> ExprId {
    ast.add_expr(Expr::Ident(name.to_string()))
}

fn call(ast: &mut Ast, callee: &str, args: Vec<ExprId>) -> ExprId {
    let c = ident(ast, callee);
    ast.add_expr(Expr::Call { callee: c, args })
}

fn assign_bool(ast: &mut Ast, name: &str, v: bool) -> Stmt {
    let target = ident(ast, name);
    let value = ast.add_expr(Expr::Bool(v));
    Stmt::Expr(ast.add_expr(Expr::Assign { target, value }))
}

fn let_any(name: &str, ann: &str, init: ExprId) -> Stmt {
    Stmt::LetDecl {
        mutable: true,
        name: name.to_string(),
        type_ann: Some(ann.to_string()),
        init,
        is_var: false,
    }
}

/// `__gflive_N = false;` — placed at the loop top, before the step.
pub(super) fn clear_live(ast: &mut Ast, n: usize) -> Stmt {
    assign_bool(ast, &format!("__gflive_{n}"), false)
}

/// `__gflive_N = true;` — placed after the element is bound.
pub(super) fn set_live(ast: &mut Ast, n: usize) -> Stmt {
    assign_bool(ast, &format!("__gflive_{n}"), true)
}

/// The yield the close awaits through — plain or marked.
fn suspend_operand(ast: &mut Ast, inner: ExprId, suspend: Suspend) -> ExprId {
    match suspend {
        Suspend::Mark => super::desugar_generators_async::mint_await_mark(ast, inner),
        Suspend::Plain | Suspend::None => inner,
    }
}

/// Build the module-doc shape around `while_loop` (which already
/// carries the live-flag writes), answering the whole `Multi`.
pub(super) fn wrap_with_close(
    ast: &mut Ast,
    n: usize,
    is_await: bool,
    suspend: Suspend,
    mut head: Vec<Stmt>,
    while_loop: Stmt,
) -> Stmt {
    let it_name = format!("__gfit_{n}");
    let am_name = format!("__gfam_{n}");
    let live = format!("__gflive_{n}");
    let threw = format!("__gfthrew_{n}");
    let err = format!("__gferr_{n}");
    let catch_name = format!("__gfc_{n}");

    let f = ast.add_expr(Expr::Bool(false));
    head.push(let_any(&live, "boolean", f));
    let f = ast.add_expr(Expr::Bool(false));
    head.push(let_any(&threw, "boolean", f));
    let u = ident(ast, "undefined");
    head.push(let_any(&err, "any", u));

    // catch: __gfthrew = true; __gferr = __gfc;
    let set_threw = assign_bool(ast, &threw, true);
    let err_target = ident(ast, &err);
    let caught = ident(ast, &catch_name);
    let store = Stmt::Expr(ast.add_expr(Expr::Assign {
        target: err_target,
        value: caught,
    }));

    // finally: if (__gflive) { close } if (__gfthrew) throw __gferr;
    let close: Vec<Stmt> = if is_await {
        let it = ident(ast, &it_name);
        let am = ident(ast, &am_name);
        let th = ident(ast, &threw);
        let cp_name = format!("__gfcp_{n}");
        let cp_init = call(ast, "__gfor_close_async", vec![it, am, th]);
        let cp_decl = let_any(&cp_name, "any", cp_init);
        let cp_ref = ident(ast, &cp_name);
        let null = ast.add_expr(Expr::Null);
        let has_close = ast.add_expr(Expr::BinOp {
            op: BinOp::LooseNeq,
            left: cp_ref,
            right: null,
        });
        // throw path: try { yield cp } catch (__gfce) { }
        let cp_ref = ident(ast, &cp_name);
        let op = suspend_operand(ast, cp_ref, suspend);
        let swallowed = Stmt::Try {
            body: vec![Stmt::Yield(op)],
            had_catch: true,
            catch_param: Some(format!("__gfce_{n}")),
            catch_type: Some("any".into()),
            catch_body: Vec::new(),
            finally_body: None,
        };
        // other completions: let cr = yield cp; __gfor_close_result(cr);
        let cr_name = format!("__gfcr_{n}");
        let cp_ref = ident(ast, &cp_name);
        let op = suspend_operand(ast, cp_ref, suspend);
        let awaited = Stmt::YieldInto {
            var: cr_name.clone(),
            type_ann: Some("any".into()),
            value: op,
        };
        let cr_ref = ident(ast, &cr_name);
        let check = Stmt::Expr(call(ast, "__gfor_close_result", vec![cr_ref]));
        let th = ident(ast, &threw);
        let by_completion = Stmt::If {
            cond: th,
            then_branch: Box::new(swallowed),
            else_branch: Some(Box::new(Stmt::Block(vec![awaited, check]))),
        };
        vec![
            cp_decl,
            Stmt::If {
                cond: has_close,
                then_branch: Box::new(Stmt::Block(vec![by_completion])),
                else_branch: None,
            },
        ]
    } else {
        let it = ident(ast, &it_name);
        let th = ident(ast, &threw);
        vec![Stmt::Expr(call(ast, "__gfor_close", vec![it, th]))]
    };
    let live_ref = ident(ast, &live);
    let if_live = Stmt::If {
        cond: live_ref,
        then_branch: Box::new(Stmt::Block(close)),
        else_branch: None,
    };
    let th = ident(ast, &threw);
    let err_ref = ident(ast, &err);
    let rethrow = Stmt::If {
        cond: th,
        then_branch: Box::new(Stmt::Throw(err_ref)),
        else_branch: None,
    };

    head.push(Stmt::Try {
        body: vec![while_loop],
        had_catch: true,
        catch_param: Some(catch_name),
        catch_type: Some("any".into()),
        catch_body: vec![set_threw, store],
        finally_body: Some(vec![if_live, rethrow]),
    });
    Stmt::Multi(head)
}
