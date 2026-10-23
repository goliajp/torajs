//! The manual iterator protocol's STEP (`desugar_generators_forof`,
//! RFC 20260915): `let __gfstep_N: any = <next call>` — a `for await`
//! awaits it through the machine's own suspend (an async generator's
//! `__AwaitMark`, an async function's plain yield; the step comes from
//! `__agen_step_call`, whose Async-from-Sync posture is decided by the
//! `@@asyncIterator` probe riding along), a sync loop calls `next()`
//! with no argument (§7.4.4), and a sync `yield*` delegation passes its
//! one `received` argument (§27.5.3.2 step 7.a.i). Also the
//! `(src as any)[Symbol.<well_known>]` probe both protocols derive the
//! iterator from. Split out when the parent crossed the file-size line.

use super::desugar_generators_forof::Cx;
use super::desugar_generators_forof_close::Suspend;
use super::{Ast, Expr, Stmt};

/// The loop's step: `let __gfstep_N: any = <next call>` — awaited through
/// the machine's own suspend for a `for await`, a plain call for a sync
/// loop (`delegation_recv` is the `next(received)` argument of a sync
/// `yield*`).
pub(super) fn build_step_decl(
    ast: &mut Ast,
    cx: &Cx,
    it_name: &str,
    am_name: &str,
    step_name: &str,
    is_await: bool,
    delegation_recv: Option<&str>,
) -> Stmt {
    let it_ref = ast.add_expr(Expr::Ident(it_name.to_string()));
    if is_await {
        let helper = ast.add_expr(Expr::Ident("__agen_step_call".into()));
        // The `@@asyncIterator` probe rides along: it decides whether a
        // throwing `next()` is the body's own throw (async) or the
        // Async-from-Sync wrapper's rejection (sync).
        let am_ref = ast.add_expr(Expr::Ident(am_name.to_string()));
        let call = ast.add_expr(Expr::Call {
            callee: helper,
            args: vec![it_ref, am_ref],
        });
        let operand = match cx.suspend {
            Suspend::Mark => super::desugar_generators_async::mint_await_mark(ast, call),
            // the driver awaits what an async function's machine
            // yields, so the step it sends back is this call's settled
            // value, as for a `let v = await e` the async rewrite made
            // (`yield_into_await_origins`)
            Suspend::Plain => {
                ast.await_yield_values.insert(call);
                call
            }
            Suspend::None => call,
        };
        Stmt::YieldInto {
            var: step_name.to_string(),
            type_ann: Some("any".into()),
            value: operand,
        }
    } else {
        let next_member = ast.add_expr(Expr::Member {
            obj: it_ref,
            name: "next".into(),
        });
        // A sync `yield*` delegation (`__ysv_` / `__ysxv_`, the parser's
        // generic lane) drives the inner iterator with `next(received)`
        // — §27.5.3.2 step 7.a.i always passes ONE argument: undefined
        // first, then whatever the outer `next(v)` was resumed with
        // (`this.__sent`, refreshed after each yield below). It used to
        // ride a name-keyed default padding by accident
        // (`staging/sm/generators/delegating-yield-11`). A plain
        // `for…of` calls `next()` with none (§7.4.4).
        let args = match delegation_recv {
            Some(recv) => vec![ast.add_expr(Expr::Ident(recv.to_string()))],
            None => Vec::new(),
        };
        let next_call = ast.add_expr(Expr::Call {
            callee: next_member,
            args,
        });
        Stmt::LetDecl {
            mutable: false,
            name: step_name.to_string(),
            type_ann: Some("any".into()),
            init: next_call,
            is_var: false,
        }
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
