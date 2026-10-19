//! What the body of an async function that suspends over a frame
//! returns (`ast::desugar_async_ssa_gate` chose it): its return settles
//! the frame's result promise, so the body returns the promise's
//! value, and a body that may return a thenable returns `any` for the
//! frame to adopt it.

use std::collections::HashMap;

use crate::ast::{self, Ast, Expr, ExprId, GenericStructDecls, PropKey, Stmt};
use crate::ssa::{self, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_parse_type::parse_type;

/// Whether `name`'s body suspends over a frame, and the annotation its
/// return slot takes.
pub(crate) fn ret_ann(
    ast: &Ast,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    fn_table: &HashMap<String, ssa::FuncId>,
    name: &str,
    return_type: Option<&str>,
    body: &[Stmt],
) -> (bool, Option<String>) {
    if !ast.async_ssa.fns.contains(name) {
        return (false, return_type.map(str::to_string));
    }
    if returns_thenable(ast, expr_types, fn_table, body) {
        return (true, Some("any".to_string()));
    }
    (true, inner_ann(return_type))
}

/// The inner annotation of a suspending body's `Promise<T>` return:
/// the body itself returns `T`.
fn inner_ann(return_type: Option<&str>) -> Option<String> {
    let ann = return_type?;
    Some(
        match ann
            .strip_prefix("Promise<")
            .and_then(|r| r.strip_suffix('>'))
        {
            Some(inner) => inner.to_string(),
            None => ann.to_string(),
        },
    )
}

/// The return slot's type for the annotation `ann`. A body suspending
/// over a frame returns what settles its promise, so its width is that
/// promise's value point, the key every `await` of a call to it reads
/// (`promise_value_is_f64`); a field read also answers f64 when the
/// analysis gave up on containers.
#[allow(clippy::too_many_arguments)]
pub(super) fn ret_slot_ty(
    name: &str,
    ann: Option<&str>,
    suspend_body: bool,
    params: &[ast::Param],
    body: &[Stmt],
    ast: &Ast,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut crate::ssa::ArrLayouts,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &GenericStructDecls,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
    widths: &crate::num_width::WidthTable,
) -> Type {
    use crate::num_width::SlotKey;
    let parsed = parse_type(
        ann,
        aliases,
        arr_layouts,
        fn_sigs,
        generic_struct_decls,
        struct_layouts,
        inst_memo,
    );
    let ret = SlotKey::Ret(name.to_string());
    let key = if suspend_body {
        SlotKey::Field(Box::new(ret.clone()), "value".into())
    } else {
        ret.clone()
    };
    let ty = super::promote_and_widen(
        crate::ssa_lower::effective_ret_ty(parsed, ast, params, body),
        ann,
        &key,
        widths,
        arr_layouts,
        struct_layouts,
        fn_sigs,
    );
    if suspend_body
        && ty == Type::I64
        && ann == Some("number")
        && widths.field_is_f64(&ret, "value")
    {
        Type::F64
    } else {
        ty
    }
}

/// Does a `return` of the body hand back something the result must
/// adopt (§27.2.1.3.2: a promise or another thenable)? Such a body
/// returns `any`, as one declared `Promise<any>` does, so every return
/// reaches the frame boxed and the runtime adopts what is a thenable.
/// Any other object the body returns is still probed for a `then` when
/// it settles the result (`torajs-promise` `then_box`).
fn returns_thenable(
    ast: &Ast,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    fn_table: &HashMap<String, ssa::FuncId>,
    body: &[Stmt],
) -> bool {
    use crate::ast::desugar_with::walk;
    let may_adopt = |ty: Option<&crate::check::Type>| match ty {
        Some(crate::check::Type::Promise(_) | crate::check::Type::Any) => true,
        Some(t) => crate::check::awaited::may_be_thenable(t, ast, &|n| fn_table.contains_key(n)),
        None => false,
    };
    fn stmt(
        ast: &Ast,
        may_adopt: &dyn Fn(Option<&crate::check::Type>) -> bool,
        types: &HashMap<ExprId, crate::check::Type>,
        s: &Stmt,
    ) -> bool {
        if let Stmt::Return(Some(e)) = s
            && ast.async_ssa.return_calls.contains(e)
            && let Expr::Call { args, .. } = ast.get_expr(*e)
            && let Some(&arg) = args.first()
            && may_adopt(types.get(&arg))
        {
            return true;
        }
        walk::stmt_children_ref(s)
            .into_iter()
            .any(|c| stmt(ast, may_adopt, types, c))
    }
    body.iter().any(|s| stmt(ast, &may_adopt, expr_types, s))
}

/// The expression a `return` in a suspending body hands back: the
/// argument of the `Promise.resolve` `desugar_async` wrapped it in
/// (the frame settles the result with it).
pub(crate) fn returned_expr(ctx: &LowerCtx<'_>, maybe: Option<ExprId>) -> Option<ExprId> {
    let eid = maybe?;
    if !ctx.suspend_body || !ctx.ast.async_ssa.return_calls.contains(&eid) {
        return Some(eid);
    }
    match ctx.ast.get_expr(eid) {
        Expr::Call { args, .. } if args.len() == 1 => Some(args[0]),
        _ => Some(eid),
    }
}
