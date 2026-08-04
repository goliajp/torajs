//! RFC 20260804-method-rebind-generic-body blade 2 — mint the
//! receiver-polymorphic twin `__cmany_<C>__<m>` beside each
//! `__cm_<C>__<m>` instance method whose body reads its receiver.
//!
//! The mono body reads `__this` at static class offsets, which is
//! only sound when the receiver really is a `C` (direct calls,
//! vtable dispatch). A method VALUE that escaped into the any world
//! can be re-bound onto anything (`c.getM().call({v: 42})` —
//! §10.2.1.2 OrdinaryCallBindThis), so the boxed adapter (blade 3)
//! guards on the receiver's `class_tag@+8` and routes a foreign
//! receiver here: same source body, every param (receiver included)
//! typed `any`, so member reads lower through the any-member lane
//! (GetV — reflection reads on a `Tag::Obj`, dynobj reads on a
//! DynObj) instead of baked offsets.
//!
//! Receiver-free bodies mint nothing: the mono body never touches
//! `this`, so it runs correctly under any receiver (the S2.38
//! `this_free` fact, probed here at the AST level off the clone
//! map — an `Expr::This` / `Ident("__this")` among the cloned
//! nodes). A twin-less method keeps today's direct-mono dispatch.
//!
//! Ledger: a receiver-free probe miss leaves the cloned subtree as
//! unreferenced arena entries — append-only arena garbage the same
//! way every desugar rewrite strands its pre-rewrite nodes; nothing
//! lowers them.

use super::ast_def::Ast;
use super::clone_body::BodyCloner;
use super::expr::{Expr, Param};
use super::stmt::Stmt;

pub(in crate::ast) fn mint_generic_twin(
    ast: &mut Ast,
    mono_name: &str,
    params: &[Param],
    body: &[Stmt],
    type_params: &[String],
    span: crate::lexer::Span,
    appended: &mut Vec<Stmt>,
) {
    let mut cloner = BodyCloner::new(ast);
    let cloned_body = cloner.clone_stmts(body);
    let cloned_params: Vec<Param> = params
        .iter()
        .map(|p| Param {
            name: p.name.clone(),
            type_ann: Some(if p.is_rest { "any[]" } else { "any" }.to_string()),
            default: p.default.map(|d| cloner.clone_expr(d)),
            is_rest: p.is_rest,
        })
        .collect();
    let reads_receiver = cloner.map.keys().any(|old| {
        matches!(cloner.ast.get_expr(*old), Expr::This | Expr::NewTarget)
            || matches!(cloner.ast.get_expr(*old), Expr::Ident(n) if n == "__this")
    });
    if !reads_receiver {
        return;
    }
    super::clone_body_tables::migrate(&mut cloner);
    let twin_name = format!("__cmany_{}", &mono_name["__cm_".len()..]);
    ast.cmany_twins
        .insert(mono_name.to_string(), twin_name.clone());
    appended.push(Stmt::FnDecl {
        name: twin_name,
        type_params: type_params.to_vec(),
        params: cloned_params,
        return_type: Some("any".to_string()),
        body: cloned_body,
        is_generator: false,
        span,
    });
}
